//! Scheduler rules from SPEC §3, §5 and §7, driven by simulated tick streams.

use seq_core::scheduler::{
    bar_beat, Action, EndReason, LaneState, Origin, RunLane, RunSpec, RunStep, Scheduler,
    StartError, StepTarget, StopMode,
};
use seq_core::{AfterLastPass, LaneEnd, Repeat};
use uuid::Uuid;

const E: u64 = 12; // ticks per eighth
const BAR: u64 = 96;

use StepTarget::{Clip, Rest, Slot, Unresolved};

fn lane(layer: u64, end: LaneEnd, steps: &[(u64, StepTarget)]) -> RunLane {
    RunLane {
        layer_id: Some(layer),
        end,
        steps: steps
            .iter()
            .map(|&(eighths, target)| RunStep {
                ticks: eighths * E,
                target,
            })
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    let start = s.start(sp, Origin::SeqClip { clip_id: 77 }).unwrap();
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    s.start(sp, Origin::Ui).unwrap();
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
    s.start(v1, Origin::Ui).unwrap();
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
    let acts = s.start(v2, Origin::Ui).unwrap();
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
    s.start(a, Origin::Ui).unwrap();
    s.on_tick();
    let b = spec(
        2,
        Repeat::Infinite,
        AfterLastPass::Clear,
        vec![lane(200, LaneEnd::LoopToEnd, &[(1, Clip(9))])],
    );
    let acts = s.start(b, Origin::Ui).unwrap();
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
    s.start(sp, Origin::Ui).unwrap();
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
    s.start(sp, Origin::Ui).unwrap();
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
    s.start(two_lanes(), Origin::Ui).unwrap();
    s.yield_layer(200);
    assert_eq!(
        clear_layers(&s.stop(seq(1), StopMode::AfterLastPass)),
        vec![100]
    );

    s.start(two_lanes(), Origin::Ui).unwrap();
    s.yield_layer(200);
    assert_eq!(clear_layers(&s.stop_all(StopMode::Clear)), vec![100, 200]);

    s.start(two_lanes(), Origin::Ui).unwrap();
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    assert_eq!(s.start(empty, Origin::Ui), Err(StartError::Empty));
    let mut l = lane(100, LaneEnd::Hold, &[(1, Clip(1))]);
    l.layer_id = None;
    let no_layer = spec(1, Repeat::Once, AfterLastPass::Clear, vec![l]);
    assert_eq!(
        s.start(no_layer, Origin::Ui),
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
    s.start(sp, Origin::Ui).unwrap();
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
        lanes: vec![
            Lane {
                layer: "BG".into(),
                mode: seq_core::model::LaneMode::Clip,
                end: LaneEnd::Clear,
                steps: vec![
                    Step {
                        column: None,
                        clip: named("A"),
                        length: 16,
                    },
                    Step {
                        column: None,
                        clip: None,
                        length: 4,
                    },
                    Step {
                        column: None,
                        clip: named("Missing"),
                        length: 2,
                    },
                ],
            },
            Lane {
                layer: "Nope".into(),
                mode: seq_core::model::LaneMode::Clip,
                end: LaneEnd::Hold,
                steps: vec![Step {
                    column: None,
                    clip: named("A"),
                    length: 8,
                }],
            },
        ],
    };
    let rs = RunSpec::new(&sequence, &resolve_sequence(&sequence, &idx));
    assert_eq!(rs.lanes[0].layer_id, Some(500));
    assert_eq!(
        rs.lanes[0].steps,
        vec![
            RunStep {
                ticks: 96,
                target: Clip(10)
            },
            RunStep {
                ticks: 24,
                target: Rest
            },
            RunStep {
                ticks: 12,
                target: Unresolved
            },
        ]
    );
    assert_eq!(rs.lanes[1].layer_id, None);
    assert_eq!(rs.pass_ticks(), 132);
    assert_eq!(rs.passes(), Some(2));
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
    let start = s.start(sp, Origin::Ui).unwrap();
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
    s.start(sp, Origin::Ui).unwrap();
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
    s.start(sp, Origin::Ui).unwrap();
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

#[test]
fn a_sixteenth_is_six_ticks() {
    use seq_core::index::CompositionIndex;
    use seq_core::resolve::resolve_sequence;
    use seq_core::{Lane, Sequence, Step};
    let idx = CompositionIndex::new("S".into(), vec![], None, vec![], vec![], vec![]);
    let sequence = Sequence {
        id: seq(4),
        name: "S".into(),
        start: seq_core::Start::Immediate,
        repeat: Repeat::Once,
        after_last_pass: AfterLastPass::Clear,
        lanes: vec![Lane {
            layer: "BG".into(),
            mode: seq_core::LaneMode::Slot,
            end: LaneEnd::Hold,
            steps: vec![Step::slot(Some(1), 1), Step::slot(Some(1), 3)],
        }],
    };
    let rs = RunSpec::new(&sequence, &resolve_sequence(&sequence, &idx));
    let ticks: Vec<_> = rs.lanes[0].steps.iter().map(|s| s.ticks).collect();
    assert_eq!(ticks, [6, 18]);
    assert_eq!(rs.pass_ticks(), 24);
}
