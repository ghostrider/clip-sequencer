//! Composition index: the live deck's layers, columns and clips as plain values (SPEC §4.2).
//!
//! Built by `resolume-client` from Resolume's composition JSON; consumed here for clip
//! resolution and later by the scheduler. Indices (`layer`, `column`) are 1-based like Resolume's
//! REST and OSC addresses, and are valid only for this snapshot: they shift when layers or
//! columns are inserted, which is why sequences never store them.
//!
//! The index holds structure only. Volatile state (which clip is connected) lives in
//! [`ClipState`] maps next to it, so `old_index == new_index` tells whether a composition push
//! changed anything that matters for resolution.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A Resolume deck. Only the live deck's layers and clips are visible in an index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckInfo {
    pub id: u64,
    /// 1-based position, as used by `/composition/decks/{n}/select`.
    pub index: usize,
    pub name: String,
    pub closed: bool,
    /// Parameter id of the deck's `selected` flag (WS subscriptions).
    pub select_param: u64,
    /// The live deck. Tells decks with the same name apart; `live_deck` only holds the name.
    #[serde(default)]
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerInfo {
    pub id: u64,
    /// 1-based; layer 1 is the bottom layer in Resolume's UI.
    pub index: usize,
    /// Raw name as the API returns it (unnamed layers are all `"Layer #"`).
    pub name: String,
    pub name_param: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub id: u64,
    pub index: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thumbnail {
    /// Change marker, a millisecond timestamp as a string; `"0"` when there is no own thumbnail.
    pub last_update: String,
    pub is_default: bool,
}

/// One clip slot. Empty slots are included (they have ids too) but never match a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipInfo {
    /// Identifies the grid cell object: follows drag/swap/move, but survives loading other
    /// media into the slot and clearing it (`docs/resolume-api-notes.md` §2.1).
    pub id: u64,
    pub layer: usize,
    pub column: usize,
    pub name: String,
    /// Full media path for file clips; `None` for sources (generators) and empty slots.
    pub file_path: Option<String>,
    pub empty: bool,
    pub thumbnail: Thumbnail,
    pub connected_param: u64,
    pub name_param: u64,
    /// The `Sequence` text parameter of the "1000A Sequence" FFGL source, when the clip holds that
    /// plugin (SPEC §12). The sequencer keeps it set to the clip's sequence name.
    #[serde(default)]
    pub sequence_param: Option<TextParam>,
}

/// A text parameter of a clip's source plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextParam {
    pub id: u64,
    pub value: String,
}

impl ClipInfo {
    /// File name part of `file_path` (what [`crate::ClipRef::file`] stores).
    pub fn file_name(&self) -> Option<&str> {
        self.file_path.as_deref().map(file_name_of)
    }
}

