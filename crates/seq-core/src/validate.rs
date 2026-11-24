//! Rules a sequence file must satisfy beyond what serde already enforces.
//!
//! [`validate`] collects every violation instead of stopping at the first, and each error
//! carries a path such as `decks["Act 1"].sequences[0] "Drop A".lanes[1].steps[3].length`
//! so the user can find it in the file or the UI.

use std::collections::{HashMap, HashSet};
use std::fmt;

use uuid::Uuid;

use crate::model::{
    Choice, LaneMode, Repeat, SeqFile, Sequence, Start, Step, Transition, When, MAX_CHOICES,
    MAX_CYCLE, MAX_NAME_CHARS, MAX_SECTIONS, MAX_TRANSITION_LENGTH, START_BARS,
};

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
    /// Slot lane column 0; columns are 1-based (SPEC §4.5).
    ZeroColumn,
    /// A clip lane step with a column, or a slot lane step with a clip.
    StepModeMismatch,
    /// Two lanes of one sequence target the same layer.
    DuplicateLayer {
        layer: String,
        first_lane: usize,
    },
    /// `start.bars` not one of 1, 2, 4, 8, 16 (format v4).
    StartBars {
        bars: u32,
    },
    /// More than [`MAX_SECTIONS`] sections in one sequence.
    TooManySections {
        count: usize,
    },
    /// A section or blend mode name with no characters or more than [`MAX_NAME_CHARS`].
    NameLength {
        chars: usize,
    },
    /// Sections must be sorted by `at`.
    SectionsNotSorted {
        at: u64,
        previous: u64,
    },
    /// Two sections at the same position.
    DuplicateSection {
        at: u64,
    },
    /// A transition longer than [`MAX_TRANSITION_LENGTH`] sixteenths.
    TransitionTooLong {
        length: u32,
    },
    /// A `cycle` condition outside `2 ≤ of ≤ 16`, `1 ≤ pass ≤ of`.
    CycleOutOfRange {
        pass: u32,
        of: u32,
    },
    /// More than [`MAX_CHOICES`] choices.
    TooManyChoices {
        count: usize,
    },
    /// A choice of the other kind than the lane's mode (a column in a clip lane, a clip in a slot
    /// lane).
    ChoiceModeMismatch,
    /// `pick` without `choices`.
    PickWithoutChoices,
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
            Self::ZeroColumn => write!(f, "column numbers start at 1"),
            Self::StepModeMismatch => {
                write!(
                    f,
                    "step does not match the lane mode (clip lanes use clip, slot lanes column)"
                )
            }
            Self::DuplicateLayer { layer, first_lane } => write!(
                f,
                "layer {layer:?} is already used by lanes[{first_lane}] of this sequence"
            ),
            Self::StartBars { bars } => write!(
                f,
                "start bars must be 1, 2, 4, 8 or 16 (got {bars})"
            ),
            Self::TooManySections { count } => write!(
                f,
                "at most {MAX_SECTIONS} sections per sequence (got {count})"
            ),
            Self::NameLength { chars } => write!(
                f,
                "name must have 1 to {MAX_NAME_CHARS} characters (got {chars})"
            ),
            Self::SectionsNotSorted { at, previous } => write!(
                f,
                "sections must be sorted by position ({at} comes after {previous})"
            ),
            Self::DuplicateSection { at } => {
                write!(f, "another section is already at position {at}")
            }
            Self::TransitionTooLong { length } => write!(
                f,
                "transition length must be 0 to {MAX_TRANSITION_LENGTH} sixteenths (got {length})"
            ),
            Self::CycleOutOfRange { pass, of } => write!(
                f,
                "condition cycle needs 2 <= of <= {MAX_CYCLE} and 1 <= pass <= of (got pass {pass} of {of})"
            ),
            Self::TooManyChoices { count } => write!(
                f,
                "at most {MAX_CHOICES} choices besides the step's own (got {count})"
            ),
            Self::ChoiceModeMismatch => write!(
                f,
                "choice does not match the lane mode (clip lanes use clips, slot lanes columns)"
            ),
            Self::PickWithoutChoices => write!(f, "pick is only allowed together with choices"),
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
    if let Start::Bar { bars } = seq.start {
        if !START_BARS.contains(&bars) {
            err(
                format!("{seq_path}.start.bars"),
                ValidationErrorKind::StartBars { bars },
            );
        }
    }
    validate_sections(seq, seq_path, err);
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
        if let Some(t) = &lane.transition {
            validate_transition(t, &format!("{lane_path}.transition"), err);
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
            if step.column == Some(0) {
                err(
                    format!("{step_path}.column"),
                    ValidationErrorKind::ZeroColumn,
                );
            }
            let mismatch = match lane.mode {
                LaneMode::Clip => step.column.is_some(),
                LaneMode::Slot => step.clip.is_some(),
            };
            if mismatch {
                err(step_path.clone(), ValidationErrorKind::StepModeMismatch);
            }
            validate_step_v4(step, lane.mode, &step_path, err);
        }
    }
}

