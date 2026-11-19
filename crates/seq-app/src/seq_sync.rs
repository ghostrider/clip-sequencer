//! "Sync SEQ layer" worker (SPEC §7.6). Runs off the engine task: it reads the composition over
//! REST, plans with [`seq_core::seq_sync::plan`], then creates the SEQ layer, clears orphaned `§`
//! clips (when asked), adds columns and fills empty slots with the "1000A Sequence" source (or
//! `Blank` without the plugin), and names them `§ <sequence>`. Every Resolume call it makes is
//! recorded in `docs/resolume-api-notes.md` §16. The engine binds the plugin's `Sequence`
//! parameter as for any `§` clip (SPEC §12).

use std::collections::HashSet;
use std::time::Duration;

use resolume_client::composition::CompositionSnapshot;
use resolume_client::rest::RestClient;
use resolume_client::ws::WsCommand;
use seq_core::index::LayerLookup;
use seq_core::seq_sync::{plan, SeqLayerState, SeqSyncPlan};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::engine::Aux;

/// Source name of the FFGL plugin (`ffgl/`, SPEC §12) in Resolume's source list.
pub const PLUGIN_SOURCE: &str = "1000A Sequence";
const BLANK_SOURCE: &str = "Blank";

/// How long to wait for Resolume to show a change in the composition.
const SETTLE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct SyncRequest {
    /// The live deck when the user confirmed; the sync stops if another deck is live.
    pub deck: String,
    /// The deck's sequence names, in list order.
    pub sequences: Vec<String>,
    pub layer: String,
    pub prefix: String,
    /// Also clear `§` clips whose name matches no sequence (not playing ones).
    pub remove_orphans: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub created_layer: bool,
    pub columns_added: usize,
    /// Sequence names that got a `§` clip.
    pub added: Vec<String>,
    /// Names of cleared `§` clips.
    pub removed: Vec<String>,
    /// Orphaned `§` clips left alone because they were playing.
    pub kept_playing: Vec<String>,
    /// The source loaded into new clips: the plugin, or `Blank` when it isn't installed.
    pub source: String,
}

impl SyncReport {
    /// One line for the status bar.
    pub fn summary(&self, layer: &str) -> String {
        let mut parts = Vec::new();
        if self.created_layer {
            parts.push(format!("created layer {layer}"));
        }
        if !self.added.is_empty() {
            parts.push(format!(
                "added {} clip{} ({})",
                self.added.len(),
                if self.added.len() == 1 { "" } else { "s" },
                self.source
            ));
        }
        if self.columns_added > 0 {
            parts.push(format!("added {} column(s)", self.columns_added));
        }
        if !self.removed.is_empty() {
            parts.push(format!("removed {}", self.removed.join(", ")));
        }
        if !self.kept_playing.is_empty() {
            parts.push(format!("kept playing {}", self.kept_playing.join(", ")));
        }
        if parts.is_empty() {
            format!("{layer} layer already in sync")
        } else {
            format!("Sync {layer}: {}", parts.join("; "))
        }
    }
}

/// Starts the worker. Requests run one after the other; each answers with [`Aux::SeqSynced`].
pub fn spawn(
    rest: RestClient,
    ws: mpsc::UnboundedSender<WsCommand>,
    aux: mpsc::UnboundedSender<Aux>,
) -> mpsc::UnboundedSender<SyncRequest> {
    let (tx, mut rx) = mpsc::unbounded_channel::<SyncRequest>();
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            let result = run(&rest, &ws, &req).await;
            if let Err(e) = &result {
                log::warn!("sync {} layer: {e}", req.layer);
            }
            let _ = aux.send(Aux::SeqSynced(result));
        }
    });
    tx
}

