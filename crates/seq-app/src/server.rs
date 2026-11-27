//! UI server (SPEC §9.6): static UI (embedded with rust-embed), one WebSocket `/ws` per browser,
//! and thumbnails under `/thumbs/...`. It is also a worker for the engine: thumbnail fetching,
//! mosaic rendering and pushing thumbnails to Resolume happen here, never on the engine task.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use resolume_client::rest::RestClient;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, watch};

use crate::logging::{self, LogRecord};
use crate::settings::{CustomTheme, Settings};
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
    /// The data folder (settings: the chosen FFGL plugin folder).
    pub data_dir: std::path::PathBuf,
    /// `--ffgl-dir` for this run.
    pub ffgl_dir: Option<std::path::PathBuf>,
    /// Set once the first batch of the live deck's thumbnails is cached (startup splash).
    pub thumbs_ready: Arc<watch::Sender<bool>>,
    /// The MIDI clock input, for the Preferences dialog (`None` in tests with a clock channel).
    pub clock: Option<Arc<crate::app::ClockControl>>,
    /// Colour theme settings (SPEC §9.7); every UI gets them on connect and on each change.
    pub appearance: Arc<watch::Sender<Appearance>>,
    /// The LUT library (SPEC §14); every UI gets it on connect and on each change.
    pub luts: Arc<crate::luts::LutLibrary>,
    /// In-app update (SPEC §15); every UI gets its state (`update`) on connect and on each change.
    pub update: Arc<crate::update_ctl::UpdateCtl>,
}

/// The UI part of the settings, as sent to every UI (`appearance` message): theme, the editor's guide lines
/// and whether debug messages are logged.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Appearance {
    pub theme: Option<String>,
    pub custom_themes: Vec<CustomTheme>,
    /// Guide lines every this many bars (SPEC §9.3); `None` = off.
    pub guide_lines: Option<u32>,
    /// Preferences → Log debug messages.
    pub debug_log: bool,
}

impl From<&Settings> for Appearance {
    fn from(s: &Settings) -> Self {
        Appearance {
            theme: s.theme.clone(),
            custom_themes: s.custom_themes.clone(),
            guide_lines: s.guide_lines,
            debug_log: s.debug_log,
        }
    }
}

/// Window actions the UI asks the desktop shell for: the page's menu bar and Ctrl+Q / the zoom keys
/// (the desktop app has no native menu). Without a shell (headless, browser UI) only `Quit` is used: it stops
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
    /// Open Resolume's plugin folder in Explorer (Install FFGL plugin dialog).
    OpenFfglDir,
    /// File → Open data folder.
    OpenDataDir,
    /// Help → User manual / Report an issue, in the system browser.
    OpenManual,
    OpenIssue,
}

pub fn router(ctx: ServerCtx) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .route("/api/version", get(api_version))
        .route("/api/licenses", get(api_licenses))
        .route("/api/ffgl", get(ffgl_status))
        .route("/api/ffgl/install", post(ffgl_install))
        .route("/api/ffgl/remove", post(ffgl_remove))
        .route("/api/ffgl/dir", post(ffgl_set_dir))
        .route("/api/logs/current", get(current_log))
        .route("/api/midi/ports", get(midi_ports))
        .route("/api/luts/table", get(lut_table))
        .route("/api/luts/cube", get(lut_cube))
        .route("/api/luts/import", post(lut_import))
        .route("/thumbs/clip/{file}", get(clip_thumb))
        .route("/thumbs/seq/{file}", get(seq_thumb))
        .route("/plugin/v1/view.txt", get(plugin_view))
        .fallback(get(static_file))
        .layer(axum::middleware::from_fn(same_origin_only))
        .with_state(ctx)
}

