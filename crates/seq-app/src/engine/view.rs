//! UI-facing state (SPEC §9.6), serialized to the browser.
//!
//! - [`StructureState`]: decks, sequences, clip browser data, resolution results. Published when
//!   something structural changes.
//! - [`RuntimeState`]: connections, clock, running sequences, notices. Published at most 30 times
//!   per second while it changes.

use std::collections::BTreeMap;

use serde::Serialize;
use uuid::Uuid;

use seq_core::index::CompositionIndex;
use seq_core::resolve::{LayerResolution, Methods, ResolvedBy, SequenceResolution, StepResolution};
use seq_core::scheduler::{LaneState, Origin};
use seq_core::Sequence;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StructureState {
    pub composition: Option<String>,
    pub live_deck: Option<String>,
    pub decks: Vec<DeckView>,
    /// Problem with the sequence file (it is then read-only).
    pub model_error: Option<String>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub seq_layer: String,
    pub seq_prefix: String,
    /// Resolume's LUT effects (composition, layers, live deck's clips) with their file paths (SPEC §14).
    pub lut_effects: Vec<resolume_client::composition::LutEffect>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeckView {
    pub name: String,
    /// 1-based Resolume deck position; `None` for orphaned decks (only in the file).
    pub index: Option<usize>,
    pub live: bool,
    /// The deck exists in the file but not in Resolume.
    pub orphaned: bool,
    /// Several Resolume decks carry this name. Sequences are stored per deck name, so they all
    /// share one set; the UI asks to rename them.
    pub ambiguous: bool,
    /// Clip data: live, cached from the last time the deck was live, or none.
    pub clips_source: ClipsSource,
    pub layers: Vec<LayerView>,
    /// Number of columns in the deck's clip data (0 without clip data); slot lane steps pick a
    /// column up to this (SPEC §4.5).
    pub columns: usize,
    pub sequences: Vec<SequenceView>,
    /// `§` clips on the SEQ layer whose name matches no sequence of this deck.
    pub unmatched_seq_clips: Vec<String>,
    /// What "Sync SEQ layer" would do (SPEC §7.6); only for the live deck.
    pub seq_sync: Option<seq_core::seq_sync::SeqSyncPlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipsSource {
    Live,
    Cached,
    None,
}

/// Clip browser row: one layer with its clips (empty slots left out).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayerView {
    pub id: u64,
    pub index: usize,
    pub name: String,
    /// More than one layer carries this name; lanes cannot target it.
    pub ambiguous: bool,
    pub is_seq_layer: bool,
    pub clips: Vec<ClipView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipView {
    pub id: u64,
    pub column: usize,
    pub name: String,
    pub file: Option<String>,
    /// Change marker for the thumbnail URL (`/thumbs/clip/{id}.png?v=...`).
    pub thumb: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SequenceView {
    pub sequence: Sequence,
    pub pass_sixteenths: u64,
    /// Its `§` clip on the SEQ layer of this deck, if the deck's clips are known.
    pub binding: Option<Binding>,
    pub resolution: Option<ResolutionView>,
    /// Changes whenever the mosaic would look different (`/thumbs/seq/{id}.png?v=...`).
    pub mosaic_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Binding {
    pub column: usize,
    pub clip_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolutionView {
    pub complete: bool,
    pub lanes: Vec<LaneResolutionView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LaneResolutionView {
    /// `ok`, `missing`, `ambiguous` or `seq_layer`.
    pub layer: &'static str,
    pub steps: Vec<StepResolutionView>,
    /// Per step, its `choices` (options 2…, format v4), resolved one by one; empty without choices.
    pub choices: Vec<Vec<StepResolutionView>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepResolutionView {
    /// `rest`, `ok` or `unresolved`; slot lane steps (SPEC §4.5): `slot` (the slot holds
    /// `clip_id`) or `empty` (plays as a rest).
    pub state: &'static str,
    pub clip_id: Option<u64>,
    pub by: Option<&'static str>,
    pub ambiguous: bool,
    pub other_layer: bool,
    pub methods: MethodsView,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct MethodsView {
    pub id: Option<bool>,
    pub name: Option<bool>,
    pub file: Option<bool>,
}

impl From<Methods> for MethodsView {
    fn from(m: Methods) -> Self {
        Self {
            id: m.id,
            name: m.name,
            file: m.file,
        }
    }
}

fn step_view(s: &StepResolution) -> StepResolutionView {
    match s {
        StepResolution::Rest => StepResolutionView {
            state: "rest",
            clip_id: None,
            by: None,
            ambiguous: false,
            other_layer: false,
            methods: MethodsView::default(),
        },
        StepResolution::Resolved { clip, methods } => StepResolutionView {
            state: "ok",
            clip_id: Some(clip.clip_id),
            by: Some(match clip.by {
                ResolvedBy::Id => "id",
                ResolvedBy::Name => "name",
                ResolvedBy::File => "file",
                ResolvedBy::Slot => "slot",
            }),
            ambiguous: clip.ambiguous,
            other_layer: !clip.on_lane_layer,
            methods: (*methods).into(),
        },
        StepResolution::Slot { clip, .. } => StepResolutionView {
            state: if clip.is_some() { "slot" } else { "empty" },
            clip_id: clip.map(|c| c.clip_id),
            by: clip.map(|_| "slot"),
            ambiguous: false,
            other_layer: false,
            methods: MethodsView::default(),
        },
        StepResolution::Unresolved { methods } => StepResolutionView {
            state: "unresolved",
            clip_id: None,
            by: None,
            ambiguous: false,
            other_layer: false,
            methods: (*methods).into(),
        },
    }
}

pub fn resolution_view(res: &SequenceResolution, seq_layer_lanes: &[bool]) -> ResolutionView {
    let lanes = res
        .lanes
        .iter()
        .enumerate()
        .map(|(i, l)| LaneResolutionView {
            layer: if seq_layer_lanes.get(i).copied().unwrap_or(false) {
                "seq_layer"
            } else {
                match l.layer {
                    LayerResolution::Found { .. } => "ok",
                    LayerResolution::Missing => "missing",
                    LayerResolution::Ambiguous { .. } => "ambiguous",
                }
            },
            steps: l.steps.iter().map(step_view).collect(),
            choices: l
                .choices
                .iter()
                .map(|c| c.iter().map(step_view).collect())
                .collect(),
        })
        .collect::<Vec<_>>();
    ResolutionView {
        complete: lanes.iter().all(|l| {
            l.layer == "ok"
                && l.steps
                    .iter()
                    .chain(l.choices.iter().flatten())
                    .all(|s| s.state != "unresolved")
        }),
        lanes,
    }
}

/// Version of a clip's thumbnail for the cache and the UI: Resolume's `last_update` plus a
/// fingerprint of the content. Media dropped onto a slot keeps the slot's id and, as seen in the
/// M13 probe (notes §15), its `last_update`, so the marker alone would keep the old picture.
pub fn thumb_version(c: &seq_core::index::ClipInfo) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    c.file_path.hash(&mut h);
    c.name.hash(&mut h);
    format!("{}x{:08x}", c.thumbnail.last_update, h.finish() as u32)
}

pub fn layer_views(index: &CompositionIndex, seq_layer: &str) -> Vec<LayerView> {
    index
        .layers
        .iter()
        .rev() // top layer first, as in Resolume
        .map(|l| LayerView {
            id: l.id,
            index: l.index,
            name: l.name.clone(),
            ambiguous: index.layers.iter().filter(|o| o.name == l.name).count() > 1,
            is_seq_layer: l.name == seq_layer,
            clips: index
                .filled_clips()
                .filter(|c| c.layer == l.index)
                .map(|c| ClipView {
                    id: c.id,
                    column: c.column,
                    name: c.name.clone(),
                    file: c.file_name().map(str::to_owned),
                    thumb: (!c.thumbnail.is_default).then(|| thumb_version(c)),
                })
                .collect(),
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RuntimeState {
    pub clock: ClockView,
    pub ws: bool,
    /// Every parameter subscription is acknowledged: clip state changes will be seen.
    pub ws_synced: bool,
    pub rest: bool,
    /// The last OSC send worked on this computer (UDP: says nothing about Resolume hearing it).
    pub osc: bool,
    /// Several triggers in a row were sent but never confirmed while the WebSocket was up: Resolume's
    /// OSC input is probably off or on another port.
    pub osc_unanswered: bool,
    pub running: Vec<RunView>,
    /// Starts waiting for their beat or bar (SPEC §3.2), in the order they were made.
    pub pending: Vec<PendingView>,
    /// Sequence being started after a deck switch.
    pub switching: Option<SwitchView>,
    pub last_osc: Option<OscView>,
    pub last_confirm: Option<String>,
    /// Triggers confirmed by Resolume since the app started.
    pub confirmations: u64,
    /// Recent notices for the status bar (newest last).
    pub notices: Vec<String>,
    /// Bumped on every save.
    pub saved: u64,
    pub save_error: Option<String>,
    /// "Sync SEQ layer" is running (SPEC §7.6).
    pub seq_sync_busy: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ClockView {
    /// `midi` or `resolume` (SPEC §3.1).
    pub source: crate::settings::ClockSourceKind,
    /// The MIDI port (also kept while the source is Resolume).
    pub port: Option<String>,
    /// MIDI: the port is open. Resolume: the WebSocket is connected and reports a tempo controller.
    pub connected: bool,
    pub running: bool,
    pub bpm: Option<f64>,
    /// Resolume source: `Play`, `Pause` or `Stop`, as Resolume reports it.
    pub transport: Option<String>,
    /// Resolume source: in phase with Resolume's beat since the last resync.
    pub synced: bool,
    /// The bar phase for quantized starts comes from a MIDI Start or a Resync (SPEC §3.2); false
    /// while it counts from the first tick (`bar phase not synced`).
    pub bar_synced: bool,
}

/// A start waiting for its boundary (SPEC §3.2).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingView {
    pub seq: Uuid,
    pub name: String,
    pub from_seq_clip: bool,
    /// The start mode boundary it waits for; `None` when a cue waits for `end_of_pass`.
    pub mode: Option<seq_core::Start>,
    /// A cue waiting for the end of the current pass of this running sequence (SPEC §3.3).
    pub end_of_pass: Option<Uuid>,
    /// A cue: what plays stops when it starts.
    pub cue: bool,
    /// Ticks until it starts, if the clock keeps running (24 per beat).
    pub wait_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SwitchView {
    pub seq: Uuid,
    pub deck: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OscView {
    pub addr: String,
    /// Microseconds the send call took.
    pub send_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunView {
    pub seq: Uuid,
    pub name: String,
    pub deck: Option<String>,
    pub from_seq_clip: bool,
    pub pass: u32,
    pub passes: Option<u32>,
    pub pos: u64,
    pub pass_ticks: u64,
    /// `bar.beat` of the position.
    pub bar_beat: String,
    pub lanes: Vec<RunLaneView>,
    /// The stored sequence differs from the running snapshot.
    pub edited: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunLaneView {
    /// Muted by hand, soloed, and whether it sends nothing now (SPEC §7.8).
    pub mute: bool,
    pub solo: bool,
    pub silent: bool,
    /// The option the current step played (0 = its own clip or column); `None` when its pass
    /// condition kept it from playing (format v4).
    pub option: Option<usize>,
    /// `idle`, `playing`, `yielded`, `resume_pending`, `taken_over`.
    pub state: &'static str,
    pub step: Option<usize>,
    /// Steps whose trigger was not confirmed by Resolume within the window.
    pub not_confirmed: Vec<usize>,
}

pub fn lane_state_name(s: LaneState) -> &'static str {
    match s {
        LaneState::Inactive => "idle",
        LaneState::Playing => "playing",
        LaneState::Yielded {
            resume_pending: false,
        } => "yielded",
        LaneState::Yielded {
            resume_pending: true,
        } => "resume_pending",
        LaneState::TakenOver => "taken_over",
    }
}

pub fn origin_is_seq_clip(o: Origin) -> bool {
    matches!(o, Origin::SeqClip { .. })
}

/// Deterministic map for JSON output.
pub type NotConfirmed = BTreeMap<(Uuid, usize), Vec<usize>>;
