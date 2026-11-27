//! Builds a [`CompositionIndex`] from Resolume's composition JSON.
//!
//! The same shape arrives from REST `GET /api/v1/composition` and as the untyped WebSocket
//! composition push (`docs/resolume-api-notes.md` §2, §5.3). Only the fields the sequencer needs
//! are deserialized; everything else is skipped, which keeps parsing a 250–380 KB push cheap.
//!
//! Do not feed this a REST response taken during a deck switch: Resolume returns mixed states
//! then (notes §4). Use the WebSocket push that follows the switch.

use std::collections::HashMap;

use seq_core::index::{
    ClipInfo, ClipState, ClipTransition, ColumnInfo, CompositionIndex, DeckInfo, LayerInfo,
    TextParam, Thumbnail, TransitionParams,
};
use seq_core::transition::LayerTransition;

/// Display name of the text parameter of the "1000A Sequence" FFGL source (SPEC §12).
pub const SEQUENCE_PARAM: &str = "Sequence";
/// Name of Resolume's built-in LUT effect and of its file parameter (notes §18).
pub const LUT_EFFECT: &str = "LUT";
pub const LUT_FILE_PARAM: &str = "LUT 3D";
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid composition JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// One composition push: structure plus the clips' connection states at that moment.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositionSnapshot {
    pub index: CompositionIndex,
    /// By clip id. Clips whose state string is unknown are left out.
    pub states: HashMap<u64, ClipState>,
    /// `tempocontroller` (notes §17); `None` if the composition JSON has none.
    pub tempo: Option<TempoController>,
    /// Resolume's own LUT effects on the composition, the layers and the live deck's clips, in that
    /// order (notes §18). Layer groups are not read (their JSON is not probed).
    pub lut_effects: Vec<LutEffect>,
    /// Each layer's transition at the time of the push, by layer id (notes §19).
    pub transitions: HashMap<u64, LayerTransition>,
}

/// One instance of Resolume's LUT effect: its `LUT 3D` file parameter and where it sits.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LutEffect {
    pub effect_id: u64,
    /// The `LUT 3D` `ParamFile`: set it to a path over the WebSocket to change the file.
    pub file_param: u64,
    /// The full path of the `.cube` file; empty if none is chosen.
    pub path: String,
    pub place: LutPlace,
}

/// Where a LUT effect sits. Indexes are 1-based, like everywhere else.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LutPlace {
    Composition,
    Layer {
        layer: usize,
        name: String,
    },
    Clip {
        layer: usize,
        column: usize,
        name: String,
    },
}

/// The composition's tempo controller (notes §17): parameter ids to subscribe to and the values at
/// the time of the push. Subscribe by id; Resolume refuses its paths.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoController {
    pub tempo_param: u64,
    pub bpm: f64,
    /// `play_state` in REST, `…/tempocontroller/play` over the WebSocket: `Play`, `Pause`, `Stop`.
    pub play_param: u64,
    pub play: String,
    /// A `ParamEvent`: one update on press and one on release, no value.
    pub resync_param: u64,
}

pub fn parse_composition(json: &str) -> Result<CompositionSnapshot, ParseError> {
    let raw: RawComposition = serde_json::from_str(json)?;
    Ok(build(raw))
}

/// Same as [`parse_composition`] for JSON that is already parsed (e.g. a WS message).
pub fn parse_composition_value(
    value: &serde_json::Value,
) -> Result<CompositionSnapshot, ParseError> {
    let raw = RawComposition::deserialize(value)?;
    Ok(build(raw))
}

/// One slot from `GET /composition/layers/{L}/clips/{C}` (notes §15), placed at `layer`/`column`
/// (1-based, the request's position). Returns its connection state with it.
pub fn parse_clip(
    json: &str,
    layer: usize,
    column: usize,
) -> Result<(ClipInfo, Option<ClipState>), ParseError> {
    let raw: RawClip = serde_json::from_str(json)?;
    Ok(clip_info(raw, layer, column, false))
}

