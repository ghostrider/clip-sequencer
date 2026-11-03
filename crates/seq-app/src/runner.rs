//! Startup as a library function (docs/M12-desktop-shell.md, Phase C): binds the UI port, starts
//! the engine, server and friends, and shuts them down when the token is cancelled. `main` runs
//! it on its own tokio runtime, in both headless and desktop mode.
//!
//! Startup reports its steps (Phase D2) to an optional channel: the desktop splash shows them,
//! headless mode prints them. Missing Resolume or MIDI only produce warnings; no step waits
//! longer than [`STEP_LIMIT`].

use std::time::Duration;

use resolume_client::ResolumeAddr;
use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::app::{self, App};
use crate::config::Config;
use crate::engine::{EngineCommand, EngineConfig, Outputs};
use crate::saver::DebounceConfig;
use crate::server::ShellAction;
use crate::storage::Storage;
use crate::ui_port::{bind_ui, BindError, UiBind};

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// Another sequencer answers on the configured UI port; no second engine was started.
    #[error("The sequencer is already running ({url}); not starting a second one")]
    AlreadyRunning { url: String },
    #[error(transparent)]
    Bind(#[from] BindError),
    #[error("The data folder {path} is not writable: {reason}")]
    DataDir { path: String, reason: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl StartError {
    /// Whether trying again can help (a port or folder that may become free or writable).
    pub fn retryable(&self) -> bool {
        matches!(self, StartError::Bind(_) | StartError::DataDir { .. })
    }
}

/// One startup step (or a warning about it) for the splash / console.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StartupStep {
    /// 1-based; `STEPS` = ready.
    pub index: u8,
    pub total: u8,
    pub text: String,
    /// Not fatal: startup goes on (shown for a moment so it can be read).
    pub warning: bool,
}

pub const STEPS: u8 = 7;
/// Longest wait of any startup step (M12 D2: "each step has a timeout of ≤3 s").
pub const STEP_LIMIT: Duration = Duration::from_secs(3);

pub type Progress = Option<mpsc::UnboundedSender<StartupStep>>;

fn report(progress: &Progress, index: u8, text: impl Into<String>, warning: bool) {
    let text = text.into();
    if warning {
        log::warn!("startup {index}/{STEPS}: {text}");
    } else {
        log::info!("startup {index}/{STEPS}: {text}");
    }
    if let Some(tx) = progress {
        let _ = tx.send(StartupStep {
            index,
            total: STEPS,
            text,
            warning,
        });
    }
}

/// Waits until `pred` holds for the watched value, at most `limit`.
async fn wait_for<T>(
    rx: &mut watch::Receiver<T>,
    limit: Duration,
    pred: impl Fn(&T) -> bool,
) -> bool {
    tokio::time::timeout(limit, rx.wait_for(|v| pred(v)))
        .await
        .is_ok_and(|r| r.is_ok())
}

pub struct RunningApp {
    /// `http://127.0.0.1:<actual port>`.
    pub ui_url: String,
    pub commands: mpsc::UnboundedSender<EngineCommand>,
    pub outputs: Outputs,
    /// Ends after the shutdown completed (token cancelled, or `--exit-when-done`).
    pub done: JoinHandle<()>,
}

/// `shell`: the desktop shell's channel for window actions from the UI; `None` when headless.
/// `progress`: receives the startup steps.
pub async fn run(
    config: Config,
    shutdown: CancellationToken,
    shell: Option<mpsc::UnboundedSender<ShellAction>>,
    progress: Progress,
) -> Result<RunningApp, StartError> {
    // 1. Configuration and data folder.
    report(&progress, 1, "Loading configuration", false);
    let data_dir = config.data_dir();
    check_writable(&data_dir).map_err(|e| StartError::DataDir {
        path: data_dir.display().to_string(),
        reason: e.to_string(),
    })?;
    // The MIDI port: --midi-port for this run, else the one picked in Preferences.
    let midi_port = config
        .midi_port
        .clone()
        .or_else(|| crate::settings::Settings::load(&data_dir).midi_port);
    let storage = Storage::new(data_dir);
    log::info!("sequence files in {}", storage.dir().display());

    // 2. UI server.
    report(&progress, 2, "Starting UI server", false);
    let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    let (listener, requested) = match bind_ui(ip, config.ui_port, config.strict_port).await? {
        UiBind::Bound {
            listener,
            requested,
        } => (listener, requested),
        UiBind::AlreadyRunning { url } => return Err(StartError::AlreadyRunning { url }),
    };
    let port = listener
        .local_addr()
        .map_err(|e| StartError::Other(e.into()))?
        .port();
    let fallback = port != requested && requested != 0;
    report(
        &progress,
        2,
        if fallback {
            format!("Starting UI server on port {port} (port {requested} is in use)")
        } else {
            format!("Starting UI server on port {port}")
        },
        fallback,
    );

    let app = app::start(app::AppConfig {
        resolume: ResolumeAddr {
            host: config.resolume_host.clone(),
            rest_port: config.rest_port,
            osc_port: config.osc_port,
        },
        storage,
        engine: EngineConfig {
            seq_layer: config.seq_layer.clone(),
            seq_prefix: config.seq_prefix.clone(),
            ..EngineConfig::default()
        },
        clock: match &midi_port {
            Some(p) => app::ClockSource::Midi(p.clone()),
            None => app::ClockSource::None,
        },
        debounce: DebounceConfig::default(),
        ui: Some(listener),
        // Headless (no shell): the browser UI may quit the app (File → Quit server).
        quit: shell.is_none().then(|| shutdown.clone()),
        shell,
    })?;
    let mut runtime = app.outputs.runtime.clone();
    let mut structure = app.outputs.structure.clone();

    // 3. MIDI clock.
    match &midi_port {
        Some(name) => {
            report(
                &progress,
                3,
                format!("Opening MIDI clock input \"{name}\""),
                false,
            );
            if !wait_for(&mut runtime, STEP_LIMIT, |r| r.clock.connected).await {
                report(
                    &progress,
                    3,
                    format!("MIDI port \"{name}\" not found, will keep retrying"),
                    true,
                );
            }
        }
        None => report(
            &progress,
            3,
            "No MIDI clock port selected (Preferences, or --midi-port): sequences will not advance",
            true,
        ),
    }

    // 4. Resolume.
    let resolume = format!("{}:{}", config.resolume_host, config.rest_port);
    report(
        &progress,
        4,
        format!("Connecting to Resolume at {resolume} (REST, WebSocket, OSC)"),
        false,
    );
    let connected = wait_for(&mut runtime, STEP_LIMIT, |r| r.ws).await;
    if !connected {
        report(
            &progress,
            4,
            format!("Resolume not reachable at {resolume}, will keep retrying"),
            true,
        );
    }

    // 5. Composition and sequences.
    report(&progress, 5, "Loading composition and sequences", false);
    if connected {
        let synced = wait_for(&mut runtime, STEP_LIMIT, |r| r.ws_synced).await;
        let loaded = wait_for(&mut structure, STEP_LIMIT, |s| s.composition.is_some()).await;
        if synced && loaded {
            let (name, sequences) = {
                let s = structure.borrow();
                let sequences: usize = s.decks.iter().map(|d| d.sequences.len()).sum();
                (s.composition.clone().unwrap_or_default(), sequences)
            };
            report(
                &progress,
                5,
                format!("Loaded composition \"{name}\" ({sequences} sequence(s))"),
                false,
            );
        } else {
            report(&progress, 5, "The composition is still loading", true);
        }
    }

    // 6. Thumbnails: the first batch; the rest continues in the background.
    if connected {
        report(&progress, 6, "Fetching thumbnails", false);
        let mut ready = app.thumbs_ready.clone();
        wait_for(&mut ready, STEP_LIMIT, |r| *r).await;
    }

    let ui_url = format!(
        "http://{}",
        app.ui_addr
            .expect("the UI server was started with a listener")
    );
    report(&progress, STEPS, "Ready", false);

    if let Some(name) = &config.start {
        let _ = app
            .commands
            .send(EngineCommand::StartByName { name: name.clone() });
    }
    let running = RunningApp {
        ui_url,
        commands: app.commands.clone(),
        outputs: Outputs {
            structure: app.outputs.structure.clone(),
            runtime: app.outputs.runtime.clone(),
        },
        done: tokio::spawn(supervise(app, shutdown, config.exit_when_done)),
    };
    Ok(running)
}

/// The data folder must exist and take a file (sequence files, caches, `runtime.json`).
fn check_writable(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(probe)
}

/// Waits for the token (or, with `--exit-when-done`, for the started sequences to end), then
/// shuts down: running sequences end per their `after_last_pass` (SPEC §7.4), the pending save is
/// flushed, WebSocket and OSC are closed and `runtime.json` is removed.
async fn supervise(app: App, shutdown: CancellationToken, exit_when_done: bool) {
    let mut runtime = app.outputs.runtime.clone();
    let mut seen_running = false;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            changed = runtime.changed() => {
                if changed.is_err() {
                    break;
                }
                let running = !runtime.borrow_and_update().running.is_empty();
                seen_running |= running;
                if exit_when_done && seen_running && !running {
                    log::info!("no sequence running any more; exiting");
                    break;
                }
            }
        }
    }
    let started = std::time::Instant::now();
    log::info!("shutting down");
    if tokio::time::timeout(SHUTDOWN_LIMIT, app.shutdown())
        .await
        .is_err()
    {
        log::warn!("shutdown took longer than {} s", SHUTDOWN_LIMIT.as_secs());
    }
    log::info!("stopped in {} ms", started.elapsed().as_millis());
}

/// The whole shutdown, save flush included (M12: "exit within ~2 s").
const SHUTDOWN_LIMIT: Duration = Duration::from_secs(2);
