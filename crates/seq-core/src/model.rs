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
    /// Named markers (format v4), sorted by `at`; shown in the editor only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<Section>,
    pub lanes: Vec<Lane>,
}

impl Sequence {
    /// Length of one pass in sixteenths: the length of the longest lane.
    pub fn pass_length(&self) -> u64 {
        self.lanes.iter().map(Lane::length).max().unwrap_or(0)
    }
}

/// When a sequence starts after it is triggered (SPEC §3.2). JSON (format v4):
/// `{"mode":"immediate"}`, `{"mode":"beat"}`, `{"mode":"bar","bars":4}`. Versions 1–3 wrote the
/// string `"immediate"`; `file` reads that form for them only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StartRepr", into = "StartRepr")]
pub enum Start {
    #[default]
    Immediate,
    /// The next beat.
    Beat,
    /// The next bar whose index is a multiple of `bars` ∈ {1, 2, 4, 8, 16} (checked by validation).
    Bar { bars: u32 },
}

/// Bar counts a start can wait for (SPEC §6 `start.bars`).
pub const START_BARS: [u32; 5] = [1, 2, 4, 8, 16];

// Same pattern as `RepeatRepr`: "bars only with mode bar" is enforced while parsing.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRepr {
    mode: StartMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bars: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StartMode {
    Immediate,
    Beat,
    Bar,
}

impl TryFrom<StartRepr> for Start {
    type Error = String;

    fn try_from(r: StartRepr) -> Result<Self, Self::Error> {
        match (r.mode, r.bars) {
            (StartMode::Immediate, None) => Ok(Start::Immediate),
            (StartMode::Beat, None) => Ok(Start::Beat),
            // `bars` defaults to 1 and may be left out.
            (StartMode::Bar, bars) => Ok(Start::Bar {
                bars: bars.unwrap_or(1),
            }),
            (_, Some(_)) => Err("\"bars\" is only allowed with start mode \"bar\"".into()),
        }
    }
}

impl From<Start> for StartRepr {
    fn from(s: Start) -> Self {
        match s {
            Start::Immediate => StartRepr {
                mode: StartMode::Immediate,
                bars: None,
            },
            Start::Beat => StartRepr {
                mode: StartMode::Beat,
                bars: None,
            },
            Start::Bar { bars } => StartRepr {
                mode: StartMode::Bar,
                bars: Some(bars),
            },
        }
    }
}

/// A named marker at a grid position of a sequence (format v4, SPEC §6 `sections`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// In sixteenths from the start of the sequence; may lie beyond its length.
    pub at: u64,
    /// 1–64 characters (checked by validation).
    pub name: String,
}

/// At most this many sections per sequence.
pub const MAX_SECTIONS: usize = 64;
/// Section and blend mode names: 1 to this many characters.
pub const MAX_NAME_CHARS: usize = 64;

/// How a step's clip comes in (format v4, SPEC §6 `transition`, §7.7). Lane default or per step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// Fade length in sixteenths, 0–[`MAX_TRANSITION_LENGTH`] (`0` = cut).
    pub length: u32,
    /// A Resolume blend mode name; absent keeps the layer's original blend mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<String>,
}

/// Longest transition in sixteenths (16 bars).
pub const MAX_TRANSITION_LENGTH: u32 = 256;

/// On which passes a step plays (format v4, SPEC §6 `when`). JSON: `{"is":"first"}`, …,
/// `{"is":"cycle","pass":2,"of":4}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "WhenRepr", into = "WhenRepr")]
pub enum When {
    First,
    NotFirst,
    Last,
    NotLast,
    /// Pass `p` plays if `(p − 1) mod of == pass − 1`; `2 ≤ of ≤ 16`, `1 ≤ pass ≤ of` (checked by
    /// validation).
    Cycle {
        pass: u32,
        of: u32,
    },
}

