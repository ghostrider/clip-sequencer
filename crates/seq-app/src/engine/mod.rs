//! The engine task (SPEC §10): owns the scheduler, the composition index and the sequence model.
//!
//! It reacts to clock events, WebSocket events, UI commands and worker results, sends OSC
//! directly, and publishes UI state on watch channels. It never awaits HTTP or disk: those run
//! in worker tasks that report back through [`Aux`] messages.

mod fades;
mod record;
pub mod view;

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::time::{Duration, Instant};

use clock_in::{ClockEvent, ClockMsg, ResolumeClock, TempoEstimator, Transport};
use resolume_client::composition::{CompositionSnapshot, LutEffect, TempoController};
use resolume_client::osc::OscSender;
use resolume_client::ws::{ParamMsg, WsCommand, WsEvent};
use seq_core::edit::EditOp;
use seq_core::index::{ClipInfo, ClipState, CompositionIndex, LayerLookup};
use seq_core::resolve::{refresh_ids, resolve_sequence, SequenceResolution};
use seq_core::scheduler::{
    bar_beat, Action, EndReason, LaneState, Origin, RunSpec, Scheduler, StopMode, Wait,
};
use seq_core::{SeqFile, Sequence, Start};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::saver::SaverHandle;
use crate::settings::ClockSourceKind;
use crate::storage::{Storage, StorageError};
use view::*;

/// Where a cue starts (SPEC §3.3). JSON: `"at": "start_mode" | "bars_8" | "bars_16"`, or
/// `"at": "end_of_pass", "of": <running sequence id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum CuePoint {
    StartMode,
    EndOfPass {
        of: Uuid,
    },
    #[serde(rename = "bars_8")]
    Bars8,
    #[serde(rename = "bars_16")]
    Bars16,
}

/// Commands from the UI (and the CLI).
#[derive(Debug, Clone)]
pub enum EngineCommand {
    Start {
        seq: Uuid,
    },
    /// Cue a sequence of the live deck (SPEC §3.3).
    Cue {
        seq: Uuid,
        at: CuePoint,
    },
    CancelCue,
    /// Mute or solo a lane of a running sequence (SPEC §7.8).
    SetLaneMute {
        seq: Uuid,
        lane: usize,
        on: bool,
    },
    SetLaneSolo {
        seq: Uuid,
        lane: usize,
        on: bool,
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
    /// Write the pending sequence file change now (menu "Save now").
    SaveNow,
    /// Reconnect to Resolume: a fresh composition and subscriptions (menu "Reload composition").
    ReloadComposition,
    /// The clock input now listens to this MIDI port (Preferences); `None` = no clock input.
    ClockPortChanged {
        port: Option<String>,
    },
    /// The clock now comes from this source (Preferences, SPEC §3.1).
    ClockSourceChanged {
        source: ClockSourceKind,
    },
    /// Give every sequence of the live deck a `§` clip on the SEQ layer, creating the layer if
    /// needed (SPEC §7.6). With `remove_orphans`, also clear `§` clips without a sequence.
    SyncSeqLayer {
        remove_orphans: bool,
    },
    /// Arm a sequence of the live deck for recording, or disarm (SPEC §7.9). `bars`: the pattern
    /// length of a sequence without steps.
    SetArmed {
        seq: Option<Uuid>,
        bars: Option<u32>,
    },
    /// Point one of Resolume's LUT effects at a `.cube` file (LUT tab, SPEC §14): a `set` on its `LUT 3D`
    /// parameter. Shown at once in the LUT effect list; Resolume's update or next push confirms it.
    SetLutFile {
        file_param: u64,
        path: String,
    },
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
    /// A slot read just before a slot lane step (SPEC §4.5). `None`: the request failed.
    SlotRead {
        request: SlotRequest,
        result: Option<(ClipInfo, Option<ClipState>)>,
    },
    /// The "Sync SEQ layer" worker finished (SPEC §7.6).
    SeqSynced(Result<crate::seq_sync::SyncReport, String>),
}

/// Read one slot over REST (`GET /composition/layers/{layer}/clips/{column}`, notes §15); the
/// worker answers with [`Aux::SlotRead`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotRequest {
    /// 1-based position at the time of the request.
    pub layer: usize,
    pub column: usize,
    pub layer_id: u64,
    pub sent: Instant,
}

/// How long before a slot lane step its slot is read (SPEC §4.5).
const SLOT_LOOKAHEAD: Duration = Duration::from_millis(150);

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
    /// The tempo controller (notes §17, SPEC §3.1).
    Tempo,
    TempoPlay,
    TempoResync,
    /// The `LUT 3D` file of one of Resolume's LUT effects (SPEC §14), by effect id.
    LutFile(u64),
    /// A layer's transition duration and blend mode (SPEC §7.7), by layer id.
    LayerFadeDuration(u64),
    LayerFadeBlend(u64),
}

