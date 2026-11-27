//! Recording from Resolume (SPEC §7.9, M25): the armed sequence loops while it records; clicks and
//! clears on Resolume's layers become a take per pass ([`seq_core::record`]), punched into the
//! sequence as they come (shown at once). The first click of a pass makes one undo step for the
//! whole pass. The run plays the result from its next pass on; the lanes keep playing meanwhile.

use seq_core::index::ClipInfo;
use seq_core::model::Transition;
use seq_core::record::{apply_take, sixteenths, snap, Click, Hit, Take};
use seq_core::resolve::clip_ref_for;
use seq_core::scheduler::TICKS_PER_BAR;
use seq_core::{Repeat, TICKS_PER_SIXTEENTH};

use super::*;

/// A cut switch sends the old clip's `Disconnected` and the new one's `Connected` together (notes
/// §20): a `Disconnected` is a clear only if no `Connected` on its layer follows within this time.
const CLEAR_SETTLE: Duration = Duration::from_millis(20);
/// The app's own clear: its `Disconnected` may come one fade later; this much on top of the fade.
const OWN_CLEAR_SLACK: Duration = Duration::from_millis(300);
/// Pattern lengths an empty sequence may record (bars).
pub(super) const MAX_PATTERN_BARS: u32 = 64;

/// A `Disconnected` that may be a clear, with where in the run it happened.
struct MaybeClear {
    layer_id: u64,
    seen: Instant,
    pass: u32,
    ticks: f64,
}

#[derive(Default)]
pub(super) struct Recorder {
    /// The armed sequence (one at a time).
    armed: Option<Uuid>,
    /// Pass length for a sequence without steps (SPEC §7.9), in ticks.
    min_ticks: u64,
    /// Its run is going and records.
    recording: bool,
    /// Pass being recorded (1-based, of the run).
    pass: u32,
    /// The clicks of this pass, and those snapped into the next one.
    clicks: Vec<Click>,
    carry: Vec<Click>,
    /// The sequence as it was before this pass's first click (the take is applied to it).
    base: Option<Sequence>,
    /// Last known position (ticks into the pass, pass length) while recording.
    pos: Option<(u64, u64)>,
    last_tick: Option<Instant>,
    maybe_clears: Vec<MaybeClear>,
    /// The app's own clears: layer id → (time, fade seconds).
    own_clears: HashMap<u64, (Instant, f64)>,
    /// Layers that already warned about an ambiguous name in this recording.
    warned: Vec<String>,
    /// Clips held by hand (Piano trigger style), by layer id: their lanes are quiet until the release.
    held: HashMap<u64, u64>,
}

impl Recorder {
    pub(super) fn view(&self) -> Option<RecordView> {
        self.armed.map(|seq| RecordView {
            seq,
            recording: self.recording,
            pass: self.pass,
            clicks: self.clicks.len(),
        })
    }

    /// The run of `seq` loops (SPEC §7.9), at least this long; `None` when not armed.
    pub(super) fn loops(&self, seq: Uuid) -> Option<u64> {
        (self.armed == Some(seq)).then_some(self.min_ticks)
    }
}

impl Engine {
    /// Arm `seq` (or disarm with `None`). `bars`: the pattern length for a sequence without steps.
    pub(super) fn set_armed(&mut self, seq: Option<Uuid>, bars: Option<u32>) {
        if self.rec.armed.is_some() && self.rec.armed != seq {
            self.end_recording();
            log::info!("recording disarmed");
        }
        let Some(seq) = seq else {
            self.runtime_dirty = true;
            return;
        };
        let live = self.index.as_ref().and_then(|i| i.live_deck.clone());
        let found = self
            .model
            .as_ref()
            .and_then(|m| m.find_sequence(seq))
            .filter(|(deck, _)| Some(*deck) == live.as_deref())
            .map(|(_, s)| s.lanes.iter().any(|l| !l.steps.is_empty()));
        let Some(has_steps) = found else {
            self.notice("only a sequence of the live deck can record");
            self.runtime_dirty = true;
            return;
        };
        let bars = bars.map(|b| b.clamp(1, MAX_PATTERN_BARS));
        if !has_steps && bars.is_none() {
            self.notice("recording: choose a pattern length for a sequence without steps");
            self.runtime_dirty = true;
            return;
        }
        self.rec.armed = Some(seq);
        self.rec.min_ticks = bars.map_or(0, |b| u64::from(b) * TICKS_PER_BAR);
        if self.scheduler.is_running(seq) && !self.rec.recording {
            // Records from now; from its next pass it loops.
            self.rec.recording = true;
            self.rec.pass = self
                .scheduler
                .status()
                .iter()
                .find(|r| r.seq == seq)
                .map_or(1, |r| r.pass);
            self.rec.pos = self.scheduler.position(seq);
            self.queue_next_pass(seq);
            self.phase_notice(seq);
        }
        log::info!("recording armed");
        self.runtime_dirty = true;
    }

