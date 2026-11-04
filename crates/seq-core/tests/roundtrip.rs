//! Round-trip tests of the sequence file format against fixture files (SPEC §6).

use seq_core::{
    from_json, to_json, AfterLastPass, LaneEnd, LaneMode, LoadError, Repeat, Start, Step,
    ValidationErrorKind,
};
use serde_json::Value;

const SPEC_EXAMPLE: &str = include_str!("fixtures/spec-example.seq.json");
/// The same document in format version 1 (lengths in eighths).
const SPEC_EXAMPLE_V1: &str = include_str!("fixtures/spec-example-v1.seq.json");
const FULL: &str = include_str!("fixtures/full.seq.json");
const INVALID: &str = include_str!("fixtures/invalid.seq.json");

/// parse → serialize → parse gives the same model, and the serialized JSON carries exactly the
/// same data as the fixture (compared as JSON values, so whitespace and key order don't matter).
fn assert_round_trip(fixture: &str) {
    let file = from_json(fixture).expect("fixture parses");
    let text = to_json(&file);
    assert_eq!(from_json(&text).expect("output parses"), file);
    let original: Value = serde_json::from_str(fixture).unwrap();
    let written: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(written, original);
    // Writing is deterministic: a second save produces identical bytes.
    assert_eq!(to_json(&from_json(&text).unwrap()), text);
}

#[test]
fn spec_example_round_trips() {
    assert_round_trip(SPEC_EXAMPLE);
}

#[test]
fn full_fixture_round_trips() {
    assert_round_trip(FULL);
}

#[test]
fn spec_example_maps_to_model() {
    let file = from_json(SPEC_EXAMPLE).unwrap();
    assert_eq!(file.version, 3);
    assert_eq!(file.composition, "MyShow");
    let seq = &file.decks["Act 1"].sequences[0];
    assert_eq!(seq.id.to_string(), "6f1c2a9e-0d5b-4c1e-9a0f-2f7d4c1b8e11");
    assert_eq!(seq.name, "Drop A");
    assert_eq!(seq.start, Start::Immediate);
    assert_eq!(seq.repeat, Repeat::Count { count: 4 });
    assert_eq!(seq.after_last_pass, AfterLastPass::Clear);
    let lane = &seq.lanes[0];
    assert_eq!(lane.layer, "BG");
    assert_eq!(lane.end, LaneEnd::LoopToEnd);
    let clip = lane.steps[0].clip.as_ref().unwrap();
    assert_eq!(clip.id, Some(1712345));
    assert_eq!(clip.name.as_deref(), Some("Tunnel Red"));
    assert_eq!(clip.file.as_deref(), Some("tunnel_red.mov"));
    assert_eq!(lane.steps[0].length, 32);
    assert_eq!(lane.steps[1].clip, None);
    assert_eq!(lane.mode, LaneMode::Clip);
    let slot = &seq.lanes[1];
    assert_eq!(slot.layer, "FG");
    assert_eq!(slot.mode, LaneMode::Slot);
    assert_eq!(
        slot.steps,
        [
            Step::slot(Some(1), 16),
            Step::slot(Some(2), 16),
            Step::slot(None, 16)
        ]
    );
    assert_eq!(seq.pass_length(), 48);
}

#[test]
fn v1_file_migrates_to_the_current_document() {
    // Lengths double (eighths -> sixteenths), everything else is unchanged. The v3 example also has
    // a slot lane, which v1 cannot have: compare without it.
    let mut current = from_json(SPEC_EXAMPLE).unwrap();
    for d in current.decks.values_mut() {
        for s in &mut d.sequences {
            s.lanes.retain(|l| l.mode == LaneMode::Clip);
        }
    }
    assert_eq!(from_json(SPEC_EXAMPLE_V1).unwrap(), current);
}

#[test]
fn full_fixture_covers_all_variants() {
    let file = from_json(FULL).unwrap();
    let all: Vec<_> = file.decks.values().flat_map(|d| &d.sequences).collect();
    let repeats: Vec<_> = all.iter().map(|s| s.repeat).collect();
    assert!(repeats.contains(&Repeat::Once));
    assert!(repeats.contains(&Repeat::Infinite));
    assert!(repeats.contains(&Repeat::Count { count: 2 }));
    let ends: Vec<_> = all.iter().flat_map(|s| &s.lanes).map(|l| l.end).collect();
    for e in [LaneEnd::LoopToEnd, LaneEnd::Hold, LaneEnd::Clear] {
        assert!(ends.contains(&e), "{e:?} not covered");
    }
    assert!(file.decks["empty"].sequences.is_empty());
    let (deck, seq) = file
        .find_sequence("0b5e3a52-8f0e-4e8e-b1d2-7c4f6a9d1e03".parse().unwrap())
        .unwrap();
    assert_eq!(deck, "2132323");
    assert_eq!(seq.name, "§ odd name: \"quotes\" & ümlauts");
}

#[test]
fn invalid_fixture_reports_every_problem() {
    let Err(LoadError::Invalid(errors)) = from_json(INVALID) else {
        panic!("expected validation errors");
    };
    let kinds: Vec<_> = errors.iter().map(|e| &e.kind).collect();
    assert_eq!(errors.len(), 6, "{errors:#?}");
    assert!(matches!(
        kinds[0],
        ValidationErrorKind::CountTooSmall { count: 1 }
    ));
    assert!(matches!(kinds[1], ValidationErrorKind::ZeroLength));
    assert!(matches!(kinds[2], ValidationErrorKind::EmptyClipRef));
    assert!(matches!(kinds[3], ValidationErrorKind::ZeroColumn));
    assert!(matches!(
        kinds[4],
        ValidationErrorKind::DuplicateName { first_index: 0, .. }
    ));
    assert!(matches!(kinds[5], ValidationErrorKind::NoLanes));
    let message = LoadError::Invalid(errors).to_string();
    assert!(
        message.starts_with("6 problem(s) in sequence file:"),
        "{message}"
    );
    assert!(message.contains(r#"decks["Act 1"].sequences[0] "Drop A".lanes[0].steps[0].length"#));
}
