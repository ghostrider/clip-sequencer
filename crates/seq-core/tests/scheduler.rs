//! Scheduler rules from SPEC §3, §5 and §7, driven by simulated tick streams.

use seq_core::scheduler::{
    bar_beat, Action, EndReason, LaneState, Origin, RunLane, RunSpec, RunStep, Scheduler,
    StartError, StepTarget, StopMode, Wait,
};
use seq_core::{AfterLastPass, LaneEnd, Repeat, Start};
use uuid::Uuid;

const E: u64 = 12; // ticks per eighth
const S: u64 = 6; // ticks per sixteenth
const BAR: u64 = 96;

use StepTarget::{Clip, Rest, Slot, Unresolved};

fn lane(layer: u64, end: LaneEnd, steps: &[(u64, StepTarget)]) -> RunLane {
    RunLane {
        layer_id: Some(layer),
        end,
        steps: steps
            .iter()
            .map(|&(eighths, target)| RunStep::new(eighths * E, target))
            .collect(),
    }
}

fn spec(id: u128, repeat: Repeat, after: AfterLastPass, lanes: Vec<RunLane>) -> RunSpec {
    RunSpec {
        seq_id: Uuid::from_u128(id),
        name: format!("seq{id}"),
        repeat,
        after_last_pass: after,
        lanes,
        seed: 7,
    }
}

fn seq(id: u128) -> Uuid {
    Uuid::from_u128(id)
}

/// Runs `n` ticks; returns (tick number, action) with tick 0 = actions passed in from start.
fn run(s: &mut Scheduler, start: Vec<Action>, n: u64) -> Vec<(u64, Action)> {
    let mut out: Vec<(u64, Action)> = start.into_iter().map(|a| (0, a)).collect();
    for t in 1..=n {
        out.extend(s.on_tick().into_iter().map(|a| (t, a)));
    }
    out
}

/// (tick, clip id) of every trigger.
fn triggers(log: &[(u64, Action)]) -> Vec<(u64, u64)> {
    log.iter()
        .filter_map(|(t, a)| match a {
            Action::Trigger { clip_id, .. } => Some((*t, *clip_id)),
            _ => None,
        })
        .collect()
}

/// (tick, layer id) of every clear.
fn clears(log: &[(u64, Action)]) -> Vec<(u64, u64)> {
    log.iter()
        .filter_map(|(t, a)| match a {
            Action::Clear { layer_id, .. } => Some((*t, *layer_id)),
            _ => None,
        })
        .collect()
}

fn ended(log: &[(u64, Action)]) -> Vec<(u64, Uuid, EndReason)> {
    log.iter()
        .filter_map(|(t, a)| match a {
            Action::Ended { seq, reason, .. } => Some((*t, *seq, *reason)),
            _ => None,
        })
        .collect()
}

#[test]
fn start_triggers_first_steps_immediately_and_boundaries_fall_on_exact_ticks() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![
            lane(
                100,
                LaneEnd::LoopToEnd,
                &[(8, Clip(1)), (4, Clip(2)), (1, Clip(3))],
            ),
            lane(200, LaneEnd::LoopToEnd, &[(13, Clip(9))]),
        ],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    assert!(matches!(start[0], Action::Started { .. }));
    assert!(matches!(start[1], Action::PassStarted { pass: 1, .. }));
    let log = run(&mut s, start, 13 * E);
    assert_eq!(
        triggers(&log),
        vec![
            (0, 1),
            (0, 9),
            (BAR, 2),
            (BAR + 4 * E, 3),
            (13 * E, 1),
            (13 * E, 9)
        ]
    );
}

#[test]
fn loop_to_end_restarts_short_lane_until_pass_end() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Once,
        AfterLastPass::Hold,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(3, Clip(1))]),
            lane(200, LaneEnd::Hold, &[(8, Clip(2))]),
        ],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, BAR);
    let l1: Vec<_> = triggers(&log)
        .into_iter()
        .filter(|t| t.1 == 1)
        .map(|t| t.0)
        .collect();
    // 3-eighth loop inside an 8-eighth pass: 0, 3, 6 (the third is cut off by the pass end).
    assert_eq!(l1, vec![0, 3 * E, 6 * E]);
}

#[test]
fn sixteenth_steps_fire_every_six_ticks() {
    let mut s = Scheduler::new();
    // 1/16, 1/16, 3/16 looped; a 1-bar lane sets the pass length.
    let mut sp = spec(
        1,
        Repeat::Once,
        AfterLastPass::Hold,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[]),
            lane(200, LaneEnd::Hold, &[(8, Clip(9))]),
        ],
    );
    sp.lanes[0].steps = [(1, Clip(1)), (1, Clip(2)), (3, Clip(3))]
        .iter()
        .map(|&(sixteenths, target)| RunStep::new(sixteenths * S, target))
        .collect();
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, BAR);
    let l1: Vec<_> = triggers(&log).into_iter().filter(|t| t.1 != 9).collect();
    // 5-sixteenth loop: 0, 6, 12, then again from 30.
    assert_eq!(
        &l1[..6],
        &[
            (0, 1),
            (S, 2),
            (2 * S, 3),
            (5 * S, 1),
            (6 * S, 2),
            (7 * S, 3)
        ]
    );
}

#[test]
fn hold_lane_keeps_last_clip_and_clear_lane_clears_at_its_end() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Count { count: 2 },
        AfterLastPass::Hold,
        vec![
            lane(100, LaneEnd::Hold, &[(2, Clip(1)), (2, Clip(2))]),
            lane(200, LaneEnd::Clear, &[(3, Clip(3))]),
            lane(300, LaneEnd::LoopToEnd, &[(8, Clip(4))]),
        ],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 2 * BAR);
    let t = triggers(&log);
    assert!(t.contains(&(2 * E, 2)));
    assert!(
        !t.iter()
            .any(|&(tick, c)| c == 2 && tick > 2 * E && tick < BAR),
        "hold must not retrigger"
    );
    // Clear lane: cleared at 3 eighths in each pass; nothing else clears (after_last_pass hold).
    assert_eq!(clears(&log), vec![(3 * E, 200), (BAR + 3 * E, 200)]);
}

#[test]
fn rest_clears_the_layer_and_unresolved_does_nothing() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Hold,
        vec![lane(
            100,
            LaneEnd::LoopToEnd,
            &[(1, Clip(1)), (1, Rest), (1, Unresolved), (1, Clip(2))],
        )],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 4 * E - 1);
    assert_eq!(triggers(&log), vec![(0, 1), (3 * E, 2)]);
    assert_eq!(clears(&log), vec![(E, 100)]);
    let steps: Vec<_> = log
        .iter()
        .filter_map(|(t, a)| matches!(a, Action::StepStarted { .. }).then_some(*t))
        .collect();
    assert_eq!(
        steps,
        vec![0, E, 2 * E, 3 * E],
        "unresolved steps still report a boundary"
    );
}