/// `piano_default`: the composition's `cliptriggerstyle` is Piano (for clips that follow it).
fn clip_info(
    clip: RawClip,
    layer: usize,
    column: usize,
    piano_default: bool,
) -> (ClipInfo, Option<ClipState>) {
    let piano = match clip.triggerstyle.as_ref().map(|p| p.value.as_str()) {
        Some("Piano") => true,
        Some("Composition Determined") | None => piano_default,
        Some(_) => false,
    };
    let state = ClipState::from_resolume(&clip.connected.value);
    let (file_path, sequence_param) = match clip.video {
        Some(v) => (
            v.fileinfo.and_then(|f| f.path).filter(|p| !p.is_empty()),
            v.sourceparams
                .and_then(|mut ps| ps.remove(SEQUENCE_PARAM))
                .filter(|p| p.valuetype == "ParamString" && p.id != 0)
                .map(|p| TextParam {
                    id: p.id,
                    value: p.value.as_str().unwrap_or_default().to_owned(),
                }),
        ),
        None => (None, None),
    };
    let thumbnail = clip.thumbnail.unwrap_or_default();
    let (layer_determined, own_transition) = match clip.transition {
        Some(t) => {
            let ld = t.layer_determined.is_none_or(|p| p.value);
            let own = (!ld).then(|| ClipTransition {
                millis: t
                    .duration
                    .map_or(0, |d| (d.value.max(0.0) * 1000.0).round() as u32),
                blend: t.blend_mode.map(|b| b.value).unwrap_or_default(),
            });
            (ld, own)
        }
        None => (true, None),
    };
    let info = ClipInfo {
        id: clip.id,
        layer,
        column,
        name: clip.name.value,
        file_path,
        empty: state == Some(ClipState::Empty),
        thumbnail: Thumbnail {
            last_update: thumbnail.last_update,
            is_default: thumbnail.is_default,
        },
        connected_param: clip.connected.id,
        name_param: clip.name.id,
        sequence_param,
        layer_determined,
        own_transition,
        piano,
    };
    (info, state)
}

fn build(raw: RawComposition) -> CompositionSnapshot {
    let decks: Vec<DeckInfo> = raw
        .decks
        .iter()
        .enumerate()
        .map(|(i, d)| DeckInfo {
            id: d.id,
            index: i + 1,
            name: d.name.value.clone(),
            closed: d.closed,
            select_param: d.selected.id,
            selected: d.selected.value,
        })
        .collect();
    let live_deck = raw
        .decks
        .iter()
        .find(|d| d.selected.value)
        .map(|d| d.name.value.clone());
    let columns = raw
        .columns
        .into_iter()
        .enumerate()
        .map(|(i, c)| ColumnInfo {
            id: c.id,
            index: i + 1,
            name: c.name.value,
        })
        .collect();

    let mut layers = Vec::with_capacity(raw.layers.len());
    let mut clips = Vec::new();
    let mut states = HashMap::new();
    let mut lut_effects: Vec<LutEffect> = Vec::new();
    let mut transitions = HashMap::new();
    let piano_default = raw
        .cliptriggerstyle
        .as_ref()
        .is_some_and(|p| p.value == "Piano");
    let mut add_luts = |effects: Vec<serde_json::Value>, place: &dyn Fn() -> LutPlace| {
        lut_effects.extend(effects.into_iter().filter_map(|e| lut_effect(e, place())));
    };
    add_luts(raw.video.map(|v| v.effects).unwrap_or_default(), &|| {
        LutPlace::Composition
    });
    for (li, layer) in raw.layers.into_iter().enumerate() {
        let layer_index = li + 1;
        let layer_name = layer.name.value.clone();
        add_luts(layer.video.map(|v| v.effects).unwrap_or_default(), &|| {
            LutPlace::Layer {
                layer: layer_index,
                name: layer_name.clone(),
            }
        });
        let transition = layer.transition.and_then(|t| {
            let (d, b) = (t.duration?, t.blend_mode?);
            (d.id != 0 && b.id != 0).then(|| {
                transitions.insert(
                    layer.id,
                    LayerTransition {
                        seconds: d.value,
                        blend: b.value.clone(),
                    },
                );
                TransitionParams {
                    duration_param: d.id,
                    blend_param: b.id,
                    blend_options: b.options,
                }
            })
        });
        layers.push(LayerInfo {
            id: layer.id,
            index: layer_index,
            name: layer.name.value,
            name_param: layer.name.id,
            transition,
        });
        for (ci, mut clip) in layer.clips.into_iter().enumerate() {
            let effects = clip.video.as_mut().map(|v| std::mem::take(&mut v.effects));
            let clip_name = clip.name.value.clone();
            add_luts(effects.unwrap_or_default(), &|| LutPlace::Clip {
                layer: layer_index,
                column: ci + 1,
                name: clip_name.clone(),
            });
            let (info, state) = clip_info(clip, layer_index, ci + 1, piano_default);
            if let Some(s) = state {
                states.insert(info.id, s);
            }
            clips.push(info);
        }
    }

    let tempo = raw.tempocontroller.and_then(|t| {
        let tempo = t.tempo?;
        let play = t.play_state?;
        let resync = t.resync?;
        (tempo.id != 0 && play.id != 0 && resync.id != 0).then_some(TempoController {
            tempo_param: tempo.id,
            bpm: tempo.value,
            play_param: play.id,
            play: play.value,
            resync_param: resync.id,
        })
    });
    CompositionSnapshot {
        index: CompositionIndex::new(raw.name.value, decks, live_deck, layers, columns, clips),
        states,
        tempo,
        lut_effects,
        transitions,
    }
}

