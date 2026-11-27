//! The scheduler state machine (SPEC §3, §5, §7). Pure: it advances only when the caller
//! passes ticks in, so a missing clock simply freezes every running sequence (SPEC §3 "clock
//! loss") and they continue from where they stopped when ticks resume.
//!
//! A sequence is started from a [`RunSpec`]: a snapshot of the sequence with its clips already
//! resolved against the live deck. Later edits do not touch a running sequence (SPEC §7.1).
//!
//! Actions name clips and layers by **Resolume id**, not position: ids follow clips and layers
//! when columns or layers are inserted while a sequence runs (`docs/resolume-api-notes.md`
//! §2.1). The engine maps them to the current OSC address at send time. Slot lane steps are the
//! exception (SPEC §4.5): they name a column on the lane's layer, and the engine triggers
//! whatever is in that slot when the step starts.
//!
//! Timing: a start emits its first steps immediately (position 0). Each [`Scheduler::on_tick`]
//! advances every running sequence by one tick (24 PPQN, 6 ticks per sixteenth) and emits what
//! falls on that tick.
//!
//! Quantized starts (SPEC §3.2): the scheduler counts the **bar phase**, the ticks since the start
//! of bar index 0. A start with mode `beat` or `bar` that is not processed on a matching boundary
//! waits (*pending*) and begins on the first tick that is one; waiting freezes with the clock like
//! running sequences do. The engine resets the phase on MIDI Start and Resync.
//!
//! A **cue** (SPEC §3.3) is a waiting start that replaces what plays: when it begins, every other
//! run ends. There is at most one. Its point is a start mode or the end of a running sequence's pass.
//!
//! **Mute and solo** (SPEC §7.8) are per lane of a run: a silenced lane keeps counting but sends
//! nothing; its layer is cleared once when it falls silent.
//!
//! **Pass conditions and choices** (format v4, SPEC §6): at each step boundary the step's `when` is
//! checked against the pass counter (a step that doesn't play sends nothing), then one of its
//! options is picked by the step's play counter: in turn (`cycle`) or from the run's seed
//! (`random`, a pure function of seed, lane, step and counter, so it can be known ahead).
//!
//! **Transitions** (SPEC §7.7): a lane that has a transition anywhere gives each of its steps a
//! [`Fade`] (its own, the lane's, or the layer's original); other lanes never touch the layer's
//! transition. [`Action::SetTransition`] goes out one sixteenth before the step, or, while the fade
//! of the step before still runs (a new value would rescale it, notes §19), when that fade ends, or
//! at the latest just before the step's trigger on the same tick. The condition and pick are decided
//! then, as at the boundary, so a step that will not play sets nothing.
//! [`Action::TransitionReleased`] says that no running lane with transitions holds a layer any more;
//! it comes before any clear of that layer, so the clear uses the original.

use uuid::Uuid;

use crate::model::{
    AfterLastPass, LaneEnd, Pick, Repeat, Sequence, Start, Transition, When, TICKS_PER_SIXTEENTH,
};

/// A transition goes out this many ticks before its step at the earliest (one sixteenth).
pub const FADE_LEAD: u64 = TICKS_PER_SIXTEENTH as u64;

/// The transition a step of a lane with transitions gets (SPEC §7.7 rules 1–3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fade {
    /// This length (sixteenths) and, if named, blend mode (the step's own or the lane's).
    Set(Transition),
    /// The layer's original transition (rule 3).
    Original,
}

impl Fade {
    /// Ticks the fade runs, as far as the scheduler knows (the original's length is in seconds
    /// and unknown here: 0).
    pub fn ticks(&self) -> u64 {
        match self {
            Fade::Set(t) => u64::from(t.length) * FADE_LEAD,
            Fade::Original => 0,
        }
    }
}
use crate::resolve::{LayerResolution, SequenceResolution, StepResolution};

/// Ticks per beat (quarter note) and per bar in 4/4.
pub const TICKS_PER_BEAT: u64 = 24;
pub const TICKS_PER_BAR: u64 = 96;

