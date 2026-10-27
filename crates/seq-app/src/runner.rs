//! Startup as a library function (docs/M12-desktop-shell.md, Phase C): binds the UI port, starts
//! the engine, server and friends, and shuts them down when the token is cancelled. `main` runs
//! it on its own tokio runtime, in both headless and desktop mode.

use std::time::Duration;

use resolume_client::ResolumeAddr;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::app::{self, App};
use crate::config::Config;
use crate::engine::{EngineCommand, EngineConfig, Outputs};
use crate::saver::DebounceConfig;
use crate::storage::Storage;
use crate::ui_port::{bind_ui, BindError, UiBind};

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// Another sequencer answers on the configured UI port; no second engine was started.
    #[error("The sequencer is already running ({url}); not starting a second one")]
    AlreadyRunning { url: String },
    #[error(transparent)]
    Bind(#[from] BindError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct RunningApp {
    /// `http://127.0.0.1:<actual port>`.
    pub ui_url: String,
    pub commands: tokio::sync::mpsc::UnboundedSender<EngineCommand>,
    pub outputs: Outputs,
    /// Ends after the shutdown completed (token cancelled, or `--exit-when-done`).
    pub done: JoinHandle<()>,
}

pub async fn run(config: Config, shutdown: CancellationToken) -> Result<RunningApp, StartError> {
    let storage = Storage::new(config.data_dir());
    log::info!("sequence files in {}", storage.dir().display());
    let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    let listener = match bind_ui(ip, config.ui_port, config.strict_port).await? {
        UiBind::Bound { listener, .. } => listener,
        UiBind::AlreadyRunning { url } => return Err(StartError::AlreadyRunning { url }),
    };
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
        clock: match &config.midi_port {
            Some(p) => app::ClockSource::Midi(p.clone()),
            None => app::ClockSource::None,
        },
        debounce: DebounceConfig::default(),
        ui: Some(listener),
    })?;
    let ui_url = format!(
        "http://{}",
        app.ui_addr
            .expect("the UI server was started with a listener")
    );
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