/// Last component of a Windows or POSIX path.
pub fn file_name_of(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Connection state of a clip, as Resolume reports it in `clip.connected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipState {
    Empty,
    Disconnected,
    Previewing,
    Connected,
    ConnectedAndPreviewing,
}

impl ClipState {
    /// Parses Resolume's `ParamState` value string.
    pub fn from_resolume(value: &str) -> Option<Self> {
        Some(match value {
            "Empty" => Self::Empty,
            "Disconnected" => Self::Disconnected,
            "Previewing" => Self::Previewing,
            "Connected" => Self::Connected,
            "Connected & previewing" => Self::ConnectedAndPreviewing,
            _ => return None,
        })
    }

    /// Playing on the output (previewing alone does not count).
    pub fn is_connected(self) -> bool {
        matches!(self, Self::Connected | Self::ConnectedAndPreviewing)
    }
}

/// Result of looking up a layer by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerLookup<'a> {
    Found(&'a LayerInfo),
    Missing,
    /// Several layers carry this name (typically unnamed `"Layer #"` layers).
    Ambiguous(usize),
}

/// Serialized without the lookup table, which is rebuilt on load (deck cache on disk).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "IndexRepr", into = "IndexRepr")]
pub struct CompositionIndex {
    pub composition: String,
    pub decks: Vec<DeckInfo>,
    /// Name of the live (selected) deck; the layers and clips below belong to it.
    pub live_deck: Option<String>,
    pub layers: Vec<LayerInfo>,
    pub columns: Vec<ColumnInfo>,
    /// All slots, layer by layer, column by column.
    clips: Vec<ClipInfo>,
    by_id: HashMap<u64, usize>,
}

#[derive(Serialize, Deserialize)]
struct IndexRepr {
    composition: String,
    decks: Vec<DeckInfo>,
    live_deck: Option<String>,
    layers: Vec<LayerInfo>,
    columns: Vec<ColumnInfo>,
    clips: Vec<ClipInfo>,
}

impl From<IndexRepr> for CompositionIndex {
    fn from(r: IndexRepr) -> Self {
        Self::new(
            r.composition,
            r.decks,
            r.live_deck,
            r.layers,
            r.columns,
            r.clips,
        )
    }
}

impl From<CompositionIndex> for IndexRepr {
    fn from(i: CompositionIndex) -> Self {
        Self {
            composition: i.composition,
            decks: i.decks,
            live_deck: i.live_deck,
            layers: i.layers,
            columns: i.columns,
            clips: i.clips,
        }
    }
}

impl CompositionIndex {
    pub fn new(
        composition: String,
        decks: Vec<DeckInfo>,
        live_deck: Option<String>,
        layers: Vec<LayerInfo>,
        columns: Vec<ColumnInfo>,
        mut clips: Vec<ClipInfo>,
    ) -> Self {
        clips.sort_by_key(|c| (c.layer, c.column));
        let by_id = clips.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        Self {
            composition,
            decks,
            live_deck,
            layers,
            columns,
            clips,
            by_id,
        }
    }

    /// All slots including empty ones, ordered by (layer, column).
    pub fn clips(&self) -> &[ClipInfo] {
        &self.clips
    }

    /// Slots with content, ordered by (layer, column).
    pub fn filled_clips(&self) -> impl Iterator<Item = &ClipInfo> {
        self.clips.iter().filter(|c| !c.empty)
    }

    pub fn clip_by_id(&self, id: u64) -> Option<&ClipInfo> {
        self.by_id.get(&id).map(|&i| &self.clips[i])
    }

    pub fn clip_at(&self, layer: usize, column: usize) -> Option<&ClipInfo> {
        self.clips
            .iter()
            .find(|c| c.layer == layer && c.column == column)
    }

    pub fn layer_by_name(&self, name: &str) -> LayerLookup<'_> {
        let mut matches = self.layers.iter().filter(|l| l.name == name);
        match (matches.next(), matches.count()) {
            (None, _) => LayerLookup::Missing,
            (Some(l), 0) => LayerLookup::Found(l),
            (Some(_), more) => LayerLookup::Ambiguous(more + 1),
        }
    }

    pub fn layer_by_index(&self, index: usize) -> Option<&LayerInfo> {
        self.layers.iter().find(|l| l.index == index)
    }

    pub fn deck_by_name(&self, name: &str) -> Option<&DeckInfo> {
        self.decks.iter().find(|d| d.name == name)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Compact builder for hand-made indexes in tests.
    use super::*;

    pub struct Builder {
        layers: Vec<LayerInfo>,
        clips: Vec<ClipInfo>,
        columns: usize,
        next_id: u64,
    }

    impl Builder {
        pub fn new(columns: usize) -> Self {
            Self {
                layers: Vec::new(),
                clips: Vec::new(),
                columns,
                next_id: 1000,
            }
        }

        /// Adds the next layer (bottom up) with one entry per column: `None` = empty slot,
        /// `Some((id, name, file))` = clip (`file` is a name; a path is synthesized).
        pub fn layer(mut self, name: &str, slots: &[Option<(u64, &str, Option<&str>)>]) -> Self {
            assert_eq!(slots.len(), self.columns);
            let index = self.layers.len() + 1;
            self.next_id += 1;
            self.layers.push(LayerInfo {
                id: self.next_id,
                index,
                name: name.into(),
                name_param: 0,
            });
            for (ci, slot) in slots.iter().enumerate() {
                self.next_id += 1;
                let (id, name, file) = slot.unwrap_or((self.next_id, "", None));
                self.clips.push(ClipInfo {
                    id,
                    layer: index,
                    column: ci + 1,
                    name: name.into(),
                    file_path: file.map(|f| format!("Q:\\media\\{f}")),
                    empty: slot.is_none(),
                    thumbnail: Thumbnail {
                        last_update: "0".into(),
                        is_default: true,
                    },
                    connected_param: 0,
                    name_param: 0,
                    sequence_param: None,
                });
            }
            self
        }

        pub fn build(self) -> CompositionIndex {
            let columns = (1..=self.columns)
                .map(|i| ColumnInfo {
                    id: i as u64,
                    index: i,
                    name: "Column #".into(),
                })
                .collect();
            CompositionIndex::new(
                "Show".into(),
                vec![],
                Some("Act 1".into()),
                self.layers,
                columns,
                self.clips,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::Builder;
    use super::*;

    #[test]
    fn lookups() {
        let idx = Builder::new(2)
            .layer("BG", &[Some((1, "Tunnel", Some("tunnel.mov"))), None])
            .layer("Layer #", &[None, None])
            .layer("Layer #", &[None, Some((2, "Gen", None))])
            .build();
        assert_eq!(idx.clip_by_id(1).unwrap().column, 1);
        assert_eq!(idx.clip_at(3, 2).unwrap().id, 2);
        assert_eq!(idx.filled_clips().count(), 2);
        assert!(matches!(idx.layer_by_name("BG"), LayerLookup::Found(l) if l.index == 1));
        assert_eq!(idx.layer_by_name("Layer #"), LayerLookup::Ambiguous(2));
        assert_eq!(idx.layer_by_name("FX"), LayerLookup::Missing);
        assert_eq!(idx.clip_by_id(1).unwrap().file_name(), Some("tunnel.mov"));
        assert_eq!(idx.clip_by_id(2).unwrap().file_name(), None);
    }

    #[test]
    fn serde_round_trip_rebuilds_lookup() {
        let idx = Builder::new(1)
            .layer("BG", &[Some((7, "A", Some("a.mov")))])
            .build();
        let json = serde_json::to_string(&idx).unwrap();
        let back: CompositionIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(back, idx);
        assert_eq!(back.clip_by_id(7).unwrap().name, "A");
    }

    #[test]
    fn file_names_from_paths() {
        assert_eq!(file_name_of(r"D:\media\a b\x.mov"), "x.mov");
        assert_eq!(file_name_of("/Users/r/x.mov"), "x.mov");
        assert_eq!(file_name_of("x.mov"), "x.mov");
    }

    #[test]
    fn clip_states() {
        assert_eq!(
            ClipState::from_resolume("Connected & previewing"),
            Some(ClipState::ConnectedAndPreviewing)
        );
        assert!(ClipState::ConnectedAndPreviewing.is_connected());
        assert!(!ClipState::Previewing.is_connected());
        assert_eq!(ClipState::from_resolume("bogus"), None);
    }
}