/// Resolume sends two `resync` updates per press (press and release, ~100 ms apart, no value;
/// notes §17): the second one within this time is the release.
const RESYNC_RELEASE: Duration = Duration::from_millis(300);

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
    /// Slot reads for upcoming slot lane steps (SPEC §4.5); `None` in tests without a worker.
    slot_reads: Option<mpsc::UnboundedSender<SlotRequest>>,
    /// The "Sync SEQ layer" worker (SPEC §7.6); `None` in tests without a worker.
    seq_sync: Option<mpsc::UnboundedSender<crate::seq_sync::SyncRequest>>,
    /// A sync is running: further requests are refused until it reports.
    seq_sync_busy: bool,
    /// When the last composition push was applied: slot reads sent before it are stale.
    composition_at: Instant,
    index: Option<CompositionIndex>,
    states: HashMap<u64, ClipState>,
    deck_cache: HashMap<String, CompositionIndex>,
    params: HashMap<u64, ParamTarget>,
    /// Subscriptions requested but not yet acknowledged by Resolume.
    unacked: std::collections::HashSet<u64>,
    /// When subscriptions were last (re)sent while some are unanswered.
    unacked_since: Instant,

    composition: Option<String>,
    model: Option<SeqFile>,
    model_error: Option<String>,
    /// Snapshots of running sequences (for "edited · applies at next start") and their deck.
    snapshots: HashMap<Uuid, (String, Sequence)>,
    /// Snapshots of starts waiting for their boundary (SPEC §3.2); moved to `snapshots` when the
    /// scheduler reports them started.
    queued: HashMap<Uuid, (String, Sequence)>,
    undo: Vec<SeqFile>,
    redo: Vec<SeqFile>,

    last_layer_trigger: HashMap<u64, Instant>,
    /// Clip each running lane triggered last (it may live on another layer than the lane).
    lane_last_clip: HashMap<(Uuid, usize), u64>,
    /// Last value sent per "1000A Sequence" plugin parameter (avoids resending until Resolume's
    /// next composition push shows it).
    plugin_sent: HashMap<u64, String>,
    pending: Vec<Pending>,
    not_confirmed: NotConfirmed,
    /// Triggers past `confirm_timeout`: their `connected` may still come (SPEC §7.5) and must not
    /// count as a manual trigger. Kept until the lane triggers again, at most `LATE_LIMIT`.
    late: Vec<Pending>,
    switch: Option<Switch>,
    deferred_start: Option<String>,

    tempo: TempoEstimator,
    /// BPM as last shown (tenths), to republish when the display changes.
    shown_bpm: Option<i64>,
    clock_port: Option<String>,
    clock_connected: bool,
    clock_running: bool,
    /// Where ticks come from (SPEC §3, §3.1).
    clock_source: ClockSourceKind,
    /// The tick generator for the Resolume source; fed while that source is selected.
    resolume_clock: Option<ResolumeClock>,
    /// Ids and last known values of Resolume's tempo controller.
    tempo_ctl: Option<TempoController>,
    /// Resolume's LUT effects from the last composition push (SPEC §14): shown in the LUT tab.
    lut_effects: Vec<LutEffect>,
    /// Layer transitions: originals, values, the pending file (SPEC §7.7).
    fades: fades::Fades,
    /// Recording from Resolume (SPEC §7.9).
    rec: record::Recorder,
    /// True after a resync: the tick grid is in phase with Resolume's beat.
    phase_synced: bool,
    /// The scheduler's bar phase comes from a MIDI Start or a Resync, not the first tick (SPEC §3.2).
    bar_synced: bool,
    last_resync: Option<Instant>,
    ws_connected: bool,
    rest_ok: bool,
    osc_ok: bool,
    /// Triggers timed out in a row while Resolume could have confirmed them; reset by any confirmation.
    unanswered_triggers: u32,
    last_osc: Option<OscView>,
    last_confirm: Option<String>,
    confirmations: u64,
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
/// Unanswered subscriptions are requested again after this long.
const RESUBSCRIBE_AFTER: Duration = Duration::from_secs(1);
/// How long a timed-out trigger's late `connected` still counts as ours (seen live: ~700 ms while
/// Resolume was busy).
const LATE_LIMIT: Duration = Duration::from_secs(5);
/// Triggers in a row that time out, with the WebSocket up and subscribed, before the OSC indicator turns red:
/// OSC is UDP, so a send that works says nothing about Resolume hearing it (OSC input off, wrong port).
const OSC_UNANSWERED_AFTER: u32 = 3;

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
        let fades = fades::Fades::load(storage.dir());
        let engine = Self {
            cfg,
            osc,
            ws_cmd,
            saver,
            storage,
            aux_tx,
            scheduler: Scheduler::new(),
            slot_reads: None,
            seq_sync: None,
            seq_sync_busy: false,
            composition_at: Instant::now(),
            index: None,
            states: HashMap::new(),
            deck_cache: HashMap::new(),
            params: HashMap::new(),
            unacked: std::collections::HashSet::new(),
            unacked_since: Instant::now(),
            composition: None,
            model: None,
            model_error: None,
            snapshots: HashMap::new(),
            queued: HashMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_layer_trigger: HashMap::new(),
            lane_last_clip: HashMap::new(),
            plugin_sent: HashMap::new(),
            pending: Vec::new(),
            not_confirmed: NotConfirmed::new(),
            late: Vec::new(),
            switch: None,
            deferred_start: None,
            tempo: TempoEstimator::new(),
            shown_bpm: None,
            clock_port: None,
            clock_connected: false,
            clock_running: false,
            clock_source: ClockSourceKind::Midi,
            resolume_clock: None,
            tempo_ctl: None,
            lut_effects: Vec::new(),
            fades,
            rec: record::Recorder::default(),
            phase_synced: false,
            bar_synced: false,
            last_resync: None,
            ws_connected: false,
            rest_ok: false,
            osc_ok: true,
            unanswered_triggers: 0,
            last_osc: None,
            last_confirm: None,
            confirmations: 0,
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

    /// The clock source at startup and the Resolume tick generator (`None` in tests that feed
    /// clock events themselves).
    pub fn set_clock_source(&mut self, source: ClockSourceKind, clock: Option<ResolumeClock>) {
        self.clock_source = source;
        self.resolume_clock = clock;
        self.feed_resolume_state();
    }

    fn follows_resolume(&self) -> bool {
        self.clock_source == ClockSourceKind::Resolume
    }

    /// Hands Resolume's current tempo and transport to the generator, without a resync (SPEC §3.1):
    /// at startup, after a reconnect or reload, and when the source changes. Stops it while the
    /// source is MIDI or Resolume is not connected.
    fn feed_resolume_state(&mut self) {
        let Some(clock) = &self.resolume_clock else {
            return;
        };
        let now = Instant::now();
        match (
            &self.tempo_ctl,
            self.follows_resolume() && self.ws_connected,
        ) {
            (Some(t), true) => {
                clock.tempo(t.bpm);
                clock.set_state(
                    Transport::from_resolume(&t.play).unwrap_or(Transport::Stop),
                    now,
                );
            }
            _ => clock.set_state(Transport::Stop, now),
        }
    }

    /// Resync (SPEC §3.1): `at` is beat 1; every running sequence jumps to its start.
    fn on_resync(&mut self, at: Instant) {
        self.phase_synced = true;
        self.bar_synced = true;
        let running = self.scheduler.running().count();
        log::info!("resync from Resolume: {running} running sequence(s) back to bar 1");
        self.request_upcoming_slots(at);
        let actions = self.scheduler.rewind_all();
        self.execute(actions);
        // This moment is bar 0 (SPEC §3.2): waiting starts begin together with the rewind.
        let actions = self.scheduler.phase_reset_now();
        self.execute(actions);
        self.runtime_dirty = true;
    }

    /// The bar phase is unknown again (clock (re)connected, source switched, Resolume lost): it
    /// counts from the next tick until a MIDI Start or Resync (SPEC §3.2).
    fn bar_phase_unknown(&mut self) {
        self.scheduler.phase_reset_next_tick();
        self.bar_synced = false;
        self.runtime_dirty = true;
    }

    /// Where slot reads for upcoming slot lane steps go (SPEC §4.5). See [`spawn_slot_reader`].
    pub fn set_slot_reader(&mut self, tx: mpsc::UnboundedSender<SlotRequest>) {
        self.slot_reads = Some(tx);
    }

    /// Where "Sync SEQ layer" requests go (SPEC §7.6). See [`crate::seq_sync::spawn`].
    pub fn set_seq_sync(&mut self, tx: mpsc::UnboundedSender<crate::seq_sync::SyncRequest>) {
        self.seq_sync = Some(tx);
    }

    /// Sends the live deck's sequence names to the sync worker (SPEC §7.6).
    fn sync_seq_layer(&mut self, remove_orphans: bool) {
        if self.seq_sync_busy {
            self.notice("Sync is already running");
            return;
        }
        let Some(tx) = self.seq_sync.clone() else {
            self.notice("Sync is not available in this app");
            return;
        };
        let Some(deck) = self.index.as_ref().and_then(|i| i.live_deck.clone()) else {
            self.notice("Sync: no composition from Resolume yet");
            return;
        };
        if self.switch.is_some() {
            self.notice("Sync: wait until the deck switch is done");
            return;
        }
        let sequences = self.live_sequence_names(&deck);
        let req = crate::seq_sync::SyncRequest {
            deck,
            sequences,
            layer: self.cfg.seq_layer.clone(),
            prefix: self.cfg.seq_prefix.clone(),
            remove_orphans,
        };
        if tx.send(req).is_ok() {
            self.seq_sync_busy = true;
            self.runtime_dirty = true;
        }
    }

    fn live_sequence_names(&self, deck: &str) -> Vec<String> {
        self.model
            .as_ref()
            .and_then(|m| m.decks.get(deck))
            .map(|d| d.sequences.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default()
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
                self.request_upcoming_slots(at);
                let actions = self.scheduler.on_tick();
                self.execute(actions);
                self.record_tick(at);
                if !self.snapshots.is_empty() {
                    self.runtime_dirty = true;
                }
            }
            ClockEvent::Msg {
                msg: ClockMsg::Start,
                ..
            } => {
                // Sets the bar phase only; never restarts running sequences (SPEC §3).
                log::info!("MIDI Start: the next tick is bar 1 for quantized starts");
                self.scheduler.phase_reset_next_tick();
                self.bar_synced = true;
                self.runtime_dirty = true;
            }
            ClockEvent::Msg { msg, .. } => log::info!("MIDI {msg:?} (ignored by sequences)"),
            ClockEvent::Resync { at } => self.on_resync(at),
            ClockEvent::Connected { port } => {
                self.clock_connected = true;
                self.clock_port = Some(port);
                self.bar_phase_unknown();
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
                self.unacked_since = Instant::now();
                self.ws_connected = true;
                self.runtime_dirty = true;
            }
            WsEvent::Disconnected { .. } => {
                if self.ws_connected {
                    self.notice("Resolume WebSocket disconnected");
                }
                self.ws_connected = false;
                // Without the WebSocket nothing can be confirmed: the OSC verdict starts over.
                self.unanswered_triggers = 0;
                self.transitions_connection_lost();
                // Resolume tempo clock: no ticks without Resolume, and its beat is unknown after.
                self.phase_synced = false;
                if self.follows_resolume() {
                    self.bar_phase_unknown();
                }
                self.feed_resolume_state();
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
        self.composition_at = Instant::now();
        let CompositionSnapshot {
            index,
            states,
            tempo,
            lut_effects,
            transitions,
        } = snap;
        let old_live = self.index.as_ref().and_then(|i| i.live_deck.clone());
        let new_live = index.live_deck.clone();
        let tempo_ids = |t: &Option<TempoController>| {
            t.as_ref()
                .map(|t| (t.tempo_param, t.play_param, t.resync_param))
        };
        let lut_ids = |l: &[LutEffect]| -> Vec<(u64, u64)> {
            l.iter().map(|e| (e.effect_id, e.file_param)).collect()
        };
        let structural = self.index.as_ref() != Some(&index)
            || tempo_ids(&self.tempo_ctl) != tempo_ids(&tempo)
            || lut_ids(&self.lut_effects) != lut_ids(&lut_effects);
        if lut_effects != self.lut_effects {
            self.lut_effects = lut_effects;
            self.structure_dirty = true;
        }
        self.states = states;
        self.tempo_ctl = tempo;
        self.feed_resolume_state();

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
        self.transitions_on_composition(transitions, &index);
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
        self.sync_plugin_params();
        self.try_deferred_start();
    }

    /// Sets the `Sequence` parameter of every "1000A Sequence" plugin on the SEQ layer to its clip's
    /// sequence name (SPEC §12). A clip that is not a `§` clip gets an empty name.
    fn sync_plugin_params(&mut self) {
        let Some(index) = &self.index else { return };
        let LayerLookup::Found(seq_layer) = index.layer_by_name(&self.cfg.seq_layer) else {
            return;
        };
        let sends: Vec<(u64, String)> = index
            .filled_clips()
            .filter(|c| c.layer == seq_layer.index)
            .filter_map(|c| {
                let p = c.sequence_param.as_ref()?;
                let want = c
                    .name
                    .strip_prefix(&self.cfg.seq_prefix)
                    .map(|n| n.trim().to_owned())
                    .unwrap_or_default();
                (p.value != want && self.plugin_sent.get(&p.id) != Some(&want))
                    .then_some((p.id, want))
            })
            .collect();
        for (id, value) in sends {
            log::info!("plugin parameter {id}: Sequence = {value:?}");
            let msg = serde_json::json!({
                "action": "set",
                "parameter": format!("/parameter/by-id/{id}"),
                "value": value,
            });
            let _ = self.ws_cmd.send(WsCommand::Raw(msg.to_string()));
            self.plugin_sent.insert(id, value);
        }
    }

    fn change_composition(&mut self, name: String) {
        log::info!("composition {name:?}");
        let old = self.composition.clone();
        self.transitions_composition_changed(old.as_deref());
        let acts = self.scheduler.stop_all(StopMode::Release);
        self.execute(acts);
        self.composition = Some(name.clone());
        self.model = None;
        self.model_error = None;
        self.undo.clear();
        self.redo.clear();
        self.plugin_sent.clear();
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
            if let Some(t) = &l.transition {
                self.params
                    .insert(t.duration_param, ParamTarget::LayerFadeDuration(l.id));
                self.params
                    .insert(t.blend_param, ParamTarget::LayerFadeBlend(l.id));
            }
        }
        // Always followed (cheap: Resolume only sends changes), so switching the clock source
        // needs no new subscriptions.
        if let Some(t) = &self.tempo_ctl {
            self.params.insert(t.tempo_param, ParamTarget::Tempo);
            self.params.insert(t.play_param, ParamTarget::TempoPlay);
            self.params.insert(t.resync_param, ParamTarget::TempoResync);
        }
        for e in &self.lut_effects {
            self.params
                .insert(e.file_param, ParamTarget::LutFile(e.effect_id));
        }
        self.params.remove(&0);
        let ids: BTreeSet<u64> = self.params.keys().copied().collect();
        self.unacked.retain(|id| ids.contains(id));
        self.unacked.extend(ids.iter().copied());
        self.unacked_since = Instant::now();
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
                    if let Some(layer_id) = self.layer_of_clip(clip_id) {
                        self.record_disconnect(layer_id, at);
                    }
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
                    self.sync_plugin_params();
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
            ParamTarget::Tempo => {
                let Some(bpm) = value.as_f64() else { return };
                if let Some(t) = &mut self.tempo_ctl {
                    t.bpm = bpm;
                }
                if self.follows_resolume() {
                    if let Some(clock) = &self.resolume_clock {
                        clock.tempo(bpm);
                    }
                }
                self.runtime_dirty = true;
            }
            ParamTarget::TempoPlay => {
                let Some(play) = value.as_str() else { return };
                let Some(transport) = Transport::from_resolume(play) else {
                    return;
                };
                let changed = self.tempo_ctl.as_ref().is_some_and(|t| t.play != play);
                if let Some(t) = &mut self.tempo_ctl {
                    t.play = play.to_owned();
                }
                if self.follows_resolume() {
                    if let Some(clock) = &self.resolume_clock {
                        // A subscription reply only reports the state; an update is a press.
                        if kind == ParamMsg::Update && changed {
                            clock.transport(transport, at);
                        } else {
                            clock.set_state(transport, at);
                        }
                    }
                }
                self.runtime_dirty = true;
            }
            ParamTarget::TempoResync => {
                if kind != ParamMsg::Update {
                    return;
                }
                if self
                    .last_resync
                    .is_some_and(|t| at.saturating_duration_since(t) < RESYNC_RELEASE)
                {
                    return; // the release of the same press
                }
                self.last_resync = Some(at);
                if self.follows_resolume() {
                    if let Some(clock) = &self.resolume_clock {
                        clock.resync(at);
                    }
                }
            }
            ParamTarget::LayerFadeDuration(layer) => {
                self.transition_param(layer, false, kind, value);
            }
            ParamTarget::LayerFadeBlend(layer) => {
                self.transition_param(layer, true, kind, value);
            }
            ParamTarget::LutFile(effect_id) => {
                let Some(path) = value.as_str() else { return };
                if let Some(e) = self
                    .lut_effects
                    .iter_mut()
                    .find(|e| e.effect_id == effect_id && e.path != path)
                {
                    e.path = path.to_owned();
                    self.structure_dirty = true;
                }
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
        // Our own trigger, confirmed after its timeout (Resolume was busy): clears the red mark,
        // and is no manual override.
        if confirmed.is_empty() {
            if let Some(i) = self.late.iter().position(|p| p.clip_id == clip_id) {
                let p = self.late.remove(i);
                let latency = at.saturating_duration_since(p.sent);
                log::info!(
                    "trigger of clip {clip_id} (lane {}, step {}) confirmed late, after {:.0} ms",
                    p.lane,
                    p.step,
                    latency.as_secs_f64() * 1000.0
                );
                confirmed.push((p.seq, p.lane, p.step, latency));
            }
        }
        if !confirmed.is_empty() && self.unanswered_triggers > 0 {
            if self.osc_unanswered() {
                self.notice("Resolume reacts to OSC again");
            }
            self.unanswered_triggers = 0;
        }
        for (seq, lane, step, latency) in &confirmed {
            log::debug!(
                "trigger of clip {clip_id} (lane {lane}, step {step}) confirmed after {:.1} ms",
                latency.as_secs_f64() * 1000.0
            );
            if let Some(v) = self.not_confirmed.get_mut(&(*seq, *lane)) {
                v.retain(|s| s != step);
            }
            self.confirmations += 1;
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
        // A cut sends the old clip's `Disconnected` just before this: no clear (SPEC §7.9).
        self.record_connected(layer.id);

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
                        // Waiting for its boundary: the § clip takes over the waiting start
                        // instead of making a second one (SPEC §7.1).
                        Some(seq) if self.scheduler.is_pending(seq) => {
                            self.scheduler
                                .link_pending(seq, Origin::SeqClip { clip_id });
                            self.runtime_dirty = true;
                        }
                        // Already running (started from the app): the § clip adopts the run, no
                        // restart (SPEC §7.1).
                        Some(seq) if self.scheduler.adopt(seq, Origin::SeqClip { clip_id }) => {
                            log::info!("§ clip {name:?} connected: linked to the running sequence");
                            self.runtime_dirty = true;
                        }
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
        if before != self.pending.len() || !confirmed.is_empty() {
            return;
        }
        let recent = self
            .last_layer_trigger
            .get(&layer.id)
            .is_some_and(|t| at.saturating_duration_since(*t) < self.cfg.override_window);
        // Recording (SPEC §7.9): a click on the recording sequence's layer or a free one is recorded.
        if !recent && self.record_click(&clip, layer.id, &layer.name, at) {
            return;
        }
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
        let origin = Origin::SeqClip { clip_id };
        let runs: Vec<Uuid> = self
            .scheduler
            .status()
            .into_iter()
            .filter(|r| r.origin == origin)
            .map(|r| r.seq)
            .chain(
                self.scheduler
                    .pending()
                    .into_iter()
                    .filter(|p| p.origin == origin)
                    .map(|p| p.seq),
            )
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
            EngineCommand::Cue { seq, at } => self.cue(seq, at),
            EngineCommand::CancelCue => {
                let acts = self.scheduler.cancel_cue();
                self.execute(acts);
            }
            EngineCommand::SetArmed { seq, bars } => self.set_armed(seq, bars),
            EngineCommand::SetLaneMute { seq, lane, on } => {
                let acts = self.scheduler.set_mute(seq, lane, on);
                self.execute(acts);
            }
            EngineCommand::SetLaneSolo { seq, lane, on } => {
                let acts = self.scheduler.set_solo(seq, lane, on);
                self.execute(acts);
            }
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
            EngineCommand::SaveNow => {
                if let Some(saver) = self.saver.clone() {
                    let aux = self.aux_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = saver.flush().await {
                            let _ = aux.send(Aux::SaveFailed(e));
                        }
                    });
                }
            }
            EngineCommand::SetLutFile { file_param, path } => {
                if !self.ws_connected {
                    self.notice("LUT: not connected to Resolume");
                    return;
                }
                let msg = serde_json::json!({
                    "action": "set",
                    "parameter": format!("/parameter/by-id/{file_param}"),
                    "value": path,
                });
                let _ = self.ws_cmd.send(WsCommand::Raw(msg.to_string()));
                if let Some(e) = self
                    .lut_effects
                    .iter_mut()
                    .find(|e| e.file_param == file_param)
                {
                    e.path = path;
                    self.structure_dirty = true;
                }
            }
            EngineCommand::ClockSourceChanged { source } => {
                if source != self.clock_source {
                    self.clock_source = source;
                    self.clock_connected = false;
                    self.clock_running = false;
                    self.phase_synced = false;
                    self.bar_phase_unknown();
                    self.tempo = TempoEstimator::new();
                    self.feed_resolume_state();
                    self.notice(match source {
                        ClockSourceKind::Resolume => {
                            "clock: following Resolume's tempo (press Resync in Resolume to line up)"
                                .to_owned()
                        }
                        ClockSourceKind::Midi => match &self.clock_port {
                            Some(p) => format!("clock: MIDI clock on \"{p}\""),
                            None => "clock: MIDI clock, no port selected".to_owned(),
                        },
                    });
                }
            }
            EngineCommand::ClockPortChanged { port } if self.follows_resolume() => {
                // Only remembered for switching back to MIDI.
                self.clock_port = port;
                self.runtime_dirty = true;
            }
            EngineCommand::ClockPortChanged { port } => {
                self.clock_connected = false;
                self.clock_running = false;
                self.tempo = TempoEstimator::new();
                self.notice(match &port {
                    Some(p) => format!("MIDI clock: listening on \"{p}\""),
                    None => "MIDI clock: no port selected".to_owned(),
                });
                self.clock_port = port;
            }
            EngineCommand::SyncSeqLayer { remove_orphans } => self.sync_seq_layer(remove_orphans),
            EngineCommand::ReloadComposition => {
                self.notice("reloading the composition from Resolume");
                let _ = self.ws_cmd.send(WsCommand::Reconnect);
            }
            EngineCommand::Undo => {
                self.record_undo();
                if let (Some(prev), Some(cur)) = (self.undo.pop(), self.model.clone()) {
                    self.redo.push(cur);
                    self.set_model(prev);
                }
            }
            EngineCommand::Redo => {
                self.record_undo();
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
        // Lane mode changes convert steps against the clips of the sequence's deck (SPEC §9.3).
        let index = match op {
            EditOp::SetLaneMode { seq, .. } => model
                .find_sequence(*seq)
                .and_then(|(deck, _)| self.deck_index(deck)),
            _ => None,
        };
        match seq_core::edit::apply_with(model, op, index) {
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

    /// Clips of a deck: the live index, or the last one seen for a deck that is not live.
    fn deck_index(&self, deck: &str) -> Option<&CompositionIndex> {
        self.index
            .as_ref()
            .filter(|i| i.live_deck.as_deref() == Some(deck))
            .or_else(|| self.deck_cache.get(deck))
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

    /// Resolves and starts a sequence of the live deck by its start mode (SPEC §3.2). Starting a
    /// sequence that already waits for its boundary starts it at once.
    fn start_now(&mut self, seq: Uuid, origin: Origin) {
        let at_once = self.scheduler.is_pending(seq);
        let Some((live, sequence, spec)) = self.run_spec(seq) else {
            return;
        };
        let mode = if at_once {
            Start::Immediate
        } else {
            sequence.start
        };
        let name = sequence.name.clone();
        self.queued.insert(seq, (live, sequence));
        match self.scheduler.start(spec, origin, mode) {
            Ok(acts) => self.execute(acts),
            Err(e) => {
                self.queued.remove(&seq);
                self.notice(format!("cannot start {name:?}: {e}"));
            }
        }
        self.runtime_dirty = true;
    }

    /// Cues a sequence of the live deck (SPEC §3.3).
    fn cue(&mut self, seq: Uuid, at: CuePoint) {
        let Some((live, sequence, spec)) = self.run_spec(seq) else {
            return;
        };
        let wait = match at {
            CuePoint::StartMode => Wait::Mode(sequence.start),
            CuePoint::Bars8 => Wait::Mode(Start::Bar { bars: 8 }),
            CuePoint::Bars16 => Wait::Mode(Start::Bar { bars: 16 }),
            CuePoint::EndOfPass { of } => Wait::EndOfPass(of),
        };
        let name = sequence.name.clone();
        self.queued.insert(seq, (live, sequence));
        match self.scheduler.cue(spec, Origin::Ui, wait) {
            Ok(acts) => {
                log::info!("cued {name:?} ({at:?})");
                self.execute(acts);
            }
            Err(e) => {
                self.queued.remove(&seq);
                self.notice(format!("cannot cue {name:?}: {e}"));
            }
        }
        self.runtime_dirty = true;
    }

    /// The snapshot and run spec of a sequence of the live deck, resolved now (ids refreshed and
    /// saved). `None` with a notice when it is not in the live deck.
    fn run_spec(&mut self, seq: Uuid) -> Option<(String, Sequence, RunSpec)> {
        let index = self.index.clone()?;
        let live = index.live_deck.clone()?;
        let model = self.model.as_mut()?;
        let Some(sequence) = model
            .decks
            .get_mut(&live)
            .and_then(|d| d.sequences.iter_mut().find(|s| s.id == seq))
        else {
            self.notice("sequence is not in the live deck");
            return None;
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
        // An armed sequence loops while it records, an empty one over its pattern length (SPEC §7.9).
        if let Some(min_ticks) = self.rec.loops(seq) {
            spec.repeat = seq_core::Repeat::Infinite;
            spec.min_ticks = min_ticks;
        }
        // A fresh seed per start for `random` picks (SPEC §6); seq-core itself reads no clock.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        spec.seed = nanos ^ seq.as_u128() as u64;
        for (lane, run_lane) in sequence.lanes.iter().zip(spec.lanes.iter_mut()) {
            if lane.layer == self.cfg.seq_layer {
                run_lane.layer_id = None; // lanes may not play on the SEQ layer
            }
        }
        Some((live, sequence, spec))
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
                Action::TriggerSlot {
                    seq,
                    lane,
                    step,
                    column,
                    layer_id,
                } => self.trigger_slot(seq, lane, step, column, layer_id),
                Action::Clear {
                    seq,
                    lane,
                    layer_id,
                } => self.clear_lane(seq, lane, layer_id),
                Action::Ended {
                    seq,
                    reason,
                    origin,
                } => {
                    self.record_ended(seq);
                    self.snapshots.remove(&seq);
                    self.transitions_run_ended(seq);
                    self.pending.retain(|p| p.seq != seq);
                    self.lane_last_clip.retain(|(s, _), _| *s != seq);
                    if let (EndReason::Finished, Origin::SeqClip { clip_id }) = (reason, origin) {
                        self.clear_finished_seq_clip(clip_id);
                    }
                    self.runtime_dirty = true;
                }
                Action::Started { seq, .. } => {
                    if let Some(snapshot) = self.queued.remove(&seq) {
                        self.not_confirmed.retain(|(s, _), _| *s != seq);
                        self.snapshots.insert(seq, snapshot);
                    }
                    self.record_started(seq);
                    self.runtime_dirty = true;
                }
                Action::PassStarted { seq, pass } => {
                    self.record_pass(seq, pass);
                    self.runtime_dirty = true;
                }
                Action::Pending { seq, .. } => {
                    if let Some(p) = self.scheduler.pending().iter().find(|p| p.seq == seq) {
                        log::info!(
                            "{:?} starts in {} tick(s) ({:?}{})",
                            p.name,
                            p.wait_ticks,
                            p.wait,
                            if p.cue { ", cue" } else { "" }
                        );
                    }
                    self.runtime_dirty = true;
                }
                Action::LaneMuteChanged { seq, lane, silent } => {
                    let layer = self
                        .snapshots
                        .get(&seq)
                        .and_then(|(_, s)| s.lanes.get(lane))
                        .map(|l| l.layer.clone())
                        .unwrap_or_default();
                    let (bar, beat) = self
                        .scheduler
                        .status()
                        .iter()
                        .find(|r| r.seq == seq)
                        .map(|r| bar_beat(r.pos))
                        .unwrap_or((1, 1));
                    if silent {
                        log::info!("{layer} muted at {bar}.{beat}");
                    } else {
                        log::info!("{layer} unmuted at {bar}.{beat} (plays from its next step)");
                    }
                    self.runtime_dirty = true;
                }
                Action::PendingCancelled { seq, .. } => {
                    self.queued.remove(&seq);
                    self.runtime_dirty = true;
                }
                Action::StepStarted { .. } | Action::LaneStateChanged { .. } => {
                    self.runtime_dirty = true
                }
                Action::SetTransition {
                    seq,
                    lane,
                    step,
                    layer_id,
                    fade,
                } => self.set_transition(seq, lane, step, layer_id, &fade),
                Action::TransitionReleased { layer_id } => self.release_transition(layer_id),
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
        self.send_trigger(seq, lane, step, clip_id, layer, column, layer_id);
    }

    /// Slot lane step (SPEC §4.5): trigger the slot by position, or clear the lane's layer when
    /// the slot is empty or beyond the last column. The slot's content comes from the index,
    /// which the slot read before this step has brought up to date (if it arrived in time).
    fn trigger_slot(&mut self, seq: Uuid, lane: usize, step: usize, column: u32, layer_id: u64) {
        let Some(index) = &self.index else { return };
        let Some(layer) = index
            .layers
            .iter()
            .find(|l| l.id == layer_id)
            .map(|l| l.index)
        else {
            return;
        };
        match seq_core::resolve::slot_clip(index, layer, column) {
            Some(c) => self.send_trigger(
                seq,
                lane,
                step,
                c.clip_id,
                layer,
                column as usize,
                Some(layer_id),
            ),
            None => self.clear_lane(seq, lane, layer_id),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn send_trigger(
        &mut self,
        seq: Uuid,
        lane: usize,
        step: usize,
        clip_id: u64,
        layer: usize,
        column: usize,
        layer_id: Option<u64>,
    ) {
        let t0 = Instant::now();
        match self.osc.connect_clip(layer, column) {
            Ok(addr) => self.osc_sent(addr, t0.elapsed()),
            Err(e) => return self.osc_error(e),
        }
        if let Some(id) = layer_id {
            self.last_layer_trigger.insert(id, t0);
        }
        self.lane_last_clip.insert((seq, lane), clip_id);
        if self.states.get(&clip_id).is_some_and(|s| s.is_connected()) {
            // Re-trigger of a playing clip: Resolume reports no change; counts as confirmed.
            return;
        }
        self.late.retain(|p| (p.seq, p.lane) != (seq, lane));
        self.pending.push(Pending {
            seq,
            lane,
            step,
            clip_id,
            sent: t0,
        });
    }

    /// A lane's clear (rest, lane end, after last pass, stop all): clears the lane's layer and,
    /// if the clip this lane started lives on another layer and is still playing, that layer too
    /// ("stop what the lane started", decided 2026-10-26).
    fn clear_lane(&mut self, seq: Uuid, lane: usize, layer_id: u64) {
        self.clear_layer(layer_id);
        let Some(clip_id) = self.lane_last_clip.get(&(seq, lane)).copied() else {
            return;
        };
        if !self.states.get(&clip_id).is_some_and(|s| s.is_connected()) {
            return;
        }
        let other = self.index.as_ref().and_then(|i| {
            let clip = i.clip_by_id(clip_id)?;
            i.layer_by_index(clip.layer).map(|l| l.id)
        });
        if let Some(other) = other.filter(|o| *o != layer_id) {
            self.clear_layer(other);
        }
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
        self.record_own_clear(layer_id, t0);
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
        log::debug!(
            "OSC → {addr} (sent in {:.1} ms)",
            took.as_secs_f64() * 1000.0
        );
        self.osc_ok = true;
        self.last_osc = Some(OscView {
            addr,
            send_us: took.as_micros() as u64,
        });
        self.runtime_dirty = true;
    }

    /// Several triggers in a row went unanswered although Resolume was listening on the WebSocket.
    fn osc_unanswered(&self) -> bool {
        self.unanswered_triggers >= OSC_UNANSWERED_AFTER
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
            Aux::SlotRead { request, result } => self.on_slot_read(request, result),
            Aux::SeqSynced(result) => {
                self.seq_sync_busy = false;
                self.runtime_dirty = true;
                let text = match result {
                    Ok(report) => report.summary(&self.cfg.seq_layer),
                    Err(e) => format!("Sync {} layer failed: {e}", self.cfg.seq_layer),
                };
                self.notice(text);
            }
        }
    }

    // ---- slot lanes (SPEC §4.5) ----

    /// Asks for a read of every slot whose step starts ~150 ms from now. Called before each
    /// tick is applied; each upcoming step is reported once by the scheduler.
    fn request_upcoming_slots(&mut self, now: Instant) {
        let (Some(tx), Some(index)) = (&self.slot_reads, &self.index) else {
            return;
        };
        let tick = self
            .tempo
            .bpm()
            .filter(|b| *b > 0.0)
            .map_or(Duration::from_millis(20), |bpm| {
                Duration::from_secs_f64(60.0 / bpm / 24.0)
            });
        let ahead = (SLOT_LOOKAHEAD.as_secs_f64() / tick.as_secs_f64()).round() as u64;
        for up in self.scheduler.slot_steps_ahead(ahead.clamp(1, 96)) {
            let Some(layer) = index.layers.iter().find(|l| l.id == up.layer_id) else {
                continue;
            };
            let _ = tx.send(SlotRequest {
                layer: layer.index,
                column: up.column as usize,
                layer_id: up.layer_id,
                sent: now,
            });
        }
    }

    /// Applies a slot read to the index. Resolume moves grid cells by swapping them (notes §2.1,
    /// §15), so a different id in the slot means that cell and the one the index had there
    /// traded places. A composition push newer than the request wins.
    fn on_slot_read(&mut self, req: SlotRequest, result: Option<(ClipInfo, Option<ClipState>)>) {
        let Some((info, state)) = result else { return };
        if req.sent < self.composition_at {
            return;
        }
        let Some(index) = &self.index else { return };
        if index.layer_by_index(req.layer).map(|l| l.id) != Some(req.layer_id) {
            return; // layers moved since the request
        }
        if let Some(st) = state {
            self.states.insert(info.id, st);
        }
        let mut clips = index.clips().to_vec();
        let Some(here) = clips
            .iter()
            .position(|c| c.layer == req.layer && c.column == req.column)
        else {
            return; // column not in the index; the next push brings it
        };
        if clips[here] == info {
            return;
        }
        if clips[here].id != info.id {
            if let Some(there) = clips.iter().position(|c| c.id == info.id) {
                let (layer, column) = (clips[there].layer, clips[there].column);
                clips[here].layer = layer;
                clips[here].column = column;
                clips.swap_remove(there);
            } else {
                clips.swap_remove(here);
            }
        } else {
            clips.swap_remove(here);
        }
        log::debug!(
            "slot {}/{} now holds clip {} {:?}",
            req.layer,
            req.column,
            info.id,
            info.name
        );
        clips.push(info);
        let index = self.index.take().expect("checked");
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

    // ---- timer ----

    pub fn on_timer(&mut self, now: Instant) {
        self.retime_transitions(now);
        self.record_settle(now);
        // Subscriptions sent while Resolume loads a composition can be refused ("Invalid parameter
        // path", without saying which) or go unanswered: ask again for the ones still missing.
        if self.ws_connected
            && !self.unacked.is_empty()
            && now.saturating_duration_since(self.unacked_since) > RESUBSCRIBE_AFTER
        {
            log::debug!(
                "re-subscribing {} unanswered parameter(s)",
                self.unacked.len()
            );
            self.unacked_since = now;
            let _ = self.ws_cmd.send(WsCommand::Resubscribe(
                self.unacked.iter().copied().collect(),
            ));
        }
        // Confirmation timeouts (SPEC §7.5).
        let timeout = self.cfg.confirm_timeout;
        let mut late = Vec::new();
        self.pending.retain(|p| {
            if now.saturating_duration_since(p.sent) > timeout {
                late.push((p.seq, p.lane, p.step, p.clip_id, p.sent));
                false
            } else {
                true
            }
        });
        self.late
            .retain(|p| now.saturating_duration_since(p.sent) < LATE_LIMIT);
        for (seq, lane, step, clip_id, sent) in late {
            self.late.push(Pending {
                seq,
                lane,
                step,
                clip_id,
                sent,
            });
            log::warn!("trigger of clip {clip_id} (lane {lane}, step {step}) not confirmed");
            self.not_confirmed
                .entry((seq, lane))
                .or_default()
                .push(step);
            self.runtime_dirty = true;
            // Only when Resolume could have confirmed it: WebSocket up and every subscription answered.
            if self.ws_connected && self.unacked.is_empty() {
                self.unanswered_triggers += 1;
                if self.unanswered_triggers == OSC_UNANSWERED_AFTER {
                    self.notice(
                        "Resolume doesn't react to OSC triggers: is OSC Input on in Resolume \
                         (Preferences → OSC), on the port the sequencer sends to?",
                    );
                }
            }
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
            let play = self.tempo_ctl.as_ref().map(|t| t.play.as_str());
            self.notice(match (self.follows_resolume(), self.ws_connected, play) {
                (true, true, Some("Pause")) => "Resolume paused: sequences frozen",
                (true, true, Some("Stop")) => "Resolume stopped: sequences frozen",
                (true, false, _) => "Resolume not connected: sequences frozen",
                _ => "no clock: sequences frozen",
            });
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
        let live_index = index.and_then(|i| i.decks.iter().find(|d| d.selected).map(|d| d.index));
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
            let ambiguous = resolume_decks.iter().filter(|(n, _)| *n == name).count() > 1;
            let live = match live_index {
                // Decks with the same name: only the selected one is live.
                Some(li) => deck_index == Some(li),
                None => index.and_then(|i| i.live_deck.as_deref()) == Some(name.as_str()),
            };
            let deck_idx = if live {
                index
            } else if ambiguous {
                None // the cache is per name: it may hold the other deck's clips
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
                            pass_sixteenths: s.pass_length(),
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
            let seq_sync = match (live, index) {
                (true, Some(i)) => Some(seq_core::seq_sync::plan(
                    i,
                    &self.cfg.seq_layer,
                    &self.cfg.seq_prefix,
                    &self.live_sequence_names(&name),
                    |id| self.states.get(&id).is_some_and(|s| s.is_connected()),
                )),
                _ => None,
            };
            decks.push(DeckView {
                seq_sync,
                orphaned: deck_index.is_none(),
                ambiguous,
                index: deck_index,
                live,
                clips_source,
                layers: deck_idx
                    .map(|i| layer_views(i, &self.cfg.seq_layer))
                    .unwrap_or_default(),
                columns: deck_idx.map_or(0, |i| i.columns.len()),
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
            lut_effects: self.lut_effects.clone(),
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
                            mute: l.mute,
                            solo: l.solo,
                            silent: l.silent,
                            option: l.option,
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
        let pending = self
            .scheduler
            .pending()
            .into_iter()
            .map(|p| PendingView {
                seq: p.seq,
                name: p.name,
                from_seq_clip: origin_is_seq_clip(p.origin),
                mode: match p.wait {
                    Wait::Mode(mode) => Some(mode),
                    Wait::EndOfPass(_) => None,
                },
                end_of_pass: match p.wait {
                    Wait::EndOfPass(of) => Some(of),
                    Wait::Mode(_) => None,
                },
                cue: p.cue,
                wait_ticks: p.wait_ticks,
            })
            .collect();
        RuntimeState {
            clock: if self.follows_resolume() {
                let ctl = self.tempo_ctl.as_ref().filter(|_| self.ws_connected);
                ClockView {
                    source: self.clock_source,
                    port: self.clock_port.clone(),
                    connected: ctl.is_some(),
                    running: self.clock_running,
                    bpm: ctl.map(|t| t.bpm),
                    transport: ctl.map(|t| t.play.clone()),
                    synced: self.phase_synced,
                    bar_synced: self.bar_synced,
                }
            } else {
                ClockView {
                    source: self.clock_source,
                    port: self.clock_port.clone(),
                    connected: self.clock_connected,
                    running: self.clock_running,
                    bpm: self.clock_running.then(|| self.tempo.bpm()).flatten(),
                    transport: None,
                    synced: false,
                    bar_synced: self.bar_synced,
                }
            },
            ws: self.ws_connected,
            ws_synced: self.ws_connected && self.index.is_some() && self.unacked.is_empty(),
            rest: self.rest_ok,
            osc: self.osc_ok,
            osc_unanswered: self.osc_unanswered(),
            running,
            pending,
            switching: self.switch.as_ref().map(|s| SwitchView {
                seq: s.seq,
                deck: s.deck.clone(),
            }),
            last_osc: self.last_osc.clone(),
            last_confirm: self.last_confirm.clone(),
            confirmations: self.confirmations,
            notices: self.notices.iter().cloned().collect(),
            saved: self.saved,
            save_error: self.save_error.clone(),
            seq_sync_busy: self.seq_sync_busy,
            record: self.rec.view(),
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
                if let Some(c) = step
                    .current_clip()
                    .and_then(|c| index.clip_by_id(c.clip_id))
                {
                    c.id.hash(&mut h);
                    c.thumbnail.last_update.hash(&mut h);
                }
            }
        }
    }
    format!("{:016x}", h.finish())
}

/// Worker for [`SlotRequest`]s: each read runs as its own task so a slow one never delays the
/// next; results come back to the engine as [`Aux::SlotRead`].
pub fn spawn_slot_reader(
    rest: resolume_client::rest::RestClient,
    aux: mpsc::UnboundedSender<Aux>,
) -> mpsc::UnboundedSender<SlotRequest> {
    let (tx, mut rx) = mpsc::unbounded_channel::<SlotRequest>();
    tokio::spawn(async move {
        while let Some(request) = rx.recv().await {
            let rest = rest.clone();
            let aux = aux.clone();
            tokio::spawn(async move {
                let result = match rest.clip_at(request.layer, request.column).await {
                    Ok(r) => Some(r),
                    Err(e) => {
                        log::debug!("slot read {}/{}: {e}", request.layer, request.column);
                        None
                    }
                };
                let _ = aux.send(Aux::SlotRead { request, result });
            });
        }
    });
    tx
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
    // Shutting down: running sequences end as if stopped (SPEC §7.4: `after_last_pass`).
    let acts = engine.scheduler.stop_all(StopMode::AfterLastPass);
    engine.execute(acts);
    engine.on_timer(Instant::now());
}