    fn end_recording(&mut self) {
        self.log_take();
        let held: Vec<u64> = self.rec.held.keys().copied().collect();
        self.rec = Recorder::default();
        for layer_id in held {
            self.resume_held_lane(layer_id);
        }
        self.runtime_dirty = true;
    }

    /// A held clip was let go (or the recording ended): its lane plays again from its next step.
    fn resume_held_lane(&mut self, layer_id: u64) {
        if let Some((seq, lane)) = self.scheduler.owner_of(layer_id) {
            let acts = self.scheduler.resume_lane(seq, lane);
            self.execute(acts);
        }
    }

    /// After each tick: remember where the recording run is (for the clicks until the next one).
    pub(super) fn record_tick(&mut self, at: Instant) {
        let Some(seq) = self.rec.armed.filter(|_| self.rec.recording) else {
            return;
        };
        self.rec.pos = self.scheduler.position(seq);
        self.rec.last_tick = Some(at);
    }

    /// The armed sequence started: record from its first pass.
    pub(super) fn record_started(&mut self, seq: Uuid) {
        if self.rec.armed == Some(seq) {
            self.rec.recording = true;
            self.rec.pass = 1;
            self.rec.clicks.clear();
            self.rec.carry.clear();
            self.rec.base = None;
            self.rec.pos = self.scheduler.position(seq);
            log::info!("recording started");
            self.phase_notice(seq);
            self.runtime_dirty = true;
        }
    }

    /// Clicks are snapped to the run's grid: if it isn't lined up with the music, they land a
    /// sixteenth off (notes §17: Resolume reports no beat phase).
    fn phase_notice(&mut self, seq: Uuid) {
        let immediate = self
            .model
            .as_ref()
            .and_then(|m| m.find_sequence(seq))
            .is_some_and(|(_, s)| s.start == Start::Immediate);
        if !self.bar_synced {
            self.notice(if self.follows_resolume() {
                "recording: the grid isn't lined up with Resolume's beat; press Resync in Resolume on a bar's first beat, \
                 or clicks may land a sixteenth early or late"
            } else {
                "recording: the grid isn't lined up with the beat (no MIDI Start yet); clicks may land a sixteenth early or late"
            });
        } else if immediate {
            self.notice(
                "recording: this sequence starts \"now\", off the beat; a bar start lines its grid up with the music",
            );
        }
    }

    /// A pass of the recording run began: this pass's take is done (it is already in the model).
    pub(super) fn record_pass(&mut self, seq: Uuid, pass: u32) {
        if self.rec.armed != Some(seq) || !self.rec.recording || pass <= self.rec.pass {
            return;
        }
        self.log_take();
        // Clips still playing at the loop go on into this pass, tied (SPEC §7.9); the run already
        // plays them so (`queue_next_pass`).
        let runs_on = self.continuations_of_take(seq);
        self.rec.pass = pass;
        self.rec.base = None;
        self.rec.clicks = std::mem::take(&mut self.rec.carry);
        self.rec.maybe_clears.retain(|m| m.pass >= pass);
        self.rec.pos = self.scheduler.position(seq);
        for c in runs_on {
            if !self
                .rec
                .clicks
                .iter()
                .any(|o| o.layer == c.layer && o.at == 0)
            {
                self.rec.clicks.push(c);
            }
        }
        if !self.rec.clicks.is_empty() {
            self.punch_in(seq);
        }
    }

    /// The recording run ended (Stop, Stop all, a deck change): recording ends (and disarms).
    pub(super) fn record_ended(&mut self, seq: Uuid) {
        if self.rec.armed == Some(seq) {
            self.end_recording();
            log::info!("recording ended");
        }
    }

    /// Undo or redo while recording: the take so far is gone; new clicks start a new one.
    pub(super) fn record_undo(&mut self) {
        self.rec.base = None;
        self.rec.clicks.clear();
    }

    fn log_take(&self) {
        if self.rec.clicks.is_empty() {
            return;
        }
        let mut layers: Vec<&str> = Vec::new();
        for c in &self.rec.clicks {
            if !layers.contains(&c.layer.as_str()) {
                layers.push(&c.layer);
            }
        }
        let name = self
            .rec
            .armed
            .and_then(|s| {
                self.model
                    .as_ref()?
                    .find_sequence(s)
                    .map(|(_, q)| q.name.clone())
            })
            .unwrap_or_default();
        let n = self.rec.clicks.len();
        log::info!(
            "recorded pass {} of {name:?}: {} ({n} step{})",
            self.rec.pass,
            layers.join(", "),
            if n == 1 { "" } else { "s" }
        );
    }

