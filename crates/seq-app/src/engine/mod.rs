//! The engine task (SPEC §10): owns the scheduler, the composition index and the sequence model.
//!
//! It reacts to clock events, WebSocket events, UI commands and worker results, sends OSC
//! directly, and publishes UI state on watch channels. It never awaits HTTP or disk: those run
//! in worker tasks that report back through [`Aux`] messages.

pub mod view;

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::time::{Duration, Instant};

use clock_in::{ClockEvent, ClockMsg, TempoEstimator};
use resolume_client::composition::CompositionSnapshot;
use resolume_client::osc::OscSender;
use resolume_client::ws::{ParamMsg, WsCommand, WsEvent};
use seq_core::edit::EditOp;
use seq_core::index::{ClipState, CompositionIndex, LayerLookup};
use seq_core::resolve::{refresh_ids, resolve_sequence, SequenceResolution};
use seq_core::scheduler::{
    bar_beat, Action, EndReason, LaneState, Origin, RunSpec, Scheduler, StopMode,
};
use seq_core::{SeqFile, Sequence};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::saver::SaverHandle;
use crate::storage::{Storage, StorageError};
use view::*;

/// Commands from the UI (and the CLI).
#[derive(Debug, Clone)]
pub enum EngineCommand {
    Start {
        seq: Uuid,
    },
    /// Start a sequence of the live deck by name (CLI `--start`). Waits until Resolume and the
    /// sequence file are loaded.
    StartByName {
        name: String,
    },
    Stop {
        seq: Uuid,
    },
    StopAll,
    ResumeLane {
        seq: Uuid,
        lane: usize,
    },
    /// Apply an edit operation to the stored model (SPEC §9.6).
    Edit {
        op: EditOp,
    },
    Undo,
    Redo,
    /// A message for the status bar (from workers such as the UI server).
    Notice(String),
}

/// Results from worker tasks.
#[derive(Debug)]
pub enum Aux {
    ModelLoaded {
        composition: String,
        result: Result<SeqFile, String>,
    },
    RestHealth(bool),
    /// Clip indexes of decks seen live in earlier sessions (for this composition).
    DeckCacheLoaded {
        composition: String,
        decks: Vec<CompositionIndex>,
    },
    Saved,
    SaveFailed(String),
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub seq_layer: String,
    pub seq_prefix: String,
    pub confirm_timeout: Duration,
    pub override_window: Duration,
    pub switch_timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            seq_layer: "SEQ".into(),
            seq_prefix: "§ ".into(),
            confirm_timeout: Duration::from_millis(250),
            override_window: Duration::from_millis(100),
            switch_timeout: Duration::from_secs(2),
        }
    }
}

/// What a parameter id we subscribed to stands for.
#[derive(Debug, Clone, Copy)]
enum ParamTarget {
    ClipConnected(u64),
    ClipName(u64),
    LayerName(u64),
}

struct Pending {
    seq: Uuid,
    lane: usize,
    step: usize,
    clip_id: u64,
    sent: Instant,
}

struct Switch {
    seq: Uuid,
    deck: String,
    deadline: Instant,
}

pub struct Outputs {
    pub structure: watch::Receiver<StructureState>,
    pub runtime: watch::Receiver<RuntimeState>,
}

pub struct Engine {
    cfg: EngineConfig,
    osc: OscSender,
    ws_cmd: mpsc::UnboundedSender<WsCommand>,
    saver: Option<SaverHandle>,
    storage: Storage,
    aux_tx: mpsc::UnboundedSender<Aux>,

    scheduler: Scheduler,
    index: Option<CompositionIndex>,
    states: HashMap<u64, ClipState>,
    deck_cache: HashMap<String, CompositionIndex>,
    params: HashMap<u64, ParamTarget>,
    /// Subscriptions requested but not yet acknowledged by Resolume.
    unacked: std::collections::HashSet<u64>,

    composition: Option<String>,
    model: Option<SeqFile>,
    model_error: Option<String>,
    /// Snapshots of running sequences (for "edited · applies at next start") and their deck.
    snapshots: HashMap<Uuid, (String, Sequence)>,
    undo: Vec<SeqFile>,
    redo: Vec<SeqFile>,

    last_layer_trigger: HashMap<u64, Instant>,
    pending: Vec<Pending>,
    not_confirmed: NotConfirmed,
    switch: Option<Switch>,
    deferred_start: Option<String>,

    tempo: TempoEstimator,
    /// BPM as last shown (tenths), to republish when the display changes.
    shown_bpm: Option<i64>,
    clock_port: Option<String>,
    clock_connected: bool,
    clock_running: bool,
    ws_connected: bool,
    rest_ok: bool,
    osc_ok: bool,
    last_osc: Option<OscView>,
    last_confirm: Option<String>,
    notices: VecDeque<String>,
    saved: u64,
    save_error: Option<String>,

    structure_dirty: bool,
    runtime_dirty: bool,
    last_runtime_publish: Instant,
    structure_tx: watch::Sender<StructureState>,
    runtime_tx: watch::Sender<RuntimeState>,
}