/// Largest `of` of a `cycle` condition.
pub const MAX_CYCLE: u32 = 16;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WhenRepr {
    is: WhenIs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pass: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    of: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WhenIs {
    First,
    NotFirst,
    Last,
    NotLast,
    Cycle,
}

impl TryFrom<WhenRepr> for When {
    type Error = String;

    fn try_from(r: WhenRepr) -> Result<Self, Self::Error> {
        match (r.is, r.pass, r.of) {
            (WhenIs::First, None, None) => Ok(When::First),
            (WhenIs::NotFirst, None, None) => Ok(When::NotFirst),
            (WhenIs::Last, None, None) => Ok(When::Last),
            (WhenIs::NotLast, None, None) => Ok(When::NotLast),
            (WhenIs::Cycle, Some(pass), Some(of)) => Ok(When::Cycle { pass, of }),
            (WhenIs::Cycle, _, _) => {
                Err("condition \"cycle\" needs both \"pass\" and \"of\"".into())
            }
            _ => Err("\"pass\" and \"of\" are only allowed with condition \"cycle\"".into()),
        }
    }
}

impl From<When> for WhenRepr {
    fn from(w: When) -> Self {
        let (is, pass, of) = match w {
            When::First => (WhenIs::First, None, None),
            When::NotFirst => (WhenIs::NotFirst, None, None),
            When::Last => (WhenIs::Last, None, None),
            When::NotLast => (WhenIs::NotLast, None, None),
            When::Cycle { pass, of } => (WhenIs::Cycle, Some(pass), Some(of)),
        };
        WhenRepr { is, pass, of }
    }
}

/// How one of a step's options is picked each time it plays (format v4, SPEC §6 `pick`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pick {
    /// Any option each time (the default when `pick` is left out).
    #[default]
    Random,
    /// Option 1, 2, 3 … in turn, wrapping; each start begins at option 1.
    Cycle,
}

/// One more option of a step (format v4, SPEC §6 `choices`): a clip reference in clip lanes, a
/// column in slot lanes; `None` (JSON `null`) is a rest. The kind must match the lane's mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Choice {
    Column(u32),
    Clip(ClipRef),
}

/// At most this many `choices` (besides the step's own clip or column).
pub const MAX_CHOICES: usize = 3;

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
#[serde(try_from = "LaneRepr", into = "LaneRepr")]
pub struct Lane {
    /// Resolume layer name; never an index (SPEC §4.4).
    pub layer: String,
    /// What the steps reference (SPEC §4.5). Clip lanes use [`Step::clip`], slot lanes
    /// [`Step::column`]; the other field stays `None`.
    pub mode: LaneMode,
    pub end: LaneEnd,
    /// Default transition of the lane's steps (format v4, SPEC §7.7).
    pub transition: Option<Transition>,
    pub steps: Vec<Step>,
}

/// What a lane's steps reference (SPEC §4.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneMode {
    /// A clip by identity (id, name, file), following it when it moves.
    #[default]
    Clip,
    /// A column number on the lane's layer: whatever sits in that slot plays.
    Slot,
}

impl LaneMode {
    fn is_clip(&self) -> bool {
        *self == LaneMode::Clip
    }
}

// The step field depends on the lane's mode: `clip` in clip lanes, `column` in slot lanes, each
// written even when `null` (a rest). The lane-level form checks that and writes only that field.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaneRepr {
    layer: String,
    #[serde(default, skip_serializing_if = "LaneMode::is_clip")]
    mode: LaneMode,
    end: LaneEnd,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transition: Option<Transition>,
    #[serde(default)]
    steps: Vec<StepRepr>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepRepr {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    clip: Option<Option<ClipRef>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    column: Option<Option<u32>>,
    length: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    when: Option<When>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<Option<Choice>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pick: Option<Pick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transition: Option<Transition>,
}

/// The format v4 step fields, shared by the lane form ([`StepRepr`]) and the flat form
/// ([`StepFlat`]), which spell them out (serde's `flatten` would bypass `deny_unknown_fields`).
/// `choices` is `None` when absent so that `"choices": []` can be refused.
struct StepExtra {
    when: Option<When>,
    choices: Option<Vec<Option<Choice>>>,
    pick: Option<Pick>,
    transition: Option<Transition>,
}

impl StepExtra {
    fn from_fields(
        when: Option<When>,
        choices: Option<Vec<Option<Choice>>>,
        pick: Option<Pick>,
        transition: Option<Transition>,
    ) -> Self {
        StepExtra {
            when,
            choices,
            pick,
            transition,
        }
    }