    /// A clip connected that the app didn't trigger (SPEC §7.9). Returns true when it was recorded
    /// on a layer the recording sequence owns or no sequence owns (then it is no manual override).
    pub(super) fn record_click(
        &mut self,
        clip: &ClipInfo,
        layer_id: u64,
        layer_name: &str,
        at: Instant,
    ) -> bool {
        let Some(seq) = self.rec.armed.filter(|_| self.rec.recording) else {
            return false;
        };
        if self
            .scheduler
            .owner_of(layer_id)
            .is_some_and(|(s, _)| s != seq)
        {
            return false; // another sequence's layer: its manual override, not ours
        }
        let Some(ticks) = self.ticks_at(at, 0.0) else {
            return true;
        };
        let fade = self.click_transition(clip, layer_id);
        let held = clip.piano;
        let hit = Hit::Clip {
            clip: clip_ref_for(clip),
            column: clip.column as u32,
            held,
        };
        // The layer plays this now (not what the lane sent): a tied step continues only this clip.
        self.scheduler
            .layer_now_plays(layer_id, Some((clip.id, clip.column as u32)));
        if held {
            // While it is held, the lane must not trigger its own steps over it.
            self.rec.held.insert(layer_id, clip.id);
            if self
                .scheduler
                .owner_of(layer_id)
                .is_some_and(|(s, _)| s == seq)
            {
                let acts = self.scheduler.yield_layer(layer_id);
                self.execute(acts);
            }
        } else {
            self.rec.held.remove(&layer_id);
        }
        self.add_click(seq, layer_name, ticks, hit, fade);
        true
    }

    /// A clip connected on the layer: a `Disconnected` just before was a switch, not a clear.
    pub(super) fn record_connected(&mut self, layer_id: u64) {
        self.rec.maybe_clears.retain(|m| m.layer_id != layer_id);
    }

    pub(super) fn layer_of_clip(&self, clip_id: u64) -> Option<u64> {
        let index = self.index.as_ref()?;
        let clip = index.clip_by_id(clip_id)?;
        index.layer_by_index(clip.layer).map(|l| l.id)
    }

    /// A clip disconnected: perhaps a clear (or a Piano release) of its layer, checked after
    /// [`CLEAR_SETTLE`]. Its place in the run is taken now, one fade earlier (notes §20).
    pub(super) fn record_disconnect(&mut self, clip_id: u64, layer_id: u64, at: Instant) {
        if !self.rec.recording {
            return;
        }
        // A held clip let go: its lane plays again from its next step.
        if self.rec.held.get(&layer_id) == Some(&clip_id) {
            self.rec.held.remove(&layer_id);
            self.resume_held_lane(layer_id);
            if let Some(seq) = self.rec.armed {
                self.queue_next_pass(seq);
            }
        }
        // The end of a fade: another clip of the layer plays.
        if self.layer_has_connected(layer_id) {
            return;
        }
        let fade = self.fades_layer_seconds(layer_id);
        if let Some((t, f)) = self.rec.own_clears.get(&layer_id) {
            let window = Duration::from_secs_f64(f.max(fade)) + OWN_CLEAR_SLACK;
            if at.saturating_duration_since(*t) < window {
                return;
            }
        }
        if let Some(ticks) = self.ticks_at(at, fade) {
            self.rec.maybe_clears.push(MaybeClear {
                layer_id,
                seen: at,
                pass: self.rec.pass,
                ticks,
            });
        }
    }

    /// The app clears a layer itself.
    pub(super) fn record_own_clear(&mut self, layer_id: u64, at: Instant) {
        if self.rec.recording {
            let fade = self.fades_layer_seconds(layer_id);
            self.rec.own_clears.insert(layer_id, (at, fade));
        }
    }