async fn run(
    rest: &RestClient,
    ws: &mpsc::UnboundedSender<WsCommand>,
    req: &SyncRequest,
) -> Result<SyncReport, String> {
    let mut report = SyncReport::default();
    let snap = fresh(rest, &req.deck).await?;
    let first = plan_of(&snap, req);
    log::info!(
        "sync {}: layer {:?}, add {:?}, {} orphan(s)",
        req.layer,
        first.layer,
        first.add,
        first.orphans.len()
    );

    // 1. The layer.
    let mut snap = match first.layer {
        SeqLayerState::Ambiguous { count } => {
            return Err(format!(
                "{count} layers are named {:?}; rename all but one",
                req.layer
            ))
        }
        SeqLayerState::Missing => {
            report.created_layer = true;
            create_layer(rest, ws, req, &snap).await?
        }
        SeqLayerState::Exists { .. } => snap,
    };

    // 2. Orphans.
    let p = plan_of(&snap, req);
    if req.remove_orphans {
        for o in &p.orphans {
            if o.connected {
                report.kept_playing.push(o.name.clone());
            } else {
                rest.clear_clip(o.clip_id)
                    .await
                    .map_err(|e| format!("clear {:?}: {e}", o.name))?;
                report.removed.push(o.name.clone());
            }
        }
        if !report.removed.is_empty() {
            let cleared: Vec<u64> = p
                .orphans
                .iter()
                .filter(|o| !o.connected)
                .map(|o| o.clip_id)
                .collect();
            snap = wait_for(rest, &req.deck, |s| {
                cleared
                    .iter()
                    .all(|id| s.index.clip_by_id(*id).is_some_and(|c| c.empty))
            })
            .await?;
        }
    }

    // 3. Room for the new clips.
    let p = plan_of(&snap, req);
    if p.add.is_empty() {
        return Ok(report);
    }
    let missing = p.add.len().saturating_sub(p.empty_slots.len());
    if missing > 0 {
        let columns = snap.index.columns.len();
        for _ in 0..missing {
            rest.add_column()
                .await
                .map_err(|e| format!("add column: {e}"))?;
        }
        report.columns_added = missing;
        snap = wait_for(rest, &req.deck, |s| {
            s.index.columns.len() >= columns + missing
        })
        .await?;
    }

    // 4. Fill slots with the plugin (or Blank), then name them.
    let sources = rest
        .video_sources()
        .await
        .map_err(|e| format!("read sources: {e}"))?;
    let source = if sources.iter().any(|s| s == PLUGIN_SOURCE) {
        PLUGIN_SOURCE
    } else {
        log::warn!("sync: {PLUGIN_SOURCE:?} is not installed in Resolume; using {BLANK_SOURCE}");
        BLANK_SOURCE
    };
    report.source = source.to_owned();
    let p = plan_of(&snap, req);
    let targets: Vec<(String, u64)> = p
        .add
        .iter()
        .cloned()
        .zip(p.empty_slots.iter().map(|(_, id)| *id))
        .collect();
    if targets.len() < p.add.len() {
        return Err("not enough empty slots on the layer".into());
    }
    let url = source_url(source);
    for (name, id) in &targets {
        rest.open_clip(*id, &url)
            .await
            .map_err(|e| format!("load {source} for {name:?}: {e}"))?;
    }
    let snap = wait_for(rest, &req.deck, |s| {
        targets
            .iter()
            .all(|(_, id)| s.index.clip_by_id(*id).is_some_and(|c| !c.empty))
    })
    .await?;
    for (name, id) in &targets {
        let Some(clip) = snap.index.clip_by_id(*id) else {
            return Err(format!("slot for {name:?} disappeared"));
        };
        set_param(ws, clip.name_param, format!("{}{name}", req.prefix).into());
    }
    let prefix = req.prefix.clone();
    wait_for(rest, &req.deck, |s| {
        targets.iter().all(|(name, id)| {
            s.index
                .clip_by_id(*id)
                .is_some_and(|c| c.name == format!("{prefix}{name}"))
        })
    })
    .await
    .map_err(|e| format!("naming the new clips: {e}"))?;
    report.added = targets.into_iter().map(|(n, _)| n).collect();
    Ok(report)
}

fn plan_of(snap: &CompositionSnapshot, req: &SyncRequest) -> SeqSyncPlan {
    plan(&snap.index, &req.layer, &req.prefix, &req.sequences, |id| {
        snap.states.get(&id).is_some_and(|s| s.is_connected())
    })
}