    /// Checks the "only with" rules and returns (when, choices, pick, transition).
    #[allow(clippy::type_complexity)]
    fn check(
        self,
    ) -> Result<
        (
            Option<When>,
            Vec<Option<Choice>>,
            Option<Pick>,
            Option<Transition>,
        ),
        String,
    > {
        let choices = match self.choices {
            Some(c) if c.is_empty() => {
                return Err(format!(
                    "\"choices\" needs 1 to {MAX_CHOICES} options (leave it out for none)"
                ))
            }
            c => c.unwrap_or_default(),
        };
        if self.pick.is_some() && choices.is_empty() {
            return Err("\"pick\" is only allowed together with \"choices\"".into());
        }
        Ok((self.when, choices, self.pick, self.transition))
    }

    fn of(step: &Step) -> Self {
        StepExtra {
            when: step.when,
            choices: Some(step.choices.clone()).filter(|c| !c.is_empty()),
            pick: step.pick,
            transition: step.transition.clone(),
        }
    }
}

/// Choices of a lane step must be of the lane's kind (clip references or columns, or `null`).
fn check_choice_kinds(mode: LaneMode, choices: &[Option<Choice>]) -> Result<(), String> {
    for (i, c) in choices.iter().enumerate() {
        let n = i + 1;
        match (mode, c) {
            (_, None) | (LaneMode::Clip, Some(Choice::Clip(_))) => {}
            (LaneMode::Slot, Some(Choice::Column(_))) => {}
            (LaneMode::Clip, Some(Choice::Column(_))) => {
                return Err(format!(
                    "choice {n}: a clip lane's choices are clips (or null), not columns"
                ))
            }
            (LaneMode::Slot, Some(Choice::Clip(_))) => {
                return Err(format!(
                    "choice {n}: a slot lane's choices are columns (or null), not clips"
                ))
            }
        }
    }
    Ok(())
}

/// Tells a field given as `null` (`Some(None)`) from a missing one (`None`).
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

impl TryFrom<LaneRepr> for Lane {
    type Error = String;

    fn try_from(r: LaneRepr) -> Result<Self, Self::Error> {
        let layer = &r.layer;
        let steps = r
            .steps
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let n = i + 1;
                let (when, choices, pick, transition) = StepExtra::from_fields(
                    s.when,
                    s.choices,
                    s.pick,
                    s.transition,
                )
                .check()
                    .map_err(|e| format!("lane \"{layer}\" step {n}: {e}"))?;
                check_choice_kinds(r.mode, &choices)
                    .map_err(|e| format!("lane \"{layer}\" step {n}: {e}"))?;
                let with = |step: Step| Step {
                    when,
                    choices,
                    pick,
                    transition,
                    ..step
                };
                match (r.mode, s.clip, s.column) {
                    (LaneMode::Clip, Some(clip), None) => Ok(with(Step::clip(clip, s.length))),
                    (LaneMode::Slot, None, Some(column)) => Ok(with(Step::slot(column, s.length))),
                    (LaneMode::Clip, _, Some(_)) => Err(format!(
                        "lane \"{layer}\" step {n}: \"column\" needs lane mode \"slot\""
                    )),
                    (LaneMode::Slot, Some(_), _) => Err(format!(
                        "lane \"{layer}\" step {n}: steps of a slot lane have \"column\", not \"clip\""
                    )),
                    (LaneMode::Clip, None, None) => Err(format!(
                        "lane \"{layer}\" step {n}: missing \"clip\" (null for a rest)"
                    )),
                    (LaneMode::Slot, None, None) => Err(format!(
                        "lane \"{layer}\" step {n}: missing \"column\" (null for a rest)"
                    )),
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(Lane {
            layer: r.layer,
            mode: r.mode,
            end: r.end,
            transition: r.transition,
            steps,
        })
    }
}

impl From<Lane> for LaneRepr {
    fn from(l: Lane) -> Self {
        let mode = l.mode;
        LaneRepr {
            layer: l.layer,
            mode,
            end: l.end,
            transition: l.transition,
            steps: l
                .steps
                .into_iter()
                .map(|s| {
                    let StepExtra {
                        when,
                        choices,
                        pick,
                        transition,
                    } = StepExtra::of(&s);
                    let (clip, column) = match mode {
                        LaneMode::Clip => (Some(s.clip), None),
                        LaneMode::Slot => (None, Some(s.column)),
                    };
                    StepRepr {
                        clip,
                        column,
                        length: s.length,
                        when,
                        choices,
                        pick,
                        transition,
                    }
                })
                .collect(),
        }
    }
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

/// One step. In the file, steps are written by their [`Lane`], whose mode decides the field.
/// Its own serde form (edit operations, SPEC §9.6) is flat: `{"clip": …, "length": 8}` or
/// `{"column": 3, "length": 8}`, plus the optional `when`, `choices`, `pick`, `transition`;
/// validation checks it against the lane's mode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StepFlat", into = "StepFlat")]
pub struct Step {
    /// Clip lanes: the clip; `None` (JSON `null`) is a rest. Always `None` in slot lanes.
    pub clip: Option<ClipRef>,
    /// Slot lanes: 1-based column on the lane's layer (SPEC §4.5); `None` is a rest. Always
    /// `None` in clip lanes.
    pub column: Option<u32>,
    /// In sixteenths, ≥ 1 (checked by validation). Version 1 files counted eighths.
    pub length: u32,
    /// Plays only on some passes (format v4).
    pub when: Option<When>,
    /// Options 2… (format v4); the step's own `clip` / `column` is option 1. Empty = none.
    pub choices: Vec<Option<Choice>>,
    /// Only with `choices`; `None` means the default, [`Pick::Random`].
    pub pick: Option<Pick>,
    /// How the clip comes in (format v4); wins over the lane's (SPEC §7.7).
    pub transition: Option<Transition>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepFlat {
    #[serde(default)]
    clip: Option<ClipRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    column: Option<u32>,
    length: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    when: Option<When>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<Option<Choice>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pick: Option<Pick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transition: Option<Transition>,
}

impl TryFrom<StepFlat> for Step {
    type Error = String;

