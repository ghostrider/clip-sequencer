//! Layer transitions (SPEC §7.7, M21): the engine side of [`Action::SetTransition`] and
//! [`Action::TransitionReleased`]. Values go out as WebSocket sets by parameter id (notes §19),
//! never awaited; [`TransitionBook`] keeps the originals and tells the app's own echoes from the
//! user's changes. Originals of held layers are also kept in `<data-dir>/transitions-pending.json`,
//! so a crash or quit while holding a layer is undone at the next connection to that composition.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use seq_core::index::TransitionParams;
use seq_core::scheduler::Fade;
use seq_core::transition::{seconds, Change, LayerTransition, Send, TransitionBook};
use serde::{Deserialize, Serialize};

use super::*;

pub(super) const PENDING_FILE: &str = "transitions-pending.json";
/// How often the tempo is compared with the one the held durations were computed at.
const RETIME_EVERY: Duration = Duration::from_millis(500);
/// A tempo change smaller than this (relative) sends nothing (the MIDI estimate jitters).
const RETIME_DRIFT: f64 = 0.005;
/// Until a tempo is known.
const DEFAULT_BPM: f64 = 120.0;

/// `transitions-pending.json`: originals to restore, by composition name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct PendingFile {
    compositions: BTreeMap<String, Vec<PendingLayer>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PendingLayer {
    layer_id: u64,
    seconds: f64,
    blend: String,
}

pub(super) struct Fades {
    book: TransitionBook,
    /// The tempo the held durations were computed at.
    bpm: f64,
    retime_checked: Instant,
    path: PathBuf,
    /// Originals of other compositions (and, until the next connection, of this one) from the file.
    restore: BTreeMap<String, Vec<PendingLayer>>,
    /// What the file holds now (`None`: no file).
    written: Option<PendingFile>,
    /// Steps already warned about in this run: (sequence, lane, step).
    warned: HashSet<(Uuid, usize, usize)>,
}

impl Fades {
    /// Reads the pending file of an earlier run (small; once at startup).
    pub(super) fn load(dir: &Path) -> Self {
        let path = dir.join(PENDING_FILE);
        let file: Option<PendingFile> =
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| match serde_json::from_str(&t) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        log::warn!("{}: {e}; ignored", path.display());
                        None
                    }
                });
        if let Some(f) = &file {
            for (c, layers) in &f.compositions {
                log::info!(
                    "{} layer transition(s) of {c:?} to restore from the last run",
                    layers.len()
                );
            }
        }
        Self {
            book: TransitionBook::new(),
            bpm: DEFAULT_BPM,
            retime_checked: Instant::now(),
            path,
            restore: file.clone().unwrap_or_default().compositions,
            written: file,
            warned: HashSet::new(),
        }
    }
}

impl Engine {
    fn bpm_now(&self) -> f64 {
        self.tempo
            .bpm()
            .or(self.tempo_ctl.as_ref().map(|t| t.bpm))
            .filter(|b| *b > 0.0)
            .unwrap_or(DEFAULT_BPM)
    }

    fn layer_transition(&self, layer_id: u64) -> Option<(String, TransitionParams)> {
        let layer = self
            .index
            .as_ref()?
            .layers
            .iter()
            .find(|l| l.id == layer_id)?;
        Some((layer.name.clone(), layer.transition.clone()?))
    }

    pub(super) fn set_transition(
        &mut self,
        seq: Uuid,
        lane: usize,
        step: usize,
        layer_id: u64,
        fade: &Fade,
    ) {
        let Some((name, params)) = self.layer_transition(layer_id) else {
            return;
        };
        let bpm = self.bpm_now();
        self.fades.bpm = bpm;
        let planned = self
            .fades
            .book
            .set(layer_id, fade, bpm, &params.blend_options);
        if (planned.capped || planned.unknown_blend) && self.fades.warned.insert((seq, lane, step))
        {
            if let Fade::Set(t) = fade {
                if planned.capped {
                    self.notice(format!(
                        "{name}, step {}: a fade of {:.1} s is longer than Resolume's 10 s; capped",
                        step + 1,
                        seconds(t.length, bpm)
                    ));
                }
                if planned.unknown_blend {
                    self.notice(format!(
                        "{name}, step {}: Resolume has no blend mode {:?} on this layer; only the length is set",
                        step + 1,
                        t.blend.as_deref().unwrap_or_default()
                    ));
                }
            }
        }
        if !planned.send.is_empty() {
            log::debug!("transition on {name} → {}", describe(&planned.send));
        }
        self.send_transition(&params, &planned.send);
        self.write_pending();
    }

