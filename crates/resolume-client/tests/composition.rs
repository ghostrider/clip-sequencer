//! Composition index from the M0 samples, and resolution scenarios that edit the sample JSON the
//! way Resolume was observed to behave (`docs/resolume-api-notes.md` §2.1): clip ids travel with
//! clips through moves, new layers/columns bring new ids, clearing keeps the id on an empty slot.

use resolume_client::composition::{parse_composition, parse_composition_value};
use seq_core::index::{ClipState, LayerLookup};
use seq_core::resolve::{
    clip_ref_for, refresh_ids, resolve_sequence, LayerResolution, ResolvedBy, StepResolution,
};
use seq_core::{AfterLastPass, Lane, LaneEnd, Repeat, Sequence, Start, Step};
use serde_json::Value;
use uuid::Uuid;

const DECK2: &str = include_str!("../../../docs/samples/composition.json");
const DECK1: &str = include_str!("../../../docs/samples/deck1-composition.json");
const CONNECTED: &str = include_str!("../../../docs/samples/composition-clip-connected.json");

#[test]
fn deck2_sample_structure() {
    let snap = parse_composition(DECK2).unwrap();
    let idx = &snap.index;
    assert_eq!(idx.composition, "dev");
    let decks: Vec<_> = idx
        .decks
        .iter()
        .map(|d| (d.index, d.name.as_str()))
        .collect();
    assert_eq!(
        decks,
        vec![(1, "1000A Text3D"), (2, "2132323"), (3, "empty")]
    );
    assert_eq!(idx.live_deck.as_deref(), Some("2132323"));
    assert_eq!(idx.decks[1].select_param, 1792956584811);
    assert_eq!(idx.layers.len(), 4);
    assert_eq!(idx.columns.len(), 10);
    assert_eq!(idx.clips().len(), 40);
    assert_eq!(idx.filled_clips().count(), 3);

    let c = idx.clip_by_id(1792956592496).unwrap();
    assert_eq!((c.layer, c.column), (2, 1));
    assert_eq!(c.file_name(), Some("Clip 13.mov"));
    assert_eq!(c.connected_param, 1792956592480);
    assert!(!c.thumbnail.is_default);

    let empty = idx.clip_at(1, 1).unwrap();
    assert!(empty.empty && empty.file_path.is_none() && empty.thumbnail.is_default);

    assert_eq!(idx.layer_by_name("Layer #"), LayerLookup::Ambiguous(3));
    assert!(matches!(idx.layer_by_name("SEQ"), LayerLookup::Found(l) if l.index == 4));
    assert!(snap.states.values().all(|s| !s.is_connected()));
}

#[test]
fn deck1_sample_has_generators_and_duplicates() {
    let idx = parse_composition(DECK1).unwrap().index;
    assert_eq!(idx.live_deck.as_deref(), Some("1000A Text3D"));
    assert_eq!(idx.columns.len(), 9);
    assert_eq!(idx.filled_clips().count(), 14);
    let generator = idx.clip_at(3, 2).unwrap();
    assert_eq!(generator.name, "1000A Shake");
    assert_eq!(generator.file_path, None);
    let curly: Vec<_> = idx
        .filled_clips()
        .filter(|c| c.name.starts_with("Clip 02"))
        .map(|c| (c.layer, c.column))
        .collect();
    assert_eq!(curly, vec![(2, 3), (2, 6)]);
}

#[test]
fn connected_sample_reports_state_and_connect_changes_do_not_change_the_index() {
    let snap = parse_composition(CONNECTED).unwrap();
    assert_eq!(snap.states[&1792956592496], ClipState::Connected);

    let mut v: Value = serde_json::from_str(DECK2).unwrap();
    clip_mut(&mut v, 2, 1)["connected"]["value"] = "Connected".into();
    let connected = parse_composition_value(&v).unwrap();
    let plain = parse_composition(DECK2).unwrap();
    assert_eq!(
        connected.index, plain.index,
        "a connect must not look like a structural change"
    );
    assert_ne!(connected.states, plain.states);
}

