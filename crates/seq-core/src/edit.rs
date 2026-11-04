//! Edit operations on the sequence file (SPEC §9.6: "Edits are sent as operations, the engine
//! applies them to the stored model"). [`apply`] returns a new, validated file or a reason; the
//! input is never modified, which makes undo a matter of keeping the old value.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::index::CompositionIndex;
use crate::model::{
    AfterLastPass, ClipRef, Deck, Lane, LaneEnd, LaneMode, Repeat, SeqFile, Sequence, Start, Step,
};
use crate::resolve::convert_steps;
use crate::validate::{validate, ValidationError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    /// New sequence with one empty lane on `layer`.
    CreateSequence {
        deck: String,
        id: Uuid,
        name: String,
        layer: String,
    },
    RenameSequence {
        seq: Uuid,
        name: String,
    },
    DuplicateSequence {
        seq: Uuid,
        id: Uuid,
        name: String,
    },
    DeleteSequence {
        seq: Uuid,
    },
    /// Move to another deck (used for orphaned sequences).
    MoveSequence {
        seq: Uuid,
        deck: String,
    },
    SetRepeat {
        seq: Uuid,
        repeat: Repeat,
    },
    SetAfterLastPass {
        seq: Uuid,
        value: AfterLastPass,
    },
    AddLane {
        seq: Uuid,
        layer: String,
    },
    RemoveLane {
        seq: Uuid,
        lane: usize,
    },
    /// Moves a lane so that it ends up at position `to`.
    MoveLane {
        seq: Uuid,
        from: usize,
        to: usize,
    },
    SetLaneLayer {
        seq: Uuid,
        lane: usize,
        layer: String,
    },
    SetLaneEnd {
        seq: Uuid,
        lane: usize,
        end: LaneEnd,
    },
    /// Switches a lane between clip and slot mode, converting its steps (SPEC §9.3).
    SetLaneMode {
        seq: Uuid,
        lane: usize,
        mode: LaneMode,
    },
    /// `index` may equal the step count (append).
    InsertStep {
        seq: Uuid,
        lane: usize,
        index: usize,
        step: Step,
    },
    RemoveStep {
        seq: Uuid,
        lane: usize,
        index: usize,
    },
    SetStepLength {
        seq: Uuid,
        lane: usize,
        index: usize,
        length: u32,
    },
    /// Moves a step so that it ends up at position `to`.
    MoveStep {
        seq: Uuid,
        lane: usize,
        from: usize,
        to: usize,
    },
    /// Relink (or turn into a rest with `None`).
    SetStepClip {
        seq: Uuid,
        lane: usize,
        index: usize,
        clip: Option<ClipRef>,
    },
    /// Slot lanes: set the column (or a rest with `None`).
    SetStepColumn {
        seq: Uuid,
        lane: usize,
        index: usize,
        column: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("unknown sequence")]
    UnknownSequence,
    #[error("lane {0} does not exist")]
    NoSuchLane(usize),
    #[error("step {0} does not exist")]
    NoSuchStep(usize),
    #[error("{}", .0.first().map(ToString::to_string).unwrap_or_default())]
    Invalid(Vec<ValidationError>),
}

/// Applies `op` to a copy of `file` and validates the result. Without an index,
/// [`EditOp::SetLaneMode`] turns every step into a rest; use [`apply_with`].
pub fn apply(file: &SeqFile, op: &EditOp) -> Result<SeqFile, EditError> {
    apply_with(file, op, None)
}