#[test]
fn repeat_once_finishes_with_after_last_pass_clear() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Once,
        AfterLastPass::Clear,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(8, Clip(1))]),
            lane(200, LaneEnd::Hold, &[(4, Clip(2))]),
        ],
    );
    let start = s
        .start(sp, Origin::SeqClip { clip_id: 77 }, Start::Immediate)
        .unwrap();
    let log = run(&mut s, start, BAR + 10);
    assert_eq!(triggers(&log), vec![(0, 1), (0, 2)]);
    assert_eq!(clears(&log), vec![(BAR, 100), (BAR, 200)]);
    assert_eq!(ended(&log), vec![(BAR, seq(1), EndReason::Finished)]);
    assert!(log.iter().any(|(_, a)| matches!(
        a,
        Action::Ended {
            origin: Origin::SeqClip { clip_id: 77 },
            ..
        }
    )));
    assert!(!s.is_running(seq(1)));
}

#[test]
fn after_last_pass_hold_leaves_layers_alone() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Once,
        AfterLastPass::Hold,
        vec![lane(100, LaneEnd::Hold, &[(1, Clip(1))])],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 2 * E);
    assert!(clears(&log).is_empty());
    assert_eq!(ended(&log), vec![(E, seq(1), EndReason::Finished)]);
}

#[test]
fn repeat_count_plays_exactly_n_passes() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Count { count: 3 },
        AfterLastPass::Clear,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(2, Clip(1)), (2, Clip(2))]),
            lane(200, LaneEnd::Clear, &[(1, Clip(3))]),
        ],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 20 * E);
    let passes: Vec<_> = log
        .iter()
        .filter_map(|(t, a)| match a {
            Action::PassStarted { pass, .. } => Some((*t, *pass)),
            _ => None,
        })
        .collect();
    assert_eq!(passes, vec![(0, 1), (4 * E, 2), (8 * E, 3)]);
    // All lanes restart at step 1 in the same tick as the pass starts.
    let t = triggers(&log);
    for p in [0, 4 * E, 8 * E] {
        assert!(t.contains(&(p, 1)) && t.contains(&(p, 3)));
    }
    assert_eq!(t.len(), 9);
    assert_eq!(ended(&log), vec![(12 * E, seq(1), EndReason::Finished)]);
}

#[test]
fn infinite_repeat_keeps_going_without_drift() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![lane(100, LaneEnd::LoopToEnd, &[(8, Clip(1)), (8, Clip(2))])],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 1000 * BAR);
    let t = triggers(&log);
    assert_eq!(t.len(), 1001);
    assert!(
        t.iter()
            .enumerate()
            .all(|(i, &(tick, clip))| tick == i as u64 * BAR
                && clip == if i % 2 == 0 { 1 } else { 2 })
    );
    assert!(ended(&log).is_empty());
}

#[test]
fn no_ticks_freezes_and_ticks_resume_where_it_stopped() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![lane(100, LaneEnd::LoopToEnd, &[(1, Clip(1)), (1, Clip(2))])],
    );
    s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    for _ in 0..5 {
        s.on_tick();
    }
    // Clock lost: the caller simply delivers no ticks. Position stays.
    assert_eq!(s.status()[0].pos, 5);
    let mut rest = Vec::new();
    for t in 6..=E {
        rest.extend(s.on_tick().into_iter().map(|a| (t, a)));
    }
    assert_eq!(triggers(&rest), vec![(E, 2)]);
}

#[test]
fn restart_takes_a_new_snapshot_and_releases_shared_layers_without_clearing() {
    let mut s = Scheduler::new();
    let v1 = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(8, Clip(1))]),
            lane(200, LaneEnd::LoopToEnd, &[(8, Clip(2))]),
        ],
    );
    s.start(v1, Origin::Ui, Start::Immediate).unwrap();
    for _ in 0..30 {
        s.on_tick();
    }
    // Edited version: lane on 200 removed, clip on 100 changed.
    let v2 = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![lane(100, LaneEnd::LoopToEnd, &[(8, Clip(5))])],
    );
    let acts = s.start(v2, Origin::Ui, Start::Immediate).unwrap();
    let log: Vec<_> = acts.into_iter().map(|a| (0, a)).collect();
    assert_eq!(
        clears(&log),
        vec![(0, 200)],
        "only the layer the new snapshot no longer uses"
    );
    assert_eq!(triggers(&log), vec![(0, 5)]);
    assert_eq!(ended(&log), vec![(0, seq(1), EndReason::Restarted)]);
    assert_eq!(s.status().len(), 1);
    assert_eq!(s.status()[0].pos, 0);
}

#[test]
fn starting_on_an_owned_layer_takes_over_only_that_lane() {
    let mut s = Scheduler::new();
    let a = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(1, Clip(1))]),
            lane(200, LaneEnd::LoopToEnd, &[(1, Clip(2))]),
        ],
    );
    s.start(a, Origin::Ui, Start::Immediate).unwrap();
    s.on_tick();
    let b = spec(
        2,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![lane(200, LaneEnd::LoopToEnd, &[(1, Clip(9))])],
    );
    let acts = s.start(b, Origin::Ui, Start::Immediate).unwrap();
    assert!(acts.contains(&Action::LaneStateChanged {
        seq: seq(1),
        lane: 1,
        state: LaneState::TakenOver
    }));
    assert!(
        !acts.iter().any(|a| matches!(a, Action::Clear { .. })),
        "no clear on takeover"
    );
    assert_eq!(s.owner_of(200), Some((seq(2), 0)));
    assert_eq!(s.owner_of(100), Some((seq(1), 0)));
    // A (one tick ahead) keeps looping its other lane; B owns 200; A's clip 2 never fires again.
    let log = run(&mut s, Vec::new(), E);
    assert_eq!(triggers(&log), vec![(E - 1, 1), (E, 9)]);
}

#[test]
fn sequence_whose_lanes_are_all_taken_over_ends() {
    let mut s = Scheduler::new();
    s.start(
        spec(
            1,
            Repeat::Infinite,
            AfterLastPass::Clear,
            vec![lane(100, LaneEnd::LoopToEnd, &[(1, Clip(1))])],
        ),
        Origin::Ui,
        Start::Immediate,
    )
    .unwrap();
    let acts = s
        .start(
            spec(
                2,
                Repeat::Infinite,
                AfterLastPass::Clear,
                vec![lane(100, LaneEnd::LoopToEnd, &[(1, Clip(2))])],
            ),
            Origin::Ui,
            Start::Immediate,
        )
        .unwrap();
    let log: Vec<_> = acts.into_iter().map(|a| (0, a)).collect();
    assert_eq!(ended(&log), vec![(0, seq(1), EndReason::TakenOver)]);
    assert!(clears(&log).is_empty());
    assert_eq!(s.running().collect::<Vec<_>>(), vec![seq(2)]);
}

