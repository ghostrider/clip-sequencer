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
    pub sequences: Vec<SequenceView>,
    /// `§` clips on the SEQ layer whose name matches no sequence of this deck.
    pub unmatched_seq_clips: Vec<String>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepResolutionView {
    /// `rest`, `ok` or `unresolved`.
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
            steps: l
                .steps
                .iter()
                .map(|s| match s {
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
                        }),
                        ambiguous: clip.ambiguous,
                        other_layer: !clip.on_lane_layer,
                        methods: (*methods).into(),
                    },
                    StepResolution::Unresolved { methods } => StepResolutionView {
                        state: "unresolved",
                        clip_id: None,
                        by: None,
                        ambiguous: false,
                        other_layer: false,
                        methods: (*methods).into(),
                    },
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    ResolutionView {
        complete: lanes
            .iter()
            .all(|l| l.layer == "ok" && l.steps.iter().all(|s| s.state != "unresolved")),
        lanes,
    }
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
                    thumb: (!c.thumbnail.is_default).then(|| c.thumbnail.last_update.clone()),
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
    pub osc: bool,
    pub running: Vec<RunView>,
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
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ClockView {
    pub port: Option<String>,
    pub connected: bool,
    pub running: bool,
    pub bpm: Option<f64>,
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
