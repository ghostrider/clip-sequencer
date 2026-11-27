//! Recording from Resolume (SPEC §7.9): a **take** is the clicks of one pass, each snapped to a
//! sixteenth; [`apply_take`] writes it into a sequence. Pure: the engine turns WebSocket events
//! into [`Click`]s and saves the result as one edit.
//!
//! In a take, a lane clicked on is overwritten from its first click to the end of the take; each
//! recorded step lasts until the next click on that layer or the end. A lane shorter than the end
//! is first written out as it plays (loop, hold or clear), so its steps before the first click stay
//! where they were heard.

use crate::model::{
    ClipRef, Lane, LaneEnd, LaneMode, Sequence, Step, Transition, MAX_TRANSITION_LENGTH,
};
use crate::TICKS_PER_SIXTEENTH;

/// What a click put on a layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A clip, with its 1-based column (a slot lane records the column).
    Clip { clip: ClipRef, column: u32 },
    /// The layer was cleared.
    Rest,
}

/// One click, already snapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    /// Resolume layer name.
    pub layer: String,
    /// Sixteenth within the pass.
    pub at: u32,
    pub hit: Hit,
    /// The transition Resolume used, in sixteenths (`length: 0` = a cut); `None` when unknown or
    /// for a rest.
    pub transition: Option<Transition>,
}

/// The clicks of one pass, in the order they arrived, up to `end` (the pass length, or where Stop was).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Take {
    pub clicks: Vec<Click>,
    pub end: u32,
}

/// The sixteenth nearest to `ticks` into a pass of `pass_ticks`; `None` when it rounds to the end
/// of the pass (it belongs to the start of the next one).
pub fn snap(ticks: f64, pass_ticks: u64) -> Option<u32> {
    let s = (ticks.max(0.0) / f64::from(TICKS_PER_SIXTEENTH)).round() as u64;
    (s * u64::from(TICKS_PER_SIXTEENTH) < pass_ticks).then_some(s as u32)
}

/// A fade of `seconds` at `bpm` in sixteenths: the nearest, at most [`MAX_TRANSITION_LENGTH`].
pub fn sixteenths(seconds: f64, bpm: f64) -> u32 {
    if bpm <= 0.0 || seconds <= 0.0 {
        return 0;
    }
    ((seconds * bpm / 15.0).round() as u32).min(MAX_TRANSITION_LENGTH)
}

/// What a take changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeResult {
    pub sequence: Sequence,
    /// Layers recorded, in the order of their first click.
    pub layers: Vec<String>,
    /// Steps recorded.
    pub steps: usize,
    /// Layers that got a new lane.
    pub new_lanes: Vec<String>,
}

/// Writes `take` into `seq` (SPEC §7.9). `can_add(layer)` says whether a layer without a lane may
/// get one (its name is unique and not the SEQ layer); clicks on other layers are dropped.
pub fn apply_take(seq: &Sequence, take: &Take, can_add: impl Fn(&str) -> bool) -> TakeResult {
    let mut out = TakeResult {
        sequence: seq.clone(),
        layers: Vec::new(),
        steps: 0,
        new_lanes: Vec::new(),
    };
    // Layers in the order of their first click; per layer the clicks by position, the later of
    // two on one sixteenth winning. A rest on the sixteenth of a clip (a tap shorter than a
    // sixteenth: Piano trigger style, notes §20) moves one sixteenth on, so the tap stays.
    let mut layers: Vec<(String, Vec<Click>)> = Vec::new();
    for c in take.clicks.iter().filter(|c| c.at < take.end) {
        let mut c = c.clone();
        match layers.iter_mut().find(|(l, _)| *l == c.layer) {
            Some((_, v)) => {
                let tapped = v
                    .iter()
                    .any(|o| o.at == c.at && matches!(o.hit, Hit::Clip { .. }));
                if c.hit == Hit::Rest && tapped && c.at + 1 < take.end {
                    c.at += 1;
                }
                v.retain(|o| o.at != c.at);
                v.push(c);
            }
            None => layers.push((c.layer.clone(), vec![c])),
        }
    }
    for (layer, mut clicks) in layers {
        clicks.sort_by_key(|c| c.at);
        let lane_index = match out.sequence.lanes.iter().position(|l| l.layer == layer) {
            Some(i) => i,
            None if can_add(&layer) => {
                out.sequence.lanes.push(Lane {
                    layer: layer.clone(),
                    mode: LaneMode::Clip,
                    end: LaneEnd::LoopToEnd,
                    transition: None,
                    steps: Vec::new(),
                });
                out.new_lanes.push(layer.clone());
                out.sequence.lanes.len() - 1
            }
            None => continue,
        };
        let lane = &mut out.sequence.lanes[lane_index];
        out.steps += clicks.len();
        out.layers.push(layer);
        write_out(lane, take.end);
        let clicks: Vec<&Click> = clicks.iter().collect();
        overwrite(lane, &clicks, take.end);
    }
    out
}