    /// Settled clears become rests (from the timer).
    pub(super) fn record_settle(&mut self, now: Instant) {
        if self.rec.maybe_clears.is_empty() {
            return;
        }
        let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.rec.maybe_clears)
            .into_iter()
            .partition(|m| now.saturating_duration_since(m.seen) >= CLEAR_SETTLE);
        self.rec.maybe_clears = wait;
        let Some(seq) = self.rec.armed.filter(|_| self.rec.recording) else {
            return;
        };
        for m in due {
            if m.pass != self.rec.pass || self.layer_has_connected(m.layer_id) {
                continue;
            }
            if self
                .scheduler
                .owner_of(m.layer_id)
                .is_some_and(|(s, _)| s != seq)
            {
                continue;
            }
            let Some(name) = self.layer_name(m.layer_id) else {
                continue;
            };
            self.scheduler.layer_now_plays(m.layer_id, None);
            self.add_click(seq, &name, m.ticks, Hit::Rest, None);
        }
    }

    fn layer_has_connected(&self, layer_id: u64) -> bool {
        let Some(index) = &self.index else {
            return false;
        };
        let Some(layer) = index.layers.iter().find(|l| l.id == layer_id) else {
            return false;
        };
        index
            .clips()
            .iter()
            .filter(|c| c.layer == layer.index)
            .any(|c| self.states.get(&c.id).is_some_and(|s| s.is_connected()))
    }

    fn layer_name(&self, layer_id: u64) -> Option<String> {
        let index = self.index.as_ref()?;
        Some(index.layers.iter().find(|l| l.id == layer_id)?.name.clone())
    }

    fn fades_layer_seconds(&self, layer_id: u64) -> f64 {
        self.fades_current(layer_id).map_or(0.0, |(s, _)| s)
    }

    /// The transition Resolume used for a click on `clip` (SPEC §7.9), in sixteenths at the tempo.
    fn click_transition(&self, clip: &ClipInfo, layer_id: u64) -> Option<Transition> {
        let layer = self.fades_current(layer_id);
        let (seconds, blend) = match &clip.own_transition {
            Some(own) if !clip.layer_determined => {
                let blend = if own.blend.is_empty() || own.blend == "Layer Determined" {
                    layer.as_ref().map(|(_, b)| b.clone()).unwrap_or_default()
                } else {
                    own.blend.clone()
                };
                (f64::from(own.millis) / 1000.0, blend)
            }
            _ => layer?,
        };
        let length = sixteenths(seconds, self.record_bpm());
        Some(Transition {
            length,
            blend: (length > 0 && !blend.is_empty()).then_some(blend),
        })
    }

    fn record_bpm(&self) -> f64 {
        self.tempo
            .bpm()
            .or(self.tempo_ctl.as_ref().map(|t| t.bpm))
            .filter(|b| *b > 0.0)
            .unwrap_or(120.0)
    }

    /// Ticks into the pass at `at − back_seconds`: the last tick's position plus the time since it
    /// (at most one tick: the clock may have stopped).
    fn ticks_at(&self, at: Instant, back_seconds: f64) -> Option<f64> {
        let (pos, _) = self.rec.pos?;
        let tick = 60.0 / self.record_bpm() / 24.0;
        let since = match self.rec.last_tick {
            Some(t) if at > t => (at.duration_since(t).as_secs_f64() / tick).min(1.0),
            _ => 0.0,
        };
        Some(pos as f64 + since - back_seconds / tick)
    }

    /// Places a click (or clear) at `ticks` into this pass or the start of the next.
    fn add_click(
        &mut self,
        seq: Uuid,
        layer_name: &str,
        ticks: f64,
        hit: Hit,
        transition: Option<Transition>,
    ) {
        if layer_name == self.cfg.seq_layer {
            return;
        }
        let Some((_, pass_ticks)) = self.rec.pos else {
            return;
        };
        let unique = self
            .index
            .as_ref()
            .is_some_and(|i| i.layers.iter().filter(|l| l.name == layer_name).count() == 1);
        let has_lane = self
            .model
            .as_ref()
            .and_then(|m| m.find_sequence(seq))
            .is_some_and(|(_, s)| s.lanes.iter().any(|l| l.layer == layer_name));
        if !unique && !has_lane {
            if !self.rec.warned.iter().any(|w| w == layer_name) {
                self.rec.warned.push(layer_name.to_owned());
                self.notice(format!(
                    "recording: layer {layer_name:?} is not unique; give it a name in Resolume to record it"
                ));
            }
            return;
        }
        let click = |at16| Click {
            layer: layer_name.to_owned(),
            at: at16,
            hit: hit.clone(),
            transition: transition.clone(),
            tied: false,
        };
        match snap(ticks, pass_ticks) {
            Some(s) => {
                self.rec.clicks.push(click(s));
                self.punch_in(seq);
            }
            None => self.rec.carry.push(click(0)),
        }
        self.runtime_dirty = true;
    }

    /// Writes this pass's take into the model now (the first click of a pass makes the undo step)
    /// and lets the run play it from its next pass.
    fn punch_in(&mut self, seq: Uuid) {
        let Some((_, pass_ticks)) = self.rec.pos.or_else(|| self.scheduler.position(seq)) else {
            return;
        };
        let Some(model) = self.model.clone() else {
            return;
        };
        let Some((deck, current)) = model
            .find_sequence(seq)
            .map(|(d, s)| (d.to_owned(), s.clone()))
        else {
            return;
        };
        let first = self.rec.base.is_none();
        let base = self.rec.base.get_or_insert(current).clone();
        let take = Take {
            clicks: self.rec.clicks.clone(),
            end: (pass_ticks / u64::from(TICKS_PER_SIXTEENTH)) as u32,
        };
        let index = self.index.as_ref();
        let seq_layer = self.cfg.seq_layer.clone();
        let result = apply_take(&base, &take, |name| {
            name != seq_layer
                && index.is_some_and(|i| i.layers.iter().filter(|l| l.name == name).count() == 1)
        });
        let mut new = model.clone();
        let Some(slot) = new
            .decks
            .get_mut(&deck)
            .and_then(|d| d.sequences.iter_mut().find(|s| s.id == seq))
        else {
            return;
        };
        *slot = result.sequence.clone();
        if let Err(errors) = seq_core::validate(&new) {
            let first_error = errors.first().map(ToString::to_string).unwrap_or_default();
            self.notice(format!("recording not saved: {first_error}"));
            self.rec.clicks.pop();
            return;
        }
        if first {
            self.undo.push(model);
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.set_model(new);
        // What runs is what was recorded: no "edited" badge.
        self.snapshots.insert(seq, (deck, result.sequence));
        self.queue_next_pass(seq);
    }

    /// Clips of this pass's take that still play at its end and go on into the next pass (only
    /// those still connected now).
    fn continuations_of_take(&self, seq: Uuid) -> Vec<Click> {
        let Some((_, pass_ticks)) = self.rec.pos else {
            return Vec::new();
        };
        let Some(base) = self.rec.base.clone().or_else(|| {
            self.model
                .as_ref()
                .and_then(|m| m.find_sequence(seq))
                .map(|(_, s)| s.clone())
        }) else {
            return Vec::new();
        };
        let take = Take {
            clicks: self.rec.clicks.clone(),
            end: (pass_ticks / u64::from(TICKS_PER_SIXTEENTH)) as u32,
        };
        let playing = |c: &Click| {
            let Hit::Clip { clip, .. } = &c.hit else {
                return false;
            };
            let Some(index) = &self.index else {
                return false;
            };
            index
                .clips()
                .iter()
                .filter(|i| {
                    i.id == clip.id.unwrap_or_default() || Some(&i.name) == clip.name.as_ref()
                })
                .any(|i| self.states.get(&i.id).is_some_and(|s| s.is_connected()))
        };
        seq_core::record::continuations(&base, &take)
            .into_iter()
            .filter(playing)
            .collect()
    }

    /// The run plays the model's sequence from its next pass on, looping; clips that would still
    /// play at the loop go on there, tied (SPEC §7.9).
    fn queue_next_pass(&mut self, seq: Uuid) {
        let Some(mut sequence) = self
            .model
            .as_ref()
            .and_then(|m| m.find_sequence(seq))
            .map(|(_, s)| s.clone())
        else {
            return;
        };
        if let Some((_, pass_ticks)) = self.rec.pos {
            let base = self.rec.base.clone().unwrap_or_else(|| sequence.clone());
            let take = Take {
                clicks: self.rec.clicks.clone(),
                end: (pass_ticks / u64::from(TICKS_PER_SIXTEENTH)) as u32,
            };
            let runs_on = seq_core::record::continuations(&base, &take);
            if !runs_on.is_empty() {
                let next = Take {
                    clicks: runs_on,
                    end: take.end,
                };
                sequence = apply_take(&sequence, &next, |_| true).sequence;
            }
        }
        let Some(index) = self.index.clone() else {
            return;
        };
        let resolution = resolve_sequence(&sequence, &index);
        let mut spec = RunSpec::new(&sequence, &resolution);
        spec.repeat = Repeat::Infinite;
        spec.min_ticks = self.rec.min_ticks;
        spec.yielded_layers = self.rec.held.keys().copied().collect();
        spec.seed = seq.as_u128() as u64 ^ u64::from(self.rec.pass);
        for (lane, run_lane) in sequence.lanes.iter().zip(spec.lanes.iter_mut()) {
            if lane.layer == self.cfg.seq_layer {
                run_lane.layer_id = None;
            }
        }
        self.scheduler.set_next_pass(spec);
    }
}