/// [`apply`] with the clip index of the edited sequence's deck, which lane mode changes use to
/// convert steps (a clip's column, or the clip in a slot).
pub fn apply_with(
    file: &SeqFile,
    op: &EditOp,
    index: Option<&CompositionIndex>,
) -> Result<SeqFile, EditError> {
    let mut f = file.clone();
    match op {
        EditOp::CreateSequence {
            deck,
            id,
            name,
            layer,
        } => {
            f.decks
                .entry(deck.clone())
                .or_default()
                .sequences
                .push(Sequence {
                    id: *id,
                    name: name.clone(),
                    start: Start::Immediate,
                    repeat: Repeat::Once,
                    after_last_pass: AfterLastPass::Clear,
                    lanes: vec![Lane {
                        layer: layer.clone(),
                        mode: LaneMode::Clip,
                        end: LaneEnd::LoopToEnd,
                        steps: Vec::new(),
                    }],
                });
        }
        EditOp::RenameSequence { seq, name } => seq_mut(&mut f, *seq)?.name = name.clone(),
        EditOp::DuplicateSequence { seq, id, name } => {
            let (deck, s) = f.find_sequence(*seq).ok_or(EditError::UnknownSequence)?;
            let mut copy = s.clone();
            copy.id = *id;
            copy.name = name.clone();
            let deck = deck.to_owned();
            let list = &mut f.decks.get_mut(&deck).expect("found").sequences;
            let pos = list.iter().position(|s| s.id == *seq).expect("found");
            list.insert(pos + 1, copy);
        }
        EditOp::DeleteSequence { seq } => {
            take_sequence(&mut f, *seq)?;
        }
        EditOp::MoveSequence { seq, deck } => {
            let s = take_sequence(&mut f, *seq)?;
            f.decks.entry(deck.clone()).or_default().sequences.push(s);
        }
        EditOp::SetRepeat { seq, repeat } => seq_mut(&mut f, *seq)?.repeat = *repeat,
        EditOp::SetAfterLastPass { seq, value } => seq_mut(&mut f, *seq)?.after_last_pass = *value,
        EditOp::AddLane { seq, layer } => seq_mut(&mut f, *seq)?.lanes.push(Lane {
            layer: layer.clone(),
            mode: LaneMode::Clip,
            end: LaneEnd::LoopToEnd,
            steps: Vec::new(),
        }),
        EditOp::RemoveLane { seq, lane } => {
            let s = seq_mut(&mut f, *seq)?;
            if *lane >= s.lanes.len() {
                return Err(EditError::NoSuchLane(*lane));
            }
            s.lanes.remove(*lane);
        }
        EditOp::MoveLane { seq, from, to } => {
            let s = seq_mut(&mut f, *seq)?;
            for i in [*from, *to] {
                if i >= s.lanes.len() {
                    return Err(EditError::NoSuchLane(i));
                }
            }
            let l = s.lanes.remove(*from);
            s.lanes.insert(*to, l);
        }
        EditOp::SetLaneLayer { seq, lane, layer } => {
            lane_mut(&mut f, *seq, *lane)?.layer = layer.clone()
        }
        EditOp::SetLaneEnd { seq, lane, end } => lane_mut(&mut f, *seq, *lane)?.end = *end,
        EditOp::SetLaneMode { seq, lane, mode } => {
            let l = lane_mut(&mut f, *seq, *lane)?;
            if l.mode != *mode {
                l.steps = convert_steps(l, *mode, index);
                l.mode = *mode;
            }
        }
        EditOp::InsertStep {
            seq,
            lane,
            index,
            step,
        } => {
            let l = lane_mut(&mut f, *seq, *lane)?;
            if *index > l.steps.len() {
                return Err(EditError::NoSuchStep(*index));
            }
            l.steps.insert(*index, step.clone());
        }
        EditOp::RemoveStep { seq, lane, index } => {
            let l = lane_mut(&mut f, *seq, *lane)?;
            if *index >= l.steps.len() {
                return Err(EditError::NoSuchStep(*index));
            }
            l.steps.remove(*index);
        }
        EditOp::SetStepLength {
            seq,
            lane,
            index,
            length,
        } => step_mut(&mut f, *seq, *lane, *index)?.length = *length,
        EditOp::MoveStep {
            seq,
            lane,
            from,
            to,
        } => {
            let l = lane_mut(&mut f, *seq, *lane)?;
            if *from >= l.steps.len() {
                return Err(EditError::NoSuchStep(*from));
            }
            if *to >= l.steps.len() {
                return Err(EditError::NoSuchStep(*to));
            }
            let s = l.steps.remove(*from);
            l.steps.insert(*to, s);
        }
        EditOp::SetStepClip {
            seq,
            lane,
            index,
            clip,
        } => step_mut(&mut f, *seq, *lane, *index)?.clip = clip.clone(),
        EditOp::SetStepColumn {
            seq,
            lane,
            index,
            column,
        } => step_mut(&mut f, *seq, *lane, *index)?.column = *column,
    }
    validate(&f).map_err(EditError::Invalid)?;
    Ok(f)
}

