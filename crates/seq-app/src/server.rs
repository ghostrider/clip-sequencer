//! UI server (SPEC §9.6): static UI (embedded with rust-embed), one WebSocket `/ws` per browser,
//! and thumbnails under `/thumbs/...`. It is also a worker for the engine: thumbnail fetching,
//! mosaic rendering and pushing thumbnails to Resolume happen here, never on the engine task.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use resolume_client::rest::RestClient;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::engine::view::{RuntimeState, SequenceView, StructureState};
use crate::engine::EngineCommand;
use crate::thumbs::{mosaic_cells, render_mosaic, ThumbCache};
use seq_core::edit::EditOp;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Assets;

#[derive(Clone)]
pub struct ServerCtx {
    pub commands: mpsc::UnboundedSender<EngineCommand>,
    pub structure: watch::Receiver<StructureState>,
    pub runtime: watch::Receiver<RuntimeState>,
    pub thumbs: Arc<ThumbCache>,
    pub rest: RestClient,
}

pub fn router(ctx: ServerCtx) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .route("/thumbs/clip/{file}", get(clip_thumb))
        .route("/thumbs/seq/{file}", get(seq_thumb))
        .route("/plugin/v1/view.txt", get(plugin_view))
        .fallback(get(static_file))
        .with_state(ctx)
}

pub async fn serve(listener: tokio::net::TcpListener, ctx: ServerCtx) -> anyhow::Result<()> {
    log::info!("UI on http://{}", listener.local_addr()?);
    tokio::spawn(prefetch_live_thumbnails(ctx.clone()));
    axum::serve(listener, router(ctx)).await?;
    Ok(())
}

// ---- WebSocket ----

/// Browser → app (SPEC §9.6).
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum ClientMsg {
    Start { seq: Uuid },
    Stop { seq: Uuid },
    StopAll,
    ResumeLane { seq: Uuid, lane: usize },
    Edit { op: EditOp },
    Undo,
    Redo,
    PushThumbnail { seq: Uuid },
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(ctx): State<ServerCtx>) -> Response {
    ws.on_upgrade(move |socket| session(socket, ctx))
}

fn msg(kind: &str, data: &impl serde::Serialize) -> Message {
    Message::Text(json!({"type": kind, "data": data}).to_string().into())
}

async fn session(mut socket: WebSocket, ctx: ServerCtx) {
    let mut structure = ctx.structure.clone();
    let mut runtime = ctx.runtime.clone();
    let first = [
        msg("structure", &*structure.borrow_and_update()),
        msg("runtime", &*runtime.borrow_and_update()),
    ];
    for m in first {
        if socket.send(m).await.is_err() {
            return;
        }
    }
    loop {
        let out = tokio::select! {
            r = structure.changed() => match r {
                Ok(()) => msg("structure", &*structure.borrow_and_update()),
                Err(_) => return,
            },
            r = runtime.changed() => match r {
                Ok(()) => msg("runtime", &*runtime.borrow_and_update()),
                Err(_) => return,
            },
            incoming = socket.recv() => {
                let Some(Ok(m)) = incoming else { return };
                if let Message::Text(text) = m {
                    handle(&ctx, &text);
                }
                continue;
            }
        };
        if socket.send(out).await.is_err() {
            return;
        }
    }
}

fn handle(ctx: &ServerCtx, text: &str) {
    let cmd = match serde_json::from_str::<ClientMsg>(text) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("bad UI message {text:?}: {e}");
            let _ = ctx
                .commands
                .send(EngineCommand::Notice(format!("bad UI message: {e}")));
            return;
        }
    };
    let engine_cmd = match cmd {
        ClientMsg::Start { seq } => EngineCommand::Start { seq },
        ClientMsg::Stop { seq } => EngineCommand::Stop { seq },
        ClientMsg::StopAll => EngineCommand::StopAll,
        ClientMsg::ResumeLane { seq, lane } => EngineCommand::ResumeLane { seq, lane },
        ClientMsg::Edit { op } => EngineCommand::Edit { op },
        ClientMsg::Undo => EngineCommand::Undo,
        ClientMsg::Redo => EngineCommand::Redo,
        ClientMsg::PushThumbnail { seq } => {
            tokio::spawn(push_thumbnail(ctx.clone(), seq));
            return;
        }
    };
    let _ = ctx.commands.send(engine_cmd);
}

/// Thumbnail versions of every clip the UI knows (live and cached decks).
fn versions(s: &StructureState) -> HashMap<u64, Option<String>> {
    s.decks
        .iter()
        .flat_map(|d| &d.layers)
        .flat_map(|l| &l.clips)
        .map(|c| (c.id, c.thumb.clone()))
        .collect()
}