#[test]
fn yielded_lane_counts_silently_and_resumes_at_next_boundary() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![
            lane(
                100,
                LaneEnd::LoopToEnd,
                &[(1, Clip(1)), (1, Clip(2)), (1, Clip(3))],
            ),
            lane(200, LaneEnd::LoopToEnd, &[(1, Clip(9))]),
        ],
    );
    s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    s.on_tick();
    let y = s.yield_layer(100);
    assert_eq!(
        y,
        vec![Action::LaneStateChanged {
            seq: seq(1),
            lane: 0,
            state: LaneState::Yielded {
                resume_pending: false
            }
        }]
    );
    assert_eq!(
        s.owner_of(100),
        Some((seq(1), 0)),
        "yielded lane still owns its layer"
    );
    let mut log = run(&mut s, Vec::new(), E + 3);
    assert!(
        !triggers(&log).iter().any(|&(_, c)| c == 2),
        "yielded lane must not trigger"
    );
    assert_eq!(s.status()[0].lanes[0].step, Some(1), "but keeps counting");
    // Resume requested mid-step: nothing until the next boundary (position 2E = step 3).
    s.resume_lane(seq(1), 0);
    assert_eq!(
        s.status()[0].lanes[0].state,
        LaneState::Yielded {
            resume_pending: true
        }
    );
    let mut resumed = Vec::new();
    for _ in (E + 5)..=(2 * E) {
        resumed.extend(s.on_tick());
    }
    assert_eq!(s.status()[0].pos, 2 * E);
    assert!(resumed.contains(&Action::Trigger {
        seq: seq(1),
        lane: 0,
        step: 2,
        clip_id: 3,
        layer_id: 100
    }));
    assert_eq!(s.status()[0].lanes[0].state, LaneState::Playing);
    log.clear();
}

#[test]
fn yield_survives_pass_boundaries_and_is_not_cleared_on_finish() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Count { count: 2 },
        AfterLastPass::Clear,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(1, Clip(1))]),
            lane(200, LaneEnd::LoopToEnd, &[(1, Clip(2))]),
        ],
    );
    s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    s.yield_layer(100);
    let log = run(&mut s, Vec::new(), 2 * E);
    assert!(!triggers(&log).iter().any(|&(_, c)| c == 1));
    assert_eq!(
        clears(&log),
        vec![(2 * E, 200)],
        "a manually played layer is left alone"
    );
}

#[test]
fn stop_modes() {
    let two_lanes = || {
        spec(
            1,
            Repeat::Infinite,
            AfterLastPass::Clear,
            vec![
                lane(100, LaneEnd::LoopToEnd, &[(1, Clip(1))]),
                lane(200, LaneEnd::LoopToEnd, &[(1, Clip(2))]),
            ],
        )
    };
    let clear_layers = |acts: &[Action]| -> Vec<u64> {
        acts.iter()
            .filter_map(|a| match a {
                Action::Clear { layer_id, .. } => Some(*layer_id),
                _ => None,
            })
            .collect()
    };

    let mut s = Scheduler::new();
    s.start(two_lanes(), Origin::Ui, Start::Immediate).unwrap();
    s.yield_layer(200);
    assert_eq!(
        clear_layers(&s.stop(seq(1), StopMode::AfterLastPass)),
        vec![100]
    );

    s.start(two_lanes(), Origin::Ui, Start::Immediate).unwrap();
    s.yield_layer(200);
    assert_eq!(clear_layers(&s.stop_all(StopMode::Clear)), vec![100, 200]);

    s.start(two_lanes(), Origin::Ui, Start::Immediate).unwrap();
    let acts = s.stop(seq(1), StopMode::Release);
    assert!(clear_layers(&acts).is_empty());
    assert!(matches!(
        acts.last(),
        Some(Action::Ended {
            reason: EndReason::Stopped,
            ..
        })
    ));
    assert!(
        s.stop(seq(1), StopMode::Clear).is_empty(),
        "stopping a stopped sequence is a no-op"
    );
}

#[test]
fn silent_lanes_neither_play_nor_own() {
    let mut s = Scheduler::new();
    let mut unresolved = lane(100, LaneEnd::LoopToEnd, &[(16, Clip(1))]);
    unresolved.layer_id = None;
    let sp = spec(
        1,
        Repeat::Once,
        AfterLastPass::Clear,
        vec![
            unresolved,
            lane(200, LaneEnd::LoopToEnd, &[(8, Clip(2))]),
            lane(300, LaneEnd::Hold, &[]),
        ],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 2 * BAR);
    // The silent 16-eighth lane still sets the pass length (2 bars).
    assert_eq!(triggers(&log), vec![(0, 2), (BAR, 2)]);
    assert_eq!(clears(&log), vec![(2 * BAR, 200)]);
    assert_eq!(s.owner_of(300), None);
}

#[test]
fn start_errors() {
    let mut s = Scheduler::new();
    let empty = spec(
        1,
        Repeat::Once,
        AfterLastPass::Clear,
        vec![lane(100, LaneEnd::Hold, &[])],
    );
    assert_eq!(
        s.start(empty, Origin::Ui, Start::Immediate),
        Err(StartError::Empty)
    );
    let mut l = lane(100, LaneEnd::Hold, &[(1, Clip(1))]);
    l.layer_id = None;
    let no_layer = spec(1, Repeat::Once, AfterLastPass::Clear, vec![l]);
    assert_eq!(
        s.start(no_layer, Origin::Ui, Start::Immediate),
        Err(StartError::NoPlayableLanes)
    );
}

#[test]
fn status_reports_position_pass_and_steps() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Count { count: 4 },
        AfterLastPass::Clear,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(8, Clip(1)), (8, Clip(2))]),
            lane(200, LaneEnd::Hold, &[(4, Clip(3))]),
        ],
    );
    s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    for _ in 0..(2 * BAR + BAR + 30) {
        s.on_tick();
    }
    let st = &s.status()[0];
    assert_eq!(
        (st.pass, st.passes, st.pos, st.pass_ticks),
        (2, Some(4), BAR + 30, 2 * BAR)
    );
    assert_eq!(st.lanes[0].step, Some(1));
    assert_eq!(st.lanes[1].step, None, "hold lane past its end");
    assert_eq!(bar_beat(st.pos), (2, 2));
    assert_eq!(bar_beat(0), (1, 1));
}