fn seq_mut(f: &mut SeqFile, id: Uuid) -> Result<&mut Sequence, EditError> {
    f.decks
        .values_mut()
        .flat_map(|d| d.sequences.iter_mut())
        .find(|s| s.id == id)
        .ok_or(EditError::UnknownSequence)
}

fn lane_mut(f: &mut SeqFile, id: Uuid, lane: usize) -> Result<&mut Lane, EditError> {
    seq_mut(f, id)?
        .lanes
        .get_mut(lane)
        .ok_or(EditError::NoSuchLane(lane))
}

fn step_mut(f: &mut SeqFile, id: Uuid, lane: usize, index: usize) -> Result<&mut Step, EditError> {
    lane_mut(f, id, lane)?
        .steps
        .get_mut(index)
        .ok_or(EditError::NoSuchStep(index))
}

/// Removes a sequence; drops its deck entry when it becomes empty.
fn take_sequence(f: &mut SeqFile, id: Uuid) -> Result<Sequence, EditError> {
    let deck = f
        .find_sequence(id)
        .map(|(d, _)| d.to_owned())
        .ok_or(EditError::UnknownSequence)?;
    let d: &mut Deck = f.decks.get_mut(&deck).expect("found");
    let pos = d.sequences.iter().position(|s| s.id == id).expect("found");
    let s = d.sequences.remove(pos);
    if d.sequences.is_empty() {
        f.decks.remove(&deck);
    }
    Ok(s)
}

