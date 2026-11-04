//! What the "1000A Sequence" FFGL source plugin polls (SPEC §12): `GET /plugin/v1/view.txt`.
//!
//! A line-based text format, so the C++ plugin needs no JSON library. UTF-8, `\n`-terminated
//! lines, fields separated by single spaces; a free-text field (names) is always the last one
//! and may contain spaces.
//!
//! ```text
//! SEQVIEW 2                                  (version 1 counted lengths in eighths)
//! status ok                                  | status not_found (then only `end` follows)
//! rev <hex>                                  layout revision
//! layout                                     only when the request's `rev` differs
//! name <sequence name>
//! deck <deck name>
//! pass <pass length in sixteenths>
//! lane <ok|missing|ambiguous|seq_layer> <loop_to_end|hold|clear> <layer name>
//! step <clip|rest|unresolved> <length in sixteenths> <clip id or 0> <thumbnail version or -> <label>
//! endlayout
//! run 0                                      | run 1 <pos ticks> <pass ticks> <pass> <passes or 0> <bar.beat>
//! lanestate <lane> <idle|playing|yielded|resume_pending|taken_over> <step or -1>   (one per lane, when running)
//! clock <0|1> <bpm, 0 if unknown>
//! end
//! ```
//!
//! Thumbnails: `GET /thumbs/clip/<clip id>.png?v=<thumbnail version>` (320×240 PNG).

use std::collections::hash_map::DefaultHasher;
use std::fmt::Write;
use std::hash::{Hash, Hasher};

use crate::engine::view::{DeckView, RuntimeState, SequenceView, StructureState};

/// Builds the response for sequence `name`; `rev` is the layout revision the plugin already has.
pub fn view_text(s: &StructureState, r: &RuntimeState, name: &str, rev: Option<&str>) -> String {
    let mut out = String::from("SEQVIEW 2\n");
    // The live deck's sequence wins; otherwise the first deck that has one of that name.
    let found = s
        .decks
        .iter()
        .filter(|d| d.live)
        .chain(s.decks.iter().filter(|d| !d.live))
        .find_map(|d| {
            d.sequences
                .iter()
                .find(|v| v.sequence.name == name)
                .map(|v| (d, v))
        });
    let Some((deck, view)) = found else {
        out.push_str("status not_found\nend\n");
        return out;
    };
    out.push_str("status ok\n");

    let layout = layout_text(deck, view);
    let mut h = DefaultHasher::new();
    layout.hash(&mut h);
    let current = format!("{:016x}", h.finish());
    let _ = writeln!(out, "rev {current}");
    if rev != Some(current.as_str()) {
        out.push_str("layout\n");
        out.push_str(&layout);
        out.push_str("endlayout\n");
    }

    match r.running.iter().find(|run| run.seq == view.sequence.id) {
        Some(run) => {
            let _ = writeln!(
                out,
                "run 1 {} {} {} {} {}",
                run.pos,
                run.pass_ticks,
                run.pass,
                run.passes.unwrap_or(0),
                run.bar_beat
            );
            for (i, l) in run.lanes.iter().enumerate() {
                let step = l.step.map_or(-1, |s| s as i64);
                let _ = writeln!(out, "lanestate {i} {} {step}", l.state);
            }
        }
        None => out.push_str("run 0\n"),
    }
    let _ = writeln!(
        out,
        "clock {} {:.2}",
        u8::from(r.clock.running),
        r.clock.bpm.unwrap_or(0.0)
    );
    out.push_str("end\n");
    out
}

fn layout_text(deck: &DeckView, view: &SequenceView) -> String {
    let mut out = String::new();
    let s = &view.sequence;
    let _ = writeln!(out, "name {}", one_line(&s.name));
    let _ = writeln!(out, "deck {}", one_line(&deck.name));
    let _ = writeln!(out, "pass {}", view.pass_sixteenths);
    for (li, lane) in s.lanes.iter().enumerate() {
        let res = view.resolution.as_ref().and_then(|r| r.lanes.get(li));
        let end = match lane.end {
            seq_core::LaneEnd::LoopToEnd => "loop_to_end",
            seq_core::LaneEnd::Hold => "hold",
            seq_core::LaneEnd::Clear => "clear",
        };
        let state = res.map_or("missing", |r| r.layer);
        let _ = writeln!(out, "lane {state} {end} {}", one_line(&lane.layer));
        for (si, step) in lane.steps.iter().enumerate() {
            let r = res.and_then(|l| l.steps.get(si));
            // Slot steps (SPEC §4.5) show what the slot holds now; an empty slot is a rest.
            let (kind, clip_id) = match (step.is_rest(), r) {
                (true, _) => ("rest", 0),
                (false, Some(r)) if r.state == "ok" || r.state == "slot" => {
                    ("clip", r.clip_id.unwrap_or(0))
                }
                (false, Some(r)) if r.state == "empty" => ("rest", 0),
                (false, _) if step.column.is_some() => ("rest", 0),
                (false, _) => ("unresolved", 0),
            };
            let clip = (clip_id != 0)
                .then(|| {
                    deck.layers
                        .iter()
                        .flat_map(|l| &l.clips)
                        .find(|c| c.id == clip_id)
                })
                .flatten();
            let thumb = clip.and_then(|c| c.thumb.as_deref()).unwrap_or("-");
            let label = match (&step.clip, step.column, clip) {
                (None, None, _) => "rest".to_owned(),
                (_, Some(col), Some(c)) if !c.name.is_empty() => format!("col {col}: {}", c.name),
                (_, _, Some(c)) if !c.name.is_empty() => c.name.clone(),
                (_, Some(col), None) => format!("col {col} empty"),
                (_, Some(col), Some(_)) => format!("col {col}"),
                (Some(r), None, _) => r
                    .name
                    .clone()
                    .or_else(|| r.file.clone())
                    .unwrap_or_else(|| format!("clip {}", r.id.unwrap_or(0))),
            };
            let _ = writeln!(
                out,
                "step {kind} {} {clip_id} {} {}",
                step.length,
                one_line(thumb).replace(' ', "_"),
                one_line(&label)
            );
        }
    }
    out
}

/// Free text must stay on its line.
fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}
