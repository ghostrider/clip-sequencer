//! Text form of the sequence file: parse with version check and validation, serialize.
//!
//! Version handling: the file is read once for its `version` field only. Older versions share
//! the JSON shape of the current one (minus fields they did not have), so they parse with the
//! same types and are migrated on the model; the result is saved at the current version. A file
//! newer than this build is refused (and never overwritten by the app).
//! - Version 3: step `length` counts sixteenths (16 = one bar). v1 and v2 counted eighths, so
//!   their lengths are doubled.
//! - Version 2 (M13) adds lane `mode` and slot steps with `column` (SPEC §4.5, §6); a v1 file
//!   must not use them.

use serde::Deserialize;

use crate::model::SeqFile;
use crate::validate::{validate, ValidationError};

/// The format version this build reads and writes.
pub const FORMAT_VERSION: u32 = 3;
/// The oldest version this build migrates.
pub const OLDEST_VERSION: u32 = 1;

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
    #[error("version 1 files have no slot lanes (lane mode and step column need version 2)")]
    SlotLaneInV1,
    #[error("a step length is too large to convert to sixteenths")]
    LengthOverflow,
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

/// Parses and validates a `.seq.json` document.
pub fn from_json(text: &str) -> Result<SeqFile, LoadError> {
    let probe: VersionProbe = serde_json::from_str(text)?;
    let version = match probe.version {
        None => return Err(LoadError::MissingVersion),
        Some(v) if v > u64::from(FORMAT_VERSION) => {
            return Err(LoadError::TooNew {
                found: v,
                supported: FORMAT_VERSION,
            })
        }
        Some(v) if v < u64::from(OLDEST_VERSION) => {
            return Err(LoadError::Unsupported { found: v })
        }
        Some(v) => v,
    };
    // Parse from the text (not from a Value) so serde errors keep line and column.
    let mut file: SeqFile = serde_json::from_str(text)?;
    if version == 1 {
        check_v1(&file)?;
    }
    if version <= 2 {
        eighths_to_sixteenths(&mut file)?;
    }
    file.version = FORMAT_VERSION;
    validate(&file).map_err(LoadError::Invalid)?;
    Ok(file)
}

/// v1 has no lane modes, so every lane must have come in as a clip lane.
fn check_v1(file: &SeqFile) -> Result<(), LoadError> {
    let slot = file
        .decks
        .values()
        .flat_map(|d| &d.sequences)
        .flat_map(|s| &s.lanes)
        .any(|l| l.mode != crate::model::LaneMode::Clip);
    if slot {
        return Err(LoadError::SlotLaneInV1);
    }
    Ok(())
}

/// v1/v2 → v3: lengths were eighths, now sixteenths.
fn eighths_to_sixteenths(file: &mut SeqFile) -> Result<(), LoadError> {
    for step in file
        .decks
        .values_mut()
        .flat_map(|d| &mut d.sequences)
        .flat_map(|s| &mut s.lanes)
        .flat_map(|l| &mut l.steps)
    {
        step.length = step
            .length
            .checked_mul(2)
            .ok_or(LoadError::LengthOverflow)?;
    }
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
            from_json(r#"{"version":4,"composition":"x","decks":{}}"#),
            Err(LoadError::TooNew { found: 4, .. })
        ));
        assert!(matches!(
            from_json(r#"{"version":0,"composition":"x","decks":{}}"#),
            Err(LoadError::Unsupported { found: 0 })
        ));
    }

    const V1: &str = r#"{"version":1,"composition":"x","decks":{"A":{"sequences":[{
        "id":"6f1c2a9e-0d5b-4c1e-9a0f-2f7d4c1b8e11","name":"S","start":"immediate",
        "repeat":{"mode":"once"},"after_last_pass":"clear",
        "lanes":[{"layer":"BG","end":"hold","steps":[{"clip":{"name":"T"},"length":8},{"clip":null,"length":8}]}]}]}}}"#;

    #[test]
    fn v1_migrates_to_v3() {
        let f = from_json(V1).unwrap();
        assert_eq!(f.version, 3);
        let lane = &f.decks["A"].sequences[0].lanes[0];
        assert_eq!(lane.mode, crate::model::LaneMode::Clip);
        // One bar was 8 eighths, now 16 sixteenths.
        let lengths: Vec<_> = lane.steps.iter().map(|s| s.length).collect();
        assert_eq!(lengths, [16, 16]);
        // Saved as v3; a clip lane looks the same as in v1 apart from version and lengths.
        let out = to_json(&f);
        assert!(out.contains("\"version\": 3"), "{out}");
        assert!(!out.contains("mode\": \"clip"), "{out}");
        assert_eq!(from_json(&out).unwrap(), f);
    }

    #[test]
    fn v2_lengths_are_doubled_once() {
        let v2 = V1.replace(r#""version":1"#, r#""version":2"#);
        let f = from_json(&v2).unwrap();
        assert_eq!(f.decks["A"].sequences[0].pass_length(), 32);
        // Loading what was saved does not double again.
        assert_eq!(from_json(&to_json(&f)).unwrap(), f);
        let v3 = V1.replace(r#""version":1"#, r#""version":3"#);
        assert_eq!(
            from_json(&v3).unwrap().decks["A"].sequences[0].pass_length(),
            16
        );
        let huge = v2.replace(r#""length":8}]"#, r#""length":4294967295}]"#);
        assert!(matches!(from_json(&huge), Err(LoadError::LengthOverflow)));
    }

    #[test]
    fn v1_with_slot_lane_is_refused() {
        let bad = V1
            .replace(r#""layer":"BG","#, r#""layer":"BG","mode":"slot","#)
            .replace(
                r#"{"clip":{"name":"T"},"length":8},{"clip":null,"length":8}"#,
                r#"{"column":1,"length":8}"#,
            );
        assert!(matches!(from_json(&bad), Err(LoadError::SlotLaneInV1)));
        let ok = bad.replace(r#""version":1"#, r#""version":2"#);
        let step = from_json(&ok).unwrap().decks["A"].sequences[0].lanes[0].steps[0].clone();
        assert_eq!((step.column, step.length), (Some(1), 16));
    }

    #[test]
    fn empty_file_round_trips() {
        let f = SeqFile::new("Show");
        assert_eq!(from_json(&to_json(&f)).unwrap(), f);
    }

    #[test]
    fn syntax_errors_carry_line_numbers() {
        let err = from_json(
            "{\"version\":3,\n\"composition\": \"x\",\n\"decks\": {\"A\": {\"sequences\": [}}}",
        )
        .unwrap_err();
        assert!(err.to_string().contains("line 3"), "{err}");
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let err =
            from_json(r#"{"version":3,"composition":"x","decks":{},"extra":true}"#).unwrap_err();
        assert!(err.to_string().contains("unknown field `extra`"), "{err}");
    }
}