#[test]
fn run_spec_from_sequence_and_resolution() {
    use seq_core::index::{ClipInfo, ColumnInfo, CompositionIndex, LayerInfo, Thumbnail};
    use seq_core::resolve::resolve_sequence;
    use seq_core::{ClipRef, Lane, Sequence, Step};

    let clip = |id, column, name: &str| ClipInfo {
        id,
        layer: 1,
        column,
        name: name.into(),
        file_path: None,
        empty: false,
        thumbnail: Thumbnail {
            last_update: "0".into(),
            is_default: true,
        },
        connected_param: 0,
        name_param: 0,
        sequence_param: None,
    };
    let idx = CompositionIndex::new(
        "Show".into(),
        vec![],
        Some("D".into()),
        vec![LayerInfo {
            id: 500,
            index: 1,
            name: "BG".into(),
            name_param: 0,
        }],
        vec![ColumnInfo {
            id: 1,
            index: 1,
            name: String::new(),
        }],
        vec![clip(10, 1, "A")],
    );
    let named = |n: &str| {
        Some(ClipRef {
            name: Some(n.into()),
            ..Default::default()
        })
    };
    let sequence = Sequence {
        id: seq(3),
        name: "S".into(),
        start: seq_core::Start::Immediate,
        repeat: Repeat::Count { count: 2 },
        after_last_pass: AfterLastPass::Hold,
        sections: Vec::new(),
        lanes: vec![
            Lane {
                layer: "BG".into(),
                mode: seq_core::model::LaneMode::Clip,
                end: LaneEnd::Clear,
                transition: None,
                steps: vec![
                    Step {
                        column: None,
                        clip: named("A"),
                        length: 16,
                        ..Default::default()
                    },
                    Step {
                        column: None,
                        clip: None,
                        length: 4,
                        ..Default::default()
                    },
                    Step {
                        column: None,
                        clip: named("Missing"),
                        length: 2,
                        ..Default::default()
                    },
                ],
            },
            Lane {
                layer: "Nope".into(),
                mode: seq_core::model::LaneMode::Clip,
                end: LaneEnd::Hold,
                transition: None,
                steps: vec![Step {
                    column: None,
                    clip: named("A"),
                    length: 8,
                    ..Default::default()
                }],
            },
        ],
    };
    let rs = RunSpec::new(&sequence, &resolve_sequence(&sequence, &idx));
    assert_eq!(rs.lanes[0].layer_id, Some(500));
    assert_eq!(
        rs.lanes[0].steps,
        vec![
            RunStep::new(96, Clip(10)),
            RunStep::new(24, Rest),
            RunStep::new(12, Unresolved),
        ]
    );
    assert_eq!(rs.lanes[1].layer_id, None);
    assert_eq!(rs.pass_ticks(), 132);
    assert_eq!(rs.passes(), Some(2));

    // Format v4: condition, choices (resolved like option 1) and pick reach the snapshot (M22);
    // sections and transitions don't change it (transitions: M21).
    use seq_core::model::{Choice, Pick, Section, Transition, When};
    let mut v4 = sequence.clone();
    v4.start = seq_core::Start::Bar { bars: 4 };
    v4.sections = vec![Section {
        at: 8,
        name: "Drop".into(),
    }];
    v4.lanes[0].transition = Some(Transition {
        length: 4,
        blend: Some("Add".into()),
    });
    for (i, step) in v4.lanes[0].steps.iter_mut().enumerate() {
        step.when = Some(When::Cycle { pass: 2, of: 2 });
        step.choices = vec![None, named("Missing").map(Choice::Clip)];
        step.pick = Some(if i % 2 == 0 {
            Pick::Cycle
        } else {
            Pick::Random
        });
        step.transition = Some(Transition {
            length: 0,
            blend: None,
        });
    }
    let v4rs = RunSpec::new(&v4, &resolve_sequence(&v4, &idx));
    let mut expected = rs.clone();
    for (i, step) in expected.lanes[0].steps.iter_mut().enumerate() {
        step.when = Some(When::Cycle { pass: 2, of: 2 });
        step.choices = vec![Rest, Unresolved];
        step.pick = if i % 2 == 0 {
            Pick::Cycle
        } else {
            Pick::Random
        };
    }
    assert_eq!(v4rs, expected);
}

// ---- pass conditions and choices (format v4, SPEC §6) ----

#[test]
fn conditions_per_pass() {
    use seq_core::model::When;
    use seq_core::scheduler::plays_on;
    let on = |w, passes: Option<u32>| -> Vec<bool> {
        (1..=4).map(|p| plays_on(Some(w), p, passes)).collect()
    };
    assert_eq!(on(When::First, Some(4)), [true, false, false, false]);
    assert_eq!(on(When::NotFirst, Some(4)), [false, true, true, true]);
    assert_eq!(on(When::Last, Some(4)), [false, false, false, true]);
    assert_eq!(on(When::NotLast, Some(4)), [true, true, true, false]);
    // Infinite: no last pass.
    assert_eq!(on(When::Last, None), [false; 4]);
    assert_eq!(on(When::NotLast, None), [true; 4]);
    // `once` has one pass, which is first and last.
    assert!(plays_on(Some(When::Last), 1, Some(1)) && plays_on(Some(When::First), 1, Some(1)));
    assert_eq!(
        on(When::Cycle { pass: 2, of: 2 }, None),
        [false, true, false, true]
    );
    assert_eq!(
        on(When::Cycle { pass: 1, of: 3 }, None),
        [true, false, false, true]
    );
    assert!(plays_on(None, 3, Some(4)));
}

fn with(step: RunStep, f: impl FnOnce(&mut RunStep)) -> RunStep {
    let mut step = step;
    f(&mut step);
    step
}

#[test]
fn a_step_that_doesnt_play_sends_nothing_and_the_clip_plays_through() {
    use seq_core::model::When;
    // Two-step lane; step 2 (clip 20) only on the last of 3 passes.
    let mut sp = spec(
        1,
        Repeat::Count { count: 3 },
        AfterLastPass::Hold,
        vec![lane(
            500,
            LaneEnd::LoopToEnd,
            &[(8, Clip(10)), (8, Clip(20))],
        )],
    );
    sp.lanes[0].steps[1].when = Some(When::Last);
    let mut s = Scheduler::new();
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 3 * 2 * BAR);
    assert_eq!(
        triggers(&log),
        [(0, 10), (2 * BAR, 10), (4 * BAR, 10), (5 * BAR, 20)]
    );
    assert!(clears(&log).is_empty(), "no clear: clip 10 plays through");
    // The step still begins (for the UI), with no option played.
    assert!(log
        .iter()
        .any(|(t, a)| *t == BAR && matches!(a, Action::StepStarted { step: 1, .. })));
}

#[test]
fn loops_of_a_short_lane_share_the_pass() {
    use seq_core::model::When;
    // Lane of one bar in a two-bar pass, `first`: plays twice in pass 1 (its loops), not in pass 2.
    let mut sp = spec(
        1,
        Repeat::Count { count: 2 },
        AfterLastPass::Hold,
        vec![
            lane(500, LaneEnd::LoopToEnd, &[(8, Clip(10))]),
            lane(501, LaneEnd::Hold, &[(16, Clip(99))]),
        ],
    );
    sp.lanes[0].steps[0].when = Some(When::First);
    let mut s = Scheduler::new();
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 4 * BAR - 1);
    let tens: Vec<_> = triggers(&log)
        .into_iter()
        .filter(|&(_, c)| c == 10)
        .collect();
    assert_eq!(tens, [(0, 10), (BAR, 10)]);
}

#[test]
fn cycle_pick_goes_through_the_options_and_starts_again_each_run() {
    use seq_core::model::Pick;
    let step = with(RunStep::new(8 * E, Clip(10)), |s| {
        s.choices = vec![Clip(11), Rest, Unresolved];
        s.pick = Pick::Cycle;
    });
    let sp = || RunSpec {
        lanes: vec![RunLane {
            layer_id: Some(500),
            end: LaneEnd::LoopToEnd,
            steps: vec![step.clone()],
        }],
        ..spec(1, Repeat::Infinite, AfterLastPass::Hold, vec![])
    };
    let mut s = Scheduler::new();
    let start = s.start(sp(), Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 5 * BAR);
    // 10, 11, rest (clear), unresolved (nothing), 10, 11 — every loop counts.
    assert_eq!(
        triggers(&log),
        [(0, 10), (BAR, 11), (4 * BAR, 10), (5 * BAR, 11)]
    );
    assert_eq!(clears(&log), [(2 * BAR, 500)]);
    let options: Vec<_> = s.status()[0].lanes.iter().map(|l| l.option).collect();
    assert_eq!(options, [Some(1)]);
    // A new start begins at option 1.
    let start = s.start(sp(), Origin::Ui, Start::Immediate).unwrap();
    assert_eq!(triggers(&run(&mut s, start, 0)), [(0, 10)]);
}

