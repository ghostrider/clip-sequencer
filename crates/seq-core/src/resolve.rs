//! Clip and layer resolution against a [`CompositionIndex`] (SPEC §4.3, §4.4).
//!
//! A step's [`ClipRef`] is resolved in the sequence's deck (the caller passes that deck's index)
//! in this order:
//! 1. **id**: the slot with that id, if it has content and that content still fits the reference.
//!    Resolume keeps a slot's id when other media is loaded into it or it is cleared
//!    (`docs/resolume-api-notes.md` §2.1), so an id match is rejected when it points to an empty
//!    slot or when every stored content feature (name, file) contradicts the slot.
//! 2. **name**: clips with exactly that name, preferring the lane's layer.
//! 3. **file**: clips whose media file name matches (case-insensitive), preferring the lane's layer.
//!
//! Among several candidates the lane's layer wins, then a candidate that also matches the other
//! feature, then the lowest (layer, column). Picking from more than one is flagged `ambiguous`
//! so the UI can warn; it is still resolved, deterministically.

use crate::index::{ClipInfo, CompositionIndex, LayerLookup};
use crate::model::{ClipRef, Lane, LaneMode, Sequence, Step};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBy {
    Id,
    Name,
    File,
    /// Slot lane: whatever is in the slot (SPEC §4.5).
    Slot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedClip {
    pub clip_id: u64,
    /// 1-based OSC/REST position in this index snapshot.
    pub layer: usize,
    pub column: usize,
    pub by: ResolvedBy,
    /// False when the clip lives on another layer than the lane's (it plays on its own layer).
    pub on_lane_layer: bool,
    /// More than one clip fitted equally well; the first was taken.
    pub ambiguous: bool,
}

/// Per-method status for the step inspector (SPEC §9.4). `None`: the feature is not stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Methods {
    /// The id points to a slot whose content fits the reference.
    pub id: Option<bool>,
    /// A clip with this name exists in the deck.
    pub name: Option<bool>,
    /// A clip with this media file exists in the deck.
    pub file: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResolution {
    Rest,
    Resolved {
        clip: ResolvedClip,
        methods: Methods,
    },
    /// Treated as a rest at runtime and shown red in the UI.
    Unresolved {
        methods: Methods,
    },
    /// Slot lane step (SPEC §4.5): what the slot holds in this index snapshot. `clip` is `None`
    /// when the slot is empty, beyond the last column or the lane's layer is not found; the step
    /// then plays as a rest. At runtime the slot is read again when the step starts.
    Slot {
        column: u32,
        clip: Option<ResolvedClip>,
    },
}

impl StepResolution {
    /// The clip a clip step resolved to. Slot steps: see [`StepResolution::current_clip`].
    pub fn clip(&self) -> Option<&ResolvedClip> {
        match self {
            Self::Resolved { clip, .. } => Some(clip),
            _ => None,
        }
    }

    /// The clip this step would play now: the resolved clip, or the slot's content.
    pub fn current_clip(&self) -> Option<&ResolvedClip> {
        match self {
            Self::Resolved { clip, .. }
            | Self::Slot {
                clip: Some(clip), ..
            } => Some(clip),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerResolution {
    Found { index: usize, id: u64 },
    Missing,
    Ambiguous { count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneResolution {
    pub layer: LayerResolution,
    /// Steps are resolved even when the layer is not, so the UI can show them.
    pub steps: Vec<StepResolution>,
}

impl LaneResolution {
    /// A lane whose layer is not uniquely found is silent at runtime (SPEC §4.4).
    pub fn layer_index(&self) -> Option<usize> {
        match self.layer {
            LayerResolution::Found { index, .. } => Some(index),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceResolution {
    pub lanes: Vec<LaneResolution>,
}

impl SequenceResolution {
    /// Every lane has its layer and every non-rest clip step has a clip (slot steps never count
    /// as unresolved: an empty slot is a rest).
    pub fn is_complete(&self) -> bool {
        self.lanes.iter().all(|l| {
            l.layer_index().is_some()
                && l.steps
                    .iter()
                    .all(|s| !matches!(s, StepResolution::Unresolved { .. }))
        })
    }
}

pub fn resolve_sequence(seq: &Sequence, index: &CompositionIndex) -> SequenceResolution {
    SequenceResolution {
        lanes: seq.lanes.iter().map(|l| resolve_lane(l, index)).collect(),
    }
}

pub fn resolve_lane(lane: &Lane, index: &CompositionIndex) -> LaneResolution {
    let layer = match index.layer_by_name(&lane.layer) {
        LayerLookup::Found(l) => LayerResolution::Found {
            index: l.index,
            id: l.id,
        },
        LayerLookup::Missing => LayerResolution::Missing,
        LayerLookup::Ambiguous(count) => LayerResolution::Ambiguous { count },
    };
    let lane_layer = match layer {
        LayerResolution::Found { index, .. } => Some(index),
        _ => None,
    };
    LaneResolution {
        layer,
        steps: lane
            .steps
            .iter()
            .map(|s| match lane.mode {
                LaneMode::Clip => resolve_step(s, lane_layer, index),
                LaneMode::Slot => resolve_slot_step(s, lane_layer, index),
            })
            .collect(),
    }
}

/// A slot lane step: the content of (`lane_layer`, column) in `index`, if any.
pub fn resolve_slot_step(
    step: &Step,
    lane_layer: Option<usize>,
    index: &CompositionIndex,
) -> StepResolution {
    let Some(column) = step.column else {
        return StepResolution::Rest;
    };
    let clip = lane_layer.and_then(|layer| slot_clip(index, layer, column));
    StepResolution::Slot { column, clip }
}

/// The filled slot at (`layer`, `column`), as a [`ResolvedClip`].
pub fn slot_clip(index: &CompositionIndex, layer: usize, column: u32) -> Option<ResolvedClip> {
    let c = index.clip_at(layer, column as usize).filter(|c| !c.empty)?;
    Some(ResolvedClip {
        clip_id: c.id,
        layer: c.layer,
        column: c.column,
        by: ResolvedBy::Slot,
        on_lane_layer: true,
        ambiguous: false,
    })
}

/// A lane's steps converted to `mode` (SPEC §9.3), lengths kept:
/// - clip → slot: the column of each resolved clip (on whatever layer it was found); unresolved
///   steps become rests.
/// - slot → clip: the clip currently in each slot of the lane's layer; empty slots become rests.
///
/// Without an index (deck never seen live) every step becomes a rest.
pub fn convert_steps(lane: &Lane, mode: LaneMode, index: Option<&CompositionIndex>) -> Vec<Step> {
    if lane.mode == mode {
        return lane.steps.clone();
    }
    let lane_layer = index.and_then(|i| match i.layer_by_name(&lane.layer) {
        LayerLookup::Found(l) => Some(l.index),
        _ => None,
    });
    lane.steps
        .iter()
        .map(|s| match mode {
            LaneMode::Slot => {
                let column = index
                    .and_then(|i| resolve_step(s, lane_layer, i).clip().copied())
                    .map(|c| c.column as u32);
                Step::slot(column, s.length)
            }
            LaneMode::Clip => {
                let clip = index
                    .zip(lane_layer)
                    .zip(s.column)
                    .and_then(|((i, l), col)| {
                        i.clip_at(l, col as usize)
                            .filter(|c| !c.empty)
                            .map(clip_ref_for)
                    });
                Step::clip(clip, s.length)
            }
        })
        .collect()
}

pub fn resolve_step(
    step: &Step,
    lane_layer: Option<usize>,
    index: &CompositionIndex,
) -> StepResolution {
    match &step.clip {
        None => StepResolution::Rest,
        Some(clip_ref) => match resolve_clip(clip_ref, lane_layer, index) {
            (Some(clip), methods) => StepResolution::Resolved { clip, methods },
            (None, methods) => StepResolution::Unresolved { methods },
        },
    }
}

/// Resolves one reference. `lane_layer` (1-based) is preferred for name and file matches.
pub fn resolve_clip(
    clip_ref: &ClipRef,
    lane_layer: Option<usize>,
    index: &CompositionIndex,
) -> (Option<ResolvedClip>, Methods) {
    let name = clip_ref.name.as_deref().filter(|n| !n.is_empty());
    let file = clip_ref.file.as_deref().filter(|f| !f.is_empty());
    let name_is = |c: &ClipInfo| name == Some(c.name.as_str());
    let file_is = |c: &ClipInfo| match (file, c.file_name()) {
        (Some(want), Some(have)) => want.to_lowercase() == have.to_lowercase(),
        _ => false,
    };

    let by_id = clip_ref
        .id
        .and_then(|id| index.clip_by_id(id))
        .filter(|c| !c.empty && content_fits(c, name, file));
    let methods = Methods {
        id: clip_ref.id.map(|_| by_id.is_some()),
        name: name.map(|_| index.filled_clips().any(name_is)),
        file: file.map(|_| index.filled_clips().any(file_is)),
    };
    let found = |c: &ClipInfo, by, ambiguous| ResolvedClip {
        clip_id: c.id,
        layer: c.layer,
        column: c.column,
        by,
        on_lane_layer: lane_layer == Some(c.layer),
        ambiguous,
    };

    if let Some(c) = by_id {
        return (Some(found(c, ResolvedBy::Id, false)), methods);
    }
    if name.is_some() {
        if let Some((c, amb)) = pick(
            index.filled_clips().filter(|c| name_is(c)),
            lane_layer,
            file_is,
        ) {
            return (Some(found(c, ResolvedBy::Name, amb)), methods);
        }
    }
    if file.is_some() {
        if let Some((c, amb)) = pick(
            index.filled_clips().filter(|c| file_is(c)),
            lane_layer,
            name_is,
        ) {
            return (Some(found(c, ResolvedBy::File, amb)), methods);
        }
    }
    (None, methods)
}

/// An id match is trusted unless all stored content features contradict the slot.
/// A reference without a file (to a generator clip) fits any slot without a file, so renaming a
/// generator clip keeps it resolved by id.
fn content_fits(c: &ClipInfo, name: Option<&str>, file: Option<&str>) -> bool {
    let name_ok = name == Some(c.name.as_str());
    let file_ok = match (file, c.file_name()) {
        (Some(want), Some(have)) => want.to_lowercase() == have.to_lowercase(),
        (None, None) => true,
        _ => false,
    };
    name_ok || file_ok || (name.is_none() && file.is_none())
}

/// Chooses among candidates (already ordered by layer, column): lane layer first, then those
/// passing `also`, then the first. Returns the pick and whether its tier had several members.
fn pick<'a>(
    candidates: impl Iterator<Item = &'a ClipInfo>,
    lane_layer: Option<usize>,
    also: impl Fn(&ClipInfo) -> bool,
) -> Option<(&'a ClipInfo, bool)> {
    let candidates: Vec<&ClipInfo> = candidates.collect();
    let on_lane = |c: &&ClipInfo| lane_layer == Some(c.layer);
    for tier in [
        |on: bool, also: bool| on && also,
        |on: bool, _also: bool| on,
        |_on: bool, also: bool| also,
        |_on: bool, _also: bool| true,
    ] {
        let members: Vec<&ClipInfo> = candidates
            .iter()
            .copied()
            .filter(|c| tier(on_lane(c), also(c)))
            .collect();
        if let Some(first) = members.first() {
            return Some((first, members.len() > 1));
        }
    }
    None
}

/// Writes the id of every clip resolved by name or file back into the sequence (SPEC §4.3:
/// "When resolution succeeds via 2 or 3, the stored id is refreshed"). Returns how many changed.
pub fn refresh_ids(seq: &mut Sequence, resolution: &SequenceResolution) -> usize {
    let mut changed = 0;
    for (lane, lane_res) in seq.lanes.iter_mut().zip(&resolution.lanes) {
        for (step, step_res) in lane.steps.iter_mut().zip(&lane_res.steps) {
            let (Some(clip_ref), Some(resolved)) = (step.clip.as_mut(), step_res.clip()) else {
                continue;
            };
            if resolved.by != ResolvedBy::Id && clip_ref.id != Some(resolved.clip_id) {
                clip_ref.id = Some(resolved.clip_id);
                changed += 1;
            }
        }
    }
    changed
}

/// The reference stored when a clip from the index is put into a step (all three features).
pub fn clip_ref_for(clip: &ClipInfo) -> ClipRef {
    ClipRef {
        id: Some(clip.id),
        name: Some(clip.name.clone()).filter(|n| !n.is_empty()),
        file: clip.file_name().map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::test_support::Builder;
    use crate::model::{AfterLastPass, LaneEnd, LaneMode, Repeat, Start};
    use uuid::Uuid;

    fn r(id: Option<u64>, name: Option<&str>, file: Option<&str>) -> ClipRef {
        ClipRef {
            id,
            name: name.map(Into::into),
            file: file.map(Into::into),
        }
    }

    fn resolved(res: (Option<ResolvedClip>, Methods)) -> ResolvedClip {
        res.0.expect("expected the clip to resolve")
    }

    /// Two layers, three columns:
    /// layer 1 "BG":  [Tunnel(10, tunnel.mov), empty, Smoke(11, smoke.mov)]
    /// layer 2 "FX":  [empty, Strobe(20, strobe.mov), Gen(21, no file)]
    fn base() -> CompositionIndex {
        Builder::new(3)
            .layer(
                "BG",
                &[
                    Some((10, "Tunnel", Some("tunnel.mov"))),
                    None,
                    Some((11, "Smoke", Some("smoke.mov"))),
                ],
            )
            .layer(
                "FX",
                &[
                    None,
                    Some((20, "Strobe", Some("strobe.mov"))),
                    Some((21, "Gen", None)),
                ],
            )
            .build()
    }

    #[test]
    fn resolves_by_id_first() {
        let c = resolved(resolve_clip(
            &r(Some(10), Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &base(),
        ));
        assert_eq!(
            (c.clip_id, c.layer, c.column, c.by),
            (10, 1, 1, ResolvedBy::Id)
        );
        assert!(c.on_lane_layer && !c.ambiguous);
    }

    #[test]
    fn renamed_clip_still_resolves_by_id_because_file_matches() {
        let idx = Builder::new(1)
            .layer("BG", &[Some((10, "Tunnel (new name)", Some("tunnel.mov")))])
            .build();
        let (clip, methods) = resolve_clip(
            &r(Some(10), Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &idx,
        );
        assert_eq!(clip.unwrap().by, ResolvedBy::Id);
        assert_eq!(
            methods,
            Methods {
                id: Some(true),
                name: Some(false),
                file: Some(true)
            }
        );
    }

    #[test]
    fn renamed_clip_without_id_resolves_by_file() {
        let idx = Builder::new(1)
            .layer("BG", &[Some((10, "Tunnel (new name)", Some("Tunnel.MOV")))])
            .build();
        let c = resolved(resolve_clip(
            &r(None, Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &idx,
        ));
        assert_eq!((c.clip_id, c.by), (10, ResolvedBy::File));
    }

    #[test]
    fn renamed_generator_clip_keeps_resolving_by_id() {
        let idx = Builder::new(1)
            .layer("FX", &[Some((21, "Gen renamed", None))])
            .build();
        let c = resolved(resolve_clip(&r(Some(21), Some("Gen"), None), Some(1), &idx));
        assert_eq!(c.by, ResolvedBy::Id);
    }

    #[test]
    fn moved_column_resolves_to_the_new_position() {
        // Tunnel (id 10) moved from column 1 to column 3; ids travel with clips.
        let idx = Builder::new(3)
            .layer(
                "BG",
                &[None, None, Some((10, "Tunnel", Some("tunnel.mov")))],
            )
            .build();
        let c = resolved(resolve_clip(
            &r(Some(10), Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &idx,
        ));
        assert_eq!((c.layer, c.column), (1, 3));
    }

    #[test]
    fn inserted_layer_shifts_layer_index_and_lane_follows_by_name() {
        let idx = Builder::new(1)
            .layer("New", &[None])
            .layer("BG", &[Some((10, "Tunnel", Some("tunnel.mov")))])
            .build();
        let lane = Lane {
            layer: "BG".into(),
            mode: crate::model::LaneMode::Clip,
            end: LaneEnd::Hold,
            steps: vec![Step {
                column: None,
                clip: Some(r(Some(10), Some("Tunnel"), Some("tunnel.mov"))),
                length: 8,
            }],
        };
        let res = resolve_lane(&lane, &idx);
        assert_eq!(res.layer_index(), Some(2));
        let c = res.steps[0].clip().unwrap();
        assert_eq!((c.layer, c.column, c.on_lane_layer), (2, 1, true));
    }

    #[test]
    fn deleted_clip_leaves_empty_slot_and_is_unresolved() {
        // Resolume keeps id 10 on the now-empty slot.
        let idx = Builder::new(1).layer("BG", &[None]).build();
        let idx = {
            // Rebuild with the empty slot carrying the old id.
            let mut clips = idx.clips().to_vec();
            clips[0].id = 10;
            CompositionIndex::new(
                idx.composition,
                idx.decks,
                idx.live_deck,
                idx.layers,
                idx.columns,
                clips,
            )
        };
        let (clip, methods) = resolve_clip(
            &r(Some(10), Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &idx,
        );
        assert_eq!(clip, None);
        assert_eq!(
            methods,
            Methods {
                id: Some(false),
                name: Some(false),
                file: Some(false)
            }
        );
    }

    #[test]
    fn deleted_clip_that_still_exists_elsewhere_resolves_by_name() {
        // Slot 10 was cleared, but a copy named Tunnel sits on another layer.
        let idx = Builder::new(2)
            .layer("BG", &[None, None])
            .layer("FX", &[Some((30, "Tunnel", Some("tunnel.mov"))), None])
            .build();
        let c = resolved(resolve_clip(
            &r(Some(10), Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &idx,
        ));
        assert_eq!(
            (c.clip_id, c.by, c.on_lane_layer),
            (30, ResolvedBy::Name, false)
        );
    }

    #[test]
    fn slot_reloaded_with_other_media_does_not_match_by_id() {
        // Id 10 now holds different media; the original moved to column 2 (new id 31).
        let idx = Builder::new(2)
            .layer(
                "BG",
                &[
                    Some((10, "Other", Some("other.mov"))),
                    Some((31, "Tunnel", Some("tunnel.mov"))),
                ],
            )
            .build();
        let (clip, methods) = resolve_clip(
            &r(Some(10), Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &idx,
        );
        let c = clip.unwrap();
        assert_eq!((c.clip_id, c.by), (31, ResolvedBy::Name));
        assert_eq!(methods.id, Some(false));
    }

    #[test]
    fn duplicate_names_prefer_lane_layer_then_matching_file() {
        let idx = Builder::new(3)
            .layer(
                "BG",
                &[
                    Some((1, "Loop", Some("a.mov"))),
                    Some((2, "Loop", Some("b.mov"))),
                    None,
                ],
            )
            .layer("FX", &[Some((3, "Loop", Some("b.mov"))), None, None])
            .build();
        // On the lane layer, the one whose file also matches wins and is not ambiguous.
        let c = resolved(resolve_clip(
            &r(None, Some("Loop"), Some("b.mov")),
            Some(1),
            &idx,
        ));
        assert_eq!((c.clip_id, c.ambiguous), (2, false));
        // Lane layer FX has exactly one.
        let c = resolved(resolve_clip(&r(None, Some("Loop"), None), Some(2), &idx));
        assert_eq!((c.clip_id, c.ambiguous), (3, false));
        // Name only, lane layer BG has two: lowest column, flagged ambiguous.
        let c = resolved(resolve_clip(&r(None, Some("Loop"), None), Some(1), &idx));
        assert_eq!((c.clip_id, c.ambiguous), (1, true));
        // No lane layer (lane unresolved): deck-wide, lowest layer then column.
        let c = resolved(resolve_clip(&r(None, Some("Loop"), None), None, &idx));
        assert_eq!((c.clip_id, c.ambiguous, c.on_lane_layer), (1, true, false));
    }

    #[test]
    fn name_beats_file_in_the_chain() {
        let idx = Builder::new(2)
            .layer(
                "BG",
                &[
                    Some((1, "Tunnel", Some("x.mov"))),
                    Some((2, "Other", Some("tunnel.mov"))),
                ],
            )
            .build();
        let c = resolved(resolve_clip(
            &r(None, Some("Tunnel"), Some("tunnel.mov")),
            Some(1),
            &idx,
        ));
        assert_eq!((c.clip_id, c.by), (1, ResolvedBy::Name));
    }

    #[test]
    fn missing_and_ambiguous_layers() {
        let idx = Builder::new(1)
            .layer("Layer #", &[Some((1, "A", None))])
            .layer("Layer #", &[None])
            .build();
        let lane = |layer: &str| Lane {
            layer: layer.into(),
            mode: crate::model::LaneMode::Clip,
            end: LaneEnd::Hold,
            steps: vec![Step {
                column: None,
                clip: Some(r(Some(1), Some("A"), None)),
                length: 8,
            }],
        };
        let res = resolve_lane(&lane("Layer #"), &idx);
        assert_eq!(res.layer, LayerResolution::Ambiguous { count: 2 });
        assert_eq!(res.layer_index(), None);
        // Steps are still resolved for display.
        assert!(!res.steps[0].clip().unwrap().on_lane_layer);
        assert_eq!(
            resolve_lane(&lane("Nope"), &idx).layer,
            LayerResolution::Missing
        );
    }

    fn sequence(steps: Vec<Step>) -> Sequence {
        Sequence {
            id: Uuid::nil(),
            name: "S".into(),
            start: Start::Immediate,
            repeat: Repeat::Once,
            after_last_pass: AfterLastPass::Clear,
            lanes: vec![Lane {
                layer: "BG".into(),
                mode: crate::model::LaneMode::Clip,
                end: LaneEnd::LoopToEnd,
                steps,
            }],
        }
    }

    #[test]
    fn sequence_resolution_and_completeness() {
        let seq = sequence(vec![
            Step {
                column: None,
                clip: Some(r(Some(10), None, None)),
                length: 8,
            },
            Step {
                column: None,
                clip: None,
                length: 8,
            },
            Step {
                column: None,
                clip: Some(r(None, Some("Missing"), None)),
                length: 8,
            },
        ]);
        let res = resolve_sequence(&seq, &base());
        assert!(matches!(
            res.lanes[0].steps[0],
            StepResolution::Resolved { .. }
        ));
        assert_eq!(res.lanes[0].steps[1], StepResolution::Rest);
        assert!(matches!(
            res.lanes[0].steps[2],
            StepResolution::Unresolved { .. }
        ));
        assert!(!res.is_complete());
        let ok = sequence(vec![Step {
            column: None,
            clip: Some(r(None, Some("Smoke"), None)),
            length: 8,
        }]);
        assert!(resolve_sequence(&ok, &base()).is_complete());
    }

    #[test]
    fn refresh_ids_updates_name_and_file_matches_only() {
        let mut seq = sequence(vec![
            Step {
                column: None,
                clip: Some(r(Some(10), Some("Tunnel"), None)),
                length: 8,
            },
            Step {
                column: None,
                clip: Some(r(Some(999), Some("Smoke"), None)),
                length: 8,
            },
            Step {
                column: None,
                clip: Some(r(None, None, Some("strobe.mov"))),
                length: 8,
            },
            Step {
                column: None,
                clip: None,
                length: 8,
            },
        ]);
        let res = resolve_sequence(&seq, &base());
        assert_eq!(refresh_ids(&mut seq, &res), 2);
        let ids: Vec<_> = seq.lanes[0]
            .steps
            .iter()
            .map(|s| s.clip.as_ref().and_then(|c| c.id))
            .collect();
        assert_eq!(ids, vec![Some(10), Some(11), Some(20), None]);
        // Second pass: everything resolves by id now, nothing changes.
        let res = resolve_sequence(&seq, &base());
        assert_eq!(refresh_ids(&mut seq, &res), 0);
        assert!(res.lanes[0]
            .steps
            .iter()
            .filter_map(|s| s.clip())
            .all(|c| c.by == ResolvedBy::Id));
    }

    #[test]
    fn clip_ref_for_stores_all_features() {
        let idx = base();
        assert_eq!(
            clip_ref_for(idx.clip_by_id(10).unwrap()),
            r(Some(10), Some("Tunnel"), Some("tunnel.mov"))
        );
        assert_eq!(
            clip_ref_for(idx.clip_by_id(21).unwrap()),
            r(Some(21), Some("Gen"), None)
        );
    }

    fn slot_lane(layer: &str, columns: &[Option<u32>]) -> Lane {
        Lane {
            layer: layer.into(),
            mode: LaneMode::Slot,
            end: LaneEnd::Hold,
            steps: columns.iter().map(|&c| Step::slot(c, 8)).collect(),
        }
    }

    #[test]
    fn slot_steps_resolve_to_the_slot_content() {
        // BG: [Tunnel, empty, Smoke]; column 4 does not exist.
        let res = resolve_lane(
            &slot_lane("BG", &[Some(1), Some(2), Some(3), Some(4), None]),
            &base(),
        );
        let current: Vec<_> = res
            .steps
            .iter()
            .map(|s| s.current_clip().map(|c| c.clip_id))
            .collect();
        assert_eq!(current, [Some(10), None, Some(11), None, None]);
        assert!(matches!(
            res.steps[1],
            StepResolution::Slot {
                column: 2,
                clip: None
            }
        ));
        assert_eq!(res.steps[4], StepResolution::Rest);
        assert!(res.steps.iter().all(|s| s.clip().is_none()));
        assert_eq!(res.steps[0].current_clip().unwrap().by, ResolvedBy::Slot);
        // Empty slots are rests, not unresolved.
        let seq = Sequence {
            lanes: vec![slot_lane("BG", &[Some(2)])],
            ..sequence(Vec::new())
        };
        assert!(resolve_sequence(&seq, &base()).is_complete());
        // Missing layer: no slot content, the lane is unresolved as usual.
        let res = resolve_lane(&slot_lane("Nope", &[Some(1)]), &base());
        assert_eq!(res.layer, LayerResolution::Missing);
        assert!(matches!(
            res.steps[0],
            StepResolution::Slot {
                column: 1,
                clip: None
            }
        ));
    }

    #[test]
    fn convert_clip_lane_to_slots_and_back() {
        let idx = base();
        let clip_lane = Lane {
            layer: "BG".into(),
            mode: LaneMode::Clip,
            end: LaneEnd::Hold,
            steps: vec![
                Step::clip(Some(r(None, Some("Smoke"), None)), 8),
                Step::clip(None, 4),
                Step::clip(Some(r(None, Some("Missing"), None)), 2),
                // On another layer: keeps its column number.
                Step::clip(Some(r(None, Some("Strobe"), None)), 1),
            ],
        };
        let slots = convert_steps(&clip_lane, LaneMode::Slot, Some(&idx));
        assert_eq!(
            slots,
            [
                Step::slot(Some(3), 8),
                Step::slot(None, 4),
                Step::slot(None, 2),
                Step::slot(Some(2), 1)
            ]
        );
        let slot_lane = Lane {
            steps: slots,
            mode: LaneMode::Slot,
            ..clip_lane.clone()
        };
        let back = convert_steps(&slot_lane, LaneMode::Clip, Some(&idx));
        assert_eq!(
            back,
            [
                Step::clip(Some(r(Some(11), Some("Smoke"), Some("smoke.mov"))), 8),
                Step::clip(None, 4),
                Step::clip(None, 2),
                // BG column 2 is empty.
                Step::clip(None, 1)
            ]
        );
        // Same mode: unchanged. No index: rests with the same lengths.
        assert_eq!(
            convert_steps(&clip_lane, LaneMode::Clip, Some(&idx)),
            clip_lane.steps
        );
        assert!(convert_steps(&clip_lane, LaneMode::Slot, None)
            .iter()
            .all(Step::is_rest));
    }
}