/// Refuses requests from other web pages (SPEC §12.2): a page open in the user's browser can reach
/// `127.0.0.1` too, and neither WebSockets nor simple POSTs are held back by the same-origin policy. A request
/// whose `Origin` names another host or port than its `Host` gets 403. Requests without `Origin` (the FFGL
/// plugin, curl, same-origin GETs) pass.
async fn same_origin_only(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let headers = req.headers();
    if let Some(origin) = headers.get(header::ORIGIN) {
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
        let origin_host = origin
            .to_str()
            .ok()
            .and_then(|o| o.split_once("://"))
            .map(|(_, rest)| rest.trim_end_matches('/'));
        if origin_host.is_none() || origin_host != host {
            log::warn!(
                "refused a request from another web page: Origin {origin:?}, Host {host:?}, {} {}",
                req.method(),
                req.uri().path()
            );
            return (
                StatusCode::FORBIDDEN,
                "cross-origin requests are not allowed",
            )
                .into_response();
        }
    }
    next.run(req).await
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

// ---- FFGL plugin installer (SPEC §12.1) ----

#[derive(serde::Serialize)]
struct FfglStatus {
    #[serde(flatten)]
    status: crate::ffgl_install::Status,
    dir_source: crate::ffgl_install::DirSource,
    /// Whether this platform can tell that Resolume runs (Windows only).
    can_detect_resolume: bool,
}

fn ffgl_status_now(ctx: &ServerCtx) -> FfglStatus {
    use crate::ffgl_install as ffgl;
    let (dir, dir_source) = ffgl::configured_dir(ctx.ffgl_dir.as_deref(), &ctx.data_dir);
    let default = ffgl::default_dir();
    FfglStatus {
        status: ffgl::status(
            ffgl::bundled_plugin().as_deref(),
            dir.as_deref(),
            default.as_deref(),
            ffgl::resolume_running(),
        ),
        dir_source,
        can_detect_resolume: cfg!(windows),
    }
}

/// Runs a blocking plugin operation (file copies, the process snapshot) off the async workers.
async fn ffgl_blocking<T: Send + 'static>(
    ctx: &ServerCtx,
    f: impl FnOnce(&ServerCtx) -> T + Send + 'static,
) -> T {
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || f(&ctx))
        .await
        .expect("ffgl task")
}

async fn ffgl_status(State(ctx): State<ServerCtx>) -> Response {
    axum::Json(ffgl_blocking(&ctx, ffgl_status_now).await).into_response()
}

#[derive(Debug, Deserialize)]
struct FfglInstallReq {
    #[serde(default)]
    allow_downgrade: bool,
}

/// `{ ok, message | error, status }`; 409 when nothing was changed (Resolume running, not our DLL, …).
fn ffgl_result(ctx: &ServerCtx, result: Result<String, String>) -> Response {
    let status = ffgl_status_now(ctx);
    match result {
        Ok(message) => {
            axum::Json(json!({ "ok": true, "message": message, "status": status })).into_response()
        }
        Err(error) => (
            StatusCode::CONFLICT,
            axum::Json(json!({ "ok": false, "error": error, "status": status })),
        )
            .into_response(),
    }
}

async fn ffgl_install(
    State(ctx): State<ServerCtx>,
    axum::Json(req): axum::Json<FfglInstallReq>,
) -> Response {
    ffgl_blocking(&ctx, move |ctx| {
        use crate::ffgl_install as ffgl;
        let (dir, _) = ffgl::configured_dir(ctx.ffgl_dir.as_deref(), &ctx.data_dir);
        let result = ffgl::install(
            ffgl::bundled_plugin().as_deref(),
            dir.as_deref(),
            true,
            req.allow_downgrade,
            ffgl::resolume_running(),
            &ffgl::RealFs,
        )
        .map(|r| {
            format!(
                "{} {} installed in {}. Restart Resolume Arena to load the plugin.",
                ffgl::PLUGIN_NAME,
                r.version,
                r.path.parent().unwrap_or(&r.path).display()
            )
        })
        .map_err(|e| e.to_string());
        ffgl_result(ctx, result)
    })
    .await
}

async fn ffgl_remove(State(ctx): State<ServerCtx>) -> Response {
    ffgl_blocking(&ctx, |ctx| {
        use crate::ffgl_install as ffgl;
        let (dir, _) = ffgl::configured_dir(ctx.ffgl_dir.as_deref(), &ctx.data_dir);
        let result = ffgl::remove(dir.as_deref(), ffgl::resolume_running(), &ffgl::RealFs)
            .map(|p| {
                format!(
                    "Removed {}. Restart Resolume Arena to unload the plugin.",
                    p.display()
                )
            })
            .map_err(|e| e.to_string());
        ffgl_result(ctx, result)
    })
    .await
}

#[derive(Debug, Deserialize)]
struct FfglDirReq {
    /// The folder; `null` or empty = back to the default.
    dir: Option<String>,
}