fn validate_sections(
    seq: &Sequence,
    seq_path: &str,
    err: &mut impl FnMut(String, ValidationErrorKind),
) {
    if seq.sections.len() > MAX_SECTIONS {
        err(
            format!("{seq_path}.sections"),
            ValidationErrorKind::TooManySections {
                count: seq.sections.len(),
            },
        );
    }
    let mut previous: Option<u64> = None;
    for (i, section) in seq.sections.iter().enumerate() {
        let path = format!("{seq_path}.sections[{i}]");
        check_name(&section.name, format!("{path}.name"), err);
        match previous {
            Some(p) if section.at == p => err(
                format!("{path}.at"),
                ValidationErrorKind::DuplicateSection { at: section.at },
            ),
            Some(p) if section.at < p => err(
                format!("{path}.at"),
                ValidationErrorKind::SectionsNotSorted {
                    at: section.at,
                    previous: p,
                },
            ),
            _ => {}
        }
        previous = Some(previous.map_or(section.at, |p| p.max(section.at)));
    }
}

/// 1 to [`MAX_NAME_CHARS`] characters (Unicode scalar values).
fn check_name(name: &str, path: String, err: &mut impl FnMut(String, ValidationErrorKind)) {
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS {
        err(path, ValidationErrorKind::NameLength { chars });
    }
}

fn validate_transition(
    t: &Transition,
    path: &str,
    err: &mut impl FnMut(String, ValidationErrorKind),
) {
    if t.length > MAX_TRANSITION_LENGTH {
        err(
            format!("{path}.length"),
            ValidationErrorKind::TransitionTooLong { length: t.length },
        );
    }
    if let Some(blend) = &t.blend {
        check_name(blend, format!("{path}.blend"), err);
    }
}