    pub(super) fn release_transition(&mut self, layer_id: u64) {
        let send = self.fades.book.release(layer_id);
        if let (Some((name, params)), false) = (self.layer_transition(layer_id), send.is_empty()) {
            let now = self.fades.book.current(layer_id).cloned();
            if let Some(v) = now {
                log::info!(
                    "transition on {name} restored ({:.1} s, {})",
                    v.seconds,
                    v.blend
                );
            }
            self.send_transition(&params, &send);
        }
        self.write_pending();
    }

    fn send_transition(&self, params: &TransitionParams, send: &Send) {
        let set = |id: u64, value: serde_json::Value| {
            let msg = serde_json::json!({
                "action": "set",
                "parameter": format!("/parameter/by-id/{id}"),
                "value": value,
            });
            let _ = self.ws_cmd.send(WsCommand::Raw(msg.to_string()));
        };
        if let Some(s) = send.seconds {
            set(params.duration_param, serde_json::json!(s));
        }
        if let Some(b) = &send.blend {
            set(params.blend_param, serde_json::json!(b));
        }
    }

    /// A subscribed layer transition parameter reported a value.
    pub(super) fn transition_param(
        &mut self,
        layer_id: u64,
        blend: bool,
        kind: ParamMsg,
        value: &serde_json::Value,
    ) {
        let change = match (kind, blend) {
            // The answer to a set carries the old value.
            (ParamMsg::Set, _) => return,
            (ParamMsg::Update, false) => match value.as_f64() {
                Some(s) => self.fades.book.duration_update(layer_id, s),
                None => return,
            },
            (ParamMsg::Update, true) => match value.as_str() {
                Some(b) => self.fades.book.blend_update(layer_id, b),
                None => return,
            },
            // Subscription and get replies: the current value, no change of hands.
            (_, false) => {
                if let (Some(s), Some(cur)) = (value.as_f64(), self.fades.book.current(layer_id)) {
                    let v = LayerTransition {
                        seconds: s,
                        blend: cur.blend.clone(),
                    };
                    self.fades.book.report(layer_id, v);
                }
                return;
            }
            (_, true) => {
                if let (Some(b), Some(cur)) = (value.as_str(), self.fades.book.current(layer_id)) {
                    let v = LayerTransition {
                        seconds: cur.seconds,
                        blend: b.to_owned(),
                    };
                    self.fades.book.report(layer_id, v);
                }
                return;
            }
        };
        if change == Change::User {
            let name = self
                .layer_transition(layer_id)
                .map(|(n, _)| n)
                .unwrap_or_default();
            log::info!(
                "transition on {name} changed in Resolume: kept, not restored when the sequence lets go"
            );
            self.write_pending();
        }
    }

    /// Resends the held layers' durations when the tempo moved (throttled, SPEC §7.7). A fade
    /// running then is rescaled with it (notes §19), so it still ends on its beat.
    pub(super) fn retime_transitions(&mut self, now: Instant) {
        if now.saturating_duration_since(self.fades.retime_checked) < RETIME_EVERY {
            return;
        }
        self.fades.retime_checked = now;
        let Some(bpm) = self.tempo.bpm().filter(|b| *b > 0.0) else {
            return;
        };
        if ((bpm - self.fades.bpm) / self.fades.bpm).abs() < RETIME_DRIFT {
            return;
        }
        self.fades.bpm = bpm;
        for (layer_id, s) in self.fades.book.retime(bpm) {
            if let Some((_, params)) = self.layer_transition(layer_id) {
                self.send_transition(
                    &params,
                    &Send {
                        seconds: Some(s),
                        blend: None,
                    },
                );
            }
        }
    }