    fn try_from(f: StepFlat) -> Result<Self, Self::Error> {
        let (when, choices, pick, transition) =
            StepExtra::from_fields(f.when, f.choices, f.pick, f.transition).check()?;
        Ok(Step {
            clip: f.clip,
            column: f.column,
            length: f.length,
            when,
            choices,
            pick,
            transition,
        })
    }
}

impl From<Step> for StepFlat {
    fn from(s: Step) -> Self {
        let StepExtra {
            when,
            choices,
            pick,
            transition,
        } = StepExtra::of(&s);
        StepFlat {
            clip: s.clip,
            column: s.column,
            length: s.length,
            when,
            choices,
            pick,
            transition,
        }
    }
}

impl Step {
    /// A step of a clip lane.
    pub fn clip(clip: Option<ClipRef>, length: u32) -> Self {
        Self {
            clip,
            length,
            ..Self::default()
        }
    }

    /// A step of a slot lane.
    pub fn slot(column: Option<u32>, length: u32) -> Self {
        Self {
            column,
            length,
            ..Self::default()
        }
    }

    /// A rest in either mode (option 1; see [`Step::options`] for the others).
    pub fn is_rest(&self) -> bool {
        self.clip.is_none() && self.column.is_none()
    }

    /// Option 1, the step's own clip or column, as a [`Choice`].
    pub fn own_option(&self) -> Option<Choice> {
        match (&self.clip, self.column) {
            (Some(c), _) => Some(Choice::Clip(c.clone())),
            (None, Some(col)) => Some(Choice::Column(col)),
            (None, None) => None,
        }
    }

    /// Sets option 1 from a [`Choice`] (a clip or a column; `None` is a rest).
    pub fn set_own_option(&mut self, option: Option<Choice>) {
        (self.clip, self.column) = match option {
            Some(Choice::Clip(c)) => (Some(c), None),
            Some(Choice::Column(col)) => (None, Some(col)),
            None => (None, None),
        };
    }

    /// All options in order: the step's own clip or column, then `choices`.
    pub fn options(&self) -> Vec<Option<Choice>> {
        std::iter::once(self.own_option())
            .chain(self.choices.iter().cloned())
            .collect()
    }

