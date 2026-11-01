//! Rules a sequence file must satisfy beyond what serde already enforces.
//!
//! [`validate`] collects every violation instead of stopping at the first, and each error
//! carries a path such as `decks["Act 1"].sequences[0] "Drop A".lanes[1].steps[3].length`
//! so the user can find it in the file or the UI.

use std::collections::{HashMap, HashSet};
use std::fmt;

use uuid::Uuid;

use crate::model::{Repeat, SeqFile, Sequence};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {kind}")]
pub struct ValidationError {
    pub path: String,
    pub kind: ValidationErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationErrorKind {
    EmptyComposition,
    EmptyDeckName,
    EmptySequenceName,
    /// Another sequence in the same deck already has this name.
    DuplicateName {
        name: String,
        first_index: usize,
    },
    /// Sequence ids must be unique in the whole file.
    DuplicateId {
        id: Uuid,
        first_path: String,
    },
    CountTooSmall {
        count: u32,
    },
    NoLanes,
    EmptyLayer,
    ZeroLength,
    /// A clip reference with no id, name or file can never resolve; use `null` for a rest.
    EmptyClipRef,
    /// Two lanes of one sequence target the same layer.
    DuplicateLayer {
        layer: String,
        first_lane: usize,
    },
}

impl fmt::Display for ValidationErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyComposition => write!(f, "composition name is empty"),
            Self::EmptyDeckName => write!(f, "deck name is empty"),
            Self::EmptySequenceName => write!(f, "sequence name is empty"),
            Self::DuplicateName { name, first_index } => write!(
                f,
                "sequence name {name:?} is already used by sequences[{first_index}] in this deck"
            ),
            Self::DuplicateId { id, first_path } => {
                write!(f, "sequence id {id} is already used by {first_path}")
            }
            Self::CountTooSmall { count } => write!(
                f,
                "repeat count must be at least 2 (got {count}); use mode \"once\" for a single pass"
            ),
            Self::NoLanes => write!(f, "sequence has no lanes"),
            Self::EmptyLayer => write!(f, "layer name is empty"),
            Self::ZeroLength => write!(f, "step length must be at least 1 sixteenth"),
            Self::EmptyClipRef => write!(f, "clip has no id, name or file; use null for a rest"),
            Self::DuplicateLayer { layer, first_lane } => write!(
                f,
                "layer {layer:?} is already used by lanes[{first_lane}] of this sequence"
            ),
        }
    }
}