/// Appends a layer, names it and sets its opacity to 0 so the placeholders don't show on the
/// output until the user wants them to (the plugin still renders, notes §11).
async fn create_layer(
    rest: &RestClient,
    ws: &mpsc::UnboundedSender<WsCommand>,
    req: &SyncRequest,
    snap: &CompositionSnapshot,
) -> Result<CompositionSnapshot, String> {
    let before: HashSet<u64> = snap.index.layers.iter().map(|l| l.id).collect();
    rest.add_layer()
        .await
        .map_err(|e| format!("add layer: {e}"))?;
    let snap = wait_for(rest, &req.deck, |s| {
        s.index.layers.iter().any(|l| !before.contains(&l.id))
    })
    .await?;
    let layer = snap
        .index
        .layers
        .iter()
        .filter(|l| !before.contains(&l.id))
        .max_by_key(|l| l.index)
        .ok_or("the new layer is missing")?;
    let (index, id) = (layer.index, layer.id);
    set_path(
        ws,
        &format!("/composition/layers/{index}/name"),
        req.layer.clone().into(),
    );
    match rest.layer_json(index).await {
        Ok(v) => match v["video"]["opacity"]["id"].as_u64() {
            Some(op) => set_param(ws, op, 0.0.into()),
            None => log::warn!("sync: new layer has no opacity parameter"),
        },
        Err(e) => log::warn!("sync: read new layer: {e}"),
    }
    let name = req.layer.clone();
    wait_for(
        rest,
        &req.deck,
        |s| matches!(s.index.layer_by_name(&name), LayerLookup::Found(l) if l.id == id),
    )
    .await
    .map_err(|e| format!("naming the new layer: {e}"))
}

/// The `open` body for a source by name (notes §7, §16).
fn source_url(name: &str) -> String {
    format!("source:///video/{}", name.replace(' ', "%20"))
}

fn set_param(ws: &mpsc::UnboundedSender<WsCommand>, id: u64, value: serde_json::Value) {
    set_path(ws, &format!("/parameter/by-id/{id}"), value);
}

fn set_path(ws: &mpsc::UnboundedSender<WsCommand>, path: &str, value: serde_json::Value) {
    let msg = serde_json::json!({"action": "set", "parameter": path, "value": value});
    let _ = ws.send(WsCommand::Raw(msg.to_string()));
}

/// The composition now, if `deck` is still the live deck.
async fn fresh(rest: &RestClient, deck: &str) -> Result<CompositionSnapshot, String> {
    let snap = rest
        .composition()
        .await
        .map_err(|e| format!("read composition: {e}"))?;
    if snap.index.live_deck.as_deref() != Some(deck) {
        return Err(format!(
            "deck {deck:?} is no longer live in Resolume; nothing more was changed"
        ));
    }
    Ok(snap)
}

/// Polls the composition until `done` holds (or [`SETTLE`] passes).
async fn wait_for(
    rest: &RestClient,
    deck: &str,
    done: impl Fn(&CompositionSnapshot) -> bool,
) -> Result<CompositionSnapshot, String> {
    let until = tokio::time::Instant::now() + SETTLE;
    loop {
        let snap = fresh(rest, deck).await?;
        if done(&snap) {
            return Ok(snap);
        }
        if tokio::time::Instant::now() >= until {
            return Err("Resolume did not show the change in time".into());
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_url_encodes_spaces() {
        assert_eq!(source_url("Blank"), "source:///video/Blank");
        assert_eq!(
            source_url("1000A Sequence"),
            "source:///video/1000A%20Sequence"
        );
    }

    #[test]
    fn summary_lists_what_changed() {
        let r = SyncReport::default();
        assert_eq!(r.summary("SEQ"), "SEQ layer already in sync");
        let r = SyncReport {
            created_layer: true,
            added: vec!["Intro".into(), "Drop".into()],
            source: PLUGIN_SOURCE.into(),
            ..SyncReport::default()
        };
        assert_eq!(
            r.summary("SEQ"),
            "Sync SEQ: created layer SEQ; added 2 clips (1000A Sequence)"
        );
    }
}