#[test]
fn random_pick_is_reproducible_from_the_seed_and_uses_every_option() {
    use seq_core::model::Pick;
    use seq_core::scheduler::pick_option;
    let step = with(RunStep::new(8 * E, Clip(10)), |s| {
        s.choices = vec![Clip(11), Clip(12), Clip(13)];
        s.pick = Pick::Random;
    });
    let picks =
        |seed| -> Vec<usize> { (0..64).map(|n| pick_option(&step, seed, 0, 0, n)).collect() };
    assert_eq!(picks(1), picks(1), "same seed, same picks");
    assert_ne!(picks(1), picks(2), "another seed, other picks");
    for i in 0..4 {
        assert!(picks(1).contains(&i), "option {i} is picked sometimes");
    }
    // Played: the triggers follow pick_option with the run's seed.
    let mut sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Hold,
        vec![RunLane {
            layer_id: Some(500),
            end: LaneEnd::LoopToEnd,
            steps: vec![step.clone()],
        }],
    );
    sp.seed = 42;
    let mut s = Scheduler::new();
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 7 * BAR);
    let got: Vec<u64> = triggers(&log).into_iter().map(|(_, c)| c).collect();
    let want: Vec<u64> = (0..8)
        .map(|n| 10 + pick_option(&step, 42, 0, 0, n) as u64)
        .collect();
    assert_eq!(got, want);
}

#[test]
fn slot_reads_follow_the_condition_and_the_pick() {
    use seq_core::model::{Pick, When};
    // Slot lane, one bar: column 1 or 2 in turn; a second step on column 5 only on pass 2+.
    let mut sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Hold,
        vec![lane(500, LaneEnd::LoopToEnd, &[(8, Slot(1)), (8, Slot(5))])],
    );
    sp.lanes[0].steps[0].choices = vec![Slot(2)];
    sp.lanes[0].steps[0].pick = Pick::Cycle;
    sp.lanes[0].steps[1].when = Some(When::NotFirst);
    let mut s = Scheduler::new();
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let cols = |s: &Scheduler, ahead| -> Vec<u32> {
        s.slot_steps_ahead(ahead).iter().map(|u| u.column).collect()
    };
    // Step 2 of pass 1 does not play: nothing to read.
    assert!(cols(&s, BAR).is_empty());
    // Step 1 of pass 2 is its second play: column 2.
    assert_eq!(cols(&s, 2 * BAR), [2]);
    run(&mut s, start, 2 * BAR);
    // Pass 2: step 2 plays (column 5), then step 1 plays the third time: column 1 again.
    assert_eq!(cols(&s, BAR), [5]);
    assert_eq!(cols(&s, 2 * BAR), [1]);
    let log = run(&mut s, Vec::new(), 2 * BAR);
    let played: Vec<_> = log
        .iter()
        .filter_map(|(t, a)| match a {
            Action::TriggerSlot { column, .. } => Some((*t, *column)),
            _ => None,
        })
        .collect();
    assert_eq!(played, [(BAR, 5), (2 * BAR, 1)]);
}

/// (tick, column) of every slot trigger.
fn slot_triggers(log: &[(u64, Action)]) -> Vec<(u64, u32)> {
    log.iter()
        .filter_map(|(t, a)| match a {
            Action::TriggerSlot { column, .. } => Some((*t, *column)),
            _ => None,
        })
        .collect()
}

#[test]
fn slot_steps_trigger_their_column_each_pass() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Count { count: 2 },
        AfterLastPass::Clear,
        vec![lane(
            7,
            LaneEnd::LoopToEnd,
            &[(8, Slot(3)), (8, Rest), (8, Slot(1))],
        )],
    );
    let start = s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let log = run(&mut s, start, 6 * BAR);
    assert_eq!(
        slot_triggers(&log),
        [(0, 3), (2 * BAR, 1), (3 * BAR, 3), (5 * BAR, 1)]
    );
    assert!(log.iter().any(|(_, a)| matches!(
        a,
        Action::TriggerSlot {
            layer_id: 7,
            lane: 0,
            step: 2,
            ..
        }
    )));
    assert_eq!(clears(&log), [(BAR, 7), (4 * BAR, 7), (6 * BAR, 7)]);
}

#[test]
fn slot_lookahead_reports_each_upcoming_step_once() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Count { count: 2 },
        AfterLastPass::Clear,
        vec![
            lane(7, LaneEnd::LoopToEnd, &[(8, Slot(3)), (8, Slot(4))]),
            lane(8, LaneEnd::Hold, &[(8, Clip(5))]),
        ],
    );
    s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    let ahead = 8;
    let mut seen = Vec::new();
    for t in 1..=4 * BAR {
        for u in s.slot_steps_ahead(ahead) {
            assert_eq!((u.seq, u.lane, u.layer_id), (seq(1), 0, 7));
            seen.push((t + ahead, u.step, u.column));
        }
        s.on_tick();
    }
    // The lookahead before tick `t` runs at position `t - 1`, so a step starting at tick `x` is
    // announced once, before tick `x - ahead + 1` (recorded above as `x + 1`).
    let expected: Vec<_> = [(BAR, 1, 4), (2 * BAR, 0, 3), (3 * BAR, 1, 4)]
        .into_iter()
        .map(|(at, step, col)| (at + 1, step, col))
        .collect();
    assert_eq!(seen, expected);
    // The run has ended: nothing ahead.
    assert!(s.slot_steps_ahead(ahead).is_empty());
}

#[test]
fn slot_lookahead_skips_lanes_that_will_not_trigger() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![lane(7, LaneEnd::LoopToEnd, &[(8, Slot(1)), (8, Slot(2))])],
    );
    s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    for _ in 0..BAR - 4 {
        s.on_tick();
    }
    assert_eq!(s.slot_steps_ahead(4).len(), 1);
    s.yield_layer(7);
    assert!(s.slot_steps_ahead(4).is_empty());
    s.resume_lane(seq(1), 0);
    assert_eq!(s.slot_steps_ahead(4)[0].column, 2);
    // Infinite repeat: the wrap to the next pass is announced too.
    for _ in 0..BAR {
        s.on_tick();
    }
    let up = s.slot_steps_ahead(4);
    assert_eq!((up[0].step, up[0].column), (0, 1));
}

// ---- Resync (SPEC §3.1) ----

#[test]
fn rewind_jumps_to_step_one_now_and_keeps_counting_from_there() {
    let mut s = Scheduler::new();
    let sp = spec(
        1,
        Repeat::Count { count: 3 },
        AfterLastPass::Clear,
        vec![
            lane(100, LaneEnd::LoopToEnd, &[(8, Clip(1)), (8, Clip(2))]),
            lane(200, LaneEnd::Clear, &[(4, Clip(3))]),
        ],
    );
    s.start(sp, Origin::Ui, Start::Immediate).unwrap();
    // Into pass 2, past the clear lane's end.
    for _ in 0..(2 * BAR + BAR + 5) {
        s.on_tick();
    }
    assert_eq!((s.status()[0].pass, s.status()[0].pos), (2, BAR + 5));
    let rw = s.rewind_all();
    assert_eq!(
        triggers(&rw.iter().cloned().map(|a| (0, a)).collect::<Vec<_>>()),
        vec![(0, 1), (0, 3)],
        "step 1 of every lane, the ended clear lane included"
    );
    assert!(
        !rw.iter().any(|a| matches!(a, Action::PassStarted { .. })),
        "the pass counter is kept"
    );
    let st = &s.status()[0];
    assert_eq!((st.pass, st.pos), (2, 0));
    // From here on boundaries fall as after a start.
    let log = run(&mut s, Vec::new(), BAR);
    assert_eq!(triggers(&log), vec![(BAR, 2)]);
    assert_eq!(clears(&log), vec![(4 * E, 200)]);
}