/// Checks the whole file. Returns all violations, in file order.
pub fn validate(file: &SeqFile) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut err = |path: String, kind| errors.push(ValidationError { path, kind });

    if file.composition.trim().is_empty() {
        err("composition".into(), ValidationErrorKind::EmptyComposition);
    }

    let mut ids: HashMap<Uuid, String> = HashMap::new();
    for (deck_name, deck) in &file.decks {
        let deck_path = format!("decks[{deck_name:?}]");
        if deck_name.trim().is_empty() {
            err(deck_path.clone(), ValidationErrorKind::EmptyDeckName);
        }
        let mut names: HashMap<&str, usize> = HashMap::new();
        for (si, seq) in deck.sequences.iter().enumerate() {
            let seq_path = format!("{deck_path}.sequences[{si}] {:?}", seq.name);
            if seq.name.trim().is_empty() {
                err(seq_path.clone(), ValidationErrorKind::EmptySequenceName);
            } else if let Some(&first_index) = names.get(seq.name.as_str()) {
                err(
                    format!("{seq_path}.name"),
                    ValidationErrorKind::DuplicateName {
                        name: seq.name.clone(),
                        first_index,
                    },
                );
            } else {
                names.insert(&seq.name, si);
            }
            if let Some(first_path) = ids.get(&seq.id) {
                err(
                    format!("{seq_path}.id"),
                    ValidationErrorKind::DuplicateId {
                        id: seq.id,
                        first_path: first_path.clone(),
                    },
                );
            } else {
                ids.insert(seq.id, seq_path.clone());
            }
            validate_sequence(seq, &seq_path, &mut err);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_sequence(
    seq: &Sequence,
    seq_path: &str,
    err: &mut impl FnMut(String, ValidationErrorKind),
) {
    if let Repeat::Count { count } = seq.repeat {
        if count < 2 {
            err(
                format!("{seq_path}.repeat.count"),
                ValidationErrorKind::CountTooSmall { count },
            );
        }
    }
    if seq.lanes.is_empty() {
        err(format!("{seq_path}.lanes"), ValidationErrorKind::NoLanes);
    }
    let mut layers: HashMap<&str, usize> = HashMap::new();
    for (li, lane) in seq.lanes.iter().enumerate() {
        let lane_path = format!("{seq_path}.lanes[{li}]");
        if lane.layer.trim().is_empty() {
            err(
                format!("{lane_path}.layer"),
                ValidationErrorKind::EmptyLayer,
            );
        } else if let Some(&first_lane) = layers.get(lane.layer.as_str()) {
            err(
                format!("{lane_path}.layer"),
                ValidationErrorKind::DuplicateLayer {
                    layer: lane.layer.clone(),
                    first_lane,
                },
            );
        } else {
            layers.insert(&lane.layer, li);
        }
        for (ti, step) in lane.steps.iter().enumerate() {
            let step_path = format!("{lane_path}.steps[{ti}]");
            if step.length == 0 {
                err(
                    format!("{step_path}.length"),
                    ValidationErrorKind::ZeroLength,
                );
            }
            if step.clip.as_ref().is_some_and(|c| c.is_empty()) {
                err(
                    format!("{step_path}.clip"),
                    ValidationErrorKind::EmptyClipRef,
                );
            }
        }
    }
}

/// Names of sequences in one deck that appear more than once. Handy for UI hints while editing.
pub fn duplicate_names<'a>(sequences: impl IntoIterator<Item = &'a Sequence>) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    let mut dups = Vec::new();
    for s in sequences {
        if !seen.insert(s.name.as_str()) && !dups.contains(&s.name.as_str()) {
            dups.push(s.name.as_str());
        }
    }
    dups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn seq(name: &str, id: u128) -> Sequence {
        Sequence {
            id: Uuid::from_u128(id),
            name: name.into(),
            start: Start::Immediate,
            repeat: Repeat::Once,
            after_last_pass: AfterLastPass::Clear,
            lanes: vec![Lane {
                layer: "BG".into(),
                end: LaneEnd::LoopToEnd,
                steps: vec![Step {
                    clip: Some(ClipRef {
                        name: Some("Tunnel".into()),
                        ..Default::default()
                    }),
                    length: 8,
                }],
            }],
        }
    }

    fn file_with(deck: &str, sequences: Vec<Sequence>) -> SeqFile {
        let mut f = SeqFile::new("Show");
        f.decks.insert(deck.into(), Deck { sequences });
        f
    }

    fn kinds(f: &SeqFile) -> Vec<ValidationErrorKind> {
        validate(f)
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }

    #[test]
    fn valid_file_passes() {
        assert_eq!(
            validate(&file_with("Act 1", vec![seq("A", 1), seq("B", 2)])),
            Ok(())
        );
    }

    #[test]
    fn zero_length_step_is_reported_with_path() {
        let mut s = seq("Drop A", 1);
        s.lanes[0].steps[0].length = 0;
        let errs = validate(&file_with("Act 1", vec![s])).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(
            errs[0].to_string(),
            r#"decks["Act 1"].sequences[0] "Drop A".lanes[0].steps[0].length: step length must be at least 1 sixteenth"#
        );
    }

    #[test]
    fn repeat_count_below_two_is_rejected() {
        for count in [0, 1] {
            let mut s = seq("A", 1);
            s.repeat = Repeat::Count { count };
            assert_eq!(
                kinds(&file_with("D", vec![s])),
                vec![ValidationErrorKind::CountTooSmall { count }]
            );
        }
        let mut s = seq("A", 1);
        s.repeat = Repeat::Count { count: 2 };
        assert_eq!(validate(&file_with("D", vec![s])), Ok(()));
    }

    #[test]
    fn duplicate_names_in_one_deck_are_rejected() {
        let errs =
            validate(&file_with("D", vec![seq("A", 1), seq("B", 2), seq("A", 3)])).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].path, r#"decks["D"].sequences[2] "A".name"#);
        assert_eq!(
            errs[0].kind,
            ValidationErrorKind::DuplicateName {
                name: "A".into(),
                first_index: 0
            }
        );
    }

    #[test]
    fn same_name_in_different_decks_is_fine() {
        let mut f = file_with("D1", vec![seq("A", 1)]);
        f.decks.insert(
            "D2".into(),
            Deck {
                sequences: vec![seq("A", 2)],
            },
        );
        assert_eq!(validate(&f), Ok(()));
    }

    #[test]
    fn duplicate_ids_across_decks_are_rejected() {
        let mut f = file_with("D1", vec![seq("A", 7)]);
        f.decks.insert(
            "D2".into(),
            Deck {
                sequences: vec![seq("B", 7)],
            },
        );
        let errs = validate(&f).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            errs[0].kind,
            ValidationErrorKind::DuplicateId { .. }
        ));
        assert_eq!(errs[0].path, r#"decks["D2"].sequences[0] "B".id"#);
    }

    #[test]
    fn empty_names_and_layers_are_rejected() {
        let mut s = seq("  ", 1);
        s.lanes[0].layer = String::new();
        let mut f = file_with("", vec![s]);
        f.composition = String::new();
        assert_eq!(
            kinds(&f),
            vec![
                ValidationErrorKind::EmptyComposition,
                ValidationErrorKind::EmptyDeckName,
                ValidationErrorKind::EmptySequenceName,
                ValidationErrorKind::EmptyLayer,
            ]
        );
    }

    #[test]
    fn sequence_without_lanes_is_rejected_but_empty_lane_is_fine() {
        let mut s = seq("A", 1);
        s.lanes[0].steps.clear();
        assert_eq!(validate(&file_with("D", vec![s.clone()])), Ok(()));
        s.lanes.clear();
        assert_eq!(
            kinds(&file_with("D", vec![s])),
            vec![ValidationErrorKind::NoLanes]
        );
    }

    #[test]
    fn empty_clip_ref_is_rejected_but_rest_is_fine() {
        let mut s = seq("A", 1);
        s.lanes[0].steps[0].clip = Some(ClipRef::default());
        assert_eq!(
            kinds(&file_with("D", vec![s.clone()])),
            vec![ValidationErrorKind::EmptyClipRef]
        );
        s.lanes[0].steps[0].clip = None;
        assert_eq!(validate(&file_with("D", vec![s])), Ok(()));
    }

    #[test]
    fn all_errors_are_collected() {
        let mut s = seq("A", 1);
        s.repeat = Repeat::Count { count: 1 };
        s.lanes[0].steps[0].length = 0;
        assert_eq!(kinds(&file_with("D", vec![s, seq("A", 2)])).len(), 3);
    }

    #[test]
    fn two_lanes_on_one_layer_are_rejected() {
        let mut s = seq("A", 1);
        s.lanes.push(s.lanes[0].clone());
        assert_eq!(
            kinds(&file_with("D", vec![s])),
            vec![ValidationErrorKind::DuplicateLayer {
                layer: "BG".into(),
                first_lane: 0
            }]
        );
    }

    #[test]
    fn duplicate_names_helper() {
        let list = [seq("A", 1), seq("B", 2), seq("A", 3), seq("A", 4)];
        assert_eq!(duplicate_names(&list), vec!["A"]);
    }
}
