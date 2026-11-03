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
    ClipInfo, ClipState, ColumnInfo, CompositionIndex, DeckInfo, LayerInfo, TextParam, Thumbnail,
};

/// Display name of the text parameter of the "1000A Sequence" FFGL source (SPEC §12).
pub const SEQUENCE_PARAM: &str = "Sequence";
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid composition JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// One composition push: structure plus the clips' connection states at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionSnapshot {
    pub index: CompositionIndex,
    /// By clip id. Clips whose state string is unknown are left out.
    pub states: HashMap<u64, ClipState>,
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

/// One slot from `GET /composition/layers/{L}/clips/{C}` (notes §14), placed at `layer`/`column`
/// (1-based, the request's position). Returns its connection state with it.
pub fn parse_clip(
    json: &str,
    layer: usize,
    column: usize,
) -> Result<(ClipInfo, Option<ClipState>), ParseError> {
    let raw: RawClip = serde_json::from_str(json)?;
    Ok(clip_info(raw, layer, column))
}

fn clip_info(clip: RawClip, layer: usize, column: usize) -> (ClipInfo, Option<ClipState>) {
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
    for (li, layer) in raw.layers.into_iter().enumerate() {
        let layer_index = li + 1;
        layers.push(LayerInfo {
            id: layer.id,
            index: layer_index,
            name: layer.name.value,
            name_param: layer.name.id,
        });
        for (ci, clip) in layer.clips.into_iter().enumerate() {
            let (info, state) = clip_info(clip, layer_index, ci + 1);
            if let Some(s) = state {
                states.insert(info.id, s);
            }
            clips.push(info);
        }
    }

    CompositionSnapshot {
        index: CompositionIndex::new(raw.name.value, decks, live_deck, layers, columns, clips),
        states,
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
}

#[derive(Deserialize)]
struct RawVideo {
    /// `null` for sources (generators).
    #[serde(default)]
    fileinfo: Option<RawFileInfo>,
    /// Parameters of a source plugin, keyed by display name (notes §11).
    #[serde(default)]
    sourceparams: Option<std::collections::HashMap<String, RawSourceParam>>,
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