/// 1-based (bar, beat) of a position in ticks, for display (`bar.beat`).
pub fn bar_beat(pos: u64) -> (u64, u64) {
    (
        pos / TICKS_PER_BAR + 1,
        pos % TICKS_PER_BAR / TICKS_PER_BEAT + 1,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepTarget {
    /// Trigger this clip (Resolume clip id).
    Clip(u64),
    /// Rest: clears the lane's layer.
    Rest,
    /// Clip could not be resolved: nothing is triggered (SPEC §4.3).
    Unresolved,
    /// Slot lane: trigger what is in this 1-based column of the lane's layer when the step
    /// starts; an empty slot clears the layer like a rest (SPEC §4.5).
    Slot(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStep {
    pub ticks: u64,
    /// Option 1.
    pub target: StepTarget,
    /// Plays only on these passes (format v4); `None` = every pass.
    pub when: Option<When>,
    /// Options 2… (format v4), resolved like `target`.
    pub choices: Vec<StepTarget>,
    /// How an option is picked when there are choices.
    pub pick: Pick,
    /// Its transition; `None` in a lane without transitions (rule 4: the layer's is left alone).
    pub fade: Option<Fade>,
}

impl RunStep {
    /// A plain step: one option, every pass, no transition.
    pub fn new(ticks: u64, target: StepTarget) -> Self {
        Self {
            ticks,
            target,
            when: None,
            choices: Vec::new(),
            pick: Pick::Random,
            fade: None,
        }
    }

    /// Option `i` (0 = `target`).
    pub fn option(&self, i: usize) -> StepTarget {
        if i == 0 {
            self.target
        } else {
            self.choices[i - 1]
        }
    }
}

/// Whether a step with condition `when` plays on 1-based `pass` of `passes` (`None` = infinite)
/// (SPEC §6 `when`).
pub fn plays_on(when: Option<When>, pass: u32, passes: Option<u32>) -> bool {
    match when {
        None => true,
        Some(When::First) => pass == 1,
        Some(When::NotFirst) => pass != 1,
        // With infinite repeat there is no last pass.
        Some(When::Last) => passes == Some(pass),
        Some(When::NotLast) => passes != Some(pass),
        Some(When::Cycle { pass: a, of: b }) => {
            b > 0 && (pass.saturating_sub(1)) % b == a.saturating_sub(1)
        }
    }
}

/// splitmix64: a well-mixed hash of the inputs, for `random` picks.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// The option a step plays the `count`-th time (0-based) in a run with `seed`.
pub fn pick_option(step: &RunStep, seed: u64, lane: usize, index: usize, count: u32) -> usize {
    let n = 1 + step.choices.len();
    if n == 1 {
        return 0;
    }
    match step.pick {
        Pick::Cycle => count as usize % n,
        Pick::Random => {
            let key = seed ^ mix((lane as u64) << 40 ^ (index as u64) << 20 ^ u64::from(count));
            (mix(key) % n as u64) as usize
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLane {
    /// Resolume layer id; `None` when the lane's layer is missing or ambiguous (silent lane).
    pub layer_id: Option<u64>,
    pub end: LaneEnd,
    pub steps: Vec<RunStep>,
}

impl RunLane {
    pub fn ticks(&self) -> u64 {
        self.steps.iter().map(|s| s.ticks).sum()
    }

    /// A lane plays only with a resolved layer and at least one step.
    pub fn is_active(&self) -> bool {
        self.layer_id.is_some() && self.ticks() > 0
    }

    /// The lane sets its layer's transition (SPEC §7.7 rules 1–3).
    pub fn fades(&self) -> bool {
        self.steps.iter().any(|s| s.fade.is_some())
    }
}

/// Snapshot of a sequence taken at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    pub seq_id: Uuid,
    pub name: String,
    pub repeat: Repeat,
    pub after_last_pass: AfterLastPass,
    pub lanes: Vec<RunLane>,
    /// Seed for `random` picks; the engine sets a fresh one per start (SPEC §6: no IO here, tests
    /// pass a fixed one).
    pub seed: u64,
    /// The shortest pass: a recording of an empty sequence runs for this pattern length
    /// (SPEC §7.9). `0` = only the lanes count.
    pub min_ticks: u64,
    /// Lanes on these layers begin the pass yielded (a clip held by hand while recording, §7.9);
    /// only for [`Scheduler::set_next_pass`].
    pub yielded_layers: Vec<u64>,
}

impl RunSpec {
    /// Builds the snapshot from a sequence and its resolution against the live deck.
    pub fn new(seq: &Sequence, resolution: &SequenceResolution) -> Self {
        let lanes = seq
            .lanes
            .iter()
            .zip(&resolution.lanes)
            .map(|(lane, res)| {
                let fades =
                    lane.transition.is_some() || lane.steps.iter().any(|s| s.transition.is_some());
                RunLane {
                    layer_id: match res.layer {
                        LayerResolution::Found { id, .. } => Some(id),
                        _ => None,
                    },
                    end: lane.end,
                    steps: lane
                        .steps
                        .iter()
                        .zip(&res.steps)
                        .enumerate()
                        .map(|(si, (step, sr))| RunStep {
                            ticks: u64::from(step.length) * u64::from(TICKS_PER_SIXTEENTH),
                            target: target_of(sr),
                            when: step.when,
                            choices: res
                                .choices
                                .get(si)
                                .map(|c| c.iter().map(target_of).collect())
                                .unwrap_or_default(),
                            pick: step.pick.unwrap_or_default(),
                            fade: fades.then(|| {
                                step.transition
                                    .as_ref()
                                    .or(lane.transition.as_ref())
                                    .map_or(Fade::Original, |t| Fade::Set(t.clone()))
                            }),
                        })
                        .collect(),
                }
            })
            .collect();
        Self {
            seq_id: seq.id,
            name: seq.name.clone(),
            repeat: seq.repeat,
            after_last_pass: seq.after_last_pass,
            lanes,
            seed: 0,
            min_ticks: 0,
            yielded_layers: Vec::new(),
        }
    }

    /// One pass = the longest lane (SPEC §5), counting silent lanes too, at least `min_ticks`.
    pub fn pass_ticks(&self) -> u64 {
        self.lanes
            .iter()
            .map(RunLane::ticks)
            .max()
            .unwrap_or(0)
            .max(self.min_ticks)
    }

    /// Number of passes; `None` = infinite.
    pub fn passes(&self) -> Option<u32> {
        match self.repeat {
            Repeat::Once => Some(1),
            Repeat::Count { count } => Some(count.max(1)),
            Repeat::Infinite => None,
        }
    }
}

fn target_of(sr: &StepResolution) -> StepTarget {
    match sr {
        StepResolution::Rest => StepTarget::Rest,
        StepResolution::Resolved { clip, .. } => StepTarget::Clip(clip.clip_id),
        StepResolution::Unresolved { .. } => StepTarget::Unresolved,
        StepResolution::Slot { column, .. } => StepTarget::Slot(*column),
    }
}

/// What started a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Ui,
    /// Its `§` placeholder clip became connected in Resolume.
    SeqClip {
        clip_id: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneState {
    /// Layer unresolved or no steps: silent, owns nothing.
    Inactive,
    Playing,
    /// Manual override (SPEC §7.3): counts but does not trigger. `resume_pending` resumes at the
    /// next step boundary.
    Yielded {
        resume_pending: bool,
    },
    /// Another sequence started on this layer; silent for the rest of this run.
    TakenOver,
}

impl LaneState {
    fn owns_layer(self) -> bool {
        matches!(self, Self::Playing | Self::Yielded { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Last pass completed.
    Finished,
    /// Stopped by command.
    Stopped,
    /// Started again; this run was replaced.
    Restarted,
    /// Every lane was taken over by other sequences.
    TakenOver,
}

/// How to treat a sequence's layers when stopping it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopMode {
    /// Apply the sequence's `after_last_pass` to its playing lanes (SPEC §7.4).
    AfterLastPass,
    /// Leave layers as they are (e.g. the deck changed in Resolume).
    Release,
    /// Clear every owned layer, yielded ones included (stop all / panic).
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Connect this clip (OSC). `layer_id` is the lane's layer (for ownership bookkeeping).
    Trigger {
        seq: Uuid,
        lane: usize,
        step: usize,
        clip_id: u64,
        layer_id: u64,
    },
    /// Trigger whatever is in `column` of the lane's layer now; clear the layer if the slot is
    /// empty (SPEC §4.5). The engine decides which, from its newest view of the slot.
    TriggerSlot {
        seq: Uuid,
        lane: usize,
        step: usize,
        column: u32,
        layer_id: u64,
    },
    /// Clear this layer (OSC).
    Clear {
        seq: Uuid,
        lane: usize,
        layer_id: u64,
    },
    // State notifications for the UI and the engine.
    Started {
        seq: Uuid,
        origin: Origin,
    },
    PassStarted {
        seq: Uuid,
        pass: u32,
    },
    /// Emitted at every step boundary, including rests and unresolved steps.
    StepStarted {
        seq: Uuid,
        lane: usize,
        step: usize,
    },
    LaneStateChanged {
        seq: Uuid,
        lane: usize,
        state: LaneState,
    },
    Ended {
        seq: Uuid,
        reason: EndReason,
        origin: Origin,
    },
    /// A start waits for its boundary (SPEC §3.2); [`Action::Started`] follows when it begins.
    Pending {
        seq: Uuid,
        origin: Origin,
    },
    /// A waiting start was cancelled (Stop, Stop all, a newer cue) and will not begin.
    PendingCancelled {
        seq: Uuid,
        origin: Origin,
    },
    /// Mute or solo changed a lane (SPEC §7.8); `silent`: it sends nothing now.
    LaneMuteChanged {
        seq: Uuid,
        lane: usize,
        silent: bool,
    },
    /// Set the layer's transition for `step` of the lane, which starts within [`FADE_LEAD`]
    /// ticks or on this tick (then this comes before its trigger). Also before a lane end
    /// *Clear* (the original). The engine sends it only if the layer has another value.
    SetTransition {
        seq: Uuid,
        lane: usize,
        step: usize,
        layer_id: u64,
        fade: Fade,
    },
    /// No running lane with transitions holds this layer any more: its original goes back
    /// (before a clear of it in the same list).
    TransitionReleased {
        layer_id: u64,
    },
}

/// A slot lane step that starts a given number of ticks from now (see
/// [`Scheduler::slot_steps_ahead`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpcomingSlot {
    pub seq: Uuid,
    pub lane: usize,
    pub step: usize,
    pub layer_id: u64,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    #[error("sequence has no steps")]
    Empty,
    #[error("no lane of this sequence has a resolved layer")]
    NoPlayableLanes,
}

/// What a waiting start waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// A boundary of this start mode on the bar phase (SPEC §3.2).
    Mode(Start),
    /// The end of the current pass of this running sequence (cues only, SPEC §3.3); at once if it
    /// is not running.
    EndOfPass(Uuid),
}

/// A start waiting for its boundary (see [`Scheduler::pending`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingStatus {
    pub seq: Uuid,
    pub name: String,
    pub origin: Origin,
    pub wait: Wait,
    /// A cue (SPEC §3.3): every other run ends when it begins.
    pub cue: bool,
    /// Ticks until it begins (≥ 1), if the clock keeps running.
    pub wait_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneStatus {
    pub state: LaneState,
    /// Muted by hand (SPEC §7.8).
    pub mute: bool,
    pub solo: bool,
    /// Sends nothing: muted, or another lane of the run is soloed and this one is not.
    pub silent: bool,
    pub layer_id: Option<u64>,
    /// Current step, `None` before/after the lane's steps (hold/clear lanes past their end).
    pub step: Option<usize>,
    /// The option (0 = the step's own) the current step played; `None` when its condition kept
    /// it from playing (format v4).
    pub option: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStatus {
    pub seq: Uuid,
    pub name: String,
    pub origin: Origin,
    /// 1-based.
    pub pass: u32,
    pub passes: Option<u32>,
    /// Ticks since the start of the current pass.
    pub pos: u64,
    pub pass_ticks: u64,
    pub lanes: Vec<LaneStatus>,
}

struct LaneRt {
    state: LaneState,
    mute: bool,
    solo: bool,
    /// Effective mute: `mute`, or a solo elsewhere in the run and none here.
    silent: bool,
    /// How often each step played in this run (its condition was true), for picks.
    plays: Vec<u32>,
    /// Option the current step played (see [`LaneStatus::option`]).
    option: Option<usize>,
    /// Start position of each step within the lane.
    starts: Vec<u64>,
    len: u64,
    /// The next step's transition went out ahead.
    fade_sent: bool,
    /// Run tick ([`Run::elapsed`]) the lane last sent a step, and when that step's fade ends.
    step_sent_at: u64,
    fade_end: u64,
}

impl LaneRt {
    fn new(l: &RunLane) -> Self {
        let mut starts = Vec::with_capacity(l.steps.len());
        let mut at = 0;
        for s in &l.steps {
            starts.push(at);
            at += s.ticks;
        }
        LaneRt {
            state: if l.is_active() {
                LaneState::Playing
            } else {
                LaneState::Inactive
            },
            mute: false,
            solo: false,
            silent: false,
            plays: vec![0; l.steps.len()],
            option: None,
            starts,
            len: at,
            fade_sent: false,
            step_sent_at: 0,
            fade_end: 0,
        }
    }

    fn lane_pos(&self, end: LaneEnd, pos: u64) -> Option<u64> {
        match end {
            LaneEnd::LoopToEnd => Some(pos % self.len),
            _ if pos < self.len => Some(pos),
            _ => None,
        }
    }

    fn step_at(&self, end: LaneEnd, pos: u64) -> Option<usize> {
        let lp = self.lane_pos(end, pos)?;
        Some(self.starts.partition_point(|&s| s <= lp) - 1)
    }

    fn step_starting_at(&self, end: LaneEnd, pos: u64) -> Option<usize> {
        let lp = self.lane_pos(end, pos)?;
        self.starts.binary_search(&lp).ok()
    }
}

struct Run {
    spec: RunSpec,
    origin: Origin,
    pass: u32,
    pos: u64,
    pass_ticks: u64,
    lanes: Vec<LaneRt>,
    /// Ticks since the run began (not reset by a pass or a resync).
    elapsed: u64,
    /// Takes effect at the next pass boundary ([`Scheduler::set_next_pass`]).
    next_spec: Option<RunSpec>,
}

impl Run {
    /// Puts `next_spec` in place at a pass boundary: lanes keep their mute and solo and play again if
    /// they had yielded; taken-over lanes stay silent; new lanes play.
    fn swap_spec(&mut self) {
        let Some(spec) = self.next_spec.take() else {
            return;
        };
        let lanes = spec
            .lanes
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let mut rt = LaneRt::new(l);
                if let Some(old) = self.lanes.get(i) {
                    rt.mute = old.mute;
                    rt.solo = old.solo;
                    rt.silent = old.silent;
                    if old.state == LaneState::TakenOver {
                        rt.state = LaneState::TakenOver;
                    }
                }
                if rt.state == LaneState::Playing
                    && l.layer_id
                        .is_some_and(|id| spec.yielded_layers.contains(&id))
                {
                    rt.state = LaneState::Yielded {
                        resume_pending: false,
                    };
                }
                rt
            })
            .collect();
        self.pass_ticks = spec.pass_ticks().max(1);
        self.spec = spec;
        self.lanes = lanes;
    }

    /// The step of `lane` that starts exactly `k` ticks from now (≥ 1) and the pass it is in,
    /// if the run is still going then.
    fn step_in(&self, lane: usize, k: u64) -> Option<(usize, u32)> {
        let mut at = self.pos + k;
        let mut pass = self.pass;
        if at >= self.pass_ticks {
            let passes_left = self
                .spec
                .passes()
                .map_or(u64::MAX, |n| u64::from(n.saturating_sub(self.pass)));
            let wraps = at / self.pass_ticks;
            if wraps > passes_left {
                return None;
            }
            pass += wraps as u32;
            at %= self.pass_ticks;
        }
        let step = self.lanes[lane].step_starting_at(self.spec.lanes[lane].end, at)?;
        Some((step, pass))
    }

    /// The lane triggers at its next step boundary (playing, or yielded with a resume pending)
    /// and is not silenced.
    fn will_play(&self, lane: usize) -> bool {
        let rt = &self.lanes[lane];
        !rt.silent
            && matches!(
                rt.state,
                LaneState::Playing
                    | LaneState::Yielded {
                        resume_pending: true
                    }
            )
    }

    /// The option `step` plays next time on `pass`, or `None` if its condition keeps it silent.
    fn next_option(&self, lane: usize, step: usize, pass: u32) -> Option<usize> {
        let rs = &self.spec.lanes[lane].steps[step];
        plays_on(rs.when, pass, self.spec.passes())
            .then(|| pick_option(rs, self.spec.seed, lane, step, self.lanes[lane].plays[step]))
    }

    /// Transitions that go out ahead on this tick (see the module docs).
    fn fades_ahead(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        let now = self.elapsed;
        for lane in 0..self.lanes.len() {
            let rt = &self.lanes[lane];
            if rt.fade_sent
                || !self.spec.lanes[lane].fades()
                || !self.will_play(lane)
                || now <= rt.step_sent_at
                || now < rt.fade_end
            {
                continue;
            }
            let Some((step, pass)) = (1..=FADE_LEAD).find_map(|k| self.step_in(lane, k)) else {
                continue;
            };
            let Some(option) = self.next_option(lane, step, pass) else {
                continue;
            };
            let rs = &self.spec.lanes[lane].steps[step];
            if rs.option(option) == StepTarget::Unresolved {
                continue;
            }
            let (Some(fade), Some(layer_id)) = (rs.fade.clone(), self.spec.lanes[lane].layer_id)
            else {
                continue;
            };
            self.lanes[lane].fade_sent = true;
            out.push(Action::SetTransition {
                seq: self.spec.seq_id,
                lane,
                step,
                layer_id,
                fade,
            });
        }
        out
    }
}

struct PendingStart {
    spec: RunSpec,
    origin: Origin,
    wait: Wait,
    cue: bool,
}

/// Ticks between two boundaries of a start mode; `None` for `immediate`.
fn period(mode: Start) -> Option<u64> {
    match mode {
        Start::Immediate => None,
        Start::Beat => Some(TICKS_PER_BEAT),
        Start::Bar { bars } => Some(TICKS_PER_BAR * u64::from(bars.max(1))),
    }
}

#[derive(Default)]
pub struct Scheduler {
    runs: Vec<Run>,
    pending: Vec<PendingStart>,
    /// Bar phase of the last tick (ticks since bar index 0 began); `None` until the first tick
    /// after a reset, which is then phase 0.
    phase: Option<u64>,
    /// Layers a run that is beginning will set transitions on (while it ends the runs it
    /// replaces, so their layers are not released in between).
    incoming: Vec<u64>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self, seq: Uuid) -> bool {
        self.runs.iter().any(|r| r.spec.seq_id == seq)
    }

    /// The sequence has a start waiting for its boundary.
    pub fn is_pending(&self, seq: Uuid) -> bool {
        self.pending.iter().any(|p| p.spec.seq_id == seq)
    }

    /// Starts waiting for their boundary, in the order they were made.
    pub fn pending(&self) -> Vec<PendingStatus> {
        self.pending
            .iter()
            .map(|p| PendingStatus {
                seq: p.spec.seq_id,
                name: p.spec.name.clone(),
                origin: p.origin,
                wait: p.wait,
                cue: p.cue,
                wait_ticks: self.wait_ticks_for(p.wait),
            })
            .collect()
    }

    fn wait_ticks_for(&self, wait: Wait) -> u64 {
        match wait {
            Wait::Mode(mode) => self.wait_ticks(mode),
            Wait::EndOfPass(seq) => self
                .runs
                .iter()
                .find(|r| r.spec.seq_id == seq)
                .map_or(1, |r| r.pass_ticks.saturating_sub(r.pos).max(1)),
        }
    }

    /// The waiting start is due on the tick being processed (after the phase moved, before the
    /// runs advance).
    fn due(&self, wait: Wait) -> bool {
        match wait {
            Wait::Mode(mode) => self.on_boundary(mode),
            Wait::EndOfPass(seq) => self
                .runs
                .iter()
                .find(|r| r.spec.seq_id == seq)
                .is_none_or(|r| r.pos + 1 >= r.pass_ticks),
        }
    }

    /// Ticks from the last tick to the next boundary of `mode` (1 before the first tick: that one
    /// is phase 0, a boundary of every mode).
    fn wait_ticks(&self, mode: Start) -> u64 {
        match (self.phase, period(mode)) {
            (None, _) | (_, None) => 1,
            (Some(phase), Some(n)) => n - phase % n,
        }
    }

    /// The last tick is a boundary of `mode` (always for `immediate`).
    fn on_boundary(&self, mode: Start) -> bool {
        match (period(mode), self.phase) {
            (None, _) => true,
            (Some(n), Some(phase)) => phase % n == 0,
            (Some(_), None) => false,
        }
    }

    /// MIDI Start (SPEC §3.2): the next tick is the start of bar index 0. Waiting starts wait for
    /// their boundary counted from there.
    pub fn phase_reset_next_tick(&mut self) {
        self.phase = None;
    }

    /// Resync (SPEC §3.1): this moment is the start of bar index 0, the next tick has phase 1.
    /// Every waiting start is on its boundary now and begins.
    pub fn phase_reset_now(&mut self) -> Vec<Action> {
        self.phase = Some(0);
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| matches!(p.wait, Wait::Mode(_)));
        self.pending = waiting;
        let mut out = Vec::new();
        for p in due {
            out.extend(self.begin_pending(p));
        }
        out
    }

    /// Its `§` clip connected while the sequence runs (SPEC §7.1): the run is linked to that clip
    /// from now on, without a restart. Returns whether a run was adopted.
    pub fn adopt(&mut self, seq: Uuid, origin: Origin) -> bool {
        match self.runs.iter_mut().find(|r| r.spec.seq_id == seq) {
            Some(run) => {
                run.origin = origin;
                true
            }
            None => false,
        }
    }

    /// Links a waiting start to `origin` (a `§` clip that connected while it waits, SPEC §7.1).
    pub fn link_pending(&mut self, seq: Uuid, origin: Origin) {
        if let Some(p) = self.pending.iter_mut().find(|p| p.spec.seq_id == seq) {
            p.origin = origin;
        }
    }

    /// Cancels a waiting start; nothing if there is none.
    pub fn cancel_pending(&mut self, seq: Uuid) -> Vec<Action> {
        let mut out = Vec::new();
        self.pending.retain(|p| {
            if p.spec.seq_id == seq {
                out.push(Action::PendingCancelled {
                    seq,
                    origin: p.origin,
                });
                false
            } else {
                true
            }
        });
        out
    }

    pub fn running(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.runs.iter().map(|r| r.spec.seq_id)
    }

    /// The running lane that owns a layer (playing or yielded).
    pub fn owner_of(&self, layer_id: u64) -> Option<(Uuid, usize)> {
        self.runs.iter().find_map(|r| {
            r.lanes.iter().enumerate().find_map(|(i, l)| {
                (l.state.owns_layer() && r.spec.lanes[i].layer_id == Some(layer_id))
                    .then_some((r.spec.seq_id, i))
            })
        })
    }

    pub fn status(&self) -> Vec<RunStatus> {
        self.runs
            .iter()
            .map(|r| RunStatus {
                seq: r.spec.seq_id,
                name: r.spec.name.clone(),
                origin: r.origin,
                pass: r.pass,
                passes: r.spec.passes(),
                pos: r.pos,
                pass_ticks: r.pass_ticks,
                lanes: r
                    .lanes
                    .iter()
                    .zip(&r.spec.lanes)
                    .map(|(rt, lane)| LaneStatus {
                        state: rt.state,
                        mute: rt.mute,
                        solo: rt.solo,
                        silent: rt.silent,
                        option: rt.option,
                        layer_id: lane.layer_id,
                        step: match rt.state {
                            LaneState::Inactive => None,
                            _ => rt.step_at(lane.end, r.pos),
                        },
                    })
                    .collect(),
            })
            .collect()
    }

    /// Starts (or restarts) a sequence by its start mode (SPEC §3.2): now if `mode` is
    /// `immediate` or the last tick is one of its boundaries, otherwise when the next boundary is
    /// reached ([`Action::Pending`] now, [`Action::Started`] then). A waiting start of the same
    /// sequence is replaced. Lanes of other sequences on the same layers are taken over when it
    /// begins (SPEC §7.3).
    pub fn start(
        &mut self,
        spec: RunSpec,
        origin: Origin,
        mode: Start,
    ) -> Result<Vec<Action>, StartError> {
        if spec.pass_ticks() == 0 {
            return Err(StartError::Empty);
        }
        if !spec.lanes.iter().any(RunLane::is_active) && spec.min_ticks == 0 {
            return Err(StartError::NoPlayableLanes);
        }
        let seq = spec.seq_id;
        // Starting a cued sequence again starts it at once and still replaces what plays.
        let cue = self.pending.iter().any(|p| p.spec.seq_id == seq && p.cue);
        self.pending.retain(|p| p.spec.seq_id != seq);
        let p = PendingStart {
            spec,
            origin,
            wait: Wait::Mode(mode),
            cue,
        };
        if self.on_boundary(mode) {
            return Ok(self.begin_pending(p));
        }
        self.pending.push(p);
        Ok(vec![Action::Pending { seq, origin }])
    }

    /// Cues a sequence (SPEC §3.3): it begins when `wait` is reached and every other run ends
    /// then. Replaces an earlier cue and a waiting start of the same sequence. A cue whose point
    /// is reached now (a boundary tick, or the end of a pass of a sequence that is not running)
    /// begins at once.
    pub fn cue(
        &mut self,
        spec: RunSpec,
        origin: Origin,
        wait: Wait,
    ) -> Result<Vec<Action>, StartError> {
        if spec.pass_ticks() == 0 {
            return Err(StartError::Empty);
        }
        if !spec.lanes.iter().any(RunLane::is_active) && spec.min_ticks == 0 {
            return Err(StartError::NoPlayableLanes);
        }
        let seq = spec.seq_id;
        let mut out = Vec::new();
        self.pending.retain(|p| {
            let drop = p.cue || p.spec.seq_id == seq;
            if drop && p.spec.seq_id != seq {
                out.push(Action::PendingCancelled {
                    seq: p.spec.seq_id,
                    origin: p.origin,
                });
            }
            !drop
        });
        let p = PendingStart {
            spec,
            origin,
            wait,
            cue: true,
        };
        let now = match wait {
            Wait::Mode(mode) => self.on_boundary(mode),
            Wait::EndOfPass(of) => !self.is_running(of),
        };
        if now {
            out.extend(self.begin_pending(p));
        } else {
            self.pending.push(p);
            out.push(Action::Pending { seq, origin });
        }
        Ok(out)
    }

    /// The cue, if any: (sequence, what it waits for).
    pub fn cued(&self) -> Option<(Uuid, Wait)> {
        self.pending
            .iter()
            .find(|p| p.cue)
            .map(|p| (p.spec.seq_id, p.wait))
    }

    /// Cancels the cue, if any.
    pub fn cancel_cue(&mut self) -> Vec<Action> {
        match self.cued() {
            Some((seq, _)) => self.cancel_pending(seq),
            None => Vec::new(),
        }
    }

    /// Begins a waiting start; a cue then ends every other run.
    fn begin_pending(&mut self, p: PendingStart) -> Vec<Action> {
        let seq = p.spec.seq_id;
        let mut out = self.begin(p.spec, p.origin);
        if p.cue {
            while let Some(i) = self.runs.iter().position(|r| r.spec.seq_id != seq) {
                out.extend(self.end_run(i, StopMode::AfterLastPass, EndReason::Stopped));
            }
        }
        out
    }

    /// Mutes or unmutes a lane of a running sequence (SPEC §7.8).
    pub fn set_mute(&mut self, seq: Uuid, lane: usize, on: bool) -> Vec<Action> {
        match self.lane_rt(seq, lane) {
            Some(rt) => rt.mute = on,
            None => return Vec::new(),
        }
        self.apply_mutes(seq)
    }

    /// Solos or unsolos a lane of a running sequence (SPEC §7.8).
    pub fn set_solo(&mut self, seq: Uuid, lane: usize, on: bool) -> Vec<Action> {
        match self.lane_rt(seq, lane) {
            Some(rt) => rt.solo = on,
            None => return Vec::new(),
        }
        self.apply_mutes(seq)
    }

    fn lane_rt(&mut self, seq: Uuid, lane: usize) -> Option<&mut LaneRt> {
        self.runs
            .iter_mut()
            .find(|r| r.spec.seq_id == seq)?
            .lanes
            .get_mut(lane)
    }

    /// Recomputes which lanes of a run are silent. A lane falling silent clears its layer if it
    /// owns it; a lane coming back plays from its next step (a yielded one resumes there too).
    fn apply_mutes(&mut self, seq: Uuid) -> Vec<Action> {
        let Some(run) = self.runs.iter_mut().find(|r| r.spec.seq_id == seq) else {
            return Vec::new();
        };
        let any_solo = run.lanes.iter().any(|l| l.solo);
        let mut out = Vec::new();
        for (lane, rt) in run.lanes.iter_mut().enumerate() {
            let silent = rt.mute || (any_solo && !rt.solo);
            if silent == rt.silent {
                continue;
            }
            rt.silent = silent;
            out.push(Action::LaneMuteChanged { seq, lane, silent });
            if silent && rt.state.owns_layer() {
                out.push(Action::Clear {
                    seq,
                    lane,
                    layer_id: run.spec.lanes[lane]
                        .layer_id
                        .expect("owning lane has a layer"),
                });
            }
            if !silent && matches!(rt.state, LaneState::Yielded { .. }) {
                rt.state = LaneState::Yielded {
                    resume_pending: true,
                };
                out.push(Action::LaneStateChanged {
                    seq,
                    lane,
                    state: rt.state,
                });
            }
        }
        out
    }

    /// Starts a checked spec now. Restarting a run that is linked to its `§` clip keeps that link
    /// (SPEC §7.1: ▶ in the app restarts it, clearing the SEQ layer still stops it).
    fn begin(&mut self, spec: RunSpec, origin: Origin) -> Vec<Action> {
        let pass_ticks = spec.pass_ticks();
        let seq = spec.seq_id;
        let origin = match (
            origin,
            self.runs
                .iter()
                .find(|r| r.spec.seq_id == seq)
                .map(|r| r.origin),
        ) {
            (Origin::Ui, Some(linked @ Origin::SeqClip { .. })) => linked,
            _ => origin,
        };
        let mut out = Vec::new();

        // Take over layers from running sequences (including an earlier run of this one).
        let new_layers: Vec<u64> = spec
            .lanes
            .iter()
            .filter(|l| l.is_active())
            .filter_map(|l| l.layer_id)
            .collect();
        let new_fade_layers: Vec<u64> = spec
            .lanes
            .iter()
            .filter(|l| l.is_active() && l.fades())
            .filter_map(|l| l.layer_id)
            .collect();
        for run in &mut self.runs {
            for (i, rt) in run.lanes.iter_mut().enumerate() {
                let lane = &run.spec.lanes[i];
                let Some(layer) = lane.layer_id else { continue };
                if rt.state.owns_layer() && new_layers.contains(&layer) {
                    rt.state = LaneState::TakenOver;
                    out.push(Action::LaneStateChanged {
                        seq: run.spec.seq_id,
                        lane: i,
                        state: rt.state,
                    });
                    // Taken by a lane without transitions: the original goes back first.
                    if lane.fades() && !new_fade_layers.contains(&layer) {
                        out.push(Action::TransitionReleased { layer_id: layer });
                    }
                }
            }
        }
        self.incoming = new_fade_layers;
        if let Some(i) = self.runs.iter().position(|r| r.spec.seq_id == seq) {
            out.extend(self.end_run(i, StopMode::AfterLastPass, EndReason::Restarted));
        }
        while let Some(i) = self
            .runs
            .iter()
            .position(|r| !r.lanes.iter().any(|l| l.state.owns_layer()))
        {
            out.extend(self.end_run(i, StopMode::Release, EndReason::TakenOver));
        }
        self.incoming.clear();

        let lanes = spec.lanes.iter().map(LaneRt::new).collect();
        let mut run = Run {
            spec,
            origin,
            pass: 1,
            pos: 0,
            pass_ticks,
            lanes,
            next_spec: None,
            elapsed: 0,
        };
        out.push(Action::Started { seq, origin });
        out.push(Action::PassStarted { seq, pass: 1 });
        for lane in 0..run.lanes.len() {
            if run.lanes[lane].state == LaneState::Playing {
                out.extend(step_actions(&mut run, lane, 0));
            }
        }
        self.runs.push(run);
        out
    }

    /// Stops a sequence: cancels its waiting start and ends its run.
    pub fn stop(&mut self, seq: Uuid, mode: StopMode) -> Vec<Action> {
        let mut out = self.cancel_pending(seq);
        if let Some(i) = self.runs.iter().position(|r| r.spec.seq_id == seq) {
            out.extend(self.end_run(i, mode, EndReason::Stopped));
            // A cue waiting for the end of this run's pass starts now (SPEC §3.3).
            if let Some(i) = self
                .pending
                .iter()
                .position(|p| p.wait == Wait::EndOfPass(seq))
            {
                let p = self.pending.remove(i);
                out.extend(self.begin_pending(p));
            }
        }
        out
    }

    /// Cancels every waiting start and ends every run.
    pub fn stop_all(&mut self, mode: StopMode) -> Vec<Action> {
        let mut out: Vec<Action> = self
            .pending
            .drain(..)
            .map(|p| Action::PendingCancelled {
                seq: p.spec.seq_id,
                origin: p.origin,
            })
            .collect();
        while !self.runs.is_empty() {
            out.extend(self.end_run(0, mode, EndReason::Stopped));
        }
        out
    }

    /// Manual override detected on a layer: its playing lane stops triggering but keeps
    /// counting (SPEC §7.3).
    pub fn yield_layer(&mut self, layer_id: u64) -> Vec<Action> {
        let mut out = Vec::new();
        for run in &mut self.runs {
            for (i, rt) in run.lanes.iter_mut().enumerate() {
                if rt.state == LaneState::Playing && run.spec.lanes[i].layer_id == Some(layer_id) {
                    rt.state = LaneState::Yielded {
                        resume_pending: false,
                    };
                    out.push(Action::LaneStateChanged {
                        seq: run.spec.seq_id,
                        lane: i,
                        state: rt.state,
                    });
                }
            }
        }
        out
    }

    /// "Resume lane": a yielded lane plays again from its next step boundary.
    pub fn resume_lane(&mut self, seq: Uuid, lane: usize) -> Vec<Action> {
        let Some(run) = self.runs.iter_mut().find(|r| r.spec.seq_id == seq) else {
            return Vec::new();
        };
        match run.lanes.get_mut(lane) {
            Some(rt) if matches!(rt.state, LaneState::Yielded { .. }) => {
                rt.state = LaneState::Yielded {
                    resume_pending: true,
                };
                vec![Action::LaneStateChanged {
                    seq,
                    lane,
                    state: rt.state,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// Slot lane steps that will start exactly `ahead` ticks from now (≥ 1), assuming no start,
    /// stop or yield happens meanwhile. The engine calls this on every tick with the same
    /// `ahead` to read each such slot shortly before its step (SPEC §4.5); each upcoming step
    /// is reported once. Only lanes that will trigger are included (playing, or yielded with a
    /// resume pending).
    pub fn slot_steps_ahead(&self, ahead: u64) -> Vec<UpcomingSlot> {
        let mut out = Vec::new();
        if ahead == 0 {
            return out;
        }
        for run in &self.runs {
            for lane in 0..run.lanes.len() {
                if !run.will_play(lane) {
                    continue;
                }
                let Some((step, pass)) = run.step_in(lane, ahead) else {
                    continue;
                };
                let Some(option) = run.next_option(lane, step, pass) else {
                    continue;
                };
                let spec = &run.spec.lanes[lane];
                if let (StepTarget::Slot(column), Some(layer_id)) =
                    (spec.steps[step].option(option), spec.layer_id)
                {
                    out.push(UpcomingSlot {
                        seq: run.spec.seq_id,
                        lane,
                        step,
                        layer_id,
                        column,
                    });
                }
            }
        }
        out
    }

    /// Recording (SPEC §7.9): from its next pass boundary the run plays `spec` (same sequence),
    /// replacing one queued before. Returns whether the sequence runs.
    pub fn set_next_pass(&mut self, spec: RunSpec) -> bool {
        match self.runs.iter_mut().find(|r| r.spec.seq_id == spec.seq_id) {
            Some(run) => {
                run.next_spec = Some(spec);
                true
            }
            None => false,
        }
    }

    /// Ticks into the current pass of a running sequence, and the pass length.
    pub fn position(&self, seq: Uuid) -> Option<(u64, u64)> {
        self.runs
            .iter()
            .find(|r| r.spec.seq_id == seq)
            .map(|r| (r.pos, r.pass_ticks))
    }

    /// Resync (SPEC §3.1): every running sequence jumps to the start of its current pass, now, as
    /// if it had just started: step 1 of each lane it still owns is emitted (a yielded lane with a
    /// resume pending resumes). The pass counter is kept; taken-over lanes stay silent.
    pub fn rewind_all(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        for run in &mut self.runs {
            run.pos = 0;
            for lane in 0..run.lanes.len() {
                // A transition sent ahead was for another step.
                run.lanes[lane].fade_sent = false;
                out.extend(boundary(run, lane, Some(0)));
            }
        }
        out
    }

    /// Advances the bar phase and every running sequence by one tick. Waiting starts whose
    /// boundary this tick is begin first (at position 0, taking over their layers before the
    /// other sequences act on this tick).
    pub fn on_tick(&mut self) -> Vec<Action> {
        self.phase = Some(self.phase.map_or(0, |p| p + 1));
        let mut out = Vec::new();
        let mut begun = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        let (due, waiting): (Vec<_>, Vec<_>) = pending.into_iter().partition(|p| self.due(p.wait));
        self.pending = waiting;
        for p in due {
            begun.push(p.spec.seq_id);
            out.extend(self.begin_pending(p));
        }
        let mut finished = Vec::new();
        for (ri, run) in self.runs.iter_mut().enumerate() {
            if begun.contains(&run.spec.seq_id) {
                continue; // at position 0 on this tick
            }
            run.pos += 1;
            run.elapsed += 1;
            let seq = run.spec.seq_id;
            if run.pos >= run.pass_ticks {
                run.swap_spec();
                if run.spec.passes().is_some_and(|n| run.pass >= n) {
                    finished.push(ri);
                    continue;
                }
                run.pass += 1;
                run.pos = 0;
                out.push(Action::PassStarted {
                    seq,
                    pass: run.pass,
                });
                for lane in 0..run.lanes.len() {
                    out.extend(boundary(run, lane, Some(0)));
                }
            } else {
                for lane in 0..run.lanes.len() {
                    let rt = &run.lanes[lane];
                    if !rt.state.owns_layer() {
                        continue;
                    }
                    let end = run.spec.lanes[lane].end;
                    let step = rt.step_starting_at(end, run.pos);
                    if step.is_some() {
                        out.extend(boundary(run, lane, step));
                    } else if end == LaneEnd::Clear
                        && run.pos == rt.len
                        && rt.state == LaneState::Playing
                        && !rt.silent
                    {
                        // Lane shorter than the pass ran out: clear and stay silent. With
                        // transitions the clear uses the original (SPEC §7.7).
                        let layer_id = run.spec.lanes[lane]
                            .layer_id
                            .expect("active lane has a layer");
                        if run.spec.lanes[lane].fades() {
                            let rt = &mut run.lanes[lane];
                            rt.step_sent_at = run.elapsed;
                            rt.fade_end = run.elapsed;
                            out.push(Action::SetTransition {
                                seq,
                                lane,
                                step: rt.starts.len() - 1,
                                layer_id,
                                fade: Fade::Original,
                            });
                        }
                        out.push(Action::Clear {
                            seq,
                            lane,
                            layer_id,
                        });
                    }
                }
            }
            out.extend(run.fades_ahead());
        }
        for ri in finished.into_iter().rev() {
            out.extend(self.end_run(ri, StopMode::AfterLastPass, EndReason::Finished));
        }
        out
    }

    /// A running lane with transitions owns this layer, or a beginning run will.
    fn fade_layer_held(&self, layer_id: u64) -> bool {
        self.incoming.contains(&layer_id)
            || self.runs.iter().any(|r| {
                r.lanes.iter().zip(&r.spec.lanes).any(|(rt, l)| {
                    rt.state.owns_layer() && l.layer_id == Some(layer_id) && l.fades()
                })
            })
    }

    fn end_run(&mut self, index: usize, mode: StopMode, reason: EndReason) -> Vec<Action> {
        let run = self.runs.remove(index);
        let seq = run.spec.seq_id;
        let mut out = Vec::new();
        // Originals go back before the clears (SPEC §7.7), unless another lane with
        // transitions holds the layer or is about to.
        for (rt, lane) in run.lanes.iter().zip(&run.spec.lanes) {
            let Some(layer_id) = lane.layer_id else {
                continue;
            };
            if rt.state.owns_layer() && lane.fades() && !self.fade_layer_held(layer_id) {
                out.push(Action::TransitionReleased { layer_id });
            }
        }
        for (i, rt) in run.lanes.iter().enumerate() {
            let clear = match mode {
                StopMode::AfterLastPass => {
                    run.spec.after_last_pass == AfterLastPass::Clear
                        && rt.state == LaneState::Playing
                }
                StopMode::Release => false,
                StopMode::Clear => rt.state.owns_layer(),
            };
            if clear {
                out.push(Action::Clear {
                    seq,
                    lane: i,
                    layer_id: run.spec.lanes[i].layer_id.expect("owning lane has a layer"),
                });
            }
        }
        out.push(Action::Ended {
            seq,
            reason,
            origin: run.origin,
        });
        out
    }
}

/// Step boundary on a lane: resumes a pending yielded lane, then emits the step.
fn boundary(run: &mut Run, lane: usize, step: Option<usize>) -> Vec<Action> {
    let Some(step) = step else { return Vec::new() };
    let rt = &mut run.lanes[lane];
    if !rt.state.owns_layer() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if rt.state
        == (LaneState::Yielded {
            resume_pending: true,
        })
    {
        rt.state = LaneState::Playing;
        out.push(Action::LaneStateChanged {
            seq: run.spec.seq_id,
            lane,
            state: rt.state,
        });
    }
    out.extend(step_actions(run, lane, step));
    out
}

/// A step begins: its condition and pick are decided (and counted) on every lane that owns its
/// layer, so a muted or yielded lane keeps its rotation in time; only a playing, unmuted lane sends.
fn step_actions(run: &mut Run, lane: usize, step: usize) -> Vec<Action> {
    let seq = run.spec.seq_id;
    let mut out = vec![Action::StepStarted { seq, lane, step }];
    let passes = run.spec.passes();
    let now = run.elapsed;
    let rs = &run.spec.lanes[lane].steps[step];
    let rt = &mut run.lanes[lane];
    let fade_sent = std::mem::take(&mut rt.fade_sent);
    if !plays_on(rs.when, run.pass, passes) {
        // Not on this pass: nothing is sent, the previous clip plays through (SPEC §6).
        rt.option = None;
        return out;
    }
    let option = pick_option(rs, run.spec.seed, lane, step, rt.plays[step]);
    rt.plays[step] += 1;
    rt.option = Some(option);
    if rt.state != LaneState::Playing || rt.silent {
        return out;
    }
    let target = rs.option(option);
    let layer_id = run.spec.lanes[lane]
        .layer_id
        .expect("playing lane has a layer");
    if let (Some(fade), false) = (&rs.fade, target == StepTarget::Unresolved) {
        // Not sent ahead (first step, a fade that ran until now, a lane just back): now, before
        // the trigger (a set in the same instant counts, notes §19).
        if !fade_sent {
            out.push(Action::SetTransition {
                seq,
                lane,
                step,
                layer_id,
                fade: fade.clone(),
            });
        }
        rt.step_sent_at = now;
        rt.fade_end = now + fade.ticks();
    }
    match target {
        StepTarget::Clip(clip_id) => out.push(Action::Trigger {
            seq,
            lane,
            step,
            clip_id,
            layer_id,
        }),
        StepTarget::Rest => out.push(Action::Clear {
            seq,
            lane,
            layer_id,
        }),
        StepTarget::Unresolved => {}
        StepTarget::Slot(column) => out.push(Action::TriggerSlot {
            seq,
            lane,
            step,
            column,
            layer_id,
        }),
    }
    out
}