async fn ffgl_set_dir(
    State(ctx): State<ServerCtx>,
    axum::Json(req): axum::Json<FfglDirReq>,
) -> Response {
    ffgl_blocking(&ctx, move |ctx| {
        let dir = req
            .dir
            .map(|d| d.trim().trim_matches('"').to_owned())
            .filter(|d| !d.is_empty())
            .map(std::path::PathBuf::from);
        let result = match &dir {
            Some(d) if !d.is_absolute() => Err(format!(
                "{} is not a full path (like D:\\Resolume\\Plugins).",
                d.display()
            )),
            _ => crate::settings::Settings::update::<std::io::Error>(&ctx.data_dir, |s| {
                s.ffgl_dir = dir.clone();
                Ok(())
            })
            .map(|_| {
                log::info!(
                    "FFGL plugin folder: {}",
                    dir.as_ref()
                        .map_or("default".into(), |d| d.display().to_string())
                );
                match &ctx.ffgl_dir {
                    Some(cli) => {
                        format!("Saved. This run still uses --ffgl-dir {}.", cli.display())
                    }
                    None => "Saved.".into(),
                }
            })
            .map_err(|e| format!("Could not save the settings: {e}")),
        };
        ffgl_result(ctx, result)
    })
    .await
}

// ---- WebSocket ----