// ---- scenario helpers: edit composition JSON like Resolume does ----

fn layers_mut(v: &mut Value) -> &mut Vec<Value> {
    v["layers"].as_array_mut().unwrap()
}

fn clip_mut(v: &mut Value, layer: usize, column: usize) -> &mut Value {
    &mut layers_mut(v)[layer - 1]["clips"][column - 1]
}

fn rename_layer(v: &mut Value, layer: usize, name: &str) {
    layers_mut(v)[layer - 1]["name"]["value"] = name.into();
}

/// `POST /composition/columns/{before}/insert-move` with column `from`: the column lands just
/// before the original column `before` (probe: moving 1 before 4 puts it at 3), ids travel along.
fn move_column(v: &mut Value, from: usize, before: usize) {
    let target = if from < before {
        before - 2
    } else {
        before - 1
    };
    let cols = v["columns"].as_array_mut().unwrap();
    let c = cols.remove(from - 1);
    cols.insert(target, c);
    for layer in layers_mut(v) {
        let clips = layer["clips"].as_array_mut().unwrap();
        let c = clips.remove(from - 1);
        clips.insert(target, c);
    }
}

/// `POST /composition/layers/add` before layer `at`: a new layer of empty slots with new ids.
fn insert_layer(v: &mut Value, at: usize, name: &str, id_base: u64) {
    let template_clip = clip_mut(v, 1, 1).clone();
    let mut layer = layers_mut(v)[0].clone();
    layer["id"] = id_base.into();
    layer["name"]["value"] = name.into();
    for (i, clip) in layer["clips"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        *clip = template_clip.clone();
        clip["id"] = (id_base + 1 + i as u64).into();
        clip["name"]["value"] = "".into();
        clip["connected"]["value"] = "Empty".into();
        clip["video"] = Value::Null;
    }
    layers_mut(v).insert(at - 1, layer);
}

/// `POST /composition/clips/by-id/{id}/clear`: content gone, id stays.
fn clear_clip(v: &mut Value, layer: usize, column: usize) {
    let c = clip_mut(v, layer, column);
    c["name"]["value"] = "".into();
    c["connected"]["value"] = "Empty".into();
    c["video"] = Value::Null;
}

/// Other media loaded into an occupied slot: same id, new name and file.
fn reload_slot(v: &mut Value, layer: usize, column: usize, name: &str, path: &str) {
    let c = clip_mut(v, layer, column);
    c["name"]["value"] = name.into();
    c["video"]["fileinfo"]["path"] = path.into();
}

/// deck 1 with its three unnamed layers named L1..L3 (lanes need unique layer names).
fn named_deck1() -> Value {
    let mut v: Value = serde_json::from_str(DECK1).unwrap();
    for (i, name) in ["L1", "L2", "L3"].iter().enumerate() {
        rename_layer(&mut v, i + 1, name);
    }
    v
}

