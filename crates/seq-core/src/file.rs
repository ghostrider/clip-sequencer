//! Text form of the sequence file: parse with version check and validation, serialize.
//!
//! Version handling: the file is read once for its `version` field only, then parsed typed.
//!
//! - Version 4 (current, release 0.5): `start` is an object (`immediate` / `beat` / `bar` + `bars`);
//!   optional `sections`, lane and step `transition`, step `when`, `choices`, `pick`, `tie` (SPEC §6).
//! - Version 3 (M13): lane `mode` (`"clip"` | `"slot"`) and slot steps with `column` (SPEC §4.5).
//!   Lengths in sixteenths as in v2.
//! - Version 2: step lengths in sixteenths (6 ticks), no slot lanes.
//! - Version 1: step lengths in eighths (12 ticks), no slot lanes.
//!
//! Versions 1–3 are read with [`LegacyFile`]: `start` is the string `"immediate"`, there are no
//! sections, and their lanes (the v4 lane type) are checked not to use the v4 fields. They are
//! migrated in memory: `start` becomes `{"mode":"immediate"}`, v1 lengths double, `version` becomes
//! 4. Timing is unchanged. The file on disk keeps its version until the next save.

use std::collections::BTreeMap;

use serde::Deserialize;
use uuid::Uuid;

use crate::model::{AfterLastPass, Deck, Lane, LaneMode, Repeat, SeqFile, Sequence, Start};
use crate::validate::{validate, ValidationError};

/// The format version this build reads and writes.
pub const FORMAT_VERSION: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("not a valid sequence file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing \"version\" field")]
    MissingVersion,
    #[error("file version {found} was written by a newer sequencer (this build reads version {supported})")]
    TooNew { found: u64, supported: u32 },
    #[error("unsupported file version {found}")]
    Unsupported { found: u64 },
    #[error("version 1 step length {length} at {path} is too long to convert to sixteenths")]
    MigrationOverflow { path: String, length: u32 },
    #[error(
        "version {version} files have no slot lanes (lane mode and step column need version 3)"
    )]
    SlotLaneBeforeV3 { version: u64 },
    #[error("version {version} files have no \"{field}\" (at {path}); it needs version 4")]
    V4FieldBeforeV4 {
        version: u64,
        field: &'static str,
        path: String,
    },
    #[error("{} problem(s) in sequence file:\n{}", .0.len(), list(.0))]
    Invalid(Vec<ValidationError>),
}