#[test]
fn rewind_leaves_yielded_and_taken_over_lanes_silent() {
    let mut s = Scheduler::new();
    s.start(
        spec(
            1,
            Repeat::Infinite,
            AfterLastPass::Clear,
            vec![
                lane(100, LaneEnd::LoopToEnd, &[(8, Clip(1))]),
                lane(200, LaneEnd::LoopToEnd, &[(8, Clip(2))]),
                lane(300, LaneEnd::LoopToEnd, &[(8, Clip(3))]),
            ],
        ),
        Origin::Ui,
        Start::Immediate,
    )
    .unwrap();
    s.start(
        spec(
            2,
            Repeat::Infinite,
            AfterLastPass::Clear,
            vec![lane(300, LaneEnd::LoopToEnd, &[(8, Clip(9))])],
        ),
        Origin::Ui,
        Start::Immediate,
    )
    .unwrap();
    for _ in 0..20 {
        s.on_tick();
    }
    s.yield_layer(200);
    let rw = s.rewind_all();
    let log: Vec<_> = rw.iter().cloned().map(|a| (0, a)).collect();
    assert_eq!(
        triggers(&log),
        vec![(0, 1), (0, 9)],
        "lane 2 is yielded, lane 3 belongs to sequence 2 now"
    );
    assert!(s.status().iter().all(|r| r.pos == 0));
    // A yielded lane with a resume pending resumes on the rewind.
    s.resume_lane(seq(1), 1);
    let rw = s.rewind_all();
    assert!(rw.contains(&Action::Trigger {
        seq: seq(1),
        lane: 1,
        step: 0,
        clip_id: 2,
        layer_id: 200
    }));
}

#[test]
fn rewind_without_running_sequences_does_nothing() {
    assert!(Scheduler::new().rewind_all().is_empty());
}

// ---- quantized start (SPEC §3.2) ----

const BEAT: u64 = 24;

fn one_clip(id: u128, layer: u64, clip: u64) -> RunSpec {
    spec(
        id,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![lane(layer, LaneEnd::LoopToEnd, &[(8, Clip(clip))])], // one bar
    )
}

/// Advances `n` ticks.
fn ticks(s: &mut Scheduler, n: u64) -> Vec<(u64, Action)> {
    run(s, Vec::new(), n)
}

fn started(log: &[(u64, Action)]) -> Vec<(u64, Uuid)> {
    log.iter()
        .filter_map(|(t, a)| match a {
            Action::Started { seq, .. } => Some((*t, *seq)),
            _ => None,
        })
        .collect()
}

#[test]
fn beat_start_waits_for_the_next_beat() {
    let mut s = Scheduler::new();
    ticks(&mut s, 10); // last tick: phase 9
    let acts = s
        .start(one_clip(1, 500, 10), Origin::Ui, Start::Beat)
        .unwrap();
    assert_eq!(
        acts,
        [Action::Pending {
            seq: seq(1),
            origin: Origin::Ui
        }]
    );
    assert!(s.is_pending(seq(1)) && !s.is_running(seq(1)));
    let p = s.pending();
    assert_eq!(
        (p[0].wait_ticks, p[0].wait, p[0].cue),
        (BEAT - 9, Wait::Mode(Start::Beat), false)
    );
    let log = ticks(&mut s, 20);
    // Phase 24 is the 15th tick from here; the first step triggers on it.
    assert_eq!(started(&log), [(15, seq(1))]);
    assert_eq!(triggers(&log), [(15, 10)]);
    assert!(s.pending().is_empty());
    // From then on it counts from its own start: the next bar is 96 ticks later.
    let log = ticks(&mut s, BAR);
    assert_eq!(triggers(&log), [(BAR - 5, 10)]);
}

#[test]
fn a_start_on_a_boundary_tick_starts_on_that_tick() {
    let mut s = Scheduler::new();
    ticks(&mut s, BAR + 1); // last tick: phase 96 = bar 1
    for (i, mode) in [Start::Beat, Start::Bar { bars: 1 }, Start::Immediate]
        .into_iter()
        .enumerate()
    {
        let id = i as u128 + 1;
        let acts = s
            .start(one_clip(id, 500 + id as u64, 10), Origin::Ui, mode)
            .unwrap();
        assert!(
            acts.iter()
                .any(|a| matches!(a, Action::Started { seq: x, .. } if *x == seq(id))),
            "{mode:?}"
        );
    }
    // Not a multiple of 2 bars: waits for bar 2.
    let acts = s
        .start(one_clip(9, 509, 10), Origin::Ui, Start::Bar { bars: 2 })
        .unwrap();
    assert!(matches!(acts[..], [Action::Pending { .. }]));
    assert_eq!(s.pending()[0].wait_ticks, BAR);
}

#[test]
fn bar_starts_wait_for_a_bar_index_that_is_a_multiple() {
    let mut s = Scheduler::new();
    ticks(&mut s, 100); // last tick: phase 99, in bar 1
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Bar { bars: 4 })
        .unwrap();
    s.start(one_clip(2, 501, 20), Origin::Ui, Start::Bar { bars: 1 })
        .unwrap();
    let waits: Vec<_> = s.pending().iter().map(|p| p.wait_ticks).collect();
    assert_eq!(waits, [4 * BAR - 99, 2 * BAR - 99]);
    let log = ticks(&mut s, 4 * BAR);
    assert_eq!(
        started(&log),
        [(2 * BAR - 99, seq(2)), (4 * BAR - 99, seq(1))]
    );
}

#[test]
fn before_the_first_tick_every_mode_starts_on_it() {
    let mut s = Scheduler::new();
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Bar { bars: 16 })
        .unwrap();
    assert_eq!(s.pending()[0].wait_ticks, 1);
    let log = ticks(&mut s, 1);
    assert_eq!(started(&log), [(1, seq(1))]);
    assert_eq!(triggers(&log), [(1, 10)]);
}