    /// Replaces all options (the first becomes the step's own); drops `pick` when only one is left.
    pub fn set_options(&mut self, mut options: Vec<Option<Choice>>) {
        let own = if options.is_empty() {
            None
        } else {
            options.remove(0)
        };
        self.set_own_option(own);
        self.choices = options;
        if self.choices.is_empty() {
            self.pick = None;
        }
    }
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
            mode: LaneMode::Clip,
            end: LaneEnd::Hold,
            transition: None,
            steps: lengths
                .iter()
                .map(|&length| Step::clip(None, length))
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
            sections: Vec::new(),
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
    fn lane_modes_serde_shapes() {
        let clip = r#"{"layer":"BG","end":"hold","steps":[{"clip":{"name":"A"},"length":8},{"clip":null,"length":4}]}"#;
        let lane: Lane = serde_json::from_str(clip).unwrap();
        assert_eq!(lane.mode, LaneMode::Clip);
        assert!(lane.steps[1].is_rest());
        assert_eq!(serde_json::to_string(&lane).unwrap(), clip);

        let slot = r#"{"layer":"FG","mode":"slot","end":"hold","steps":[{"column":3,"length":8},{"column":null,"length":4}]}"#;
        let lane: Lane = serde_json::from_str(slot).unwrap();
        assert_eq!(lane.mode, LaneMode::Slot);
        assert_eq!(lane.steps[0], Step::slot(Some(3), 8));
        assert!(lane.steps[1].is_rest());
        assert_eq!(serde_json::to_string(&lane).unwrap(), slot);
    }

    #[test]
    fn step_field_must_match_lane_mode() {
        let bad = [
            r#"{"layer":"L","end":"hold","steps":[{"column":1,"length":8}]}"#,
            r#"{"layer":"L","mode":"slot","end":"hold","steps":[{"clip":null,"length":8}]}"#,
            r#"{"layer":"L","end":"hold","steps":[{"length":8}]}"#,
            r#"{"layer":"L","mode":"slot","end":"hold","steps":[{"length":8}]}"#,
            r#"{"layer":"L","mode":"slot","end":"hold","steps":[{"column":1,"clip":null,"length":8}]}"#,
            r#"{"layer":"L","mode":"grid","end":"hold","steps":[]}"#,
            r#"{"layer":"L","end":"hold","steps":[],"extra":1}"#,
        ];
        for j in bad {
            assert!(serde_json::from_str::<Lane>(j).is_err(), "{j}");
        }
    }

