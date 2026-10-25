//! Text form of the sequence file: parse with version check and validation, serialize.
//!
//! Version handling: the file is read once for its `version` field only. Version 1 is the only
//! format so far; a future version 2 adds a migration step here that turns v1 JSON into v2
//! before the typed parse (CLAUDE.md: bump `version` and write a migration).

use serde::Deserialize;

use crate::model::SeqFile;
use crate::validate::{validate, ValidationError};

/// The format version this build reads and writes.
pub const FORMAT_VERSION: u32 = 1;

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
    match probe.version {
        None => return Err(LoadError::MissingVersion),
        Some(v) if v == u64::from(FORMAT_VERSION) => {}
        Some(v) if v > u64::from(FORMAT_VERSION) => {
            return Err(LoadError::TooNew {
                found: v,
                supported: FORMAT_VERSION,
            })
        }
        Some(v) => return Err(LoadError::Unsupported { found: v }),
    }
    // Parse from the text (not from a Value) so serde errors keep line and column.
    let file: SeqFile = serde_json::from_str(text)?;
    validate(&file).map_err(LoadError::Invalid)?;
    Ok(file)
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
            from_json(r#"{"version":2,"composition":"x","decks":{}}"#),
            Err(LoadError::TooNew { found: 2, .. })
        ));
        assert!(matches!(
            from_json(r#"{"version":0,"composition":"x","decks":{}}"#),
            Err(LoadError::Unsupported { found: 0 })
        ));
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