const MAX_NOTICES: usize = 8;
const MAX_UNDO: usize = 200;
const RUNTIME_PERIOD: Duration = Duration::from_millis(33);

impl Engine {
    pub fn new(
        cfg: EngineConfig,
        osc: OscSender,
        ws_cmd: mpsc::UnboundedSender<WsCommand>,
        saver: Option<SaverHandle>,
        storage: Storage,
        aux_tx: mpsc::UnboundedSender<Aux>,
    ) -> (Self, Outputs) {
        let (structure_tx, structure) = watch::channel(StructureState::default());
        let (runtime_tx, runtime) = watch::channel(RuntimeState::default());
        let engine = Self {
            cfg,
            osc,
            ws_cmd,
            saver,
            storage,
            aux_tx,
            scheduler: Scheduler::new(),
            index: None,
            states: HashMap::new(),
            deck_cache: HashMap::new(),
            params: HashMap::new(),
            unacked: std::collections::HashSet::new(),
            composition: None,
            model: None,
            model_error: None,
            snapshots: HashMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_layer_trigger: HashMap::new(),
            pending: Vec::new(),
            not_confirmed: NotConfirmed::new(),
            switch: None,
            deferred_start: None,
            tempo: TempoEstimator::new(),
            shown_bpm: None,
            clock_port: None,
            clock_connected: false,
            clock_running: false,
            ws_connected: false,
            rest_ok: false,
            osc_ok: true,
            last_osc: None,
            last_confirm: None,
            notices: VecDeque::new(),
            saved: 0,
            save_error: None,
            structure_dirty: true,
            runtime_dirty: true,
            last_runtime_publish: Instant::now(),
            structure_tx,
            runtime_tx,
        };
        (engine, Outputs { structure, runtime })
    }

    pub fn set_clock_port(&mut self, port: Option<String>) {
        self.clock_port = port;
    }

    fn notice(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::info!("{text}");
        self.notices.push_back(text);
        while self.notices.len() > MAX_NOTICES {
            self.notices.pop_front();
        }
        self.runtime_dirty = true;
    }

    // ---- clock ----

    pub fn on_clock(&mut self, ev: ClockEvent) {
        match ev {
            ClockEvent::Msg {
                msg: ClockMsg::Tick,
                at,
            } => {
                self.tempo.on_tick(at);
                if !self.clock_running {
                    self.clock_running = true;
                    self.notice("clock running");
                }
                let actions = self.scheduler.on_tick();
                self.execute(actions);
                if !self.snapshots.is_empty() {
                    self.runtime_dirty = true;
                }
            }
            ClockEvent::Msg { msg, .. } => log::info!("MIDI {msg:?} (ignored by sequences)"),
            ClockEvent::Connected { port } => {
                self.clock_connected = true;
                self.clock_port = Some(port);
                self.runtime_dirty = true;
            }
            ClockEvent::Disconnected { reason } => {
                if self.clock_connected {
                    self.notice(format!("MIDI clock port lost: {reason}"));
                }
                self.clock_connected = false;
                self.runtime_dirty = true;
            }
        }
    }

    // ---- WebSocket ----

    pub fn on_ws(&mut self, ev: WsEvent) {
        match ev {
            WsEvent::Connected => {
                // The WS task re-sends every subscription after a reconnect.
                self.unacked = self.params.keys().copied().collect();
                self.ws_connected = true;
                self.runtime_dirty = true;
            }
            WsEvent::Disconnected { .. } => {
                if self.ws_connected {
                    self.notice("Resolume WebSocket disconnected");
                }
                self.ws_connected = false;
                self.runtime_dirty = true;
            }
            WsEvent::Composition { snapshot, .. } => self.on_composition(*snapshot),
            WsEvent::Param {
                kind,
                id,
                value,
                at,
                ..
            } => self.on_param(kind, id, &value, at),
            WsEvent::Thumbnail {
                clip_id,
                last_update,
                is_default,
            } => {
                if clip_id != 0 {
                    self.patch_thumbnail(clip_id, last_update, is_default);
                }
            }
            WsEvent::Error { message } => log::debug!("Resolume WS error reply: {message}"),
        }
    }

    fn patch_thumbnail(&mut self, clip_id: u64, last_update: String, is_default: bool) {
        let Some(index) = &self.index else { return };
        let Some(mut clip) = index.clip_by_id(clip_id).cloned() else {
            return;
        };
        if clip.thumbnail.last_update == last_update {
            return;
        }
        clip.thumbnail.last_update = last_update;
        clip.thumbnail.is_default = is_default;
        self.replace_clip(clip);
    }

    fn replace_clip(&mut self, clip: seq_core::index::ClipInfo) {
        let Some(index) = self.index.take() else {
            return;
        };
        let mut clips = index.clips().to_vec();
        if let Some(c) = clips.iter_mut().find(|c| c.id == clip.id) {
            *c = clip;
        }
        let new = CompositionIndex::new(
            index.composition,
            index.decks,
            index.live_deck,
            index.layers,
            index.columns,
            clips,
        );
        if let Some(deck) = &new.live_deck {
            self.deck_cache.insert(deck.clone(), new.clone());
        }
        self.index = Some(new);
        self.structure_dirty = true;
    }

