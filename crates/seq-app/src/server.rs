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
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, watch};

use crate::logging::{self, LogRecord};
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
    /// The port the UI server actually listens on (after any fallback).
    pub ui_port: u16,
    /// The desktop shell, if the UI runs in its window (`ShellAction`s go there).
    pub shell: Option<mpsc::UnboundedSender<ShellAction>>,
    /// Headless only: cancelling it shuts the app down like Ctrl+C (`quit` from the browser UI).
    pub quit: Option<tokio_util::sync::CancellationToken>,
    /// Set once the first batch of the live deck's thumbnails is cached (startup splash).
    pub thumbs_ready: Arc<watch::Sender<bool>>,
    /// The MIDI clock input, for the Preferences dialog (`None` in tests with a clock channel).
    pub clock: Option<Arc<crate::app::ClockControl>>,
}

/// Window actions the UI asks the desktop shell for. Keys typed into the web content never reach
/// the native menu's shortcuts (WebView2 handles input in its own process), so the UI sends
/// Ctrl+Q and the zoom keys here. Without a shell (headless, browser UI) only `Quit` is used: it stops
/// the app when `ServerCtx::quit` is set; the rest is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellAction {
    Quit,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Open `<data-dir>/logs` in Explorer (Console tab).
    OpenLogs,
}

pub fn router(ctx: ServerCtx) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .route("/api/version", get(api_version))
        .route("/api/licenses", get(api_licenses))
        .route("/api/logs/current", get(current_log))
        .route("/api/midi/ports", get(midi_ports))
        .route("/thumbs/clip/{file}", get(clip_thumb))
        .route("/thumbs/seq/{file}", get(seq_thumb))
        .route("/plugin/v1/view.txt", get(plugin_view))
        .fallback(get(static_file))
        .with_state(ctx)
}

pub async fn serve(listener: tokio::net::TcpListener, ctx: ServerCtx) -> anyhow::Result<()> {
    log::info!("UI listening on http://{}", listener.local_addr()?);
    tokio::spawn(prefetch_live_thumbnails(ctx.clone()));
    axum::serve(listener, router(ctx)).await?;
    Ok(())
}

/// Identifies the app (also used by a second instance to find this one, `ui_port`).
async fn api_version(State(ctx): State<ServerCtx>) -> impl IntoResponse {
    axum::Json(json!({
        "app": crate::APP_ID,
        "name": crate::APP_NAME,
        "version": crate::VERSION,
        "build": crate::BUILD_NUMBER,
        "commit": crate::GIT_COMMIT,
        "build_date": crate::BUILD_DATE,
        "target": crate::TARGET,
        "copyright": crate::COPYRIGHT,
        "repository": env!("CARGO_PKG_REPOSITORY"),
        "homepage": env!("CARGO_PKG_HOMEPAGE"),
        "ui_port": ctx.ui_port,
        "desktop": ctx.shell.is_some(),
    }))
}

/// The project's MIT license (repo root `LICENSE`).
const LICENSE_TEXT: &str = include_str!("../../../LICENSE");
/// Third-party notices (`THIRD_PARTY_LICENSES.json`, see build.rs; a placeholder in dev builds).
const NOTICES_JSON: &str = include_str!("../../../THIRD_PARTY_LICENSES.json");

/// License dialog (M11 Phase F): `{ "license": "<MIT text>", "notices": { packages, texts, note, … } }`.
async fn api_licenses() -> Response {
    let license = serde_json::to_string(LICENSE_TEXT).expect("string serialises");
    (
        [(header::CONTENT_TYPE, "application/json")],
        format!("{{\"license\":{license},\"notices\":{NOTICES_JSON}}}"),
    )
        .into_response()
}

// ---- WebSocket ----

/// Browser → app (SPEC §9.6).
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum ClientMsg {
    Start {
        seq: Uuid,
    },
    Stop {
        seq: Uuid,
    },
    StopAll,
    ResumeLane {
        seq: Uuid,
        lane: usize,
    },
    Edit {
        op: EditOp,
    },
    Undo,
    Redo,
    /// File menu of the browser UI (the desktop app's native menu sends these directly).
    ReloadComposition,
    SaveNow,
    PushThumbnail {
        seq: Uuid,
    },
    Shell {
        action: ShellAction,
    },
    /// Console tab opened: snapshot, then live records (M12 E).
    LogSubscribe,
    LogUnsubscribe,
    /// Preferences: listen to this MIDI port (`null` = none); saved in the settings.
    SetMidiPort {
        port: Option<String>,
    },
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
    // Log records go out only while this UI shows its Console tab; the stats (error count for
    // the tab's badge, dropped records) always.
    let mut logs: Option<broadcast::Receiver<LogRecord>> = None;
    let mut batch: Vec<LogRecord> = Vec::new();
    let mut stats = logging::stats();
    let mut log_tick = tokio::time::interval(LOG_BATCH);
    log_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let first = [
        msg("structure", &*structure.borrow_and_update()),
        msg("runtime", &*runtime.borrow_and_update()),
        msg("log_stats", &stats),
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
                let Message::Text(text) = m else { continue };
                match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(ClientMsg::LogSubscribe) => {
                        logs = logging::subscribe(); // first: the snapshot may overlap, never miss
                        batch.clear();
                        let snapshot = json!({"records": logging::snapshot(), "stats": logging::stats()});
                        msg("log_snapshot", &snapshot)
                    }
                    Ok(ClientMsg::LogUnsubscribe) => {
                        logs = None;
                        batch.clear();
                        continue;
                    }
                    _ => {
                        handle(&ctx, &text);
                        continue;
                    }
                }
            }
            record = next_log(&mut logs) => {
                match record {
                    Ok(r) => batch.push(r),
                    Err(broadcast::error::RecvError::Lagged(n)) => logging::note_dropped(n),
                    Err(broadcast::error::RecvError::Closed) => logs = None,
                }
                continue;
            }
            _ = log_tick.tick() => {
                let now = logging::stats();
                if !batch.is_empty() {
                    msg("log", &std::mem::take(&mut batch))
                } else if now != stats {
                    stats = now;
                    msg("log_stats", &stats)
                } else {
                    continue;
                }
            }
        };
        if socket.send(out).await.is_err() {
            return;
        }
    }
}

