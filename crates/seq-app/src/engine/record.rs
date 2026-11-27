//! Recording from Resolume (SPEC §7.9, M25): the armed sequence loops while it records; clicks and
//! clears on Resolume's layers become a take per pass ([`seq_core::record`]). Each click updates the
//! spec the run plays from its next pass; at the pass boundary the take is written into the model as
//! one undoable edit.

use seq_core::index::ClipInfo;
use seq_core::model::Transition;
use seq_core::record::{apply_take, sixteenths, snap, Click, Hit, Take};
use seq_core::resolve::clip_ref_for;
use seq_core::{Repeat, TICKS_PER_SIXTEENTH};

use super::*;

/// A cut switch sends the old clip's `Disconnected` and the new one's `Connected` together (notes
/// §20): a `Disconnected` is a clear only if no `Connected` on its layer follows within this time.
const CLEAR_SETTLE: Duration = Duration::from_millis(20);
/// The app's own clear: its `Disconnected` may come one fade later; this much on top of the fade.
const OWN_CLEAR_SLACK: Duration = Duration::from_millis(300);

#[derive(Default)]
pub(super) struct Recorder {
    /// The armed sequence (one at a time).
    armed: Option<Uuid>,
    /// Its run is going and records.
    recording: bool,
    /// Pass being recorded (1-based, of the run).
    pass: u32,
    /// The clicks of this pass, and those snapped into the next one.
    clicks: Vec<Click>,
    carry: Vec<Click>,
    /// Last known position (ticks into the pass, pass length) while recording.
    pos: Option<(u64, u64)>,
    last_tick: Option<Instant>,
    /// `Disconnected`s that may be clears: (layer id, time, fade seconds).
    maybe_clears: Vec<(u64, Instant, f64)>,
    /// The app's own clears: layer id → (time, fade seconds).
    own_clears: HashMap<u64, (Instant, f64)>,
    /// Layers that already warned about an ambiguous name in this recording.
    warned: Vec<String>,
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

    /// The run of `seq` loops (SPEC §7.9).
    pub(super) fn loops(&self, seq: Uuid) -> bool {
        self.armed == Some(seq)
    }
}

impl Engine {
    /// Arm `seq` (or disarm with `None`).
    pub(super) fn set_armed(&mut self, seq: Option<Uuid>) {
        if self.rec.armed == seq {
            return;
        }
        if let Some(old) = self.rec.armed {
            if self.rec.recording {
                self.finish_take(old);
            }
            self.rec = Recorder::default();
            log::info!("recording disarmed");
        }
        let Some(seq) = seq else {
            self.runtime_dirty = true;
            return;
        };
        let live = self.index.as_ref().and_then(|i| i.live_deck.clone());
        let in_live = self
            .model
            .as_ref()
            .and_then(|m| m.find_sequence(seq))
            .is_some_and(|(deck, _)| Some(deck) == live.as_deref());
        if !in_live {
            self.notice("only a sequence of the live deck can record");
            self.runtime_dirty = true;
            return;
        }
        self.rec.armed = Some(seq);
        if self.scheduler.is_running(seq) {
            // Records from now; from its next pass it loops.
            self.rec.recording = true;
            self.rec.pass = self
                .scheduler
                .status()
                .iter()
                .find(|r| r.seq == seq)
                .map_or(1, |r| r.pass);
            self.rec.pos = self.scheduler.position(seq);
            self.queue_take(seq);
        }
        log::info!("recording armed");
        self.runtime_dirty = true;
    }