/// Makes `lane` cover at least `end` sixteenths, as it plays in a pass.
fn write_out(lane: &mut Lane, end: u32) {
    let len: u32 = lane.steps.iter().map(|s| s.length).sum();
    if len >= end {
        return;
    }
    if len == 0 {
        lane.steps = vec![rest(end)];
        return;
    }
    match lane.end {
        LaneEnd::LoopToEnd => {
            let one = lane.steps.clone();
            let mut at = len;
            'fill: loop {
                for s in &one {
                    if at >= end {
                        break 'fill;
                    }
                    let mut s = s.clone();
                    s.length = s.length.min(end - at);
                    at += s.length;
                    lane.steps.push(s);
                }
            }
        }
        LaneEnd::Hold => lane.steps.last_mut().expect("len > 0").length += end - len,
        LaneEnd::Clear => lane.steps.push(rest(end - len)),
    }
}

/// Replaces `[first click, end)` of a lane that covers `end` with the clicks.
fn overwrite(lane: &mut Lane, clicks: &[&Click], end: u32) {
    let first = clicks[0].at;
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut at = 0;
    for s in std::mem::take(&mut lane.steps) {
        let (start, stop) = (at, at + s.length);
        at = stop;
        if start < first {
            let mut cut = s.clone();
            cut.length = stop.min(first) - start;
            before.push(cut);
        }
        if stop > end {
            let mut cut = s;
            cut.length = stop - start.max(end);
            after.push(cut);
        }
    }
    let slot = lane.mode == LaneMode::Slot;
    let mut recorded: Vec<(Step, bool)> = clicks
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let stop = clicks.get(i + 1).map_or(end, |n| n.at);
            let mut step = rest(stop - c.at);
            if let Hit::Clip { clip, column } = &c.hit {
                if slot {
                    step.column = Some(*column);
                } else {
                    step.clip = Some(clip.clone());
                }
            }
            let cut = matches!(&c.transition, Some(t) if t.length == 0);
            if matches!(c.hit, Hit::Clip { .. }) && !cut {
                step.transition = c.transition.clone();
            }
            (step, cut && matches!(c.hit, Hit::Clip { .. }))
        })
        .collect();
    // A cut is written only where it differs from "nothing": when the lane has fades (§7.7 rule 4).
    let fades = lane.transition.is_some()
        || before
            .iter()
            .chain(&after)
            .chain(recorded.iter().map(|(s, _)| s))
            .any(|s| s.transition.is_some());
    if fades {
        for (s, cut) in &mut recorded {
            if *cut {
                s.transition = Some(Transition {
                    length: 0,
                    blend: None,
                });
            }
        }
    }
    lane.steps = before;
    lane.steps.extend(recorded.into_iter().map(|(s, _)| s));
    lane.steps.extend(after);
}