// Minimal mirror of the composition JSON. Field names as recorded in docs/samples/.

#[derive(Deserialize)]
struct RawComposition {
    name: Param<String>,
    #[serde(default)]
    decks: Vec<RawDeck>,
    #[serde(default)]
    layers: Vec<RawLayer>,
    #[serde(default)]
    columns: Vec<RawColumn>,
    #[serde(default)]
    tempocontroller: Option<RawTempoController>,
    /// The clips' default trigger style (notes §20).
    #[serde(default)]
    cliptriggerstyle: Option<Param<String>>,
    #[serde(default)]
    video: Option<RawEffects>,
}

/// The `video` object of the composition or a layer: only its effects are read.
#[derive(Deserialize)]
struct RawEffects {
    /// Kept as raw JSON and read one by one (`lut_effect`): effects vary a lot (a built-in "RenderPass" has
    /// `"params": null`), and one odd effect must never make the whole composition unreadable.
    #[serde(default)]
    effects: Vec<serde_json::Value>,
}

/// One LUT effect entry of `video.effects[]` (notes §13, §18).
#[derive(Deserialize)]
struct RawEffect {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    params: Option<HashMap<String, RawSourceParam>>,
}

/// Resolume's LUT effect with its `LUT 3D` file parameter, or `None` for any other (or unreadable) effect.
fn lut_effect(effect: serde_json::Value, place: LutPlace) -> Option<LutEffect> {
    if effect.get("name").and_then(|n| n.as_str()) != Some(LUT_EFFECT) {
        return None;
    }
    let raw = RawEffect::deserialize(effect).ok()?;
    let p = raw.params?.remove(LUT_FILE_PARAM)?;
    (p.valuetype == "ParamFile" && p.id != 0).then(|| LutEffect {
        effect_id: raw.id,
        file_param: p.id,
        path: p.value.as_str().unwrap_or_default().to_owned(),
        place,
    })
}

#[derive(Deserialize)]
struct RawTempoController {
    #[serde(default)]
    tempo: Option<Param<f64>>,
    #[serde(default)]
    play_state: Option<Param<String>>,
    #[serde(default)]
    resync: Option<RawEvent>,
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(default)]
    id: u64,
}

#[derive(Deserialize)]
struct Param<T> {
    #[serde(default)]
    id: u64,
    value: T,
}

#[derive(Deserialize)]
struct RawDeck {
    id: u64,
    name: Param<String>,
    selected: Param<bool>,
    #[serde(default)]
    closed: bool,
}

#[derive(Deserialize)]
struct RawColumn {
    id: u64,
    name: Param<String>,
}

#[derive(Deserialize)]
struct RawLayer {
    id: u64,
    name: Param<String>,
    #[serde(default)]
    clips: Vec<RawClip>,
    #[serde(default)]
    video: Option<RawEffects>,
    #[serde(default)]
    transition: Option<RawLayerTransition>,
}

/// A layer's `transition` (notes §19).
#[derive(Deserialize)]
struct RawLayerTransition {
    #[serde(default)]
    duration: Option<Param<f64>>,
    #[serde(default)]
    blend_mode: Option<RawChoice>,
}

#[derive(Deserialize)]
struct RawChoice {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    value: String,
    #[serde(default)]
    options: Vec<String>,
}

/// A clip's `transition`: only whether it follows the layer.
#[derive(Deserialize)]
struct RawClipTransition {
    #[serde(default)]
    layer_determined: Option<Param<bool>>,
    #[serde(default)]
    duration: Option<Param<f64>>,
    #[serde(default)]
    blend_mode: Option<RawChoice>,
}

#[derive(Deserialize)]
struct RawClip {
    id: u64,
    name: Param<String>,
    connected: Param<String>,
    /// `null` for empty slots.
    #[serde(default)]
    video: Option<RawVideo>,
    #[serde(default)]
    thumbnail: Option<RawThumbnail>,
    #[serde(default)]
    transition: Option<RawClipTransition>,
    #[serde(default)]
    triggerstyle: Option<Param<String>>,
}

#[derive(Deserialize)]
struct RawVideo {
    /// `null` for sources (generators).
    #[serde(default)]
    fileinfo: Option<RawFileInfo>,
    /// Parameters of a source plugin, keyed by display name (notes §11).
    #[serde(default)]
    sourceparams: Option<std::collections::HashMap<String, RawSourceParam>>,
    #[serde(default)]
    effects: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawSourceParam {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    valuetype: String,
    #[serde(default)]
    value: serde_json::Value,
}

#[derive(Deserialize)]
struct RawFileInfo {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct RawThumbnail {
    #[serde(default)]
    last_update: String,
    #[serde(default)]
    is_default: bool,
}

impl Default for RawThumbnail {
    fn default() -> Self {
        Self {
            last_update: "0".into(),
            is_default: true,
        }
    }
}