#[test]
fn starting_again_while_waiting_starts_at_once_and_stop_cancels() {
    let mut s = Scheduler::new();
    ticks(&mut s, 30);
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Bar { bars: 8 })
        .unwrap();
    let acts = s
        .start(one_clip(1, 500, 10), Origin::Ui, Start::Immediate)
        .unwrap();
    assert!(matches!(acts[0], Action::Started { .. }), "{acts:?}");
    assert!(s.pending().is_empty() && s.is_running(seq(1)));

    s.start(one_clip(2, 501, 20), Origin::Ui, Start::Beat)
        .unwrap();
    assert_eq!(
        s.stop(seq(2), StopMode::AfterLastPass),
        [Action::PendingCancelled {
            seq: seq(2),
            origin: Origin::Ui
        }]
    );
    let log = ticks(&mut s, BAR);
    assert!(started(&log).is_empty());
    assert!(!triggers(&log).iter().any(|&(_, c)| c == 20));

    s.start(one_clip(3, 502, 30), Origin::Ui, Start::Beat)
        .unwrap();
    let acts = s.stop_all(StopMode::Clear);
    assert!(acts.contains(&Action::PendingCancelled {
        seq: seq(3),
        origin: Origin::Ui
    }));
    assert!(s.pending().is_empty() && s.running().next().is_none());
}

#[test]
fn a_waiting_start_freezes_without_ticks() {
    let mut s = Scheduler::new();
    ticks(&mut s, 5);
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Beat)
        .unwrap();
    // No ticks for a while (clock lost, Resolume paused): still waiting, same distance.
    assert_eq!(s.pending()[0].wait_ticks, BEAT - 4);
    let log = ticks(&mut s, BEAT - 5);
    assert!(started(&log).is_empty());
    assert_eq!(started(&ticks(&mut s, 1)), [(1, seq(1))]);
}

#[test]
fn phase_resets_move_the_wait() {
    // MIDI Start: the next tick is bar 0, so a waiting start begins on it.
    let mut s = Scheduler::new();
    ticks(&mut s, 50);
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Bar { bars: 4 })
        .unwrap();
    assert_eq!(s.pending()[0].wait_ticks, 4 * BAR - 49);
    s.phase_reset_next_tick();
    assert_eq!(s.pending()[0].wait_ticks, 1);
    assert_eq!(started(&ticks(&mut s, 1)), [(1, seq(1))]);

    // Resync: this moment is bar 0; waiting starts begin now, together with the rewind.
    let mut s = Scheduler::new();
    ticks(&mut s, 50);
    s.start(one_clip(2, 501, 20), Origin::Ui, Start::Bar { bars: 2 })
        .unwrap();
    let acts = s.phase_reset_now();
    assert!(acts.iter().any(|a| matches!(a, Action::Started { .. })));
    assert!(acts
        .iter()
        .any(|a| matches!(a, Action::Trigger { clip_id: 20, .. })));
    // And later starts count from there: one tick after the resync is phase 1.
    ticks(&mut s, 1);
    s.start(one_clip(3, 502, 30), Origin::Ui, Start::Beat)
        .unwrap();
    assert_eq!(s.pending()[0].wait_ticks, BEAT - 1);
}

#[test]
fn a_restart_at_the_boundary_replaces_the_run_without_a_double_trigger() {
    let mut s = Scheduler::new();
    ticks(&mut s, 1); // phase 0
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 40);
    // ▶ on the running sequence with mode bar: it keeps playing until bar 1, then restarts.
    s.start(one_clip(1, 500, 11), Origin::Ui, Start::Bar { bars: 1 })
        .unwrap();
    assert!(s.is_running(seq(1)) && s.is_pending(seq(1)));
    let log = ticks(&mut s, BAR - 40);
    let at = BAR - 40;
    assert_eq!(triggers(&log), [(at, 11)], "only the new run triggers");
    assert!(log.iter().any(|(t, a)| *t == at
        && matches!(
            a,
            Action::Ended {
                reason: EndReason::Restarted,
                ..
            }
        )));
    assert_eq!(s.status()[0].pos, 0);
}

#[test]
fn a_waiting_start_takes_over_before_the_other_sequence_acts() {
    let mut s = Scheduler::new();
    ticks(&mut s, 1);
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 10);
    s.start(one_clip(2, 500, 20), Origin::Ui, Start::Bar { bars: 1 })
        .unwrap();
    let log = ticks(&mut s, BAR - 10);
    // Sequence 1 would trigger clip 10 on this tick (its bar 2); its lane is taken first.
    assert_eq!(triggers(&log), [(BAR - 10, 20)]);
}

#[test]
fn a_seq_clip_links_a_waiting_start() {
    let mut s = Scheduler::new();
    ticks(&mut s, 3);
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Beat)
        .unwrap();
    s.link_pending(seq(1), Origin::SeqClip { clip_id: 77 });
    assert_eq!(s.pending()[0].origin, Origin::SeqClip { clip_id: 77 });
    let log = ticks(&mut s, BEAT);
    assert!(log.iter().any(|(_, a)| matches!(
        a,
        Action::Started {
            origin: Origin::SeqClip { clip_id: 77 },
            ..
        }
    )));
}

// ---- cue (SPEC §3.3) ----

fn ended_seqs(log: &[(u64, Action)]) -> Vec<(u64, Uuid)> {
    ended(log).into_iter().map(|(t, s, _)| (t, s)).collect()
}

#[test]
fn a_cue_replaces_what_plays_at_its_point() {
    let mut s = Scheduler::new();
    ticks(&mut s, 1); // phase 0
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Immediate)
        .unwrap();
    s.start(one_clip(2, 501, 20), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 30);
    // Cue 3 at the next bar; it shares layer 500 with sequence 1.
    let acts = s
        .cue(
            one_clip(3, 500, 30),
            Origin::Ui,
            Wait::Mode(Start::Bar { bars: 1 }),
        )
        .unwrap();
    assert!(matches!(acts[..], [Action::Pending { .. }]));
    assert_eq!(s.cued(), Some((seq(3), Wait::Mode(Start::Bar { bars: 1 }))));
    assert!(s.pending()[0].cue);
    let log = ticks(&mut s, BAR - 30);
    let at = BAR - 30;
    assert_eq!(started(&log), [(at, seq(3))]);
    assert_eq!(triggers(&log), [(at, 30)], "the others act no more");
    // 1 lost its only lane to the cue; 2 is stopped with its after_last_pass (clear on 501).
    let mut e = ended_seqs(&log);
    e.sort();
    assert_eq!(e, [(at, seq(1)), (at, seq(2))]);
    assert_eq!(clears(&log), [(at, 501)]);
    assert_eq!(s.running().collect::<Vec<_>>(), [seq(3)]);
}

#[test]
fn a_cue_at_the_end_of_a_pass() {
    let mut s = Scheduler::new();
    ticks(&mut s, 1);
    // Two-bar pass.
    let two_bars = spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Hold,
        vec![lane(500, LaneEnd::LoopToEnd, &[(16, Clip(10))])],
    );
    s.start(two_bars, Origin::Ui, Start::Immediate).unwrap();
    ticks(&mut s, 50);
    s.cue(one_clip(2, 501, 20), Origin::Ui, Wait::EndOfPass(seq(1)))
        .unwrap();
    assert_eq!(s.pending()[0].wait_ticks, 2 * BAR - 50);
    let log = ticks(&mut s, 2 * BAR);
    let at = 2 * BAR - 50;
    assert_eq!(started(&log), [(at, seq(2))]);
    assert_eq!(ended_seqs(&log), [(at, seq(1))]);
    assert!(
        !triggers(&log).contains(&(at, 10)),
        "1 does not start its next pass"
    );
}