fn list(errors: &[ValidationError]) -> String {
    errors
        .iter()
        .map(|e| format!("  - {e}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Deserialize)]
struct VersionProbe {
    version: Option<u64>,
}

/// The `version` field of a `.seq.json` document, if it parses as JSON and has one.
pub fn file_version(text: &str) -> Option<u64> {
    serde_json::from_str::<VersionProbe>(text).ok()?.version
}

/// Parses and validates a `.seq.json` document. Version 1–3 documents are migrated to the
/// current version (`start` string -> object; v1: lengths in eighths -> sixteenths).
pub fn from_json(text: &str) -> Result<SeqFile, LoadError> {
    let probe: VersionProbe = serde_json::from_str(text)?;
    let version = match probe.version {
        None => return Err(LoadError::MissingVersion),
        Some(v @ 1..=4) => v,
        Some(v) if v > u64::from(FORMAT_VERSION) => {
            return Err(LoadError::TooNew {
                found: v,
                supported: FORMAT_VERSION,
            })
        }
        Some(v) => return Err(LoadError::Unsupported { found: v }),
    };
    // Parse from the text (not from a Value) so serde errors keep line and column.
    let mut file: SeqFile = if version == 4 {
        serde_json::from_str(text)?
    } else {
        let legacy: LegacyFile = serde_json::from_str(text)?;
        legacy.check(version)?;
        legacy.into_current()
    };
    if version < 3 && uses_slot_lanes(&file) {
        return Err(LoadError::SlotLaneBeforeV3 { version });
    }
    if version == 1 {
        migrate_v1(&mut file)?;
    }
    file.version = FORMAT_VERSION;
    validate(&file).map_err(LoadError::Invalid)?;
    Ok(file)
}

/// A version 1–3 file: the v4 shape without `sections`, with `start` as the string `"immediate"`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyFile {
    #[allow(dead_code)] // already read by the version probe
    version: u32,
    composition: String,
    #[serde(default)]
    decks: BTreeMap<String, LegacyDeck>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDeck {
    #[serde(default)]
    sequences: Vec<LegacySequence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySequence {
    id: Uuid,
    name: String,
    start: LegacyStart,
    repeat: Repeat,
    after_last_pass: AfterLastPass,
    lanes: Vec<Lane>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyStart {
    Immediate,
}

impl LegacyFile {
    /// Lanes are parsed with the v4 lane type, which also knows the v4 lane and step fields;
    /// versions 1–3 must not use them.
    fn check(&self, version: u64) -> Result<(), LoadError> {
        let found = |field, path| {
            Err(LoadError::V4FieldBeforeV4 {
                version,
                field,
                path,
            })
        };
        for (deck, d) in &self.decks {
            for (si, seq) in d.sequences.iter().enumerate() {
                for (li, lane) in seq.lanes.iter().enumerate() {
                    let lane_path = format!("decks[{deck:?}].sequences[{si}].lanes[{li}]");
                    if lane.transition.is_some() {
                        return found("transition", lane_path);
                    }
                    for (pi, step) in lane.steps.iter().enumerate() {
                        let path = format!("{lane_path}.steps[{pi}]");
                        if step.when.is_some() {
                            return found("when", path);
                        }
                        if !step.choices.is_empty() {
                            return found("choices", path);
                        }
                        if step.transition.is_some() {
                            return found("transition", path);
                        }
                        if step.tie {
                            return found("tie", path);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn into_current(self) -> SeqFile {
        SeqFile {
            version: FORMAT_VERSION,
            composition: self.composition,
            decks: self
                .decks
                .into_iter()
                .map(|(name, d)| {
                    let sequences = d
                        .sequences
                        .into_iter()
                        .map(|s| {
                            let LegacyStart::Immediate = s.start;
                            Sequence {
                                id: s.id,
                                name: s.name,
                                start: Start::Immediate,
                                repeat: s.repeat,
                                after_last_pass: s.after_last_pass,
                                sections: Vec::new(),
                                lanes: s.lanes,
                            }
                        })
                        .collect();
                    (name, Deck { sequences })
                })
                .collect(),
        }
    }
}

/// Slot lanes (lane `mode`, step `column`) exist from version 3 on.
fn uses_slot_lanes(file: &SeqFile) -> bool {
    file.decks
        .values()
        .flat_map(|d| &d.sequences)
        .flat_map(|s| &s.lanes)
        .any(|l| l.mode != LaneMode::Clip)
}

/// v1 -> v2 and later: step lengths from eighths to sixteenths.
fn migrate_v1(file: &mut SeqFile) -> Result<(), LoadError> {
    for (deck, d) in &mut file.decks {
        for (si, seq) in d.sequences.iter_mut().enumerate() {
            for (li, lane) in seq.lanes.iter_mut().enumerate() {
                for (pi, step) in lane.steps.iter_mut().enumerate() {
                    let Some(doubled) = step.length.checked_mul(2) else {
                        return Err(LoadError::MigrationOverflow {
                            path: format!(
                                "decks[{deck:?}].sequences[{si}].lanes[{li}].steps[{pi}].length"
                            ),
                            length: step.length,
                        });
                    };
                    step.length = doubled;
                }
            }
        }
    }
    file.version = FORMAT_VERSION;
    Ok(())
}

/// Serializes a file as pretty JSON with a trailing newline. Deck names come out sorted.
pub fn to_json(file: &SeqFile) -> String {
    let mut s = serde_json::to_string_pretty(file).expect("SeqFile always serializes");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_checks() {
        assert!(matches!(
            from_json(r#"{"composition":"x","decks":{}}"#),
            Err(LoadError::MissingVersion)
        ));
        assert!(matches!(
            from_json(r#"{"version":5,"composition":"x","decks":{}}"#),
            Err(LoadError::TooNew { found: 5, .. })
        ));
        assert!(matches!(
            from_json(r#"{"version":0,"composition":"x","decks":{}}"#),
            Err(LoadError::Unsupported { found: 0 })
        ));
    }

    const V1: &str = r#"{"version":1,"composition":"x","decks":{"A":{"sequences":[{
        "id":"00000000-0000-0000-0000-000000000001","name":"S","start":"immediate",
        "repeat":{"mode":"once"},"after_last_pass":"clear",
        "lanes":[{"layer":"BG","end":"hold","steps":[{"clip":null,"length":8},{"clip":null,"length":3}]}]}]}}}"#;

    #[test]
    fn v1_lengths_are_migrated_to_sixteenths() {
        let f = from_json(V1).unwrap();
        assert_eq!(f.version, FORMAT_VERSION);
        let lane = &f.decks["A"].sequences[0].lanes[0];
        let lengths: Vec<u32> = lane.steps.iter().map(|s| s.length).collect();
        assert_eq!(lengths, [16, 6]);
        // Saved as v4 and read back: no second doubling.
        let text = to_json(&f);
        assert_eq!(file_version(&text), Some(4));
        assert_eq!(from_json(&text).unwrap(), f);
    }

    #[test]
    fn v1_migration_overflow_is_an_error() {
        let text = V1.replace(r#""length":3"#, &format!(r#""length":{}"#, u32::MAX));
        let err = from_json(&text).unwrap_err();
        assert!(matches!(err, LoadError::MigrationOverflow { .. }), "{err}");
        assert!(
            err.to_string().contains("lanes[0].steps[1].length"),
            "{err}"
        );
    }

    #[test]
    fn v2_loads_unchanged_as_v4() {
        let v2 = V1.replace(r#""version":1"#, r#""version":2"#);
        let f = from_json(&v2).unwrap();
        assert_eq!(f.version, 4);
        let lengths: Vec<u32> = f.decks["A"].sequences[0].lanes[0]
            .steps
            .iter()
            .map(|s| s.length)
            .collect();
        assert_eq!(lengths, [8, 3], "v2 already counts sixteenths");
        // A clip lane looks the same in v4 apart from the version and `start`.
        let text = to_json(&f);
        assert!(!text.contains("\"mode\": \"clip\""), "{text}");
        assert!(text.contains("\"mode\": \"immediate\""), "{text}");
        assert!(!text.contains("sections"), "{text}");
    }

    #[test]
    fn start_string_is_migrated_and_refused_in_v4() {
        for v in [1, 2, 3] {
            let text = V1.replace(r#""version":1"#, &format!(r#""version":{v}"#));
            let f = from_json(&text).unwrap();
            assert_eq!(f.decks["A"].sequences[0].start, Start::Immediate, "v{v}");
        }
        let v4 = V1.replace(r#""version":1"#, r#""version":4"#);
        assert!(matches!(from_json(&v4), Err(LoadError::Json(_))));
        let v4 = v4.replace(
            r#""start":"immediate""#,
            r#""start":{"mode":"bar","bars":4}"#,
        );
        assert_eq!(
            from_json(&v4).unwrap().decks["A"].sequences[0].start,
            Start::Bar { bars: 4 }
        );
        // The object form is not a v3 start.
        let v3 = v4.replace(r#""version":4"#, r#""version":3"#);
        assert!(matches!(from_json(&v3), Err(LoadError::Json(_))));
    }

    #[test]
    fn v4_fields_are_refused_before_v4() {
        let v3 = V1.replace(r#""version":1"#, r#""version":3"#);
        let cases = [
            (
                r#""end":"hold","#,
                r#""end":"hold","transition":{"length":4},"#,
                "transition",
            ),
            (
                r#"{"clip":null,"length":8}"#,
                r#"{"clip":null,"length":8,"when":{"is":"first"}}"#,
                "when",
            ),
            (
                r#"{"clip":null,"length":8}"#,
                r#"{"clip":null,"length":8,"choices":[null]}"#,
                "choices",
            ),
            (
                r#"{"clip":null,"length":8}"#,
                r#"{"clip":null,"length":8,"transition":{"length":0}}"#,
                "transition",
            ),
            (
                r#"{"clip":null,"length":8}"#,
                r#"{"clip":null,"length":8,"tie":true}"#,
                "tie",
            ),
        ];
        // v4 reads and writes a tie, and leaves it out when false.
        let v4 = V1
            .replace(r#""version":1"#, r#""version":4"#)
            .replace(r#""start":"immediate""#, r#""start":{"mode":"immediate"}"#)
            .replace(
                r#"{"clip":null,"length":8}"#,
                r#"{"clip":null,"length":8,"tie":true}"#,
            );
        let file = from_json(&v4).unwrap();
        let steps = &file.decks.values().next().unwrap().sequences[0].lanes[0].steps;
        assert!(steps.iter().any(|s| s.tie));
        let text = to_json(&file);
        assert_eq!(
            text.matches(r#""tie": true"#).count() + text.matches(r#""tie":true"#).count(),
            1
        );
        assert!(!text.contains(r#""tie": false"#) && !text.contains(r#""tie":false"#));
        for (from, to, field) in cases {
            let text = v3.replace(from, to);
            let err = from_json(&text).unwrap_err();
            assert!(
                matches!(&err, LoadError::V4FieldBeforeV4 { version: 3, field: f, .. } if *f == field),
                "{field}: {err}"
            );
        }
        let sections = v3.replace(r#""lanes":"#, r#""sections":[],"lanes":"#);
        assert!(matches!(from_json(&sections), Err(LoadError::Json(_))));
    }

    #[test]
    fn slot_lanes_need_version_3() {
        let slot = V1
            .replace(r#""layer":"BG","#, r#""layer":"BG","mode":"slot","#)
            .replace(
                r#"{"clip":null,"length":8},{"clip":null,"length":3}"#,
                r#"{"column":1,"length":8}"#,
            );
        for v in [1, 2] {
            let text = slot.replace(r#""version":1"#, &format!(r#""version":{v}"#));
            assert!(
                matches!(from_json(&text), Err(LoadError::SlotLaneBeforeV3 { version }) if version == v),
                "v{v}"
            );
        }
        let v3 = slot.replace(r#""version":1"#, r#""version":3"#);
        assert_eq!(from_json(&v3).unwrap().version, 4);
        let step = from_json(&v3).unwrap().decks["A"].sequences[0].lanes[0].steps[0].clone();
        assert_eq!((step.column, step.length), (Some(1), 8));
    }

    #[test]
    fn file_version_probe() {
        assert_eq!(file_version(V1), Some(1));
        assert_eq!(file_version("not json"), None);
    }

    #[test]
    fn empty_file_round_trips() {
        let f = SeqFile::new("Show");
        assert_eq!(from_json(&to_json(&f)).unwrap(), f);
    }

    #[test]
    fn syntax_errors_carry_line_numbers() {
        let err = from_json(
            "{\"version\":1,\n\"composition\": \"x\",\n\"decks\": {\"A\": {\"sequences\": [}}}",
        )
        .unwrap_err();
        assert!(err.to_string().contains("line 3"), "{err}");
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let err =
            from_json(r#"{"version":1,"composition":"x","decks":{},"extra":true}"#).unwrap_err();
        assert!(err.to_string().contains("unknown field `extra`"), "{err}");
    }
}
