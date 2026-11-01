//! Data model and serde mapping for the sequence file (SPEC §5, §6).
//!
//! The Rust types mirror the JSON one to one. Unknown fields are rejected so that a typo in a
//! hand-edited file is reported instead of silently dropped on the next autosave.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// MIDI clock resolution is 24 PPQN; one sixteenth note (the grid unit, file format v2) is 6 ticks.
pub const TICKS_PER_SIXTEENTH: u32 = 6;
/// 4/4 only: a bar is 16 sixteenths.
pub const SIXTEENTHS_PER_BAR: u32 = 16;

/// One file per composition. Decks are keyed by their Resolume deck name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeqFile {
    pub version: u32,
    pub composition: String,
    #[serde(default)]
    pub decks: BTreeMap<String, Deck>,
}

impl SeqFile {
    /// An empty file for a composition, at the current format version.
    pub fn new(composition: impl Into<String>) -> Self {
        Self {
            version: crate::FORMAT_VERSION,
            composition: composition.into(),
            decks: BTreeMap::new(),
        }
    }

    /// Finds a sequence by id in any deck. Returns the deck name with it.
    pub fn find_sequence(&self, id: Uuid) -> Option<(&str, &Sequence)> {
        self.decks.iter().find_map(|(deck, d)| {
            d.sequences
                .iter()
                .find(|s| s.id == id)
                .map(|s| (deck.as_str(), s))
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deck {
    #[serde(default)]
    pub sequences: Vec<Sequence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub id: Uuid,
    pub name: String,
    pub start: Start,
    pub repeat: Repeat,
    pub after_last_pass: AfterLastPass,
    pub lanes: Vec<Lane>,
}

impl Sequence {
    /// Length of one pass in sixteenths: the length of the longest lane.
    pub fn pass_length(&self) -> u64 {
        self.lanes.iter().map(Lane::length).max().unwrap_or(0)
    }
}

/// When a sequence starts after it is triggered. Only immediate start exists for now; the field
/// is in the format so quantized starts can be added later (SPEC §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    Immediate,
}

/// JSON: `{"mode":"once"}`, `{"mode":"count","count":4}`, `{"mode":"infinite"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RepeatRepr", into = "RepeatRepr")]
pub enum Repeat {
    Once,
    /// Exactly `count` passes; `count` ≥ 2 (checked by validation).
    Count {
        count: u32,
    },
    Infinite,
}

// serde's internally tagged enums ignore `deny_unknown_fields` for unit variants, which would
// accept `{"mode":"once","count":3}`. This flat form enforces "count only with mode count".
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepeatRepr {
    mode: RepeatMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    count: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RepeatMode {
    Once,
    Count,
    Infinite,
}

impl TryFrom<RepeatRepr> for Repeat {
    type Error = String;

    fn try_from(r: RepeatRepr) -> Result<Self, Self::Error> {
        match (r.mode, r.count) {
            (RepeatMode::Once, None) => Ok(Repeat::Once),
            (RepeatMode::Infinite, None) => Ok(Repeat::Infinite),
            (RepeatMode::Count, Some(count)) => Ok(Repeat::Count { count }),
            (RepeatMode::Count, None) => {
                Err("repeat mode \"count\" needs a \"count\" field".into())
            }
            (_, Some(_)) => Err("\"count\" is only allowed with repeat mode \"count\"".into()),
        }
    }
}

impl From<Repeat> for RepeatRepr {
    fn from(r: Repeat) -> Self {
        match r {
            Repeat::Once => RepeatRepr {
                mode: RepeatMode::Once,
                count: None,
            },
            Repeat::Count { count } => RepeatRepr {
                mode: RepeatMode::Count,
                count: Some(count),
            },
            Repeat::Infinite => RepeatRepr {
                mode: RepeatMode::Infinite,
                count: None,
            },
        }
    }
}

/// What happens to the sequence's layers after its last pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AfterLastPass {
    Clear,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    /// Resolume layer name; never an index (SPEC §4.4).
    pub layer: String,
    pub end: LaneEnd,
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl Lane {
    /// Sum of step lengths in sixteenths.
    pub fn length(&self) -> u64 {
        self.steps.iter().map(|s| u64::from(s.length)).sum()
    }
}

/// Behaviour of a lane that is shorter than the pass (SPEC §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneEnd {
    LoopToEnd,
    Hold,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// `None` (JSON `null`) is a rest.
    pub clip: Option<ClipRef>,
    /// In sixteenths, ≥ 1 (checked by validation). Version 1 files counted eighths.
    pub length: u32,
}

/// Up to three features identifying a clip, tried in order: id, name, file (SPEC §4.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

impl ClipRef {
    pub fn is_empty(&self) -> bool {
        self.id.is_none()
            && self.name.as_deref().is_none_or(str::is_empty)
            && self.file.as_deref().is_none_or(str::is_empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(lengths: &[u32]) -> Lane {
        Lane {
            layer: "BG".into(),
            end: LaneEnd::Hold,
            steps: lengths
                .iter()
                .map(|&length| Step { clip: None, length })
                .collect(),
        }
    }

    #[test]
    fn pass_length_is_longest_lane() {
        let seq = Sequence {
            id: Uuid::nil(),
            name: "A".into(),
            start: Start::Immediate,
            repeat: Repeat::Once,
            after_last_pass: AfterLastPass::Clear,
            lanes: vec![lane(&[16, 8]), lane(&[8]), lane(&[])],
        };
        assert_eq!(seq.pass_length(), 24);
    }

    #[test]
    fn lane_length_does_not_overflow() {
        assert_eq!(
            lane(&[u32::MAX, u32::MAX]).length(),
            2 * u64::from(u32::MAX)
        );
    }

    #[test]
    fn clip_ref_emptiness() {
        assert!(ClipRef::default().is_empty());
        assert!(ClipRef {
            name: Some(String::new()),
            ..Default::default()
        }
        .is_empty());
        assert!(!ClipRef {
            id: Some(1),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn repeat_serde_shapes() {
        let j = |r: Repeat| serde_json::to_string(&r).unwrap();
        assert_eq!(j(Repeat::Once), r#"{"mode":"once"}"#);
        assert_eq!(
            j(Repeat::Count { count: 4 }),
            r#"{"mode":"count","count":4}"#
        );
        assert_eq!(j(Repeat::Infinite), r#"{"mode":"infinite"}"#);
        assert!(serde_json::from_str::<Repeat>(r#"{"mode":"once","count":3}"#).is_err());
        assert!(serde_json::from_str::<Repeat>(r#"{"mode":"count"}"#).is_err());
    }

    #[test]
    fn clip_ref_omits_missing_fields() {
        let c = ClipRef {
            name: Some("Tunnel".into()),
            ..Default::default()
        };
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"{"name":"Tunnel"}"#);
    }
}