    /// Before each tick: remember where the recording run is (for the clicks of the next moment).
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
            self.rec.pos = self.scheduler.position(seq);
            log::info!("recording started");
            self.runtime_dirty = true;
        }
    }

    /// A pass of the recording run began: the take of the one before is written.
    pub(super) fn record_pass(&mut self, seq: Uuid, pass: u32) {
        if self.rec.armed != Some(seq) || !self.rec.recording || pass <= self.rec.pass {
            return;
        }
        let end = self.rec.pos.map_or(0, |(_, len)| len);
        self.commit_take(seq, end);
        self.rec.pass = pass;
        self.rec.clicks = std::mem::take(&mut self.rec.carry);
        self.rec.pos = self.scheduler.position(seq);
        if !self.rec.clicks.is_empty() {
            self.queue_take(seq);
        }
    }

    /// The recording run ended (Stop, Stop all, a deck change): the take so far is written.
    pub(super) fn record_ended(&mut self, seq: Uuid) {
        if self.rec.armed == Some(seq) {
            if self.rec.recording {
                self.finish_take(seq);
            }
            self.rec = Recorder::default();
            log::info!("recording ended");
            self.runtime_dirty = true;
        }
    }

    /// Writes what was recorded in this pass up to now.
    fn finish_take(&mut self, seq: Uuid) {
        let end = self.rec.pos.map_or(0, |(pos, _)| pos);
        self.commit_take(seq, end);
        self.rec.clicks.clear();
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
        self.rec.maybe_clears.retain(|(l, _, _)| *l != layer_id);
        let Some(seq) = self.rec.armed.filter(|_| self.rec.recording) else {
            return false;
        };
        let owner = self.scheduler.owner_of(layer_id);
        if owner.is_some_and(|(s, _)| s != seq) {
            return false; // another sequence's layer: its manual override, not ours
        }
        let fade = self.click_transition(clip, layer_id);
        let hit = Hit::Clip {
            clip: clip_ref_for(clip),
            column: clip.column as u32,
        };
        self.add_click(seq, layer_id, layer_name, at, 0.0, hit, fade);
        true
    }

    /// A clip connected on the layer: a `Disconnected` just before was a switch, not a clear.
    pub(super) fn record_connected(&mut self, layer_id: u64) {
        self.rec.maybe_clears.retain(|(l, _, _)| *l != layer_id);
    }

    pub(super) fn layer_of_clip(&self, clip_id: u64) -> Option<u64> {
        let index = self.index.as_ref()?;
        let clip = index.clip_by_id(clip_id)?;
        index.layer_by_index(clip.layer).map(|l| l.id)
    }

    /// A clip disconnected: perhaps a clear of its layer (checked after [`CLEAR_SETTLE`]).
    pub(super) fn record_disconnect(&mut self, layer_id: u64, at: Instant) {
        if !self.rec.recording {
            return;
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
        self.rec.maybe_clears.push((layer_id, at, fade));
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
            .partition(|(_, t, _)| now.saturating_duration_since(*t) >= CLEAR_SETTLE);
        self.rec.maybe_clears = wait;
        let Some(seq) = self.rec.armed.filter(|_| self.rec.recording) else {
            return;
        };
        for (layer_id, at, fade) in due {
            if self.layer_has_connected(layer_id) {
                continue;
            }
            if self
                .scheduler
                .owner_of(layer_id)
                .is_some_and(|(s, _)| s != seq)
            {
                continue;
            }
            let Some(name) = self.layer_name(layer_id) else {
                continue;
            };
            // The press was one fade before the `Disconnected` (notes §20).
            self.add_click(seq, layer_id, &name, at, fade, Hit::Rest, None);
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

    /// Places a click (or clear) that happened at `at − back_seconds` into this pass or the next.
    #[allow(clippy::too_many_arguments)]
    fn add_click(
        &mut self,
        seq: Uuid,
        layer_id: u64,
        layer_name: &str,
        at: Instant,
        back_seconds: f64,
        hit: Hit,
        transition: Option<Transition>,
    ) {
        let Some((pos, pass_ticks)) = self.rec.pos else {
            return; // before the first tick of the recording
        };
        if layer_name == self.cfg.seq_layer {
            return;
        }
        let tick = 60.0 / self.record_bpm() / 24.0;
        // Between two ticks: at most one tick past the last one (the clock may have stopped).
        let since = match self.rec.last_tick {
            Some(t) if at > t => (at.duration_since(t).as_secs_f64() / tick).min(1.0),
            _ => 0.0,
        };
        let ticks = pos as f64 + since - back_seconds / tick;
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
        };
        match snap(ticks, pass_ticks) {
            Some(s) => self.rec.clicks.push(click(s)),
            None => {
                self.rec.carry.push(click(0));
                return;
            }
        }
        // From its first click the lane yields to you and its layer gets its own transition back.
        if self
            .scheduler
            .owner_of(layer_id)
            .is_some_and(|(s, _)| s == seq)
        {
            let acts = self.scheduler.yield_layer(layer_id);
            self.execute(acts);
            if self.fades_held(layer_id) {
                self.release_transition(layer_id);
            }
        }
        self.queue_take(seq);
        self.runtime_dirty = true;
    }

    fn take_now(&self, end: u32) -> Take {
        Take {
            clicks: self.rec.clicks.clone(),
            end,
        }
    }

    /// The model's sequence with this pass's take applied, or `None` when there is nothing.
    fn sequence_with_take(
        &self,
        seq: Uuid,
        end: u32,
    ) -> Option<(String, Sequence, Vec<String>, usize)> {
        let model = self.model.as_ref()?;
        let (deck, sequence) = model.find_sequence(seq)?;
        let take = self.take_now(end);
        if take.clicks.is_empty() || end == 0 {
            return None;
        }
        let index = self.index.as_ref();
        let seq_layer = self.cfg.seq_layer.clone();
        let r = apply_take(sequence, &take, |name| {
            name != seq_layer
                && index.is_some_and(|i| i.layers.iter().filter(|l| l.name == name).count() == 1)
        });
        Some((deck.to_owned(), r.sequence, r.layers, r.steps))
    }

    /// The run plays this pass's take from its next pass on.
    fn queue_take(&mut self, seq: Uuid) {
        let Some((_, pass_ticks)) = self.rec.pos.or_else(|| self.scheduler.position(seq)) else {
            return;
        };
        let end = (pass_ticks / u64::from(TICKS_PER_SIXTEENTH)) as u32;
        let sequence = match self.sequence_with_take(seq, end) {
            Some((_, s, _, _)) => s,
            None => match self.model.as_ref().and_then(|m| m.find_sequence(seq)) {
                Some((_, s)) => s.clone(),
                None => return,
            },
        };
        let Some(index) = self.index.clone() else {
            return;
        };
        let resolution = resolve_sequence(&sequence, &index);
        let mut spec = RunSpec::new(&sequence, &resolution);
        spec.repeat = Repeat::Infinite;
        spec.seed = seq.as_u128() as u64 ^ u64::from(self.rec.pass);
        for (lane, run_lane) in sequence.lanes.iter().zip(spec.lanes.iter_mut()) {
            if lane.layer == self.cfg.seq_layer {
                run_lane.layer_id = None;
            }
        }
        self.scheduler.set_next_pass(spec);
    }

    /// Writes the take of the pass that ends at `end` ticks into the model (one undo step).
    fn commit_take(&mut self, seq: Uuid, end_ticks: u64) {
        let end = (end_ticks / u64::from(TICKS_PER_SIXTEENTH)) as u32;
        let Some((deck, sequence, layers, steps)) = self.sequence_with_take(seq, end) else {
            return;
        };
        let Some(model) = self.model.clone() else {
            return;
        };
        let mut new = model.clone();
        let Some(slot) = new
            .decks
            .get_mut(&deck)
            .and_then(|d| d.sequences.iter_mut().find(|s| s.id == seq))
        else {
            return;
        };
        let name = sequence.name.clone();
        *slot = sequence.clone();
        if let Err(errors) = seq_core::validate(&new) {
            let first = errors.first().map(ToString::to_string).unwrap_or_default();
            self.notice(format!("recording of {name:?} not saved: {first}"));
            return;
        }
        self.undo.push(model);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.set_model(new);
        // What runs now is what was recorded: no "edited" badge.
        self.snapshots.insert(seq, (deck, sequence));
        log::info!(
            "recorded pass {} of {name:?}: {} ({steps} step{})",
            self.rec.pass,
            layers.join(", "),
            if steps == 1 { "" } else { "s" }
        );
    }
}