/// Browser → app (SPEC §9.6).
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum ClientMsg {
    Start {
        seq: Uuid,
    },
    /// SPEC §3.3.
    Cue {
        seq: Uuid,
        #[serde(flatten)]
        at: crate::engine::CuePoint,
    },
    CancelCue,
    /// SPEC §7.9: arm a sequence for recording (`null` disarms).
    SetArmed {
        seq: Option<Uuid>,
        /// Pattern length in bars for a sequence without steps.
        #[serde(default)]
        bars: Option<u32>,
    },
    /// SPEC §7.8.
    SetLaneMute {
        seq: Uuid,
        lane: usize,
        on: bool,
    },
    SetLaneSolo {
        seq: Uuid,
        lane: usize,
        on: bool,
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
    /// File menu.
    ReloadComposition,
    SaveNow,
    /// "Sync SEQ layer" for the live deck (SPEC §7.6).
    SyncSeqLayer {
        #[serde(default)]
        remove_orphans: bool,
    },
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
    /// Preferences: take the clock from a MIDI port or from Resolume's tempo (SPEC §3.1); saved in
    /// the settings.
    SetClockSource {
        source: crate::settings::ClockSourceKind,
    },
    /// Preferences: use this theme (`null` = the default); saved in the settings.
    SetTheme {
        id: Option<String>,
    },
    /// Preferences: add or replace a saved theme (same id), and with `select` also use it.
    SaveTheme {
        theme: CustomTheme,
        #[serde(default)]
        select: bool,
    },
    /// Preferences: remove a saved theme (the default theme is used if it was selected).
    DeleteTheme {
        id: String,
    },
    /// Help → Check for updates… (SPEC §15).
    CheckUpdate,
    /// The answer to the start-up question: `skip` = Skip this version, else Later.
    UpdateAnswer {
        skip: bool,
    },
    /// Download and install the version found by the last check.
    InstallUpdate,
    /// Preferences → Check for updates at start-up.
    SetUpdateCheck {
        on: bool,
    },
    /// View menu: editor guide lines every 8, 16 or 32 bars, or `null` = off (SPEC §9.3).
    SetGuideLines {
        every: Option<u32>,
    },
    /// Preferences → Log debug messages (applies at once).
    SetDebugLog {
        on: bool,
    },
    /// LUT tab (SPEC §14): a new LUT, a copy of `from` or one that changes nothing.
    LutCreate {
        name: String,
        #[serde(default)]
        from: Option<String>,
    },
    LutRename {
        from: String,
        to: String,
    },
    LutDelete {
        name: String,
    },
    /// New control values (sent while dragging).
    LutUpdate {
        name: String,
        grade: lut_core::Grade,
        curves: lut_core::Curves,
    },
    /// Drop the imported table under the controls.
    LutClearBase {
        name: String,
    },
    /// Point one of Resolume's LUT effects (its `LUT 3D` parameter id) at this LUT's file.
    LutApply {
        name: String,
        file_param: u64,
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
    let mut appearance = ctx.appearance.subscribe();
    let mut luts = ctx.luts.subscribe();
    let mut update = ctx.update.subscribe();
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
        msg("appearance", &*appearance.borrow_and_update()),
        msg("update", &*update.borrow_and_update()),
        {
            luts.borrow_and_update();
            msg("luts", &ctx.luts.view())
        },
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
            r = appearance.changed() => match r {
                Ok(()) => msg("appearance", &*appearance.borrow_and_update()),
                Err(_) => return,
            },
            r = update.changed() => match r {
                Ok(()) => msg("update", &*update.borrow_and_update()),
                Err(_) => return,
            },
            r = luts.changed() => match r {
                Ok(()) => {
                    luts.borrow_and_update();
                    msg("luts", &ctx.luts.view())
                }
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
                    Ok(
                        cmd @ (ClientMsg::SetTheme { .. }
                        | ClientMsg::SaveTheme { .. }
                        | ClientMsg::DeleteTheme { .. }
                        | ClientMsg::SetGuideLines { .. }
                        | ClientMsg::SetDebugLog { .. }),
                    ) => {
                        theme_command(&ctx, cmd).await;
                        continue;
                    }
                    Ok(
                        cmd @ (ClientMsg::LutCreate { .. }
                        | ClientMsg::LutRename { .. }
                        | ClientMsg::LutDelete { .. }
                        | ClientMsg::LutUpdate { .. }
                        | ClientMsg::LutClearBase { .. }
                        | ClientMsg::LutApply { .. }),
                    ) => {
                        lut_command(&ctx, cmd).await;
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
        ClientMsg::Cue { seq, at } => EngineCommand::Cue { seq, at },
        ClientMsg::CancelCue => EngineCommand::CancelCue,
        ClientMsg::SetArmed { seq, bars } => EngineCommand::SetArmed { seq, bars },
        ClientMsg::SetLaneMute { seq, lane, on } => EngineCommand::SetLaneMute { seq, lane, on },
        ClientMsg::SetLaneSolo { seq, lane, on } => EngineCommand::SetLaneSolo { seq, lane, on },
        ClientMsg::Stop { seq } => EngineCommand::Stop { seq },
        ClientMsg::StopAll => EngineCommand::StopAll,
        ClientMsg::ResumeLane { seq, lane } => EngineCommand::ResumeLane { seq, lane },
        ClientMsg::Edit { op } => EngineCommand::Edit { op },
        ClientMsg::Undo => EngineCommand::Undo,
        ClientMsg::Redo => EngineCommand::Redo,
        ClientMsg::ReloadComposition => EngineCommand::ReloadComposition,
        ClientMsg::SaveNow => EngineCommand::SaveNow,
        ClientMsg::SyncSeqLayer { remove_orphans } => {
            EngineCommand::SyncSeqLayer { remove_orphans }
        }
        ClientMsg::PushThumbnail { seq } => {
            tokio::spawn(push_thumbnail(ctx.clone(), seq));
            return;
        }
        ClientMsg::CheckUpdate => return ctx.update.check(false),
        ClientMsg::UpdateAnswer { skip } => return ctx.update.answer(skip),
        ClientMsg::InstallUpdate => return ctx.update.install(),
        ClientMsg::SetUpdateCheck { on } => return ctx.update.set_startup_check(on),
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
        ClientMsg::SetClockSource { source } => {
            let commands = ctx.commands.clone();
            match ctx.clock.clone() {
                Some(clock) => {
                    tokio::spawn(async move {
                        if let Err(e) = clock.set_source(source).await {
                            let _ = commands.send(EngineCommand::Notice(format!("clock: {e}")));
                        }
                    });
                }
                None => {
                    let _ = commands.send(EngineCommand::Notice(
                        "clock: this app's clock source cannot be changed".into(),
                    ));
                }
            }
            return;
        }
        // Per session, in order (above).
        ClientMsg::SetTheme { .. }
        | ClientMsg::SaveTheme { .. }
        | ClientMsg::DeleteTheme { .. }
        | ClientMsg::SetGuideLines { .. }
        | ClientMsg::SetDebugLog { .. }
        | ClientMsg::LutCreate { .. }
        | ClientMsg::LutRename { .. }
        | ClientMsg::LutDelete { .. }
        | ClientMsg::LutUpdate { .. }
        | ClientMsg::LutClearBase { .. }
        | ClientMsg::LutApply { .. } => return,
    };
    let _ = ctx.commands.send(engine_cmd);
}

/// LUT commands of one UI, applied in the order it sent them (the session awaits each). Baking runs off the
/// async workers; failures show as a notice.
async fn lut_command(ctx: &ServerCtx, cmd: ClientMsg) {
    let lib = ctx.luts.clone();
    let effects = ctx.structure.borrow().lut_effects.clone();
    let result = match cmd {
        ClientMsg::LutApply { name, file_param } => lib.apply(&name, file_param).await,
        cmd => tokio::task::spawn_blocking(move || match cmd {
            ClientMsg::LutCreate { name, from } => lib.create(&name, from.as_deref()),
            ClientMsg::LutRename { from, to } => lib.rename(&from, &to, &effects).map(|_| ()),
            ClientMsg::LutDelete { name } => lib.delete(&name),
            ClientMsg::LutUpdate {
                name,
                grade,
                curves,
            } => lib.update(&name, grade, curves),
            ClientMsg::LutClearBase { name } => lib.clear_base(&name),
            _ => Ok(()),
        })
        .await
        .unwrap_or(Ok(())),
    };
    if let Err(e) = result {
        log::warn!("LUT: {e}");
        let _ = ctx
            .commands
            .send(EngineCommand::Notice(format!("LUT: {e}")));
    }
}

#[derive(Deserialize)]
struct LutQuery {
    name: String,
}

/// The baked table for the preview: `size`³ little-endian f32 RGB triples, red fastest; headers `x-lut-rev` and
/// `x-lut-size`.
async fn lut_table(State(ctx): State<ServerCtx>, Query(q): Query<LutQuery>) -> Response {
    match ctx.luts.table(&q.name) {
        Some((rev, table)) => {
            let size = table.size.to_string();
            let body = tokio::task::spawn_blocking(move || table.to_f32_le_bytes())
                .await
                .unwrap_or_default();
            (
                [
                    (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
                    (header::CACHE_CONTROL, "no-store".to_owned()),
                    (
                        header::HeaderName::from_static("x-lut-rev"),
                        rev.to_string(),
                    ),
                    (header::HeaderName::from_static("x-lut-size"), size),
                ],
                body,
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Export: the LUT as a `.cube` download.
async fn lut_cube(State(ctx): State<ServerCtx>, Query(q): Query<LutQuery>) -> Response {
    let lib = ctx.luts.clone();
    let name = q.name.clone();
    match tokio::task::spawn_blocking(move || lib.cube_text(&name))
        .await
        .ok()
        .flatten()
    {
        Some(text) => (
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_owned()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}.cube\"", q.name.replace('"', "")),
                ),
            ],
            text,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Import: the body is the `.cube` text, `name` the new LUT's name. `{ ok, name }` or `{ ok: false, error }`.
async fn lut_import(
    State(ctx): State<ServerCtx>,
    Query(q): Query<LutQuery>,
    body: String,
) -> Response {
    let lib = ctx.luts.clone();
    let name = q.name.trim().to_owned();
    let n = name.clone();
    let result = tokio::task::spawn_blocking(move || lib.import(&n, &body))
        .await
        .unwrap_or(Ok(()));
    match result {
        Ok(()) => axum::Json(json!({ "ok": true, "name": name })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Theme commands of one UI, applied in the order it sent them (the session awaits each).
async fn theme_command(ctx: &ServerCtx, cmd: ClientMsg) {
    match cmd {
        ClientMsg::SetTheme { id } => {
            update_appearance(ctx, move |s| {
                if let Some(id) = &id {
                    anyhow::ensure!(
                        crate::settings::valid_theme_id(id),
                        "invalid theme id {id:?}"
                    );
                }
                s.theme = id;
                Ok(())
            })
            .await;
        }
        ClientMsg::SaveTheme { theme, select } => {
            update_appearance(ctx, move |s| {
                let theme = theme.validated().map_err(anyhow::Error::msg)?;
                let id = theme.id.clone();
                match s.custom_themes.iter_mut().find(|t| t.id == id) {
                    Some(t) => *t = theme,
                    None => {
                        anyhow::ensure!(
                            s.custom_themes.len() < crate::settings::MAX_CUSTOM_THEMES,
                            "at most {} saved themes; delete one first",
                            crate::settings::MAX_CUSTOM_THEMES
                        );
                        s.custom_themes.push(theme);
                    }
                }
                if select {
                    s.theme = Some(id);
                }
                Ok(())
            })
            .await;
        }
        ClientMsg::DeleteTheme { id } => {
            update_appearance(ctx, move |s| {
                s.custom_themes.retain(|t| t.id != id);
                if s.theme.as_deref() == Some(id.as_str()) {
                    s.theme = None;
                }
                Ok(())
            })
            .await;
        }
        ClientMsg::SetGuideLines { every } => {
            update_appearance(ctx, move |s| {
                if let Some(n) = every {
                    anyhow::ensure!(
                        crate::settings::GUIDE_LINES.contains(&n),
                        "guide lines every 8, 16 or 32 bars (got {n})"
                    );
                }
                s.guide_lines = every;
                Ok(())
            })
            .await;
        }
        ClientMsg::SetDebugLog { on } => {
            crate::logging::set_debug(on);
            update_appearance(ctx, move |s| {
                s.debug_log = on;
                Ok(())
            })
            .await;
        }
        _ => {}
    }
}

/// Changes the theme settings off the async workers (file IO), then tells every UI. Failures show
/// as a notice in the UI.
async fn update_appearance(
    ctx: &ServerCtx,
    change: impl FnOnce(&mut Settings) -> anyhow::Result<()> + Send + 'static,
) {
    let data_dir = ctx.data_dir.clone();
    let appearance = ctx.appearance.clone();
    let commands = ctx.commands.clone();
    let done = tokio::task::spawn_blocking(move || {
        match Settings::update::<anyhow::Error>(&data_dir, change) {
            Ok(saved) => {
                log::info!("theme settings saved (theme {:?})", saved.theme);
                appearance.send_replace(Appearance::from(&saved));
            }
            Err(e) => {
                log::warn!("theme settings: {e}");
                let _ = commands.send(EngineCommand::Notice(format!("Theme: {e}")));
            }
        }
    });
    let _ = done.await;
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

#[cfg(test)]
mod tests {
    use super::*;

    const SEQ: &str = "00000000-0000-0000-0000-00000000000a";

    #[test]
    fn cue_and_mute_messages_parse() {
        let p = |t: String| serde_json::from_str::<ClientMsg>(&t).unwrap();
        let seq: Uuid = SEQ.parse().unwrap();
        for (at, want) in [
            ("start_mode", crate::engine::CuePoint::StartMode),
            ("bars_8", crate::engine::CuePoint::Bars8),
            ("bars_16", crate::engine::CuePoint::Bars16),
        ] {
            let m = p(format!(r#"{{"cmd":"cue","seq":"{SEQ}","at":"{at}"}}"#));
            assert!(matches!(m, ClientMsg::Cue { seq: s, at: a } if s == seq && a == want));
        }
        let of = Uuid::from_u128(0xb);
        let m = p(format!(
            r#"{{"cmd":"cue","seq":"{SEQ}","at":"end_of_pass","of":"{of}"}}"#
        ));
        assert!(matches!(
            m,
            ClientMsg::Cue {
                at: crate::engine::CuePoint::EndOfPass { of: o },
                ..
            } if o == of
        ));
        assert!(matches!(
            p(r#"{"cmd":"cancel_cue"}"#.into()),
            ClientMsg::CancelCue
        ));
        assert!(matches!(
            p(r#"{"cmd":"set_guide_lines","every":16}"#.into()),
            ClientMsg::SetGuideLines { every: Some(16) }
        ));
        assert!(matches!(
            p(r#"{"cmd":"set_guide_lines","every":null}"#.into()),
            ClientMsg::SetGuideLines { every: None }
        ));
        assert!(matches!(
            p(r#"{"cmd":"set_debug_log","on":true}"#.into()),
            ClientMsg::SetDebugLog { on: true }
        ));
        assert!(matches!(
            p(format!(
                r#"{{"cmd":"set_lane_mute","seq":"{SEQ}","lane":1,"on":true}}"#
            )),
            ClientMsg::SetLaneMute {
                lane: 1,
                on: true,
                ..
            }
        ));
        assert!(matches!(
            p(format!(
                r#"{{"cmd":"set_lane_solo","seq":"{SEQ}","lane":0,"on":false}}"#
            )),
            ClientMsg::SetLaneSolo {
                lane: 0,
                on: false,
                ..
            }
        ));
        assert!(serde_json::from_str::<ClientMsg>(&format!(
            r#"{{"cmd":"cue","seq":"{SEQ}","at":"later"}}"#
        ))
        .is_err());
    }
}