#[test]
fn a_cue_at_the_end_of_a_pass_starts_when_that_sequence_stops() {
    let mut s = Scheduler::new();
    ticks(&mut s, 1);
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 10);
    s.cue(one_clip(2, 501, 20), Origin::Ui, Wait::EndOfPass(seq(1)))
        .unwrap();
    let acts = s.stop(seq(1), StopMode::AfterLastPass);
    assert!(acts.iter().any(|a| matches!(
        a,
        Action::Started { seq: x, .. } if *x == seq(2)
    )));
    // Cueing at the end of the pass of a sequence that is not running starts at once.
    let acts = s
        .cue(one_clip(3, 502, 30), Origin::Ui, Wait::EndOfPass(seq(9)))
        .unwrap();
    assert!(acts.iter().any(|a| matches!(a, Action::Started { .. })));
    assert_eq!(s.running().collect::<Vec<_>>(), [seq(3)]);
}

#[test]
fn one_cue_at_a_time_and_how_it_is_cancelled() {
    let mut s = Scheduler::new();
    ticks(&mut s, 5);
    s.cue(
        one_clip(1, 500, 10),
        Origin::Ui,
        Wait::Mode(Start::Bar { bars: 8 }),
    )
    .unwrap();
    let acts = s
        .cue(
            one_clip(2, 501, 20),
            Origin::Ui,
            Wait::Mode(Start::Bar { bars: 16 }),
        )
        .unwrap();
    assert_eq!(
        acts[0],
        Action::PendingCancelled {
            seq: seq(1),
            origin: Origin::Ui
        }
    );
    assert_eq!(s.pending().len(), 1);
    assert_eq!(s.pending()[0].wait_ticks, 16 * BAR - 4);
    // A plain waiting start is not a cue and stays.
    s.start(one_clip(3, 502, 30), Origin::Ui, Start::Beat)
        .unwrap();
    assert_eq!(s.cancel_cue().len(), 1);
    assert_eq!(s.cued(), None);
    assert_eq!(s.pending().len(), 1);
    // Stop on the cued sequence cancels it.
    s.cue(one_clip(1, 500, 10), Origin::Ui, Wait::Mode(Start::Beat))
        .unwrap();
    s.stop(seq(1), StopMode::AfterLastPass);
    assert_eq!(s.cued(), None);
}

#[test]
fn starting_the_cued_sequence_starts_it_now_and_still_replaces() {
    let mut s = Scheduler::new();
    ticks(&mut s, 1);
    s.start(one_clip(1, 500, 10), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 7);
    s.cue(
        one_clip(2, 501, 20),
        Origin::Ui,
        Wait::Mode(Start::Bar { bars: 4 }),
    )
    .unwrap();
    let acts = s
        .start(one_clip(2, 501, 20), Origin::Ui, Start::Immediate)
        .unwrap();
    assert!(acts.iter().any(|a| matches!(a, Action::Started { .. })));
    assert!(acts
        .iter()
        .any(|a| matches!(a, Action::Ended { seq: x, .. } if *x == seq(1))));
    assert_eq!(s.running().collect::<Vec<_>>(), [seq(2)]);
}

// ---- mute and solo (SPEC §7.8) ----

fn three_lanes() -> RunSpec {
    spec(
        1,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![
            lane(500, LaneEnd::LoopToEnd, &[(4, Clip(10)), (4, Clip(11))]),
            lane(501, LaneEnd::LoopToEnd, &[(4, Clip(20)), (4, Clip(21))]),
            lane(502, LaneEnd::LoopToEnd, &[(8, Clip(30))]),
        ],
    )
}

#[test]
fn a_muted_lane_clears_counts_silently_and_comes_back_in_time() {
    let mut s = Scheduler::new();
    s.start(three_lanes(), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 10);
    let acts = s.set_mute(seq(1), 0, true);
    assert!(acts.contains(&Action::Clear {
        seq: seq(1),
        lane: 0,
        layer_id: 500
    }));
    assert!(acts.contains(&Action::LaneMuteChanged {
        seq: seq(1),
        lane: 0,
        silent: true
    }));
    let st = &s.status()[0].lanes[0];
    assert!(st.mute && st.silent && st.state == LaneState::Playing);
    // Half a bar later lane 0 would trigger clip 11: silent, the others play.
    let log = ticks(&mut s, 4 * E - 10);
    assert_eq!(triggers(&log), [(4 * E - 10, 21)]);
    assert!(clears(&log).is_empty());
    // Unmute mid-step: nothing now, the next step (bar 2) plays.
    let acts = s.set_mute(seq(1), 0, false);
    assert!(!acts
        .iter()
        .any(|a| matches!(a, Action::Trigger { .. } | Action::Clear { .. })));
    let log = ticks(&mut s, 4 * E);
    assert_eq!(triggers(&log), [(4 * E, 10), (4 * E, 20), (4 * E, 30)]);
}

#[test]
fn solo_mutes_the_other_lanes_until_the_last_solo_goes() {
    let mut s = Scheduler::new();
    s.start(three_lanes(), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 5);
    s.set_mute(seq(1), 2, true);
    let acts = s.set_solo(seq(1), 0, true);
    // Lane 1 falls silent (cleared); lane 2 was already muted.
    assert_eq!(
        acts.iter()
            .filter_map(|a| match a {
                Action::Clear { layer_id, .. } => Some(*layer_id),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [501]
    );
    s.set_solo(seq(1), 1, true);
    let log = ticks(&mut s, 8 * E - 5);
    let at = 8 * E - 5;
    assert_eq!(
        triggers(&log),
        [(4 * E - 5, 11), (4 * E - 5, 21), (at, 10), (at, 20)]
    );
    // Unsolo both: lane 2 stays muted by hand.
    s.set_solo(seq(1), 0, false);
    s.set_solo(seq(1), 1, false);
    let st = s.status();
    let silent: Vec<_> = st[0].lanes.iter().map(|l| l.silent).collect();
    assert_eq!(silent, [false, false, true]);
}

#[test]
fn unmuting_a_yielded_lane_resumes_it_at_the_next_step() {
    let mut s = Scheduler::new();
    s.start(three_lanes(), Origin::Ui, Start::Immediate)
        .unwrap();
    ticks(&mut s, 3);
    s.yield_layer(500);
    s.set_mute(seq(1), 0, true);
    let acts = s.set_mute(seq(1), 0, false);
    assert!(acts.contains(&Action::LaneStateChanged {
        seq: seq(1),
        lane: 0,
        state: LaneState::Yielded {
            resume_pending: true
        }
    }));
    let log = ticks(&mut s, 4 * E - 3);
    assert!(triggers(&log).contains(&(4 * E - 3, 11)));
    assert_eq!(s.status()[0].lanes[0].state, LaneState::Playing);
}

#[test]
fn a_restart_starts_unmuted_and_unknown_lanes_are_ignored() {
    let mut s = Scheduler::new();
    s.start(three_lanes(), Origin::Ui, Start::Immediate)
        .unwrap();
    s.set_mute(seq(1), 1, true);
    assert!(s.set_mute(seq(1), 9, true).is_empty());
    assert!(s.set_solo(seq(2), 0, true).is_empty());
    s.start(three_lanes(), Origin::Ui, Start::Immediate)
        .unwrap();
    assert!(s.status()[0].lanes.iter().all(|l| !l.mute && !l.silent));
}