    fn on_composition(&mut self, snap: CompositionSnapshot) {
        let CompositionSnapshot { index, states } = snap;
        let old_live = self.index.as_ref().and_then(|i| i.live_deck.clone());
        let new_live = index.live_deck.clone();
        let structural = self.index.as_ref() != Some(&index);
        self.states = states;

        if self.composition.as_deref() != Some(index.composition.as_str()) {
            self.change_composition(index.composition.clone());
        }
        if let Some(deck) = &new_live {
            self.deck_cache.insert(deck.clone(), index.clone());
            if structural {
                self.persist_deck(&index);
            }
        }
        if structural {
            self.rebuild_params(&index);
            self.structure_dirty = true;
        }
        self.index = Some(index);

        if old_live.is_some() && old_live != new_live {
            let ours = self
                .switch
                .as_ref()
                .is_some_and(|s| Some(&s.deck) == new_live.as_ref());
            if ours {
                let sw = self.switch.take().expect("checked");
                self.start_now(sw.seq, Origin::Ui);
            } else {
                if !self.snapshots.is_empty() {
                    self.notice(format!(
                        "deck changed in Resolume to {:?}: all sequences stopped",
                        new_live.as_deref().unwrap_or("?")
                    ));
                }
                let acts = self.scheduler.stop_all(StopMode::Release);
                self.execute(acts);
            }
        }
        self.try_deferred_start();
    }