    #[test]
    fn start_serde_shapes() {
        let j = |s: Start| serde_json::to_string(&s).unwrap();
        assert_eq!(j(Start::Immediate), r#"{"mode":"immediate"}"#);
        assert_eq!(j(Start::Beat), r#"{"mode":"beat"}"#);
        assert_eq!(j(Start::Bar { bars: 4 }), r#"{"mode":"bar","bars":4}"#);
        let p = |t: &str| serde_json::from_str::<Start>(t);
        assert_eq!(p(r#"{"mode":"bar"}"#).unwrap(), Start::Bar { bars: 1 });
        for bad in [
            r#""immediate""#,
            r#"{"mode":"beat","bars":2}"#,
            r#"{"mode":"immediate","bars":1}"#,
            r#"{"mode":"later"}"#,
            r#"{"mode":"bar","bars":4,"x":1}"#,
        ] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn when_serde_shapes() {
        let j = |w: When| serde_json::to_string(&w).unwrap();
        assert_eq!(j(When::First), r#"{"is":"first"}"#);
        assert_eq!(j(When::NotLast), r#"{"is":"not_last"}"#);
        assert_eq!(
            j(When::Cycle { pass: 2, of: 4 }),
            r#"{"is":"cycle","pass":2,"of":4}"#
        );
        let p = |t: &str| serde_json::from_str::<When>(t);
        assert_eq!(p(r#"{"is":"not_first"}"#).unwrap(), When::NotFirst);
        for bad in [
            r#"{"is":"cycle","pass":2}"#,
            r#"{"is":"cycle","of":2}"#,
            r#"{"is":"last","pass":1}"#,
            r#"{"is":"first","of":2}"#,
            r#"{"is":"sometimes"}"#,
            r#"{"is":"first","x":1}"#,
        ] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn transition_and_section_shapes() {
        let t: Transition = serde_json::from_str(r#"{"length":4,"blend":"Alpha"}"#).unwrap();
        assert_eq!(t.blend.as_deref(), Some("Alpha"));
        assert_eq!(
            serde_json::to_string(&Transition {
                length: 0,
                blend: None
            })
            .unwrap(),
            r#"{"length":0}"#
        );
        assert!(serde_json::from_str::<Transition>(r#"{"length":4,"fade":1}"#).is_err());
        assert!(serde_json::from_str::<Transition>(r#"{"blend":"Alpha"}"#).is_err());
        assert!(serde_json::from_str::<Section>(r#"{"at":0,"name":"Intro","x":1}"#).is_err());
    }

    #[test]
    fn v4_step_fields_in_lanes() {
        let clip = r#"{"layer":"BG","transition":{"length":4},"end":"hold","steps":[{"clip":{"name":"A"},"length":8,"when":{"is":"last"},"choices":[{"name":"B"},null],"pick":"cycle","transition":{"length":0}}]}"#;
        let lane: Lane = serde_json::from_str(clip).unwrap();
        assert_eq!(
            lane.transition,
            Some(Transition {
                length: 4,
                blend: None
            })
        );
        let s = &lane.steps[0];
        assert_eq!(s.when, Some(When::Last));
        assert_eq!(s.pick, Some(Pick::Cycle));
        assert_eq!(s.choices.len(), 2);
        assert!(matches!(&s.choices[0], Some(Choice::Clip(c)) if c.name.as_deref() == Some("B")));
        assert_eq!(s.choices[1], None);
        let back: Lane = serde_json::from_str(&serde_json::to_string(&lane).unwrap()).unwrap();
        assert_eq!(back, lane);

        let slot = r#"{"layer":"FG","mode":"slot","end":"hold","steps":[{"column":2,"length":16,"choices":[3,null,4]}]}"#;
        let lane: Lane = serde_json::from_str(slot).unwrap();
        assert_eq!(
            lane.steps[0].choices,
            [Some(Choice::Column(3)), None, Some(Choice::Column(4))]
        );
        assert_eq!(lane.steps[0].pick, None, "random is the default");
        assert_eq!(serde_json::to_string(&lane).unwrap(), slot);
    }

    #[test]
    fn v4_step_only_with_rules() {
        let bad = [
            // pick only with choices
            r#"{"layer":"L","end":"hold","steps":[{"clip":null,"length":8,"pick":"cycle"}]}"#,
            // choices needs at least one option
            r#"{"layer":"L","end":"hold","steps":[{"clip":null,"length":8,"choices":[]}]}"#,
            // kinds match the lane mode
            r#"{"layer":"L","end":"hold","steps":[{"clip":null,"length":8,"choices":[3]}]}"#,
            r#"{"layer":"L","mode":"slot","end":"hold","steps":[{"column":1,"length":8,"choices":[{"name":"A"}]}]}"#,
            // unknown fields stay errors with the new fields around
            r#"{"layer":"L","end":"hold","steps":[{"clip":null,"length":8,"when":{"is":"first"},"extra":1}]}"#,
            r#"{"layer":"L","end":"hold","steps":[{"clip":null,"length":8,"choices":[{"name":"A","x":1}]}]}"#,
        ];
        for j in bad {
            assert!(serde_json::from_str::<Lane>(j).is_err(), "{j}");
        }
    }

    #[test]
    fn flat_step_form() {
        let p = |t: &str| serde_json::from_str::<Step>(t);
        let s = p(r#"{"column":3,"length":8,"choices":[4],"pick":"random","when":{"is":"first"}}"#)
            .unwrap();
        assert_eq!(s.choices, [Some(Choice::Column(4))]);
        assert_eq!(s.pick, Some(Pick::Random));
        let back = serde_json::to_string(&s).unwrap();
        assert_eq!(p(&back).unwrap(), s);
        assert_eq!(
            serde_json::to_string(&Step::clip(None, 8)).unwrap(),
            r#"{"clip":null,"length":8}"#
        );
        for bad in [
            r#"{"clip":null,"length":8,"pick":"cycle"}"#,
            r#"{"clip":null,"length":8,"choices":[]}"#,
            r#"{"clip":null,"length":8,"extra":1}"#,
        ] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn options_helpers() {
        let mut s = Step::clip(None, 8);
        s.set_options(vec![
            Some(Choice::Clip(ClipRef {
                name: Some("A".into()),
                ..Default::default()
            })),
            None,
        ]);
        s.pick = Some(Pick::Cycle);
        assert_eq!(s.clip.as_ref().unwrap().name.as_deref(), Some("A"));
        assert_eq!(s.choices, [None]);
        assert_eq!(s.options().len(), 2);
        s.set_options(vec![None]);
        assert!(s.is_rest() && s.choices.is_empty());
        assert_eq!(s.pick, None, "pick goes with the last choice");
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
