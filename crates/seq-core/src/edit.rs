//! Edit operations on the sequence file (SPEC §9.6: "Edits are sent as operations, the engine
//! applies them to the stored model"). [`apply`] returns a new, validated file or a reason; the
//! input is never modified, which makes undo a matter of keeping the old value.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::index::CompositionIndex;
use crate::model::{
    AfterLastPass, Choice, ClipRef, Deck, Lane, LaneEnd, LaneMode, Pick, Repeat, Section, SeqFile,
    Sequence, Start, Step, Transition, When,
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
    /// Start mode (format v4, SPEC §3.2).
    SetStart {
        seq: Uuid,
        start: Start,
    },
    /// The lane's default transition; `None` clears it (format v4, SPEC §7.7).
    SetLaneTransition {
        seq: Uuid,
        lane: usize,
        transition: Option<Transition>,
    },
    /// The step's own transition; `None` clears it (the lane default applies again).
    SetStepTransition {
        seq: Uuid,
        lane: usize,
        index: usize,
        transition: Option<Transition>,
    },
    /// Ties the step to the one before (SPEC §6 `tie`) or unties it.
    SetStepTie {
        seq: Uuid,
        lane: usize,
        index: usize,
        tie: bool,
    },
    /// The step's pass condition; `None` = plays on every pass.
    SetStepWhen {
        seq: Uuid,
        lane: usize,
        index: usize,
        when: Option<When>,
    },
    /// Appends an option to the step's `choices` (a clip or column of the lane's kind, or `None`
    /// for a rest).
    AddChoice {
        seq: Uuid,
        lane: usize,
        index: usize,
        choice: Option<Choice>,
    },
    /// Removes option `option` (0 = the step's own clip or column, then the next option takes its
    /// place; 1… = `choices`). The last option cannot be removed. Without choices left, `pick` goes.
    RemoveChoice {
        seq: Uuid,
        lane: usize,
        index: usize,
        option: usize,
    },
    /// Moves an option so that it ends up at position `to` (positions as in `RemoveChoice`).
    MoveChoice {
        seq: Uuid,
        lane: usize,
        index: usize,
        from: usize,
        to: usize,
    },
    /// Relinks one option (positions as in `RemoveChoice`; 0 is the same as `SetStepClip` /
    /// `SetStepColumn`).
    SetChoice {
        seq: Uuid,
        lane: usize,
        index: usize,
        option: usize,
        choice: Option<Choice>,
    },
    /// How an option is picked; `None` = the default, random. Only with choices.
    SetPick {
        seq: Uuid,
        lane: usize,
        index: usize,
        pick: Option<Pick>,
    },
    /// Adds a section marker; the list stays sorted by `at`.
    AddSection {
        seq: Uuid,
        at: u64,
        name: String,
    },
    /// Moves section `section` (its index in the sorted list) to `at`; the list is sorted again.
    MoveSection {
        seq: Uuid,
        section: usize,
        at: u64,
    },
    RenameSection {
        seq: Uuid,
        section: usize,
        name: String,
    },
    DeleteSection {
        seq: Uuid,
        section: usize,
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
    #[error("option {0} does not exist")]
    NoSuchOption(usize),
    #[error("a step keeps at least one option")]
    LastOption,
    #[error("section {0} does not exist")]
    NoSuchSection(usize),
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
                    start: Start::default(),
                    repeat: Repeat::Once,
                    after_last_pass: AfterLastPass::Clear,
                    sections: Vec::new(),
                    lanes: vec![Lane {
                        layer: layer.clone(),
                        mode: LaneMode::Clip,
                        end: LaneEnd::LoopToEnd,
                        transition: None,
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
            transition: None,
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
        EditOp::SetStart { seq, start } => seq_mut(&mut f, *seq)?.start = *start,
        EditOp::SetLaneTransition {
            seq,
            lane,
            transition,
        } => lane_mut(&mut f, *seq, *lane)?.transition = transition.clone(),
        EditOp::SetStepTransition {
            seq,
            lane,
            index,
            transition,
        } => step_mut(&mut f, *seq, *lane, *index)?.transition = transition.clone(),
        EditOp::SetStepTie {
            seq,
            lane,
            index,
            tie,
        } => step_mut(&mut f, *seq, *lane, *index)?.tie = *tie,
        EditOp::SetStepWhen {
            seq,
            lane,
            index,
            when,
        } => step_mut(&mut f, *seq, *lane, *index)?.when = *when,
        EditOp::AddChoice {
            seq,
            lane,
            index,
            choice,
        } => step_mut(&mut f, *seq, *lane, *index)?
            .choices
            .push(choice.clone()),
        EditOp::RemoveChoice {
            seq,
            lane,
            index,
            option,
        } => {
            let step = step_mut(&mut f, *seq, *lane, *index)?;
            let mut options = step.options();
            check_option(&options, *option)?;
            if options.len() == 1 {
                return Err(EditError::LastOption);
            }
            options.remove(*option);
            step.set_options(options);
        }
        EditOp::MoveChoice {
            seq,
            lane,
            index,
            from,
            to,
        } => {
            let step = step_mut(&mut f, *seq, *lane, *index)?;
            let mut options = step.options();
            check_option(&options, *from)?;
            check_option(&options, *to)?;
            let moved = options.remove(*from);
            options.insert(*to, moved);
            step.set_options(options);
        }
        EditOp::SetChoice {
            seq,
            lane,
            index,
            option,
            choice,
        } => {
            let step = step_mut(&mut f, *seq, *lane, *index)?;
            let mut options = step.options();
            check_option(&options, *option)?;
            options[*option] = choice.clone();
            step.set_options(options);
        }
        EditOp::SetPick {
            seq,
            lane,
            index,
            pick,
        } => step_mut(&mut f, *seq, *lane, *index)?.pick = *pick,
        EditOp::AddSection { seq, at, name } => {
            let s = seq_mut(&mut f, *seq)?;
            s.sections.push(Section {
                at: *at,
                name: name.clone(),
            });
            sort_sections(s);
        }
        EditOp::MoveSection { seq, section, at } => {
            let s = seq_mut(&mut f, *seq)?;
            section_mut(s, *section)?.at = *at;
            sort_sections(s);
        }
        EditOp::RenameSection { seq, section, name } => {
            section_mut(seq_mut(&mut f, *seq)?, *section)?.name = name.clone()
        }
        EditOp::DeleteSection { seq, section } => {
            let s = seq_mut(&mut f, *seq)?;
            section_mut(s, *section)?;
            s.sections.remove(*section);
        }
    }
    validate(&f).map_err(EditError::Invalid)?;
    Ok(f)
}

fn check_option(options: &[Option<Choice>], option: usize) -> Result<(), EditError> {
    if option < options.len() {
        Ok(())
    } else {
        Err(EditError::NoSuchOption(option))
    }
}

fn section_mut(s: &mut Sequence, section: usize) -> Result<&mut Section, EditError> {
    s.sections
        .get_mut(section)
        .ok_or(EditError::NoSuchSection(section))
}

/// Keeps sections sorted by position (stable, so a moved section stays before an equal one only
/// if it was; two at one position are refused by validation anyway).
fn sort_sections(s: &mut Sequence) {
    s.sections.sort_by_key(|x| x.at);
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

    fn named(name: &str) -> Option<Choice> {
        clip(name).map(Choice::Clip)
    }

    fn step0(f: &SeqFile) -> &Step {
        &f.find_sequence(A).unwrap().1.lanes[0].steps[0]
    }

    fn ok(f: &SeqFile, op: EditOp) -> SeqFile {
        apply(f, &op).unwrap_or_else(|e| panic!("{op:?}: {e}"))
    }

    #[test]
    fn start_condition_and_transitions() {
        let f = base();
        let f = ok(
            &f,
            EditOp::SetStart {
                seq: A,
                start: Start::Bar { bars: 8 },
            },
        );
        assert_eq!(f.find_sequence(A).unwrap().1.start, Start::Bar { bars: 8 });
        assert!(matches!(
            apply(
                &f,
                &EditOp::SetStart {
                    seq: A,
                    start: Start::Bar { bars: 3 }
                }
            ),
            Err(EditError::Invalid(_))
        ));

        let t = |length, blend: Option<&str>| Transition {
            length,
            blend: blend.map(Into::into),
        };
        let f = ok(
            &f,
            EditOp::SetLaneTransition {
                seq: A,
                lane: 0,
                transition: Some(t(4, Some("Alpha"))),
            },
        );
        let f = ok(
            &f,
            EditOp::SetStepTransition {
                seq: A,
                lane: 0,
                index: 0,
                transition: Some(t(0, None)),
            },
        );
        let f = ok(
            &f,
            EditOp::SetStepWhen {
                seq: A,
                lane: 0,
                index: 0,
                when: Some(When::Cycle { pass: 2, of: 4 }),
            },
        );
        let lane = &f.find_sequence(A).unwrap().1.lanes[0];
        assert_eq!(lane.transition, Some(t(4, Some("Alpha"))));
        assert_eq!(lane.steps[0].transition, Some(t(0, None)));
        assert_eq!(lane.steps[0].when, Some(When::Cycle { pass: 2, of: 4 }));
        for bad in [
            EditOp::SetStepTransition {
                seq: A,
                lane: 0,
                index: 0,
                transition: Some(t(257, None)),
            },
            EditOp::SetStepWhen {
                seq: A,
                lane: 0,
                index: 0,
                when: Some(When::Cycle { pass: 5, of: 4 }),
            },
        ] {
            assert!(
                matches!(apply(&f, &bad), Err(EditError::Invalid(_))),
                "{bad:?}"
            );
        }
        // Clearing restores the plain step.
        let f = ok(
            &f,
            EditOp::SetStepTransition {
                seq: A,
                lane: 0,
                index: 0,
                transition: None,
            },
        );
        let f = ok(
            &f,
            EditOp::SetStepWhen {
                seq: A,
                lane: 0,
                index: 0,
                when: None,
            },
        );
        assert_eq!(step0(&f), &Step::clip(clip("a"), 8));
    }

    #[test]
    fn choices_add_move_relink_remove_and_pick() {
        let f = base(); // step 0 = a
        let add = |f: &SeqFile, c: Option<Choice>| {
            ok(
                f,
                EditOp::AddChoice {
                    seq: A,
                    lane: 0,
                    index: 0,
                    choice: c,
                },
            )
        };
        let f = add(&f, named("x"));
        let f = add(&f, None);
        let f = ok(
            &f,
            EditOp::SetPick {
                seq: A,
                lane: 0,
                index: 0,
                pick: Some(Pick::Cycle),
            },
        );
        assert_eq!(step0(&f).options(), [named("a"), named("x"), None]);
        let f = add(&f, named("y"));
        // A fourth choice and a column in a clip lane are refused.
        for bad in [
            Some(Choice::Clip(ClipRef::default())),
            Some(Choice::Column(2)),
        ] {
            assert!(matches!(
                apply(
                    &f,
                    &EditOp::SetChoice {
                        seq: A,
                        lane: 0,
                        index: 0,
                        option: 1,
                        choice: bad
                    }
                ),
                Err(EditError::Invalid(_))
            ));
        }
        assert!(matches!(
            apply(
                &f,
                &EditOp::AddChoice {
                    seq: A,
                    lane: 0,
                    index: 0,
                    choice: named("z")
                }
            ),
            Err(EditError::Invalid(_))
        ));

        // Reorder: option 3 (y) to the front becomes the step's own clip.
        let f = ok(
            &f,
            EditOp::MoveChoice {
                seq: A,
                lane: 0,
                index: 0,
                from: 3,
                to: 0,
            },
        );
        assert_eq!(step0(&f).clip, clip("y"));
        assert_eq!(step0(&f).choices, [named("a"), named("x"), None]);
        // Relink option 2 (x -> b).
        let f = ok(
            &f,
            EditOp::SetChoice {
                seq: A,
                lane: 0,
                index: 0,
                option: 2,
                choice: named("b"),
            },
        );
        assert_eq!(
            step0(&f).options(),
            [named("y"), named("a"), named("b"), None]
        );
        assert_eq!(
            apply(
                &f,
                &EditOp::SetChoice {
                    seq: A,
                    lane: 0,
                    index: 0,
                    option: 4,
                    choice: None
                }
            ),
            Err(EditError::NoSuchOption(4))
        );
        // Remove the step's own: the next option takes its place.
        let mut f = ok(
            &f,
            EditOp::RemoveChoice {
                seq: A,
                lane: 0,
                index: 0,
                option: 0,
            },
        );
        assert_eq!(step0(&f).options(), [named("a"), named("b"), None]);
        assert_eq!(step0(&f).pick, Some(Pick::Cycle));
        for _ in 0..2 {
            f = ok(
                &f,
                EditOp::RemoveChoice {
                    seq: A,
                    lane: 0,
                    index: 0,
                    option: 1,
                },
            );
        }
        // No choices left: pick went with them; the last option stays.
        assert_eq!(step0(&f), &Step::clip(clip("a"), 8));
        assert_eq!(
            apply(
                &f,
                &EditOp::RemoveChoice {
                    seq: A,
                    lane: 0,
                    index: 0,
                    option: 0
                }
            ),
            Err(EditError::LastOption)
        );
        assert!(matches!(
            apply(
                &f,
                &EditOp::SetPick {
                    seq: A,
                    lane: 0,
                    index: 0,
                    pick: Some(Pick::Random)
                }
            ),
            Err(EditError::Invalid(_))
        ));
    }

    #[test]
    fn slot_lane_choices_are_columns() {
        use crate::index::test_support::Builder;
        let idx = Builder::new(3)
            .layer("BG", &[Some((1, "a", None)), Some((2, "b", None)), None])
            .build();
        let f = ok(
            &base(),
            EditOp::AddChoice {
                seq: A,
                lane: 0,
                index: 0,
                choice: named("b"),
            },
        );
        let f = apply_with(
            &f,
            &EditOp::SetLaneMode {
                seq: A,
                lane: 0,
                mode: LaneMode::Slot,
            },
            Some(&idx),
        )
        .unwrap();
        assert_eq!(
            step0(&f).options(),
            [Some(Choice::Column(1)), Some(Choice::Column(2))]
        );
        let f = ok(
            &f,
            EditOp::AddChoice {
                seq: A,
                lane: 0,
                index: 0,
                choice: Some(Choice::Column(3)),
            },
        );
        assert!(matches!(
            apply(
                &f,
                &EditOp::AddChoice {
                    seq: A,
                    lane: 0,
                    index: 0,
                    choice: named("a")
                }
            ),
            Err(EditError::Invalid(_))
        ));
        assert_eq!(step0(&f).choices.len(), 2);
    }

    #[test]
    fn sections_stay_sorted() {
        let names = |f: &SeqFile| {
            f.find_sequence(A)
                .unwrap()
                .1
                .sections
                .iter()
                .map(|s| (s.at, s.name.clone()))
                .collect::<Vec<_>>()
        };
        let mut f = base();
        for (at, name) in [(64, "Drop"), (0, "Intro"), (32, "Build")] {
            f = ok(
                &f,
                EditOp::AddSection {
                    seq: A,
                    at,
                    name: name.into(),
                },
            );
        }
        assert_eq!(
            names(&f),
            [
                (0, "Intro".into()),
                (32, "Build".into()),
                (64, "Drop".into())
            ]
        );
        // Beyond the sequence's length is allowed.
        let f = ok(
            &f,
            EditOp::MoveSection {
                seq: A,
                section: 0,
                at: 999,
            },
        );
        let f = ok(
            &f,
            EditOp::RenameSection {
                seq: A,
                section: 0,
                name: "Build up".into(),
            },
        );
        assert_eq!(
            names(&f),
            [
                (32, "Build up".into()),
                (64, "Drop".into()),
                (999, "Intro".into())
            ]
        );
        for bad in [
            EditOp::AddSection {
                seq: A,
                at: 64,
                name: "Same place".into(),
            },
            EditOp::RenameSection {
                seq: A,
                section: 1,
                name: String::new(),
            },
        ] {
            assert!(
                matches!(apply(&f, &bad), Err(EditError::Invalid(_))),
                "{bad:?}"
            );
        }
        assert_eq!(
            apply(&f, &EditOp::DeleteSection { seq: A, section: 3 }),
            Err(EditError::NoSuchSection(3))
        );
        let f = ok(&f, EditOp::DeleteSection { seq: A, section: 1 });
        assert_eq!(names(&f), [(32, "Build up".into()), (999, "Intro".into())]);
    }

    #[test]
    fn v4_op_json_shapes() {
        let p = |t: &str| serde_json::from_str::<EditOp>(t).unwrap();
        let seq = r#""seq":"00000000-0000-0000-0000-000000000001""#;
        assert_eq!(
            p(&format!(
                r#"{{"op":"set_start",{seq},"start":{{"mode":"bar","bars":2}}}}"#
            )),
            EditOp::SetStart {
                seq: A,
                start: Start::Bar { bars: 2 }
            }
        );
        assert_eq!(
            p(&format!(
                r#"{{"op":"set_lane_transition",{seq},"lane":0,"transition":null}}"#
            )),
            EditOp::SetLaneTransition {
                seq: A,
                lane: 0,
                transition: None
            }
        );
        assert_eq!(
            p(&format!(
                r#"{{"op":"set_step_when",{seq},"lane":0,"index":1,"when":{{"is":"cycle","pass":1,"of":2}}}}"#
            )),
            EditOp::SetStepWhen {
                seq: A,
                lane: 0,
                index: 1,
                when: Some(When::Cycle { pass: 1, of: 2 })
            }
        );
        assert_eq!(
            p(&format!(
                r#"{{"op":"add_choice",{seq},"lane":0,"index":0,"choice":3}}"#
            )),
            EditOp::AddChoice {
                seq: A,
                lane: 0,
                index: 0,
                choice: Some(Choice::Column(3))
            }
        );
        assert_eq!(
            p(&format!(
                r#"{{"op":"set_choice",{seq},"lane":0,"index":0,"option":1,"choice":{{"name":"a"}}}}"#
            )),
            EditOp::SetChoice {
                seq: A,
                lane: 0,
                index: 0,
                option: 1,
                choice: named("a")
            }
        );
        assert_eq!(
            p(&format!(
                r#"{{"op":"set_pick",{seq},"lane":0,"index":0,"pick":"cycle"}}"#
            )),
            EditOp::SetPick {
                seq: A,
                lane: 0,
                index: 0,
                pick: Some(Pick::Cycle)
            }
        );
        assert_eq!(
            p(&format!(
                r#"{{"op":"add_section",{seq},"at":16,"name":"Drop"}}"#
            )),
            EditOp::AddSection {
                seq: A,
                at: 16,
                name: "Drop".into()
            }
        );
        // A step with the v4 fields in its flat form.
        let op = p(&format!(
            r#"{{"op":"insert_step",{seq},"lane":0,"index":0,"step":{{"clip":null,"length":8,"when":{{"is":"last"}},"choices":[{{"name":"a"}}],"pick":"random","transition":{{"length":4}}}}}}"#
        ));
        let EditOp::InsertStep { step, .. } = op else {
            panic!("{op:?}")
        };
        assert_eq!(step.when, Some(When::Last));
        assert_eq!(step.choices, [named("a")]);
        assert!(serde_json::from_str::<EditOp>(&format!(
            r#"{{"op":"set_pick",{seq},"lane":0,"index":0,"pick":"sometimes"}}"#
        ))
        .is_err());
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