async fn mosaic_png(
    ctx: &ServerCtx,
    view: &SequenceView,
    versions: &HashMap<u64, Option<String>>,
) -> Vec<u8> {
    let cells = mosaic_cells(view, |id| {
        let thumbs = ctx.thumbs.clone();
        let v = versions.get(&id).cloned().flatten();
        async move {
            match v {
                Some(v) => thumbs.clip(id, Some(&v)).await,
                None => None, // default (no) thumbnail
            }
        }
    })
    .await;
    let name = view.sequence.name.clone();
    tokio::task::spawn_blocking(move || render_mosaic(&name, &cells))
        .await
        .unwrap_or_default()
}

async fn push_thumbnail(ctx: ServerCtx, seq: Uuid) {
    let notice = |t: String| {
        let _ = ctx.commands.send(EngineCommand::Notice(t));
    };
    let s = ctx.structure.borrow().clone();
    let Some(live) = s.decks.iter().find(|d| d.live) else {
        return notice("push thumbnail: not connected to Resolume".into());
    };
    let Some(view) = live.sequences.iter().find(|v| v.sequence.id == seq) else {
        return notice("push thumbnail: the sequence is not in the live deck".into());
    };
    let Some(binding) = view.binding else {
        return notice(format!(
            "push thumbnail: no {}{} clip on the {} layer",
            s.seq_prefix, view.sequence.name, s.seq_layer
        ));
    };
    let png = mosaic_png(&ctx, view, &versions(&s)).await;
    match ctx.rest.set_clip_thumbnail(binding.clip_id, png).await {
        Ok(()) => notice(format!(
            "thumbnail pushed to {}{}",
            s.seq_prefix, view.sequence.name
        )),
        Err(e) => notice(format!("push thumbnail failed: {e}")),
    }
}

// ---- thumbnails ----

/// Fetches the thumbnails of the live deck's clips into the disk cache. Resolume serves
/// thumbnails of live-deck clips only (notes §3), so a deck shown from the cache later has
/// thumbnails only if they were fetched while it was live.
async fn prefetch_live_thumbnails(ctx: ServerCtx) {
    let mut rx = ctx.structure.clone();
    loop {
        let wanted: Vec<(u64, String)> = rx
            .borrow_and_update()
            .decks
            .iter()
            .filter(|d| d.live)
            .flat_map(|d| &d.layers)
            .flat_map(|l| &l.clips)
            .filter_map(|c| Some((c.id, c.thumb.clone()?)))
            .collect();
        let mut fetched = 0;
        for (id, version) in wanted {
            if rx.has_changed().unwrap_or(true) {
                break; // deck switch or edit: start over with the new live deck
            }
            if !ctx.thumbs.is_cached(id, &version).await
                && ctx.thumbs.clip(id, Some(&version)).await.is_some()
            {
                fetched += 1;
            }
        }
        if fetched > 0 {
            log::debug!("prefetched {fetched} clip thumbnail(s) of the live deck");
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

#[derive(Deserialize)]
struct VersionQuery {
    v: Option<String>,
}

fn png_response(png: Vec<u8>, immutable: bool) -> Response {
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, cache),
        ],
        png,
    )
        .into_response()
}

async fn clip_thumb(
    State(ctx): State<ServerCtx>,
    Path(file): Path<String>,
    Query(q): Query<VersionQuery>,
) -> Response {
    let Some(id) = file
        .strip_suffix(".png")
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match ctx.thumbs.clip(id, q.v.as_deref()).await {
        Some(png) => png_response(png, q.v.is_some()),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn seq_thumb(State(ctx): State<ServerCtx>, Path(file): Path<String>) -> Response {
    let Some(id) = file
        .strip_suffix(".png")
        .and_then(|s| s.parse::<Uuid>().ok())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let s = ctx.structure.borrow().clone();
    let Some(view) = s
        .decks
        .iter()
        .flat_map(|d| &d.sequences)
        .find(|v| v.sequence.id == id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let png = mosaic_png(&ctx, view, &versions(&s)).await;
    png_response(png, false)
}

// ---- "1000A Sequence" plugin (SPEC §12) ----

#[derive(Deserialize)]
struct PluginViewQuery {
    name: Option<String>,
    rev: Option<String>,
}

async fn plugin_view(State(ctx): State<ServerCtx>, Query(q): Query<PluginViewQuery>) -> Response {
    let text = {
        let s = ctx.structure.borrow();
        let r = ctx.runtime.borrow();
        crate::plugin_view::view_text(&s, &r, q.name.as_deref().unwrap_or(""), q.rev.as_deref())
    };
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        text,
    )
        .into_response()
}

// ---- static UI ----

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path).or_else(|| Assets::get("index.html")) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_owned();
            ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "UI not built: run `npm run build` in web/",
        )
            .into_response(),
    }
}