/// A name not used by any sequence of `deck`: `base`, `base 2`, `base 3`, …
pub fn unique_name(file: &SeqFile, deck: &str, base: &str) -> String {
    let taken = |n: &str| {
        file.decks
            .get(deck)
            .is_some_and(|d| d.sequences.iter().any(|s| s.name == n))
    };
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .expect("infinite")
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Uuid = Uuid::from_u128(1);
    const B: Uuid = Uuid::from_u128(2);

    fn clip(name: &str) -> Option<ClipRef> {
        Some(ClipRef {
            name: Some(name.into()),
            ..Default::default()
        })
    }

    fn base() -> SeqFile {
        let f = SeqFile::new("Show");
        let f = apply(
            &f,
            &EditOp::CreateSequence {
                deck: "Act 1".into(),
                id: A,
                name: "Drop".into(),
                layer: "BG".into(),
            },
        )
        .unwrap();
        let mut f = f;
        for (i, n) in ["a", "b", "c"].iter().enumerate() {
            f = apply(
                &f,
                &EditOp::InsertStep {
                    seq: A,
                    lane: 0,
                    index: i,
                    step: Step::clip(clip(n), 8),
                },
            )
            .unwrap();
        }
        f
    }

    fn names(f: &SeqFile) -> Vec<String> {
        f.find_sequence(A).unwrap().1.lanes[0]
            .steps
            .iter()
            .map(|s| {
                s.clip
                    .as_ref()
                    .and_then(|c| c.name.clone())
                    .unwrap_or_else(|| "-".into())
            })
            .collect()
    }

    #[test]
    fn create_insert_move_remove() {
        let f = base();
        assert_eq!(names(&f), ["a", "b", "c"]);
        let f = apply(
            &f,
            &EditOp::MoveStep {
                seq: A,
                lane: 0,
                from: 0,
                to: 2,
            },
        )
        .unwrap();
        assert_eq!(names(&f), ["b", "c", "a"]);
        let f = apply(
            &f,
            &EditOp::MoveStep {
                seq: A,
                lane: 0,
                from: 2,
                to: 0,
            },
        )
        .unwrap();
        assert_eq!(names(&f), ["a", "b", "c"]);
        let f = apply(
            &f,
            &EditOp::RemoveStep {
                seq: A,
                lane: 0,
                index: 1,
            },
        )
        .unwrap();
        assert_eq!(names(&f), ["a", "c"]);
        let f = apply(
            &f,
            &EditOp::SetStepClip {
                seq: A,
                lane: 0,
                index: 0,
                clip: None,
            },
        )
        .unwrap();
        assert_eq!(names(&f), ["-", "c"]);
    }

    #[test]
    fn invalid_results_are_rejected_and_input_untouched() {
        let f = base();
        let before = f.clone();
        assert!(matches!(
            apply(
                &f,
                &EditOp::SetStepLength {
                    seq: A,
                    lane: 0,
                    index: 0,
                    length: 0
                }
            ),
            Err(EditError::Invalid(_))
        ));
        assert!(matches!(
            apply(&f, &EditOp::RemoveLane { seq: A, lane: 0 }),
            Err(EditError::Invalid(_))
        ));
        assert_eq!(
            apply(
                &f,
                &EditOp::RemoveStep {
                    seq: A,
                    lane: 0,
                    index: 9
                }
            ),
            Err(EditError::NoSuchStep(9))
        );
        assert_eq!(
            apply(
                &f,
                &EditOp::RenameSequence {
                    seq: B,
                    name: "x".into()
                }
            ),
            Err(EditError::UnknownSequence)
        );
        assert_eq!(f, before);
    }

    #[test]
    fn duplicate_rename_move_delete() {
        let f = base();
        let f = apply(
            &f,
            &EditOp::DuplicateSequence {
                seq: A,
                id: B,
                name: unique_name(&f, "Act 1", "Drop"),
            },
        )
        .unwrap();
        assert_eq!(f.find_sequence(B).unwrap().1.name, "Drop 2");
        assert!(
            apply(
                &f,
                &EditOp::RenameSequence {
                    seq: B,
                    name: "Drop".into()
                }
            )
            .is_err(),
            "duplicate name"
        );
        let f = apply(
            &f,
            &EditOp::MoveSequence {
                seq: B,
                deck: "Act 2".into(),
            },
        )
        .unwrap();
        assert_eq!(f.find_sequence(B).unwrap().0, "Act 2");
        let f = apply(&f, &EditOp::DeleteSequence { seq: B }).unwrap();
        assert!(!f.decks.contains_key("Act 2"), "empty deck entry dropped");
    }

    #[test]
    fn lanes_and_settings() {
        let f = base();
        let f = apply(
            &f,
            &EditOp::AddLane {
                seq: A,
                layer: "FX".into(),
            },
        )
        .unwrap();
        assert!(apply(
            &f,
            &EditOp::SetLaneLayer {
                seq: A,
                lane: 1,
                layer: "BG".into()
            }
        )
        .is_err());
        let f = apply(
            &f,
            &EditOp::SetLaneEnd {
                seq: A,
                lane: 1,
                end: LaneEnd::Hold,
            },
        )
        .unwrap();
        let f = apply(
            &f,
            &EditOp::SetRepeat {
                seq: A,
                repeat: Repeat::Count { count: 4 },
            },
        )
        .unwrap();
        assert!(apply(
            &f,
            &EditOp::SetRepeat {
                seq: A,
                repeat: Repeat::Count { count: 1 }
            }
        )
        .is_err());
        let f = apply(
            &f,
            &EditOp::SetAfterLastPass {
                seq: A,
                value: AfterLastPass::Hold,
            },
        )
        .unwrap();
        let s = f.find_sequence(A).unwrap().1;
        assert_eq!(
            (s.lanes[1].end, s.repeat, s.after_last_pass),
            (
                LaneEnd::Hold,
                Repeat::Count { count: 4 },
                AfterLastPass::Hold
            )
        );
    }

    #[test]
    fn lanes_can_be_reordered() {
        let f = base();
        let f = apply(
            &f,
            &EditOp::AddLane {
                seq: A,
                layer: "FX".into(),
            },
        )
        .unwrap();
        let f = apply(
            &f,
            &EditOp::AddLane {
                seq: A,
                layer: "OVL".into(),
            },
        )
        .unwrap();
        let layers = |f: &SeqFile| {
            f.find_sequence(A)
                .unwrap()
                .1
                .lanes
                .iter()
                .map(|l| l.layer.clone())
                .collect::<Vec<_>>()
        };
        let f = apply(
            &f,
            &EditOp::MoveLane {
                seq: A,
                from: 2,
                to: 0,
            },
        )
        .unwrap();
        assert_eq!(layers(&f), ["OVL", "BG", "FX"]);
        let f = apply(
            &f,
            &EditOp::MoveLane {
                seq: A,
                from: 0,
                to: 1,
            },
        )
        .unwrap();
        assert_eq!(layers(&f), ["BG", "OVL", "FX"]);
        assert_eq!(names(&f), ["a", "b", "c"], "steps travel with their lane");
        assert_eq!(
            apply(
                &f,
                &EditOp::MoveLane {
                    seq: A,
                    from: 0,
                    to: 3
                }
            ),
            Err(EditError::NoSuchLane(3))
        );
    }

    #[test]
    fn ops_serialize_with_an_op_tag() {
        let op = EditOp::SetStepLength {
            seq: A,
            lane: 0,
            index: 1,
            length: 16,
        };
        let j = serde_json::to_value(&op).unwrap();
        assert_eq!(j["op"], "set_step_length");
        assert_eq!(serde_json::from_value::<EditOp>(j).unwrap(), op);
        let insert: EditOp = serde_json::from_str(
            r#"{"op":"insert_step","seq":"00000000-0000-0000-0000-000000000001","lane":0,"index":0,"step":{"clip":null,"length":8}}"#,
        )
        .unwrap();
        assert!(matches!(insert, EditOp::InsertStep { .. }));
    }

    #[test]
    fn lane_mode_and_step_column() {
        use crate::index::test_support::Builder;
        // BG: [a(1), b(2), empty]
        let idx = Builder::new(3)
            .layer("BG", &[Some((1, "a", None)), Some((2, "b", None)), None])
            .build();
        let f = base(); // steps a, b, c (c is not in the index)
        let to_slot = EditOp::SetLaneMode {
            seq: A,
            lane: 0,
            mode: LaneMode::Slot,
        };
        let f = apply_with(&f, &to_slot, Some(&idx)).unwrap();
        let lane = &f.find_sequence(A).unwrap().1.lanes[0];
        assert_eq!(lane.mode, LaneMode::Slot);
        let cols: Vec<_> = lane.steps.iter().map(|s| s.column).collect();
        assert_eq!(cols, [Some(1), Some(2), None]);

        let f = apply(
            &f,
            &EditOp::SetStepColumn {
                seq: A,
                lane: 0,
                index: 2,
                column: Some(3),
            },
        )
        .unwrap();
        // Column 0 and a clip in a slot lane are rejected.
        for bad in [
            EditOp::SetStepColumn {
                seq: A,
                lane: 0,
                index: 0,
                column: Some(0),
            },
            EditOp::SetStepClip {
                seq: A,
                lane: 0,
                index: 0,
                clip: clip("a"),
            },
            EditOp::InsertStep {
                seq: A,
                lane: 0,
                index: 0,
                step: Step::clip(clip("a"), 8),
            },
        ] {
            assert!(
                matches!(apply(&f, &bad), Err(EditError::Invalid(_))),
                "{bad:?}"
            );
        }
        let f = apply(
            &f,
            &EditOp::InsertStep {
                seq: A,
                lane: 0,
                index: 3,
                step: Step::slot(Some(1), 4),
            },
        )
        .unwrap();

        // Back to clips: what is in each slot now; column 3 is empty.
        let to_clip = EditOp::SetLaneMode {
            seq: A,
            lane: 0,
            mode: LaneMode::Clip,
        };
        let f = apply_with(&f, &to_clip, Some(&idx)).unwrap();
        assert_eq!(names(&f), ["a", "b", "-", "a"]);
    }

    #[test]
    fn step_edit_op_json_shapes() {
        let op: EditOp = serde_json::from_str(
            r#"{"op":"insert_step","seq":"00000000-0000-0000-0000-000000000001","lane":0,"index":0,"step":{"column":3,"length":8}}"#,
        )
        .unwrap();
        assert!(matches!(op, EditOp::InsertStep { step, .. } if step == Step::slot(Some(3), 8)));
        let op: EditOp = serde_json::from_str(
            r#"{"op":"insert_step","seq":"00000000-0000-0000-0000-000000000001","lane":0,"index":0,"step":{"clip":null,"length":8}}"#,
        )
        .unwrap();
        assert!(matches!(op, EditOp::InsertStep { step, .. } if step == Step::clip(None, 8)));
        let op: EditOp = serde_json::from_str(
            r#"{"op":"set_lane_mode","seq":"00000000-0000-0000-0000-000000000001","lane":0,"mode":"slot"}"#,
        )
        .unwrap();
        assert!(matches!(
            op,
            EditOp::SetLaneMode {
                mode: LaneMode::Slot,
                ..
            }
        ));
    }
}