fn rest(length: u32) -> Step {
    Step {
        length,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AfterLastPass, Repeat, Start};
    use uuid::Uuid;

    fn clip(name: &str) -> ClipRef {
        ClipRef {
            id: None,
            name: Some(name.into()),
            file: None,
        }
    }

    fn step(name: Option<&str>, length: u32) -> Step {
        Step {
            clip: name.map(clip),
            length,
            ..Default::default()
        }
    }

    fn lane(layer: &str, end: LaneEnd, steps: Vec<Step>) -> Lane {
        Lane {
            layer: layer.into(),
            mode: LaneMode::Clip,
            end,
            transition: None,
            steps,
        }
    }

    fn seq(lanes: Vec<Lane>) -> Sequence {
        Sequence {
            id: Uuid::nil(),
            name: "S".into(),
            start: Start::Immediate,
            repeat: Repeat::Infinite,
            after_last_pass: AfterLastPass::Clear,
            sections: Vec::new(),
            lanes,
        }
    }

    fn click(layer: &str, at: u32, name: Option<&str>, column: u32, fade: Option<u32>) -> Click {
        Click {
            layer: layer.into(),
            at,
            hit: match name {
                Some(n) => Hit::Clip {
                    clip: clip(n),
                    column,
                },
                None => Hit::Rest,
            },
            transition: fade.map(|length| Transition {
                length,
                blend: None,
            }),
        }
    }

    /// (name or "-", length) per step.
    fn shape(l: &Lane) -> Vec<(String, u32)> {
        l.steps
            .iter()
            .map(|s| {
                let n = match (&s.clip, s.column) {
                    (Some(c), _) => c.name.clone().unwrap_or_default(),
                    (None, Some(col)) => format!("col {col}"),
                    _ => "-".into(),
                };
                (n, s.length)
            })
            .collect()
    }

    fn sh(v: &[(&str, u32)]) -> Vec<(String, u32)> {
        v.iter().map(|(n, l)| ((*n).to_owned(), *l)).collect()
    }

    #[test]
    fn snapping_to_the_nearest_sixteenth() {
        assert_eq!(snap(0.0, 96), Some(0));
        assert_eq!(snap(2.9, 96), Some(0));
        assert_eq!(snap(3.0, 96), Some(1));
        assert_eq!(snap(50.0, 96), Some(8));
        assert_eq!(snap(92.9, 96), Some(15));
        assert_eq!(snap(93.0, 96), None, "the next pass's start");
        assert_eq!(snap(-2.0, 96), Some(0));
    }

    #[test]
    fn seconds_to_sixteenths() {
        assert_eq!(sixteenths(0.5, 120.0), 4);
        assert_eq!(sixteenths(2.372, 128.0), 20);
        assert_eq!(sixteenths(0.0, 120.0), 0);
        assert_eq!(sixteenths(100.0, 120.0), MAX_TRANSITION_LENGTH);
    }

    #[test]
    fn overwrite_from_the_first_click_to_the_end() {
        let s = seq(vec![
            lane(
                "BG",
                LaneEnd::LoopToEnd,
                vec![step(Some("a"), 16), step(Some("b"), 16)],
            ),
            lane("FG", LaneEnd::LoopToEnd, vec![step(Some("x"), 32)]),
        ]);
        let take = Take {
            clicks: vec![
                click("BG", 8, Some("c"), 1, None),
                click("BG", 20, None, 0, None),
                click("BG", 24, Some("d"), 2, None),
            ],
            end: 32,
        };
        let r = apply_take(&s, &take, |_| true);
        assert_eq!(
            shape(&r.sequence.lanes[0]),
            sh(&[("a", 8), ("c", 12), ("-", 4), ("d", 8)])
        );
        assert_eq!(r.sequence.lanes[1], s.lanes[1], "untouched");
        assert_eq!((r.layers, r.steps), (vec!["BG".to_owned()], 3));
    }

    #[test]
    fn a_stop_keeps_what_comes_after_it_and_same_sixteenth_keeps_the_later() {
        let s = seq(vec![lane(
            "BG",
            LaneEnd::LoopToEnd,
            vec![step(Some("a"), 16), step(Some("b"), 16)],
        )]);
        let take = Take {
            clicks: vec![
                click("BG", 4, Some("c"), 1, None),
                click("BG", 4, Some("d"), 1, None),
                click("BG", 30, Some("late"), 1, None), // after the stop: dropped
            ],
            end: 20,
        };
        let r = apply_take(&s, &take, |_| true);
        assert_eq!(
            shape(&r.sequence.lanes[0]),
            sh(&[("a", 4), ("d", 16), ("b", 12)])
        );
    }

    #[test]
    fn short_lanes_are_written_out_as_they_play() {
        let looped = lane(
            "A",
            LaneEnd::LoopToEnd,
            vec![step(Some("a"), 6), step(Some("b"), 4)],
        );
        let held = lane("B", LaneEnd::Hold, vec![step(Some("h"), 8)]);
        let cleared = lane("C", LaneEnd::Clear, vec![step(Some("c"), 8)]);
        let s = seq(vec![
            looped,
            held,
            cleared,
            lane("D", LaneEnd::Hold, vec![step(None, 32)]),
        ]);
        let take = Take {
            clicks: ["A", "B", "C"]
                .iter()
                .map(|l| click(l, 28, Some("n"), 1, None))
                .collect(),
            end: 32,
        };
        let r = apply_take(&s, &take, |_| true);
        let l = &r.sequence.lanes;
        assert_eq!(
            shape(&l[0]),
            sh(&[
                ("a", 6),
                ("b", 4),
                ("a", 6),
                ("b", 4),
                ("a", 6),
                ("b", 2),
                ("n", 4)
            ])
        );
        assert_eq!(shape(&l[1]), sh(&[("h", 28), ("n", 4)]));
        assert_eq!(shape(&l[2]), sh(&[("c", 8), ("-", 20), ("n", 4)]));
        assert_eq!(l[0].end, LaneEnd::LoopToEnd, "the end mode stays");
    }

    #[test]
    fn a_tap_shorter_than_a_sixteenth_stays_a_sixteenth() {
        let s = seq(vec![lane("BG", LaneEnd::Hold, vec![step(None, 32)])]);
        let take = Take {
            clicks: vec![
                click("BG", 8, Some("tap"), 1, None),
                click("BG", 8, None, 0, None), // released in the same sixteenth
                click("BG", 16, Some("held"), 1, None),
                click("BG", 20, None, 0, None),
            ],
            end: 32,
        };
        let r = apply_take(&s, &take, |_| true);
        assert_eq!(
            shape(&r.sequence.lanes[0]),
            sh(&[("-", 8), ("tap", 1), ("-", 7), ("held", 4), ("-", 12)])
        );
    }

    #[test]
    fn new_layers_get_a_lane_unless_refused() {
        let s = seq(vec![lane("BG", LaneEnd::Hold, vec![step(Some("a"), 32)])]);
        let take = Take {
            clicks: vec![
                click("New", 8, Some("n"), 3, None),
                click("Layer #", 8, Some("x"), 1, None),
            ],
            end: 32,
        };
        let r = apply_take(&s, &take, |l| l != "Layer #");
        assert_eq!(r.sequence.lanes.len(), 2);
        assert_eq!(shape(&r.sequence.lanes[1]), sh(&[("-", 8), ("n", 24)]));
        assert_eq!(r.new_lanes, vec!["New".to_owned()]);
        assert_eq!(r.layers, vec!["New".to_owned()]);
    }

    #[test]
    fn slot_lanes_record_columns_and_cut_steps_keep_their_extras() {
        use crate::model::When;
        let mut l = lane("BG", LaneEnd::Hold, vec![]);
        l.mode = LaneMode::Slot;
        l.steps = vec![Step {
            column: Some(2),
            length: 32,
            when: Some(When::First),
            ..Default::default()
        }];
        let s = seq(vec![l]);
        let take = Take {
            clicks: vec![click("BG", 16, Some("c"), 5, None)],
            end: 32,
        };
        let r = apply_take(&s, &take, |_| true);
        let steps = &r.sequence.lanes[0].steps;
        assert_eq!(
            shape(&r.sequence.lanes[0]),
            sh(&[("col 2", 16), ("col 5", 16)])
        );
        assert_eq!(steps[0].when, Some(When::First));
        assert_eq!(steps[1].clip, None);
    }

    #[test]
    fn transitions_are_recorded_and_cuts_only_in_lanes_with_fades() {
        let s = seq(vec![
            lane("BG", LaneEnd::Hold, vec![step(Some("a"), 32)]),
            lane("FG", LaneEnd::Hold, vec![step(Some("a"), 32)]),
        ]);
        let take = Take {
            clicks: vec![
                click("BG", 0, Some("b"), 1, Some(8)),
                click("BG", 16, Some("c"), 1, Some(0)),
                click("BG", 24, None, 0, Some(8)), // a rest gets none
                click("FG", 8, Some("d"), 1, Some(0)),
            ],
            end: 32,
        };
        let r = apply_take(&s, &take, |_| true);
        let t = |l: usize| -> Vec<Option<u32>> {
            r.sequence.lanes[l]
                .steps
                .iter()
                .map(|s| s.transition.as_ref().map(|t| t.length))
                .collect()
        };
        assert_eq!(t(0), vec![Some(8), Some(0), None]);
        assert_eq!(
            t(1),
            vec![None, None],
            "a lane played without fades stays without"
        );
    }
}
