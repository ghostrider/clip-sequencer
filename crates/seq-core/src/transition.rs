//! Layer transitions in Resolume's terms (SPEC §7.7, notes §19): what to send for a
//! [`Fade`], the original of each layer a lane with transitions holds, and telling the app's own
//! changes from the user's. Pure: the engine reports what Resolume has and sends what comes back.
//!
//! Resolume sets a layer's duration (seconds, 0–10) and blend mode (an option name) as two
//! parameters; each `set` is echoed by a `parameter_update` that carries no origin. An update
//! whose value the app sent and has not seen echoed yet is the app's own; any other one on a held
//! layer is the user's: the original is dropped and nothing is restored there for this run.

use std::collections::{HashMap, VecDeque};

use crate::scheduler::Fade;

/// Resolume's longest layer transition.
pub const MAX_SECONDS: f64 = 10.0;

/// Values closer than this are the same (Resolume echoes doubles; tempo resends are throttled).
const SAME_SECONDS: f64 = 0.000_5;

/// A sixteenth's length in seconds at `bpm`, times `sixteenths`.
pub fn seconds(sixteenths: u32, bpm: f64) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }
    f64::from(sixteenths) * 15.0 / bpm
}

/// A layer's transition as Resolume has it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerTransition {
    pub seconds: f64,
    pub blend: String,
}

/// What to send to one layer; `None` fields are already right.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Send {
    pub seconds: Option<f64>,
    pub blend: Option<String>,
}

impl Send {
    pub fn is_empty(&self) -> bool {
        self.seconds.is_none() && self.blend.is_none()
    }
}

/// A [`TransitionBook::set`] result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Planned {
    pub send: Send,
    /// The length was over [`MAX_SECONDS`] and was capped.
    pub capped: bool,
    /// The blend mode isn't one of the layer's options: only the length is set.
    pub unknown_blend: bool,
}

/// Who changed a layer's value, as far as the book can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The echo of a value the app sent.
    Own,
    /// Someone else changed a layer the app doesn't hold: just the new current value.
    Other,
    /// The user changed a held layer: its original is dropped (first time only).
    User,
}

#[derive(Debug, Default)]
struct Layer {
    /// What Resolume has: reported, or sent by the app.
    current: Option<LayerTransition>,
    /// Held by a lane with transitions.
    held: bool,
    /// The value before the app first set it; `None` once the user changed the layer.
    original: Option<LayerTransition>,
    /// Sent and not echoed yet, oldest first.
    sent_seconds: VecDeque<f64>,
    sent_blend: VecDeque<String>,
    /// The fade length in sixteenths last set, for tempo changes.
    length: Option<u32>,
}

/// Originals and current values of the layers' transitions.
#[derive(Debug, Default)]
pub struct TransitionBook {
    layers: HashMap<u64, Layer>,
}

