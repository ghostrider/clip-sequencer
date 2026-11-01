//! Text form of the sequence file: parse with version check and validation, serialize.
//!
//! Version handling: the file is read once for its `version` field only, then parsed typed.
//!
//! - Version 2 (current): step lengths in sixteenths (6 ticks).
//! - Version 1: step lengths in eighths (12 ticks). Same shape as v2, so it is parsed with the
//!   same types and migrated in memory: every length doubles, `version` becomes 2. Timing is
//!   unchanged. The file on disk stays v1 until the next save.

use serde::Deserialize;

use crate::model::SeqFile;
use crate::validate::{validate, ValidationError};

/// The format version this build reads and writes.
pub const FORMAT_VERSION: u32 = 2;

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

/// Parses and validates a `.seq.json` document. Version 1 documents are migrated to the
/// current version (lengths in eighths -> sixteenths).
pub fn from_json(text: &str) -> Result<SeqFile, LoadError> {
    let probe: VersionProbe = serde_json::from_str(text)?;
    let version = match probe.version {
        None => return Err(LoadError::MissingVersion),
        Some(v) if v == 1 || v == u64::from(FORMAT_VERSION) => v,
        Some(v) if v > u64::from(FORMAT_VERSION) => {
            return Err(LoadError::TooNew {
                found: v,
                supported: FORMAT_VERSION,
            })
        }
        Some(v) => return Err(LoadError::Unsupported { found: v }),
    };
    // Parse from the text (not from a Value) so serde errors keep line and column.
    let mut file: SeqFile = serde_json::from_str(text)?;
    if version == 1 {
        migrate_v1(&mut file)?;
    }
    validate(&file).map_err(LoadError::Invalid)?;
    Ok(file)
}

/// v1 -> v2: step lengths from eighths to sixteenths.
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
            from_json(r#"{"version":3,"composition":"x","decks":{}}"#),
            Err(LoadError::TooNew { found: 3, .. })
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
        // Saved as v2 and read back: no second doubling.
        let text = to_json(&f);
        assert_eq!(file_version(&text), Some(2));
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