/// Log records are sent in batches at most this often (M12 E).
const LOG_BATCH: Duration = Duration::from_millis(100);

async fn next_log(
    logs: &mut Option<broadcast::Receiver<LogRecord>>,
) -> Result<LogRecord, broadcast::error::RecvError> {
    match logs {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Preferences: the MIDI inputs on this machine and the port the clock listens to.
async fn midi_ports(State(ctx): State<ServerCtx>) -> Response {
    let ports = tokio::task::spawn_blocking(clock_in::list_ports).await;
    let selected = ctx.runtime.borrow().clock.port.clone();
    match ports {
        Ok(Ok(ports)) => axum::Json(json!({"ports": ports, "selected": selected})).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// "Save log…": today's log file, or the in-memory buffer when there is none.
async fn current_log() -> Response {
    let file = logging::current_file();
    let (name, body) = match &file {
        Some(path) if path.is_file() => (
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "seq-app.log".into()),
            tokio::fs::read(path).await.unwrap_or_default(),
        ),
        _ => (
            "seq-app.log".to_owned(),
            logging::snapshot()
                .iter()
                .map(|r| r.line() + "\n")
                .collect::<String>()
                .into_bytes(),
        ),
    };
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        body,
    )
        .into_response()
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
        ClientMsg::ReloadComposition => EngineCommand::ReloadComposition,
        ClientMsg::SaveNow => EngineCommand::SaveNow,
        ClientMsg::PushThumbnail { seq } => {
            tokio::spawn(push_thumbnail(ctx.clone(), seq));
            return;
        }
        ClientMsg::Shell { action } => {
            if let Some(shell) = &ctx.shell {
                let _ = shell.send(action);
            } else if let (ShellAction::Quit, Some(quit)) = (action, &ctx.quit) {
                // Browser UI of the headless app: File → Quit server (confirmed in the page).
                log::info!("quit requested from the UI");
                quit.cancel();
            }
            return;
        }
        ClientMsg::LogSubscribe | ClientMsg::LogUnsubscribe => return, // per session, above
        ClientMsg::SetMidiPort { port } => {
            let commands = ctx.commands.clone();
            match ctx.clock.clone() {
                Some(clock) => {
                    tokio::spawn(async move {
                        if let Err(e) = clock.set_port(port).await {
                            let _ =
                                commands.send(EngineCommand::Notice(format!("MIDI clock: {e}")));
                        }
                    });
                }
                None => {
                    let _ = commands.send(EngineCommand::Notice(
                        "MIDI clock: this app has no MIDI input to change".into(),
                    ));
                }
            }
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
/// Startup waits for this many thumbnails at most; the rest load in the background.
const FIRST_THUMBNAILS: usize = 12;

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
        for (i, (id, version)) in wanted.into_iter().enumerate() {
            if i == FIRST_THUMBNAILS {
                ctx.thumbs_ready.send_replace(true);
            }
            if rx.has_changed().unwrap_or(true) {
                break; // deck switch or edit: start over with the new live deck
            }
            if !ctx.thumbs.is_cached(id, &version).await
                && ctx.thumbs.clip(id, Some(&version)).await.is_some()
            {
                fetched += 1;
            }
        }
        ctx.thumbs_ready.send_replace(true);
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

/// Only this server's own origin (M12 Phase D): no external scripts, styles, fonts or requests.
/// `'unsafe-inline'` styles: Svelte sets `style:` attributes. `'self'` covers the `/ws` socket.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                   img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; \
                   object-src 'none'; base-uri 'self'; form-action 'none'; frame-ancestors 'none'";

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path).or_else(|| Assets::get("index.html")) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_owned();
            (
                [
                    (header::CONTENT_TYPE, mime.as_str()),
                    (header::CONTENT_SECURITY_POLICY, CSP),
                ],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "UI not built: run `npm run build` in web/",
        )
            .into_response(),
    }
}
