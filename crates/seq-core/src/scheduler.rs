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

use uuid::Uuid;

use crate::model::{AfterLastPass, LaneEnd, Repeat, Sequence, Start, TICKS_PER_SIXTEENTH};
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
    pub target: StepTarget,
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
}

/// Snapshot of a sequence taken at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    pub seq_id: Uuid,
    pub name: String,
    pub repeat: Repeat,
    pub after_last_pass: AfterLastPass,
    pub lanes: Vec<RunLane>,
}

impl RunSpec {
    /// Builds the snapshot from a sequence and its resolution against the live deck.
    pub fn new(seq: &Sequence, resolution: &SequenceResolution) -> Self {
        let lanes = seq
            .lanes
            .iter()
            .zip(&resolution.lanes)
            .map(|(lane, res)| RunLane {
                layer_id: match res.layer {
                    LayerResolution::Found { id, .. } => Some(id),
                    _ => None,
                },
                end: lane.end,
                steps: lane
                    .steps
                    .iter()
                    .zip(&res.steps)
                    .map(|(step, sr)| RunStep {
                        ticks: u64::from(step.length) * u64::from(TICKS_PER_SIXTEENTH),
                        target: match sr {
                            StepResolution::Rest => StepTarget::Rest,
                            StepResolution::Resolved { clip, .. } => StepTarget::Clip(clip.clip_id),
                            StepResolution::Unresolved { .. } => StepTarget::Unresolved,
                            StepResolution::Slot { column, .. } => StepTarget::Slot(*column),
                        },
                    })
                    .collect(),
            })
            .collect();
        Self {
            seq_id: seq.id,
            name: seq.name.clone(),
            repeat: seq.repeat,
            after_last_pass: seq.after_last_pass,
            lanes,
        }
    }

    /// One pass = the longest lane (SPEC §5), counting silent lanes too.
    pub fn pass_ticks(&self) -> u64 {
        self.lanes.iter().map(RunLane::ticks).max().unwrap_or(0)
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
    /// Start position of each step within the lane.
    starts: Vec<u64>,
    len: u64,
}

impl LaneRt {
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
        if !spec.lanes.iter().any(RunLane::is_active) {
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
        if !spec.lanes.iter().any(RunLane::is_active) {
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

    /// Starts a checked spec now.
    fn begin(&mut self, spec: RunSpec, origin: Origin) -> Vec<Action> {
        let pass_ticks = spec.pass_ticks();
        let seq = spec.seq_id;
        let mut out = Vec::new();

        // Take over layers from running sequences (including an earlier run of this one).
        let new_layers: Vec<u64> = spec
            .lanes
            .iter()
            .filter(|l| l.is_active())
            .filter_map(|l| l.layer_id)
            .collect();
        for run in &mut self.runs {
            for (i, rt) in run.lanes.iter_mut().enumerate() {
                let layer = run.spec.lanes[i].layer_id;
                if rt.state.owns_layer() && layer.is_some_and(|l| new_layers.contains(&l)) {
                    rt.state = LaneState::TakenOver;
                    out.push(Action::LaneStateChanged {
                        seq: run.spec.seq_id,
                        lane: i,
                        state: rt.state,
                    });
                }
            }
        }
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

        let lanes = spec
            .lanes
            .iter()
            .map(|l| {
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
                    starts,
                    len: at,
                }
            })
            .collect();
        let run = Run {
            spec,
            origin,
            pass: 1,
            pos: 0,
            pass_ticks,
            lanes,
        };
        out.push(Action::Started { seq, origin });
        out.push(Action::PassStarted { seq, pass: 1 });
        for lane in 0..run.lanes.len() {
            if run.lanes[lane].state == LaneState::Playing {
                out.extend(step_actions(&run, lane, 0));
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
            let mut at = run.pos + ahead;
            if at >= run.pass_ticks {
                let passes_left = run
                    .spec
                    .passes()
                    .map_or(u64::MAX, |n| u64::from(n.saturating_sub(run.pass)));
                let wraps = at / run.pass_ticks;
                if wraps > passes_left {
                    continue;
                }
                at %= run.pass_ticks;
            }
            for (lane, rt) in run.lanes.iter().enumerate() {
                let will_play = matches!(
                    rt.state,
                    LaneState::Playing
                        | LaneState::Yielded {
                            resume_pending: true
                        }
                );
                if !will_play || rt.silent {
                    continue;
                }
                let spec = &run.spec.lanes[lane];
                let Some(step) = rt.step_starting_at(spec.end, at) else {
                    continue;
                };
                if let (StepTarget::Slot(column), Some(layer_id)) =
                    (spec.steps[step].target, spec.layer_id)
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

    /// Resync (SPEC §3.1): every running sequence jumps to the start of its current pass, now, as
    /// if it had just started: step 1 of each lane it still owns is emitted (a yielded lane with a
    /// resume pending resumes). The pass counter is kept; taken-over lanes stay silent.
    pub fn rewind_all(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        for run in &mut self.runs {
            run.pos = 0;
            for lane in 0..run.lanes.len() {
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
            let seq = run.spec.seq_id;
            if run.pos >= run.pass_ticks {
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
                continue;
            }
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
                    // Lane shorter than the pass ran out: clear and stay silent.
                    out.push(Action::Clear {
                        seq,
                        lane,
                        layer_id: run.spec.lanes[lane]
                            .layer_id
                            .expect("active lane has a layer"),
                    });
                }
            }
        }
        for ri in finished.into_iter().rev() {
            out.extend(self.end_run(ri, StopMode::AfterLastPass, EndReason::Finished));
        }
        out
    }

    fn end_run(&mut self, index: usize, mode: StopMode, reason: EndReason) -> Vec<Action> {
        let run = self.runs.remove(index);
        let seq = run.spec.seq_id;
        let mut out = Vec::new();
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

fn step_actions(run: &Run, lane: usize, step: usize) -> Vec<Action> {
    let seq = run.spec.seq_id;
    let mut out = vec![Action::StepStarted { seq, lane, step }];
    if run.lanes[lane].state != LaneState::Playing || run.lanes[lane].silent {
        return out;
    }
    let layer_id = run.spec.lanes[lane]
        .layer_id
        .expect("playing lane has a layer");
    match run.spec.lanes[lane].steps[step].target {
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
