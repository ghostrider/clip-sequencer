//! "Sync SEQ layer" (SPEC §7.6): what it takes to give every sequence of the live deck a `§`
//! placeholder clip on the SEQ layer. Pure: the app computes the plan from an index for the
//! confirm dialog, and its worker computes it again from a fresh composition before acting.

use serde::Serialize;

use crate::index::{CompositionIndex, LayerLookup};

/// The SEQ layer in an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SeqLayerState {
    /// 1-based layer index.
    Exists { index: usize },
    /// No layer has the name: sync creates it.
    Missing,
    /// Several layers have the name: sync refuses until the user renames them.
    Ambiguous { count: usize },
}

/// A `§` clip whose name matches no sequence of the deck.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrphanClip {
    /// The clip's full name, prefix included.
    pub name: String,
    pub clip_id: u64,
    pub column: usize,
    /// Playing clips are never cleared.
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeqSyncPlan {
    pub layer: SeqLayerState,
    /// Sequence names without a `§` clip, in the order given (the deck's order).
    pub add: Vec<String>,
    /// `§` clips without a sequence (cleared only when the user asks for it).
    pub orphans: Vec<OrphanClip>,
    /// Empty slots on the SEQ layer, (column, clip id) by column.
    pub empty_slots: Vec<(usize, u64)>,
}

impl SeqSyncPlan {
    /// Nothing to create or add. Orphans alone don't count: clearing them is optional.
    pub fn in_sync(&self) -> bool {
        matches!(self.layer, SeqLayerState::Exists { .. }) && self.add.is_empty()
    }

    /// Columns to add so that every missing clip gets a slot, after clearing `orphans` when
    /// `remove_orphans` is set (cleared slots are reused).
    pub fn columns_needed(&self, remove_orphans: bool) -> usize {
        let freed = if remove_orphans {
            self.orphans.iter().filter(|o| !o.connected).count()
        } else {
            0
        };
        self.add
            .len()
            .saturating_sub(self.empty_slots.len() + freed)
    }
}

/// The sequence name a clip stands for, if it is a `§` clip (same rule as starting from
/// Resolume, SPEC §7.1: prefix stripped, surrounding spaces trimmed).
pub fn seq_clip_name<'a>(clip_name: &'a str, prefix: &str) -> Option<&'a str> {
    clip_name.strip_prefix(prefix).map(str::trim)
}

/// Plans a sync of the SEQ layer named `layer_name` against `sequences` (the live deck's
/// sequence names). `connected` tells whether a clip id is playing.
pub fn plan(
    index: &CompositionIndex,
    layer_name: &str,
    prefix: &str,
    sequences: &[String],
    connected: impl Fn(u64) -> bool,
) -> SeqSyncPlan {
    let layer = match index.layer_by_name(layer_name) {
        LayerLookup::Found(l) => SeqLayerState::Exists { index: l.index },
        LayerLookup::Missing => SeqLayerState::Missing,
        LayerLookup::Ambiguous(count) => SeqLayerState::Ambiguous { count },
    };
    let slots: Vec<_> = match layer {
        SeqLayerState::Exists { index: li } => {
            index.clips().iter().filter(|c| c.layer == li).collect()
        }
        _ => Vec::new(),
    };
    let present: Vec<&str> = slots
        .iter()
        .filter(|c| !c.empty)
        .filter_map(|c| seq_clip_name(&c.name, prefix))
        .collect();
    let mut add: Vec<String> = Vec::new();
    for name in sequences {
        let name = name.trim();
        if !name.is_empty() && !present.contains(&name) && !add.iter().any(|a| a == name) {
            add.push(name.to_owned());
        }
    }
    let orphans = slots
        .iter()
        .filter(|c| !c.empty)
        .filter(|c| {
            seq_clip_name(&c.name, prefix).is_some_and(|n| !sequences.iter().any(|s| s.trim() == n))
        })
        .map(|c| OrphanClip {
            name: c.name.clone(),
            clip_id: c.id,
            column: c.column,
            connected: connected(c.id),
        })
        .collect();
    let empty_slots = slots
        .iter()
        .filter(|c| c.empty)
        .map(|c| (c.column, c.id))
        .collect();
    SeqSyncPlan {
        layer,
        add,
        orphans,
        empty_slots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::test_support::Builder;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn missing_layer_adds_every_sequence() {
        let idx = Builder::new(2).layer("Layer #", &[None, None]).build();
        let p = plan(&idx, "SEQ", "§ ", &names(&["Intro", "Drop"]), |_| false);
        assert_eq!(p.layer, SeqLayerState::Missing);
        assert_eq!(p.add, names(&["Intro", "Drop"]));
        assert!(p.orphans.is_empty());
        assert!(p.empty_slots.is_empty());
        assert!(!p.in_sync());
        assert_eq!(p.columns_needed(false), 2);
    }

    #[test]
    fn existing_clips_are_kept_and_orphans_listed() {
        let idx = Builder::new(4)
            .layer("A", &[Some((1, "kick", Some("k.mov"))), None, None, None])
            .layer(
                "SEQ",
                &[
                    Some((10, "§ Intro", None)),
                    Some((11, "§  Old ", None)),
                    None,
                    Some((13, "Blank", None)),
                ],
            )
            .build();
        let p = plan(
            &idx,
            "SEQ",
            "§ ",
            &names(&["Intro", "Drop", "Drop"]),
            |id| id == 11,
        );
        assert_eq!(p.layer, SeqLayerState::Exists { index: 2 });
        assert_eq!(
            p.add,
            names(&["Drop"]),
            "Intro exists; duplicates added once"
        );
        assert_eq!(
            p.orphans,
            vec![OrphanClip {
                name: "§  Old ".into(),
                clip_id: 11,
                column: 2,
                connected: true,
            }]
        );
        assert_eq!(p.empty_slots.len(), 1);
        assert_eq!(p.empty_slots[0].0, 3);
        assert_eq!(p.columns_needed(false), 0);
    }

    #[test]
    fn cleared_orphans_free_slots_but_connected_ones_stay() {
        let idx = Builder::new(2)
            .layer(
                "SEQ",
                &[Some((10, "§ Gone", None)), Some((11, "§ Live", None))],
            )
            .build();
        let seqs = names(&["A", "B"]);
        let p = plan(&idx, "SEQ", "§ ", &seqs, |id| id == 11);
        assert_eq!(p.orphans.len(), 2);
        assert_eq!(p.columns_needed(false), 2);
        assert_eq!(
            p.columns_needed(true),
            1,
            "only the stopped orphan is cleared"
        );
    }

    #[test]
    fn in_sync_and_ambiguous() {
        let idx = Builder::new(1)
            .layer("SEQ", &[Some((10, "§ Intro", None))])
            .build();
        assert!(plan(&idx, "SEQ", "§ ", &names(&["Intro"]), |_| false).in_sync());

        let idx = Builder::new(1)
            .layer("SEQ", &[None])
            .layer("SEQ", &[None])
            .build();
        let p = plan(&idx, "SEQ", "§ ", &names(&["Intro"]), |_| false);
        assert_eq!(p.layer, SeqLayerState::Ambiguous { count: 2 });
        assert!(!p.in_sync());
    }

    #[test]
    fn seq_clip_name_strips_prefix_and_spaces() {
        assert_eq!(seq_clip_name("§ Intro ", "§ "), Some("Intro"));
        assert_eq!(seq_clip_name("Intro", "§ "), None);
    }
}