impl TransitionBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolume reports the layer's value (composition, subscription reply): no change of hands.
    pub fn report(&mut self, layer: u64, value: LayerTransition) {
        let l = self.layers.entry(layer).or_default();
        if l.sent_seconds.is_empty() && l.sent_blend.is_empty() {
            l.current = Some(value);
        }
    }

    /// The layer's value as the book knows it.
    pub fn current(&self, layer: u64) -> Option<&LayerTransition> {
        self.layers.get(&layer)?.current.as_ref()
    }

    /// A lane with transitions holds the layer.
    pub fn is_held(&self, layer: u64) -> bool {
        self.layers.get(&layer).is_some_and(|l| l.held)
    }

    /// An update of the layer's duration.
    pub fn duration_update(&mut self, layer: u64, seconds: f64) -> Change {
        let l = self.layers.entry(layer).or_default();
        if let Some(i) = l
            .sent_seconds
            .iter()
            .position(|s| (s - seconds).abs() < SAME_SECONDS)
        {
            l.sent_seconds.drain(..=i);
            return Change::Own;
        }
        let blend = l
            .current
            .as_ref()
            .map(|c| c.blend.clone())
            .unwrap_or_default();
        l.current = Some(LayerTransition { seconds, blend });
        l.length = None;
        Self::other(l)
    }

    /// An update of the layer's blend mode.
    pub fn blend_update(&mut self, layer: u64, blend: &str) -> Change {
        let l = self.layers.entry(layer).or_default();
        if let Some(i) = l.sent_blend.iter().position(|b| b == blend) {
            l.sent_blend.drain(..=i);
            return Change::Own;
        }
        let seconds = l.current.as_ref().map_or(0.0, |c| c.seconds);
        l.current = Some(LayerTransition {
            seconds,
            blend: blend.to_owned(),
        });
        Self::other(l)
    }

    fn other(l: &mut Layer) -> Change {
        if l.held && l.original.is_some() {
            l.original = None;
            Change::User
        } else {
            Change::Other
        }
    }

    /// A step's transition is due on the layer (SPEC §7.7). The first one on a layer that isn't
    /// held makes the current value its original. `options` are the layer's blend modes (empty:
    /// unknown, every name is tried). After the user changed the layer, an original step sets
    /// nothing.
    pub fn set(&mut self, layer: u64, fade: &Fade, bpm: f64, options: &[String]) -> Planned {
        let l = self.layers.entry(layer).or_default();
        if !l.held {
            l.held = true;
            l.original = l.current.clone();
        }
        let mut planned = Planned::default();
        let (seconds, blend, length) = match fade {
            Fade::Set(t) => {
                let s = seconds(t.length, bpm);
                planned.capped = s > MAX_SECONDS;
                let blend = match &t.blend {
                    Some(b) if options.is_empty() || options.contains(b) => Some(b.clone()),
                    Some(_) => {
                        planned.unknown_blend = true;
                        None
                    }
                    None => l.original.as_ref().map(|o| o.blend.clone()),
                };
                (Some(s.min(MAX_SECONDS)), blend, Some(t.length))
            }
            Fade::Original => match &l.original {
                Some(o) => (Some(o.seconds), Some(o.blend.clone()), None),
                None => (None, None, None),
            },
        };
        l.length = length;
        planned.send = Self::send_to(l, seconds, blend);
        planned
    }

    /// No lane with transitions holds the layer any more: what brings the original back
    /// (nothing after a user change). The layer is no longer held.
    pub fn release(&mut self, layer: u64) -> Send {
        let Some(l) = self.layers.get_mut(&layer) else {
            return Send::default();
        };
        if !l.held {
            return Send::default();
        }
        l.held = false;
        l.length = None;
        match l.original.take() {
            Some(o) => Self::send_to(l, Some(o.seconds), Some(o.blend)),
            None => Send::default(),
        }
    }

    /// The tempo changed: new durations for the held layers whose length is in sixteenths.
    pub fn retime(&mut self, bpm: f64) -> Vec<(u64, f64)> {
        let mut out = Vec::new();
        for (&id, l) in &mut self.layers {
            let (true, Some(length)) = (l.held, l.length) else {
                continue;
            };
            let s = seconds(length, bpm).min(MAX_SECONDS);
            if let Some(v) = Self::send_to(l, Some(s), None).seconds {
                out.push((id, v));
            }
        }
        out.sort_by_key(|(id, _)| *id);
        out
    }

    /// The originals of the held layers (for `transitions-pending.json`).
    pub fn originals(&self) -> Vec<(u64, LayerTransition)> {
        let mut out: Vec<_> = self
            .layers
            .iter()
            .filter(|(_, l)| l.held)
            .filter_map(|(id, l)| Some((*id, l.original.clone()?)))
            .collect();
        out.sort_by_key(|(id, _)| *id);
        out
    }

    /// The WebSocket dropped: echoes still owed will never come. Originals and holds stay; the
    /// next composition reports the current values.
    pub fn connection_lost(&mut self) {
        for l in self.layers.values_mut() {
            l.sent_seconds.clear();
            l.sent_blend.clear();
        }
    }

    /// The connection or composition changed: nothing known is true any more. Returns the
    /// originals of the layers that were held (to restore once connected again).
    pub fn reset(&mut self) -> Vec<(u64, LayerTransition)> {
        let held = self.originals();
        self.layers.clear();
        held
    }

    /// Records what is sent and returns the parts that differ from the current value.
    fn send_to(l: &mut Layer, seconds: Option<f64>, blend: Option<String>) -> Send {
        let cur = l.current.clone();
        let seconds = seconds.filter(|s| {
            cur.as_ref()
                .is_none_or(|c| (c.seconds - s).abs() >= SAME_SECONDS)
        });
        let blend = blend.filter(|b| cur.as_ref().is_none_or(|c| &c.blend != b));
        if let Some(s) = seconds {
            l.sent_seconds.push_back(s);
        }
        if let Some(b) = &blend {
            l.sent_blend.push_back(b.clone());
        }
        if seconds.is_some() || blend.is_some() {
            let mut next = cur.unwrap_or(LayerTransition {
                seconds: 0.0,
                blend: String::new(),
            });
            if let Some(s) = seconds {
                next.seconds = s;
            }
            if let Some(b) = &blend {
                next.blend = b.clone();
            }
            l.current = Some(next);
        }
        Send { seconds, blend }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Transition;

    fn lt(seconds: f64, blend: &str) -> LayerTransition {
        LayerTransition {
            seconds,
            blend: blend.into(),
        }
    }

    fn fade(length: u32, blend: Option<&str>) -> Fade {
        Fade::Set(Transition {
            length,
            blend: blend.map(Into::into),
        })
    }

    fn opts() -> Vec<String> {
        ["Alpha", "Dissolve", "Wipe Ellipse"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn seconds_at_tempo() {
        assert_eq!(seconds(4, 120.0), 0.5); // a beat
        assert_eq!(seconds(16, 60.0), 4.0); // a bar
        assert_eq!(seconds(0, 128.0), 0.0);
        assert_eq!(seconds(4, 0.0), 0.0);
    }

    #[test]
    fn the_first_set_takes_the_original_and_only_differences_are_sent() {
        let mut b = TransitionBook::new();
        b.report(1, lt(0.0, "Alpha"));
        let p = b.set(1, &fade(8, Some("Dissolve")), 120.0, &opts());
        assert_eq!(
            p.send,
            Send {
                seconds: Some(1.0),
                blend: Some("Dissolve".into())
            }
        );
        assert!(b.is_held(1));
        // Same value again: nothing to send; a step without a blend keeps the original's.
        assert!(b
            .set(1, &fade(8, Some("Dissolve")), 120.0, &opts())
            .send
            .is_empty());
        assert_eq!(
            b.set(1, &fade(4, None), 120.0, &opts()).send,
            Send {
                seconds: Some(0.5),
                blend: Some("Alpha".into())
            }
        );
        // Original: back to 0 s (the blend is already Alpha).
        assert_eq!(
            b.set(1, &Fade::Original, 120.0, &opts()).send,
            Send {
                seconds: Some(0.0),
                blend: None
            }
        );
        assert_eq!(b.originals(), vec![(1, lt(0.0, "Alpha"))]);
    }

    #[test]
    fn long_fades_are_capped_and_unknown_blends_leave_the_blend_alone() {
        let mut b = TransitionBook::new();
        b.report(1, lt(0.0, "Alpha"));
        let p = b.set(1, &fade(64, Some("Nope")), 60.0, &opts()); // 16 s
        assert!(p.capped && p.unknown_blend);
        assert_eq!(
            p.send,
            Send {
                seconds: Some(10.0),
                blend: None
            }
        );
        // Without a known option list every name is tried.
        let p = b.set(1, &fade(4, Some("Nope")), 60.0, &[]);
        assert!(!p.unknown_blend);
        assert_eq!(p.send.blend.as_deref(), Some("Nope"));
    }

    #[test]
    fn echoes_are_own_changes_others_are_the_users() {
        let mut b = TransitionBook::new();
        b.report(1, lt(0.0, "Alpha"));
        b.set(1, &fade(8, Some("Dissolve")), 120.0, &opts());
        b.set(1, &fade(4, None), 120.0, &opts());
        // Echoes arrive late and in order: the first ones are still the app's own.
        assert_eq!(b.duration_update(1, 1.0), Change::Own);
        assert_eq!(b.blend_update(1, "Dissolve"), Change::Own);
        assert_eq!(b.duration_update(1, 0.5), Change::Own);
        assert_eq!(b.blend_update(1, "Alpha"), Change::Own);
        // The user sets 2 s: the original is dropped, the release restores nothing.
        assert_eq!(b.duration_update(1, 2.0), Change::User);
        assert_eq!(b.duration_update(1, 3.0), Change::Other);
        assert!(b.originals().is_empty());
        // An original step after that sets nothing.
        assert!(b.set(1, &Fade::Original, 120.0, &opts()).send.is_empty());
        // Own steps still set their value.
        assert_eq!(
            b.set(1, &fade(8, None), 120.0, &opts()).send.seconds,
            Some(1.0)
        );
        assert!(b.release(1).is_empty());
        assert!(!b.is_held(1));
    }

    #[test]
    fn release_restores_the_original_once_and_a_new_hold_takes_a_new_one() {
        let mut b = TransitionBook::new();
        b.report(1, lt(0.25, "Alpha"));
        b.set(1, &fade(8, Some("Dissolve")), 120.0, &opts());
        assert_eq!(
            b.release(1),
            Send {
                seconds: Some(0.25),
                blend: Some("Alpha".into())
            }
        );
        assert!(b.release(1).is_empty());
        // Unheld: an update is nobody's business.
        b.duration_update(1, 0.25);
        b.blend_update(1, "Alpha");
        assert_eq!(b.duration_update(1, 0.75), Change::Other);
        b.set(1, &fade(8, None), 120.0, &opts());
        assert_eq!(b.originals(), vec![(1, lt(0.75, "Alpha"))]);
    }

    #[test]
    fn tempo_changes_retime_held_layers_with_a_length() {
        let mut b = TransitionBook::new();
        b.report(1, lt(0.0, "Alpha"));
        b.report(2, lt(0.0, "Alpha"));
        b.report(3, lt(0.0, "Alpha"));
        b.set(1, &fade(8, None), 120.0, &opts());
        b.set(2, &Fade::Original, 120.0, &opts());
        assert_eq!(b.retime(60.0), vec![(1, 2.0)]);
        assert!(b.retime(60.0).is_empty()); // already there
        b.release(1);
        assert!(b.retime(120.0).is_empty());
    }

    #[test]
    fn reset_hands_back_what_was_held() {
        let mut b = TransitionBook::new();
        b.report(1, lt(1.5, "Alpha"));
        b.set(1, &fade(8, None), 120.0, &opts());
        assert_eq!(b.reset(), vec![(1, lt(1.5, "Alpha"))]);
        assert!(!b.is_held(1));
        assert!(b.current(1).is_none());
    }
}