    fn change_composition(&mut self, name: String) {
        log::info!("composition {name:?}");
        let acts = self.scheduler.stop_all(StopMode::Release);
        self.execute(acts);
        self.composition = Some(name.clone());
        self.model = None;
        self.model_error = None;
        self.undo.clear();
        self.redo.clear();
        self.deck_cache.clear();
        let decks_dir = self.decks_dir(&name);
        let tx = self.aux_tx.clone();
        let composition = name.clone();
        tokio::task::spawn_blocking(move || {
            let decks = std::fs::read_dir(&decks_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| serde_json::from_str::<CompositionIndex>(&t).ok())
                .collect();
            let _ = tx.send(Aux::DeckCacheLoaded { composition, decks });
        });
        let storage = self.storage.clone();
        let tx = self.aux_tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = storage
                .load(&name)
                .map(|l| l.into_file())
                .map_err(|e: StorageError| e.to_string());
            let _ = tx.send(Aux::ModelLoaded {
                composition: name,
                result,
            });
        });
        self.structure_dirty = true;
    }

    fn rebuild_params(&mut self, index: &CompositionIndex) {
        self.params.clear();
        for c in index.clips() {
            self.params
                .insert(c.connected_param, ParamTarget::ClipConnected(c.id));
            self.params
                .insert(c.name_param, ParamTarget::ClipName(c.id));
        }
        for l in &index.layers {
            self.params
                .insert(l.name_param, ParamTarget::LayerName(l.id));
        }
        self.params.remove(&0);
        let ids: BTreeSet<u64> = self.params.keys().copied().collect();
        self.unacked.retain(|id| ids.contains(id));
        self.unacked.extend(ids.iter().copied());
        self.runtime_dirty = true;
        let _ = self.ws_cmd.send(WsCommand::SetSubscriptions(ids));
    }

    fn on_param(&mut self, kind: ParamMsg, id: u64, value: &serde_json::Value, at: Instant) {
        if kind == ParamMsg::Subscribed && self.unacked.remove(&id) && self.unacked.is_empty() {
            self.runtime_dirty = true;
        }
        let Some(target) = self.params.get(&id).copied() else {
            return;
        };
        match target {
            ParamTarget::ClipConnected(clip_id) => {
                let Some(state) = value.as_str().and_then(ClipState::from_resolume) else {
                    return;
                };
                let was = self.states.insert(clip_id, state);
                if kind != ParamMsg::Update || was == Some(state) {
                    return; // subscription replies only sync state
                }
                if state.is_connected() {
                    self.on_clip_connected(clip_id, at);
                } else if was.is_some_and(ClipState::is_connected) {
                    self.on_clip_disconnected(clip_id);
                }
            }
            ParamTarget::ClipName(clip_id) => {
                let Some(name) = value.as_str() else { return };
                let Some(mut clip) = self
                    .index
                    .as_ref()
                    .and_then(|i| i.clip_by_id(clip_id))
                    .cloned()
                else {
                    return;
                };
                if clip.name != name {
                    clip.name = name.to_owned();
                    self.replace_clip(clip);
                }
            }
            ParamTarget::LayerName(layer_id) => {
                let Some(name) = value.as_str() else { return };
                let Some(index) = self.index.take() else {
                    return;
                };
                let mut layers = index.layers.clone();
                let changed = layers
                    .iter_mut()
                    .find(|l| l.id == layer_id && l.name != name)
                    .map(|l| l.name = name.to_owned())
                    .is_some();
                let new = CompositionIndex::new(
                    index.composition.clone(),
                    index.decks.clone(),
                    index.live_deck.clone(),
                    layers,
                    index.columns.clone(),
                    index.clips().to_vec(),
                );
                if changed {
                    if let Some(deck) = &new.live_deck {
                        self.deck_cache.insert(deck.clone(), new.clone());
                    }
                    self.structure_dirty = true;
                }
                self.index = Some(new);
            }
        }
    }

    fn on_clip_connected(&mut self, clip_id: u64, at: Instant) {
        // Confirmation of our own trigger.
        let before = self.pending.len();
        let mut confirmed = Vec::new();
        self.pending.retain(|p| {
            if p.clip_id == clip_id {
                confirmed.push((p.seq, p.lane, p.step, at.saturating_duration_since(p.sent)));
                false
            } else {
                true
            }
        });
        for (seq, lane, step, latency) in &confirmed {
            if let Some(v) = self.not_confirmed.get_mut(&(*seq, *lane)) {
                v.retain(|s| s != step);
            }
            self.last_confirm = Some(format!(
                "confirmed connected after {:.1} ms",
                latency.as_secs_f64() * 1000.0
            ));
            self.runtime_dirty = true;
        }
        let (clip, layer, live) = {
            let Some(index) = &self.index else { return };
            let Some(clip) = index.clip_by_id(clip_id).cloned() else {
                return;
            };
            let Some(layer) = index.layer_by_index(clip.layer).cloned() else {
                return;
            };
            (clip, layer, index.live_deck.clone())
        };

        // § placeholder on the SEQ layer: start that sequence of the live deck.
        if layer.name == self.cfg.seq_layer {
            match clip.name.strip_prefix(&self.cfg.seq_prefix) {
                Some(name) => {
                    let name = name.trim().to_owned();
                    let found = live.as_ref().and_then(|d| {
                        self.model
                            .as_ref()?
                            .decks
                            .get(d)?
                            .sequences
                            .iter()
                            .find(|s| s.name == name)
                            .map(|s| s.id)
                    });
                    match found {
                        Some(seq) => self.start_now(seq, Origin::SeqClip { clip_id }),
                        None => {
                            self.notice(format!("§ clip {name:?} matches no sequence in this deck"))
                        }
                    }
                }
                None => log::debug!("non-§ clip {:?} connected on SEQ layer", clip.name),
            }
            return;
        }

        // Manual override: connected on an owned layer, not triggered by us just now.
        if before != self.pending.len() {
            return;
        }
        let recent = self
            .last_layer_trigger
            .get(&layer.id)
            .is_some_and(|t| at.saturating_duration_since(*t) < self.cfg.override_window);
        if !recent {
            if let Some((seq, lane)) = self.scheduler.owner_of(layer.id) {
                let acts = self.scheduler.yield_layer(layer.id);
                if !acts.is_empty() {
                    let (bar, beat) = self
                        .scheduler
                        .status()
                        .iter()
                        .find(|r| r.seq == seq)
                        .map(|r| bar_beat(r.pos))
                        .unwrap_or((1, 1));
                    let lane_name = self
                        .snapshots
                        .get(&seq)
                        .and_then(|(_, s)| s.lanes.get(lane))
                        .map(|l| l.layer.clone())
                        .unwrap_or_default();
                    self.notice(format!(
                        "{lane_name} yielded: manual trigger at {bar}.{beat}"
                    ));
                    self.execute(acts);
                }
            }
        }
    }

    fn on_clip_disconnected(&mut self, clip_id: u64) {
        let runs: Vec<Uuid> = self
            .scheduler
            .status()
            .into_iter()
            .filter(|r| r.origin == Origin::SeqClip { clip_id })
            .map(|r| r.seq)
            .collect();
        for seq in runs {
            let acts = self.scheduler.stop(seq, StopMode::AfterLastPass);
            self.execute(acts);
        }
    }

    // ---- commands ----

    pub fn on_command(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::Start { seq } => self.start(seq),
            EngineCommand::StartByName { name } => {
                self.deferred_start = Some(name);
                self.try_deferred_start();
            }
            EngineCommand::Stop { seq } => {
                let acts = self.scheduler.stop(seq, StopMode::AfterLastPass);
                self.execute(acts);
            }
            EngineCommand::StopAll => {
                self.switch = None;
                let acts = self.scheduler.stop_all(StopMode::Clear);
                self.execute(acts);
            }
            EngineCommand::ResumeLane { seq, lane } => {
                let acts = self.scheduler.resume_lane(seq, lane);
                self.execute(acts);
            }
            EngineCommand::Edit { op } => self.edit(&op),
            EngineCommand::Notice(text) => self.notice(text),
            EngineCommand::Undo => {
                if let (Some(prev), Some(cur)) = (self.undo.pop(), self.model.clone()) {
                    self.redo.push(cur);
                    self.set_model(prev);
                }
            }
            EngineCommand::Redo => {
                if let (Some(next), Some(cur)) = (self.redo.pop(), self.model.clone()) {
                    self.undo.push(cur);
                    self.set_model(next);
                }
            }
        }
    }

    fn edit(&mut self, op: &EditOp) {
        let Some(model) = &self.model else {
            self.notice("sequence file not loaded; edit ignored");
            return;
        };
        match seq_core::edit::apply(model, op) {
            Ok(new) => {
                let old = self.model.take().expect("checked");
                self.undo.push(old);
                if self.undo.len() > MAX_UNDO {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.set_model(new);
            }
            Err(e) => {
                self.notice(format!("edit rejected: {e}"));
                self.structure_dirty = true; // re-send the unchanged model
            }
        }
    }

    fn set_model(&mut self, file: SeqFile) {
        if let Some(s) = &self.saver {
            s.changed(file.clone());
        }
        self.model = Some(file);
        self.structure_dirty = true;
        self.runtime_dirty = true;
    }

    fn try_deferred_start(&mut self) {
        let Some(name) = self.deferred_start.clone() else {
            return;
        };
        let (Some(index), Some(model)) = (&self.index, &self.model) else {
            return;
        };
        let live = index.live_deck.clone();
        let found = live
            .as_ref()
            .and_then(|d| model.decks.get(d))
            .and_then(|d| d.sequences.iter().find(|s| s.name == name))
            .map(|s| s.id)
            .or_else(|| {
                model
                    .decks
                    .values()
                    .flat_map(|d| &d.sequences)
                    .find(|s| s.name == name)
                    .map(|s| s.id)
            });
        self.deferred_start = None;
        match found {
            Some(seq) => self.start(seq),
            None => self.notice(format!("no sequence named {name:?}")),
        }
    }

    fn start(&mut self, seq: Uuid) {
        let Some(model) = &self.model else {
            self.notice("sequence file not loaded yet");
            return;
        };
        let Some((deck, _)) = model.find_sequence(seq) else {
            self.notice("unknown sequence");
            return;
        };
        let deck = deck.to_owned();
        let Some((live, deck_index)) = self
            .index
            .as_ref()
            .map(|i| (i.live_deck.clone(), i.deck_by_name(&deck).map(|d| d.index)))
        else {
            self.notice("not connected to Resolume");
            return;
        };
        if live.as_deref() == Some(deck.as_str()) {
            self.start_now(seq, Origin::Ui);
            return;
        }
        // SPEC §7.2: stop everything, switch deck, start when the new deck is confirmed.
        let Some(n) = deck_index else {
            self.notice(format!("deck {deck:?} does not exist in Resolume"));
            return;
        };
        let acts = self.scheduler.stop_all(StopMode::AfterLastPass);
        self.execute(acts);
        match self.osc.select_deck(n) {
            Ok(addr) => self.osc_sent(addr, Duration::ZERO),
            Err(e) => {
                self.osc_error(e);
                return;
            }
        }
        self.switch = Some(Switch {
            seq,
            deck,
            deadline: Instant::now() + self.cfg.switch_timeout,
        });
        self.runtime_dirty = true;
    }

    /// Resolves and starts a sequence of the live deck.
    fn start_now(&mut self, seq: Uuid, origin: Origin) {
        let Some(index) = self.index.clone() else {
            return;
        };
        let Some(live) = index.live_deck.clone() else {
            return;
        };
        let Some(model) = self.model.as_mut() else {
            return;
        };
        let Some(sequence) = model
            .decks
            .get_mut(&live)
            .and_then(|d| d.sequences.iter_mut().find(|s| s.id == seq))
        else {
            self.notice("sequence is not in the live deck");
            return;
        };
        let resolution = resolve_sequence(sequence, &index);
        let refreshed = refresh_ids(sequence, &resolution) > 0;
        let sequence = sequence.clone();
        if refreshed {
            let snapshot = model.clone();
            if let Some(s) = &self.saver {
                s.changed(snapshot);
            }
            self.structure_dirty = true;
        }
        let mut spec = RunSpec::new(&sequence, &resolution);
        for (lane, run_lane) in sequence.lanes.iter().zip(spec.lanes.iter_mut()) {
            if lane.layer == self.cfg.seq_layer {
                run_lane.layer_id = None; // lanes may not play on the SEQ layer
            }
        }
        match self.scheduler.start(spec, origin) {
            Ok(acts) => {
                self.not_confirmed.retain(|(s, _), _| *s != seq);
                self.snapshots.insert(seq, (live, sequence));
                self.execute(acts);
            }
            Err(e) => self.notice(format!("cannot start {:?}: {e}", sequence.name)),
        }
        self.runtime_dirty = true;
    }

    // ---- actions ----

    fn execute(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Trigger {
                    seq,
                    lane,
                    step,
                    clip_id,
                    ..
                } => self.trigger(seq, lane, step, clip_id),
                Action::Clear { layer_id, .. } => self.clear_layer(layer_id),
                Action::Ended {
                    seq,
                    reason,
                    origin,
                } => {
                    self.snapshots.remove(&seq);
                    self.pending.retain(|p| p.seq != seq);
                    if let (EndReason::Finished, Origin::SeqClip { clip_id }) = (reason, origin) {
                        self.clear_finished_seq_clip(clip_id);
                    }
                    self.runtime_dirty = true;
                }
                Action::Started { .. }
                | Action::PassStarted { .. }
                | Action::StepStarted { .. }
                | Action::LaneStateChanged { .. } => self.runtime_dirty = true,
            }
        }
    }

    fn trigger(&mut self, seq: Uuid, lane: usize, step: usize, clip_id: u64) {
        let Some(index) = &self.index else { return };
        let target = index.clip_by_id(clip_id).filter(|c| !c.empty).map(|c| {
            (
                c.layer,
                c.column,
                index.layer_by_index(c.layer).map(|l| l.id),
            )
        });
        let Some((layer, column, layer_id)) = target else {
            // Deleted since the start: behaves like an unresolved step.
            self.not_confirmed
                .entry((seq, lane))
                .or_default()
                .push(step);
            return;
        };
        let t0 = Instant::now();
        match self.osc.connect_clip(layer, column) {
            Ok(addr) => self.osc_sent(addr, t0.elapsed()),
            Err(e) => return self.osc_error(e),
        }
        if let Some(id) = layer_id {
            self.last_layer_trigger.insert(id, t0);
        }
        if self.states.get(&clip_id).is_some_and(|s| s.is_connected()) {
            // Re-trigger of a playing clip: Resolume reports no change; counts as confirmed.
            return;
        }
        self.pending.push(Pending {
            seq,
            lane,
            step,
            clip_id,
            sent: t0,
        });
    }

    fn clear_layer(&mut self, layer_id: u64) {
        let Some(layer) = self
            .index
            .as_ref()
            .and_then(|i| i.layers.iter().find(|l| l.id == layer_id))
            .map(|l| l.index)
        else {
            return;
        };
        let t0 = Instant::now();
        match self.osc.clear_layer(layer) {
            Ok(addr) => self.osc_sent(addr, t0.elapsed()),
            Err(e) => self.osc_error(e),
        }
        self.last_layer_trigger.insert(layer_id, t0);
    }

    /// A §-started sequence finished by itself: clear the SEQ layer so the § clip can start it
    /// again (only if that § clip is still the connected one).
    fn clear_finished_seq_clip(&mut self, clip_id: u64) {
        if !self.states.get(&clip_id).is_some_and(|s| s.is_connected()) {
            return;
        }
        let seq_layer =
            self.index
                .as_ref()
                .and_then(|i| match i.layer_by_name(&self.cfg.seq_layer) {
                    LayerLookup::Found(l) => Some(l.id),
                    _ => None,
                });
        if let Some(id) = seq_layer {
            self.clear_layer(id);
        }
    }

    fn osc_sent(&mut self, addr: String, took: Duration) {
        log::debug!("OSC {addr}");
        self.osc_ok = true;
        self.last_osc = Some(OscView {
            addr,
            send_us: took.as_micros() as u64,
        });
        self.runtime_dirty = true;
    }

    fn osc_error(&mut self, e: std::io::Error) {
        if self.osc_ok {
            self.notice(format!("OSC send failed: {e}"));
        }
        self.osc_ok = false;
    }

    // ---- workers ----

    pub fn on_aux(&mut self, aux: Aux) {
        match aux {
            Aux::ModelLoaded {
                composition,
                result,
            } => {
                if self.composition.as_deref() != Some(composition.as_str()) {
                    return; // stale
                }
                match result {
                    Ok(file) => {
                        self.model = Some(file);
                        self.model_error = None;
                    }
                    Err(e) => {
                        self.notice(format!("sequence file problem: {e}"));
                        self.model = None;
                        self.model_error = Some(e);
                    }
                }
                self.structure_dirty = true;
                self.try_deferred_start();
            }
            Aux::DeckCacheLoaded { composition, decks } => {
                if self.composition.as_deref() != Some(composition.as_str()) {
                    return;
                }
                for idx in decks {
                    if let Some(deck) = idx.live_deck.clone() {
                        // Never replace what this session has already seen.
                        self.deck_cache.entry(deck).or_insert(idx);
                    }
                }
                self.structure_dirty = true;
            }
            Aux::RestHealth(ok) => {
                if self.rest_ok != ok {
                    self.rest_ok = ok;
                    self.runtime_dirty = true;
                }
            }
            Aux::Saved => {
                self.saved += 1;
                self.save_error = None;
                self.runtime_dirty = true;
            }
            Aux::SaveFailed(e) => {
                self.notice(format!("autosave failed: {e}"));
                self.save_error = Some(e);
            }
        }
    }

    // ---- timer ----

    pub fn on_timer(&mut self, now: Instant) {
        // Confirmation timeouts (SPEC §7.5).
        let timeout = self.cfg.confirm_timeout;
        let mut late = Vec::new();
        self.pending.retain(|p| {
            if now.saturating_duration_since(p.sent) > timeout {
                late.push((p.seq, p.lane, p.step, p.clip_id));
                false
            } else {
                true
            }
        });
        for (seq, lane, step, clip_id) in late {
            log::warn!("trigger of clip {clip_id} (lane {lane}, step {step}) not confirmed");
            self.not_confirmed
                .entry((seq, lane))
                .or_default()
                .push(step);
            self.runtime_dirty = true;
        }
        let bpm = self
            .clock_running
            .then(|| self.tempo.bpm())
            .flatten()
            .map(|b| (b * 10.0).round() as i64);
        if bpm != self.shown_bpm {
            self.shown_bpm = bpm;
            self.runtime_dirty = true;
        }
        // Clock loss (SPEC §3).
        if self.clock_running && self.tempo.is_lost(now) {
            self.clock_running = false;
            self.notice("no clock: sequences frozen");
        }
        // Deck switch timeout (SPEC §7.2).
        if self.switch.as_ref().is_some_and(|s| now > s.deadline) {
            let sw = self.switch.take().expect("checked");
            self.notice(format!(
                "deck switch to {:?} not confirmed within 2 s; nothing started",
                sw.deck
            ));
        }
        self.publish(now);
    }

    fn publish(&mut self, now: Instant) {
        if self.structure_dirty {
            self.structure_dirty = false;
            let s = self.structure_view();
            self.structure_tx.send_if_modified(|old| {
                if *old == s {
                    false
                } else {
                    *old = s;
                    true
                }
            });
        }
        if self.runtime_dirty
            && now.saturating_duration_since(self.last_runtime_publish) >= RUNTIME_PERIOD
        {
            self.runtime_dirty = false;
            self.last_runtime_publish = now;
            let r = self.runtime_view();
            self.runtime_tx.send_if_modified(|old| {
                if *old == r {
                    false
                } else {
                    *old = r;
                    true
                }
            });
        }
    }

    fn decks_dir(&self, composition: &str) -> std::path::PathBuf {
        self.storage
            .dir()
            .join("decks")
            .join(crate::storage::file_stem_for(composition))
    }

    /// Writes the live deck's index to disk (off-task) so it can be shown when the deck is not
    /// live, also after a restart (SPEC §4.2, §8).
    fn persist_deck(&self, index: &CompositionIndex) {
        let Some(deck) = index.live_deck.as_deref() else {
            return;
        };
        let dir = self.decks_dir(&index.composition);
        let path = dir.join(format!("{}.json", crate::storage::file_stem_for(deck)));
        let Ok(json) = serde_json::to_string(index) else {
            return;
        };
        tokio::task::spawn_blocking(move || {
            let tmp = path.with_extension("json.tmp");
            let result = std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::write(&tmp, json))
                .and_then(|()| std::fs::rename(&tmp, &path));
            if let Err(e) = result {
                log::warn!("deck cache {}: {e}", path.display());
            }
        });
    }

    // ---- views ----

    fn structure_view(&self) -> StructureState {
        let index = self.index.as_ref();
        let mut decks: Vec<DeckView> = Vec::new();
        let resolume_decks: Vec<(String, usize)> = index
            .map(|i| i.decks.iter().map(|d| (d.name.clone(), d.index)).collect())
            .unwrap_or_default();
        let mut names: Vec<(String, Option<usize>)> = resolume_decks
            .iter()
            .map(|(n, i)| (n.clone(), Some(*i)))
            .collect();
        if let Some(m) = &self.model {
            for name in m.decks.keys() {
                if !names.iter().any(|(n, _)| n == name) {
                    names.push((name.clone(), None));
                }
            }
        }
        for (name, deck_index) in names {
            let live = index.and_then(|i| i.live_deck.as_deref()) == Some(name.as_str());
            let deck_idx = if live {
                index
            } else {
                self.deck_cache.get(&name)
            };
            let clips_source = match (live, deck_idx.is_some()) {
                (true, _) => ClipsSource::Live,
                (false, true) => ClipsSource::Cached,
                _ => ClipsSource::None,
            };
            let seq_clips: Vec<(String, usize, u64)> =
                deck_idx.map(|i| self.seq_clips(i)).unwrap_or_default();
            let sequences: Vec<SequenceView> = self
                .model
                .as_ref()
                .and_then(|m| m.decks.get(&name))
                .map(|d| {
                    d.sequences
                        .iter()
                        .map(|s| SequenceView {
                            sequence: s.clone(),
                            pass_eighths: s.pass_length(),
                            binding: seq_clips.iter().find(|(n, _, _)| *n == s.name).map(
                                |(_, column, clip_id)| Binding {
                                    column: *column,
                                    clip_id: *clip_id,
                                },
                            ),
                            resolution: deck_idx.map(|i| self.resolution(s, i)),
                            mosaic_key: mosaic_key(s, deck_idx),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let unmatched_seq_clips = seq_clips
                .iter()
                .filter(|(n, _, _)| !sequences.iter().any(|s| &s.sequence.name == n))
                .map(|(n, _, _)| n.clone())
                .collect();
            decks.push(DeckView {
                orphaned: deck_index.is_none(),
                index: deck_index,
                live,
                clips_source,
                layers: deck_idx
                    .map(|i| layer_views(i, &self.cfg.seq_layer))
                    .unwrap_or_default(),
                sequences,
                unmatched_seq_clips,
                name,
            });
        }
        StructureState {
            composition: self.composition.clone(),
            live_deck: index.and_then(|i| i.live_deck.clone()),
            decks,
            model_error: self.model_error.clone(),
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
            seq_layer: self.cfg.seq_layer.clone(),
            seq_prefix: self.cfg.seq_prefix.clone(),
        }
    }

    /// `§` clips on the SEQ layer: (sequence name, column, clip id).
    fn seq_clips(&self, index: &CompositionIndex) -> Vec<(String, usize, u64)> {
        let LayerLookup::Found(layer) = index.layer_by_name(&self.cfg.seq_layer) else {
            return Vec::new();
        };
        index
            .filled_clips()
            .filter(|c| c.layer == layer.index)
            .filter_map(|c| {
                c.name
                    .strip_prefix(&self.cfg.seq_prefix)
                    .map(|n| (n.trim().to_owned(), c.column, c.id))
            })
            .collect()
    }

    fn resolution(&self, s: &Sequence, index: &CompositionIndex) -> ResolutionView {
        let res: SequenceResolution = resolve_sequence(s, index);
        let seq_lanes: Vec<bool> = s
            .lanes
            .iter()
            .map(|l| l.layer == self.cfg.seq_layer)
            .collect();
        resolution_view(&res, &seq_lanes)
    }

    fn runtime_view(&self) -> RuntimeState {
        let running = self
            .scheduler
            .status()
            .into_iter()
            .map(|r| {
                let (deck, snapshot) = self
                    .snapshots
                    .get(&r.seq)
                    .map(|(d, s)| (Some(d.clone()), Some(s)))
                    .unwrap_or((None, None));
                let edited = snapshot.is_some_and(|snap| {
                    self.model
                        .as_ref()
                        .and_then(|m| m.find_sequence(r.seq))
                        .is_some_and(|(_, cur)| cur != snap)
                });
                let (bar, beat) = bar_beat(r.pos);
                RunView {
                    seq: r.seq,
                    name: r.name.clone(),
                    deck,
                    from_seq_clip: origin_is_seq_clip(r.origin),
                    pass: r.pass,
                    passes: r.passes,
                    pos: r.pos,
                    pass_ticks: r.pass_ticks,
                    bar_beat: format!("{bar}.{beat}"),
                    lanes: r
                        .lanes
                        .iter()
                        .enumerate()
                        .map(|(i, l)| RunLaneView {
                            state: lane_state_name(l.state),
                            step: if l.state == LaneState::Inactive {
                                None
                            } else {
                                l.step
                            },
                            not_confirmed: self
                                .not_confirmed
                                .get(&(r.seq, i))
                                .cloned()
                                .unwrap_or_default(),
                        })
                        .collect(),
                    edited,
                }
            })
            .collect();
        RuntimeState {
            clock: ClockView {
                port: self.clock_port.clone(),
                connected: self.clock_connected,
                running: self.clock_running,
                bpm: self.clock_running.then(|| self.tempo.bpm()).flatten(),
            },
            ws: self.ws_connected,
            ws_synced: self.ws_connected && self.index.is_some() && self.unacked.is_empty(),
            rest: self.rest_ok,
            osc: self.osc_ok,
            running,
            switching: self.switch.as_ref().map(|s| SwitchView {
                seq: s.seq,
                deck: s.deck.clone(),
            }),
            last_osc: self.last_osc.clone(),
            last_confirm: self.last_confirm.clone(),
            notices: self.notices.iter().cloned().collect(),
            saved: self.saved,
            save_error: self.save_error.clone(),
        }
    }

    /// Current model (for tests and the UI server's initial edit base).
    pub fn model(&self) -> Option<&SeqFile> {
        self.model.as_ref()
    }
}

/// Hash of everything a sequence mosaic shows: the sequence and its clips' thumbnail versions.
fn mosaic_key(s: &Sequence, index: Option<&CompositionIndex>) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(s).unwrap_or_default().hash(&mut h);
    if let Some(index) = index {
        let res = resolve_sequence(s, index);
        for lane in &res.lanes {
            for step in &lane.steps {
                if let Some(c) = step.clip().and_then(|c| index.clip_by_id(c.clip_id)) {
                    c.thumbnail.last_update.hash(&mut h);
                }
            }
        }
    }
    format!("{:016x}", h.finish())
}

/// Inputs of the engine task.
pub struct Inputs {
    pub clock: mpsc::UnboundedReceiver<ClockEvent>,
    pub ws: mpsc::UnboundedReceiver<WsEvent>,
    pub commands: mpsc::UnboundedReceiver<EngineCommand>,
    pub aux: mpsc::UnboundedReceiver<Aux>,
    /// Stops the engine. Other tasks (the UI server) hold command senders, so the end of the
    /// command channel cannot be the shutdown signal.
    pub shutdown: tokio::sync::oneshot::Receiver<()>,
}

/// Runs the engine until all command senders are gone.
pub async fn run(mut engine: Engine, mut inputs: Inputs) {
    let mut timer = tokio::time::interval(Duration::from_millis(10));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            Some(ev) = inputs.clock.recv() => engine.on_clock(ev),
            Some(ev) = inputs.ws.recv() => engine.on_ws(ev),
            cmd = inputs.commands.recv() => match cmd {
                Some(c) => engine.on_command(c),
                None => break,
            },
            Some(a) = inputs.aux.recv() => engine.on_aux(a),
            _ = &mut inputs.shutdown => break,
            _ = timer.tick() => engine.on_timer(Instant::now()),
        }
    }
    let acts = engine.scheduler.stop_all(StopMode::Release);
    engine.execute(acts);
    engine.on_timer(Instant::now());
}