/// The format v4 step fields: condition, choices and pick, transition.
fn validate_step_v4(
    step: &Step,
    mode: LaneMode,
    step_path: &str,
    err: &mut impl FnMut(String, ValidationErrorKind),
) {
    if let Some(When::Cycle { pass, of }) = step.when {
        if !(2..=MAX_CYCLE).contains(&of) || pass < 1 || pass > of {
            err(
                format!("{step_path}.when"),
                ValidationErrorKind::CycleOutOfRange { pass, of },
            );
        }
    }
    if step.choices.len() > MAX_CHOICES {
        err(
            format!("{step_path}.choices"),
            ValidationErrorKind::TooManyChoices {
                count: step.choices.len(),
            },
        );
    }
    for (ci, choice) in step.choices.iter().enumerate() {
        let path = format!("{step_path}.choices[{ci}]");
        match (mode, choice) {
            (_, None) => {}
            (LaneMode::Clip, Some(Choice::Clip(c))) => {
                if c.is_empty() {
                    err(path, ValidationErrorKind::EmptyClipRef);
                }
            }
            (LaneMode::Slot, Some(Choice::Column(col))) => {
                if *col == 0 {
                    err(path, ValidationErrorKind::ZeroColumn);
                }
            }
            _ => err(path, ValidationErrorKind::ChoiceModeMismatch),
        }
    }
    if step.pick.is_some() && step.choices.is_empty() {
        err(
            format!("{step_path}.pick"),
            ValidationErrorKind::PickWithoutChoices,
        );
    }
    if let Some(t) = &step.transition {
        validate_transition(t, &format!("{step_path}.transition"), err);
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
            sections: Vec::new(),
            lanes: vec![Lane {
                layer: "BG".into(),
                mode: crate::model::LaneMode::Clip,
                end: LaneEnd::LoopToEnd,
                transition: None,
                steps: vec![Step {
                    column: None,
                    clip: Some(ClipRef {
                        name: Some("Tunnel".into()),
                        ..Default::default()
                    }),
                    length: 8,
                    ..Default::default()
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
    fn start_bars_must_be_a_power_of_two_up_to_16() {
        for bars in START_BARS {
            let mut s = seq("A", 1);
            s.start = Start::Bar { bars };
            assert_eq!(validate(&file_with("D", vec![s])), Ok(()), "{bars}");
        }
        for bars in [0, 3, 32] {
            let mut s = seq("A", 1);
            s.start = Start::Bar { bars };
            assert_eq!(
                kinds(&file_with("D", vec![s])),
                vec![ValidationErrorKind::StartBars { bars }]
            );
        }
    }

    fn section(at: u64, name: &str) -> Section {
        Section {
            at,
            name: name.into(),
        }
    }

    #[test]
    fn sections_sorted_unique_named_and_limited() {
        let mut s = seq("A", 1);
        // Beyond the sequence's length is fine; names of 1 and 64 characters too.
        s.sections = vec![
            section(0, "I"),
            section(16, &"ü".repeat(64)),
            section(10_000, "Late"),
        ];
        assert_eq!(validate(&file_with("D", vec![s.clone()])), Ok(()));

        s.sections = vec![
            section(16, "B"),
            section(8, "A"),
            section(8, ""),
            section(32, &"x".repeat(65)),
        ];
        let errs = validate(&file_with("D", vec![s.clone()])).unwrap_err();
        let got: Vec<_> = errs.iter().map(|e| (e.path.as_str(), &e.kind)).collect();
        assert_eq!(
            got,
            [
                (
                    r#"decks["D"].sequences[0] "A".sections[1].at"#,
                    &ValidationErrorKind::SectionsNotSorted {
                        at: 8,
                        previous: 16
                    }
                ),
                (
                    r#"decks["D"].sequences[0] "A".sections[2].name"#,
                    &ValidationErrorKind::NameLength { chars: 0 }
                ),
                (
                    r#"decks["D"].sequences[0] "A".sections[2].at"#,
                    &ValidationErrorKind::SectionsNotSorted {
                        at: 8,
                        previous: 16
                    }
                ),
                (
                    r#"decks["D"].sequences[0] "A".sections[3].name"#,
                    &ValidationErrorKind::NameLength { chars: 65 }
                ),
            ]
        );

        s.sections = vec![section(4, "A"), section(4, "B")];
        assert_eq!(
            kinds(&file_with("D", vec![s.clone()])),
            vec![ValidationErrorKind::DuplicateSection { at: 4 }]
        );

        s.sections = (0..65).map(|i| section(i, "S")).collect();
        assert_eq!(
            kinds(&file_with("D", vec![s])),
            vec![ValidationErrorKind::TooManySections { count: 65 }]
        );
    }

    fn transition(length: u32, blend: Option<&str>) -> Transition {
        Transition {
            length,
            blend: blend.map(Into::into),
        }
    }

    #[test]
    fn transitions_on_lanes_and_steps() {
        let mut s = seq("A", 1);
        s.lanes[0].transition = Some(transition(256, Some("Alpha")));
        s.lanes[0].steps[0].transition = Some(transition(0, None));
        assert_eq!(validate(&file_with("D", vec![s.clone()])), Ok(()));

        s.lanes[0].transition = Some(transition(257, Some("")));
        s.lanes[0].steps[0].transition = Some(transition(4, Some(&"b".repeat(65))));
        let errs = validate(&file_with("D", vec![s])).unwrap_err();
        let got: Vec<_> = errs.iter().map(|e| (e.path.as_str(), &e.kind)).collect();
        assert_eq!(
            got,
            [
                (
                    r#"decks["D"].sequences[0] "A".lanes[0].transition.length"#,
                    &ValidationErrorKind::TransitionTooLong { length: 257 }
                ),
                (
                    r#"decks["D"].sequences[0] "A".lanes[0].transition.blend"#,
                    &ValidationErrorKind::NameLength { chars: 0 }
                ),
                (
                    r#"decks["D"].sequences[0] "A".lanes[0].steps[0].transition.blend"#,
                    &ValidationErrorKind::NameLength { chars: 65 }
                ),
            ]
        );
    }

    #[test]
    fn cycle_condition_ranges() {
        let check = |pass, of| {
            let mut s = seq("A", 1);
            s.lanes[0].steps[0].when = Some(When::Cycle { pass, of });
            kinds(&file_with("D", vec![s]))
        };
        for (pass, of) in [(1, 2), (2, 2), (1, 16), (16, 16), (3, 7)] {
            assert_eq!(check(pass, of), vec![], "{pass} of {of}");
        }
        for (pass, of) in [(1, 1), (0, 4), (5, 4), (1, 17), (0, 0)] {
            assert_eq!(
                check(pass, of),
                vec![ValidationErrorKind::CycleOutOfRange { pass, of }],
                "{pass} of {of}"
            );
        }
        let mut s = seq("A", 1);
        s.lanes[0].steps[0].when = Some(When::NotLast);
        assert_eq!(validate(&file_with("D", vec![s])), Ok(()));
    }

    fn named(name: &str) -> Option<Choice> {
        Some(Choice::Clip(ClipRef {
            name: Some(name.into()),
            ..Default::default()
        }))
    }

    #[test]
    fn choices_kinds_count_and_pick() {
        let mut s = seq("A", 1);
        s.lanes[0].steps[0].choices = vec![named("B"), None, named("C")];
        s.lanes[0].steps[0].pick = Some(Pick::Cycle);
        assert_eq!(validate(&file_with("D", vec![s.clone()])), Ok(()));

        s.lanes[0].steps[0].choices = vec![
            named("B"),
            Some(Choice::Column(2)),
            Some(Choice::Clip(ClipRef::default())),
            None,
        ];
        assert_eq!(
            kinds(&file_with("D", vec![s.clone()])),
            vec![
                ValidationErrorKind::TooManyChoices { count: 4 },
                ValidationErrorKind::ChoiceModeMismatch,
                ValidationErrorKind::EmptyClipRef,
            ]
        );

        s.lanes[0].steps[0].choices.clear();
        assert_eq!(
            kinds(&file_with("D", vec![s.clone()])),
            vec![ValidationErrorKind::PickWithoutChoices]
        );

        // Slot lanes: columns >= 1 or rests.
        let mut slot = seq("B", 2);
        slot.lanes[0].mode = LaneMode::Slot;
        slot.lanes[0].steps = vec![Step {
            choices: vec![Some(Choice::Column(3)), None, Some(Choice::Column(0))],
            ..Step::slot(Some(1), 8)
        }];
        assert_eq!(
            kinds(&file_with("D", vec![slot.clone()])),
            vec![ValidationErrorKind::ZeroColumn]
        );
        slot.lanes[0].steps[0].choices = vec![named("X")];
        assert_eq!(
            kinds(&file_with("D", vec![slot])),
            vec![ValidationErrorKind::ChoiceModeMismatch]
        );
    }

    #[test]
    fn duplicate_names_helper() {
        let list = [seq("A", 1), seq("B", 2), seq("A", 3), seq("A", 4)];
        assert_eq!(duplicate_names(&list), vec!["A"]);
    }
}