    /// A composition push: the layers' current values, and the restore of an earlier run's
    /// originals once connected to that composition.
    pub(super) fn transitions_on_composition(
        &mut self,
        values: HashMap<u64, LayerTransition>,
        index: &CompositionIndex,
    ) {
        for (layer, v) in values {
            self.fades.book.report(layer, v);
        }
        let Some(layers) = self.fades.restore.remove(&index.composition) else {
            return;
        };
        for p in layers {
            let found = index
                .layers
                .iter()
                .find(|l| l.id == p.layer_id)
                .and_then(|l| Some((l.name.clone(), l.transition.clone()?)));
            match found {
                Some((name, params)) => {
                    log::info!(
                        "transition on {name} restored from the last run ({:.1} s, {})",
                        p.seconds,
                        p.blend
                    );
                    self.send_transition(
                        &params,
                        &Send {
                            seconds: Some(p.seconds),
                            blend: Some(p.blend.clone()),
                        },
                    );
                }
                None => log::info!(
                    "transition of layer {} not restored: the layer is gone",
                    p.layer_id
                ),
            }
        }
        self.write_pending();
    }

    /// Another composition is loaded: what this one held can't be restored now; it stays in the
    /// file for the next connection to it.
    pub(super) fn transitions_composition_changed(&mut self, old: Option<&str>) {
        let held = self.fades.book.reset();
        if let (Some(old), false) = (old, held.is_empty()) {
            self.fades.restore.insert(
                old.to_owned(),
                held.into_iter().map(|(id, v)| pending(id, v)).collect(),
            );
        }
        self.write_pending();
    }

    /// The layer's transition as Resolume has it now: (seconds, blend mode).
    pub(super) fn fades_current(&self, layer_id: u64) -> Option<(f64, String)> {
        self.fades
            .book
            .current(layer_id)
            .map(|v| (v.seconds, v.blend.clone()))
    }

    pub(super) fn transitions_connection_lost(&mut self) {
        self.fades.book.connection_lost();
    }

    pub(super) fn transitions_run_ended(&mut self, seq: Uuid) {
        self.fades.warned.retain(|(s, _, _)| *s != seq);
    }

    /// Writes (or removes) the pending file when what it should hold changed (off-task).
    fn write_pending(&mut self) {
        let mut file = PendingFile {
            compositions: self.fades.restore.clone(),
        };
        let held: Vec<PendingLayer> = self
            .fades
            .book
            .originals()
            .into_iter()
            .map(|(id, v)| pending(id, v))
            .collect();
        if let (Some(c), false) = (&self.composition, held.is_empty()) {
            file.compositions.entry(c.clone()).or_default().extend(held);
        }
        let want = (!file.compositions.is_empty()).then_some(file);
        if want == self.fades.written {
            return;
        }
        let path = self.fades.path.clone();
        let json = want
            .as_ref()
            .and_then(|f| serde_json::to_string_pretty(f).ok());
        self.fades.written = want;
        tokio::task::spawn_blocking(move || {
            let result = match json {
                Some(json) => {
                    let tmp = path.with_extension("json.tmp");
                    std::fs::write(&tmp, json).and_then(|()| std::fs::rename(&tmp, &path))
                }
                None => match std::fs::remove_file(&path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    r => r,
                },
            };
            if let Err(e) = result {
                log::warn!("{}: {e}", path.display());
            }
        });
    }
}

fn pending(layer_id: u64, v: LayerTransition) -> PendingLayer {
    PendingLayer {
        layer_id,
        seconds: v.seconds,
        blend: v.blend,
    }
}

fn describe(send: &Send) -> String {
    match (&send.seconds, &send.blend) {
        (Some(s), Some(b)) => format!("{s:.2} s, {b}"),
        (Some(s), None) => format!("{s:.2} s"),
        (None, Some(b)) => b.clone(),
        (None, None) => "no change".into(),
    }
}