/// A sequence whose steps reference the given (layer, column) clips of `v`, as the editor would
/// store them (id, name and file), one lane per layer named.
fn sequence_from(v: &Value, lanes: &[(&str, &[(usize, usize)])]) -> Sequence {
    let idx = parse_composition_value(v).unwrap().index;
    Sequence {
        id: Uuid::nil(),
        name: "Scenario".into(),
        start: Start::Immediate,
        repeat: Repeat::Once,
        after_last_pass: AfterLastPass::Clear,
        lanes: lanes
            .iter()
            .map(|(layer, cells)| Lane {
                layer: (*layer).into(),
                end: LaneEnd::LoopToEnd,
                steps: cells
                    .iter()
                    .map(|&(l, c)| Step {
                        clip: Some(clip_ref_for(idx.clip_at(l, c).unwrap())),
                        length: 8,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn positions(seq: &Sequence, v: &Value) -> Vec<Vec<Option<(usize, usize, ResolvedBy)>>> {
    let idx = parse_composition_value(v).unwrap().index;
    resolve_sequence(seq, &idx)
        .lanes
        .iter()
        .map(|l| {
            l.steps
                .iter()
                .map(|s| s.clip().map(|c| (c.layer, c.column, c.by)))
                .collect()
        })
        .collect()
}

use ResolvedBy::{File, Id, Name};

#[test]
fn scenario_renamed_clip() {
    let mut v = named_deck1();
    let seq = sequence_from(&v, &[("L1", &[(1, 1), (1, 2)])]);
    clip_mut(&mut v, 1, 2)["name"]["value"] = "Clip 12 (renamed)".into();
    assert_eq!(
        positions(&seq, &v),
        vec![vec![Some((1, 1, Id)), Some((1, 2, Id))]]
    );
}

#[test]
fn scenario_moved_column() {
    let mut v = named_deck1();
    let seq = sequence_from(&v, &[("L1", &[(1, 1), (1, 4)]), ("L2", &[(2, 2)])]);
    move_column(&mut v, 1, 9); // column 1 → before column 9 (now column 8)
    assert_eq!(
        positions(&seq, &v),
        vec![
            vec![Some((1, 8, Id)), Some((1, 3, Id))],
            vec![Some((2, 1, Id))]
        ]
    );
}

#[test]
fn scenario_inserted_layer() {
    let mut v = named_deck1();
    let seq = sequence_from(&v, &[("L2", &[(2, 1)]), ("L3", &[(3, 2)])]);
    insert_layer(&mut v, 1, "New Bottom", 9_000_000);
    let idx = parse_composition_value(&v).unwrap().index;
    let res = resolve_sequence(&seq, &idx);
    assert_eq!(res.lanes[0].layer_index(), Some(3));
    assert_eq!(res.lanes[1].layer_index(), Some(4));
    assert_eq!(
        positions(&seq, &v),
        vec![vec![Some((3, 1, Id))], vec![Some((4, 2, Id))]]
    );
    assert!(res
        .lanes
        .iter()
        .flat_map(|l| &l.steps)
        .all(|s| s.clip().unwrap().on_lane_layer));
}

#[test]
fn scenario_renamed_layer_makes_lane_unresolved() {
    let mut v = named_deck1();
    let seq = sequence_from(&v, &[("L1", &[(1, 1)])]);
    rename_layer(&mut v, 1, "Background");
    let idx = parse_composition_value(&v).unwrap().index;
    let res = resolve_sequence(&seq, &idx);
    assert_eq!(res.lanes[0].layer, LayerResolution::Missing);
    assert!(!res.is_complete());
}

#[test]
fn scenario_deleted_clip() {
    let mut v = named_deck1();
    let seq = sequence_from(&v, &[("L1", &[(1, 1), (1, 3)])]);
    clear_clip(&mut v, 1, 3);
    let idx = parse_composition_value(&v).unwrap().index;
    let res = resolve_sequence(&seq, &idx);
    assert!(matches!(
        res.lanes[0].steps[0],
        StepResolution::Resolved { .. }
    ));
    let StepResolution::Unresolved { methods } = res.lanes[0].steps[1] else {
        panic!(
            "deleted clip must be unresolved, got {:?}",
            res.lanes[0].steps[1]
        );
    };
    assert_eq!(
        (methods.id, methods.name, methods.file),
        (Some(false), Some(false), Some(false))
    );
}

#[test]
fn scenario_deleted_duplicate_falls_back_to_the_other_copy() {
    // Two identical "Clip 02" clips on L2 (columns 3 and 6); delete column 3.
    let mut v = named_deck1();
    let mut seq = sequence_from(&v, &[("L2", &[(2, 3)])]);
    clear_clip(&mut v, 2, 3);
    let idx = parse_composition_value(&v).unwrap().index;
    let res = resolve_sequence(&seq, &idx);
    let c = res.lanes[0].steps[0].clip().unwrap();
    assert_eq!((c.layer, c.column, c.by, c.ambiguous), (2, 6, Name, false));
    assert_eq!(refresh_ids(&mut seq, &res), 1);
    assert_eq!(
        seq.lanes[0].steps[0].clip.as_ref().unwrap().id,
        Some(1792788832244)
    );
}

#[test]
fn scenario_duplicate_names() {
    let v = named_deck1();
    // Stored with an id, the exact copy is used, even though another clip has the same name.
    let seq = sequence_from(&v, &[("L2", &[(2, 6)])]);
    assert_eq!(positions(&seq, &v), vec![vec![Some((2, 6, Id))]]);

    // Name only: lane layer L2 holds two copies → lowest column, flagged ambiguous.
    let mut by_name = seq.clone();
    by_name.lanes[0].steps[0].clip.as_mut().unwrap().id = None;
    let idx = parse_composition_value(&v).unwrap().index;
    let c = *resolve_sequence(&by_name, &idx).lanes[0].steps[0]
        .clip()
        .unwrap();
    assert_eq!((c.layer, c.column, c.by, c.ambiguous), (2, 3, Name, true));

    // "Clip 03…" exists on L2 (col 4) and L3 (col 6): the lane's layer wins.
    let mut cross = sequence_from(&v, &[("L3", &[(3, 6)])]);
    cross.lanes[0].steps[0].clip.as_mut().unwrap().id = None;
    let c = *resolve_sequence(&cross, &idx).lanes[0].steps[0]
        .clip()
        .unwrap();
    assert_eq!((c.layer, c.column, c.ambiguous), (3, 6, false));
}

#[test]
fn scenario_slot_reloaded_with_other_media() {
    let mut v = named_deck1();
    let seq = sequence_from(&v, &[("L1", &[(1, 5)])]);
    reload_slot(
        &mut v,
        1,
        5,
        "something else",
        r"Q:\media\something else.mov",
    );
    let idx = parse_composition_value(&v).unwrap().index;
    let res = resolve_sequence(&seq, &idx);
    assert!(
        matches!(res.lanes[0].steps[0], StepResolution::Unresolved { .. }),
        "an id pointing at different media must not resolve"
    );
}

#[test]
fn scenario_clip_renamed_and_moved_without_id_resolves_by_file() {
    let mut v = named_deck1();
    let mut seq = sequence_from(&v, &[("L1", &[(1, 6)])]);
    seq.lanes[0].steps[0].clip.as_mut().unwrap().id = None;
    clip_mut(&mut v, 1, 6)["name"]["value"] = "Fashion (renamed)".into();
    move_column(&mut v, 6, 1);
    assert_eq!(positions(&seq, &v), vec![vec![Some((1, 1, File))]]);
}

#[test]
fn seq_plugin_parameters_are_read_from_sourceparams() {
    // Recorded with the "1000A Sequence" FFGL source on two § clips (notes §11).
    let idx = parse_composition(include_str!(
        "../../../docs/samples/composition-seq-plugin.json"
    ))
    .unwrap()
    .index;
    let seq = match idx.layer_by_name("SEQ") {
        LayerLookup::Found(l) => l.index,
        other => panic!("{other:?}"),
    };
    let params: Vec<_> = idx
        .filled_clips()
        .filter(|c| c.layer == seq)
        .map(|c| (c.column, c.sequence_param.clone().map(|p| (p.id, p.value))))
        .collect();
    assert_eq!(
        params,
        vec![
            (1, Some((1793025852850, String::new()))),
            (2, Some((1793025852910, String::new())))
        ]
    );
    // Ordinary video clips have none.
    assert!(idx
        .filled_clips()
        .filter(|c| c.layer != seq)
        .all(|c| c.sequence_param.is_none()));
}
