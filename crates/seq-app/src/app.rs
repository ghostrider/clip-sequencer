//! Wiring: starts the saver, the Resolume WebSocket task, the OSC sender, the clock input, the
//! REST health check and the engine task, and connects their channels.

use std::time::Duration;

use clock_in::{ClockEvent, ClockIn};
use resolume_client::osc::OscSender;
use resolume_client::rest::RestClient;
use resolume_client::{ws, ResolumeAddr};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::engine::{self, Aux, Engine, EngineCommand, EngineConfig, Outputs};
use crate::saver::{self, DebounceConfig, SaveEvent};
use crate::storage::Storage;

pub enum ClockSource {
    /// MIDI input port name (substring).
    Midi(String),
    /// Events from elsewhere (tests, simulations).
    Channel(mpsc::UnboundedReceiver<ClockEvent>),
    None,
}

pub struct AppConfig {
    pub resolume: ResolumeAddr,
    pub storage: Storage,
    pub engine: EngineConfig,
    pub clock: ClockSource,
    pub debounce: DebounceConfig,
    /// The bound UI server socket (see `ui_port::bind_ui`); `None` = no UI server.
    pub ui: Option<std::net::TcpListener>,
    /// The desktop shell's channel for window actions from the UI (`None` = browser only).
    pub shell: Option<mpsc::UnboundedSender<crate::server::ShellAction>>,
    /// Headless: the run's shutdown token, so the browser UI can quit the app (`None` = not allowed).
    pub quit: Option<tokio_util::sync::CancellationToken>,
    /// `--ffgl-dir`: Resolume's plugin folder for this run (FFGL plugin installer, SPEC §12.1).
    pub ffgl_dir: Option<std::path::PathBuf>,
}

/// Owns the MIDI clock input and switches it to another port at runtime (Preferences dialog).
pub struct ClockControl {
    tx: mpsc::UnboundedSender<ClockEvent>,
    input: std::sync::Mutex<Option<ClockIn>>,
    commands: mpsc::UnboundedSender<EngineCommand>,
    data_dir: std::path::PathBuf,
}

impl ClockControl {
    /// Listens to `port` from now on (`None` = no clock input), tells the engine and saves the
    /// choice in the settings.
    pub async fn set_port(self: &std::sync::Arc<Self>, port: Option<String>) -> Result<(), String> {
        let port = port.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
        let this = self.clone();
        let chosen = port.clone();
        tokio::task::spawn_blocking(move || {
            // Dropping the old input joins its thread: keep that off the async workers.
            let old = this.input.lock().unwrap().take();
            drop(old);
            let new = chosen.clone().map(|p| ClockIn::spawn(p, this.tx.clone()));
            *this.input.lock().unwrap() = new;
            let mut settings = crate::settings::Settings::load(&this.data_dir);
            settings.midi_port = chosen;
            settings.save(&this.data_dir)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("saving the settings failed: {e}"))?;
        log::info!("MIDI clock port set to {port:?}");
        let _ = self.commands.send(EngineCommand::ClockPortChanged { port });
        Ok(())
    }
}

pub struct App {
    pub commands: mpsc::UnboundedSender<EngineCommand>,
    pub outputs: Outputs,
    pub engine_task: JoinHandle<()>,
    pub saver_task: JoinHandle<()>,
    /// Bound address of the UI server.
    pub ui_addr: Option<std::net::SocketAddr>,
    /// True once the first batch of live-deck thumbnails is cached (never without a UI server).
    pub thumbs_ready: tokio::sync::watch::Receiver<bool>,
    shutdown: tokio::sync::oneshot::Sender<()>,
    /// `<data-dir>/runtime.json`, written while the UI server runs.
    runtime_file: Option<std::path::PathBuf>,
    /// The MIDI clock input (`None` with `ClockSource::Channel`).
    pub clock: Option<std::sync::Arc<ClockControl>>,
}

impl App {
    /// Stops the engine (running sequences end per their `after_last_pass`, SPEC §7.4), closes
    /// the Resolume connections and waits for the pending save.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(());
        drop(self.commands);
        let _ = self.engine_task.await;
        let _ = tokio::time::timeout(SAVE_FLUSH_LIMIT, self.saver_task).await;
        if let Some(path) = self.runtime_file {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Shutdown budget (M12 Phase C: the whole shutdown within ~2 s).
const WS_CLOSE_LIMIT: Duration = Duration::from_millis(300);
const SAVE_FLUSH_LIMIT: Duration = Duration::from_millis(1500);

pub fn start(cfg: AppConfig) -> anyhow::Result<App> {
    let (aux_tx, aux_rx) = mpsc::unbounded_channel();

    let (save_tx, mut save_rx) = mpsc::unbounded_channel();
    let (saver_handle, saver_task) = saver::spawn(cfg.storage.clone(), cfg.debounce, Some(save_tx));
    {
        let aux = aux_tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = save_rx.recv().await {
                let _ = aux.send(match ev {
                    SaveEvent::Saved(_) => Aux::Saved,
                    SaveEvent::Failed(e) => Aux::SaveFailed(e),
                });
            }
        });
    }

    let ws = ws::spawn(cfg.resolume.ws_url());
    let osc = OscSender::new(&cfg.resolume.osc_target())?;

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (clock_rx, clock, port) = match cfg.clock {
        ClockSource::Channel(rx) => (rx, None, None),
        source => {
            let (tx, rx) = mpsc::unbounded_channel();
            let port = match source {
                ClockSource::Midi(port) => Some(port),
                _ => {
                    log::warn!("no MIDI clock port selected (Preferences, or --midi-port): sequences will not advance");
                    None
                }
            };
            let input = port.clone().map(|p| ClockIn::spawn(p, tx.clone()));
            let control = ClockControl {
                tx,
                input: std::sync::Mutex::new(input),
                commands: cmd_tx.clone(),
                data_dir: cfg.storage.dir().to_path_buf(),
            };
            (rx, Some(std::sync::Arc::new(control)), port)
        }
    };

    {
        let rest = RestClient::new(cfg.resolume.rest_base());
        let aux = aux_tx.clone();
        tokio::spawn(async move {
            loop {
                let ok = rest.product().await.is_ok();
                if aux.send(Aux::RestHealth(ok)).is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
    }

    let slot_aux = aux_tx.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let thumbs_dir = cfg.storage.dir().join("thumbs");
    let data_dir = cfg.storage.dir().to_path_buf();
    let (mut engine, outputs) = Engine::new(
        cfg.engine,
        osc,
        ws.commands.clone(),
        Some(saver_handle),
        cfg.storage,
        aux_tx,
    );
    engine.set_clock_port(port);
    engine.set_slot_reader(engine::spawn_slot_reader(
        RestClient::new(cfg.resolume.rest_base()),
        slot_aux,
    ));
    let ws_events = ws.events;
    let engine_task = tokio::spawn(async move {
        engine::run(
            engine,
            engine::Inputs {
                clock: clock_rx,
                ws: ws_events,
                commands: cmd_rx,
                aux: aux_rx,
                shutdown: shutdown_rx,
            },
        )
        .await;
        // Without commands the WS task sends a Close frame and ends; don't wait for a WS task
        // that is between reconnect attempts.
        drop(ws.commands);
        let mut ws_task = ws.task;
        if tokio::time::timeout(WS_CLOSE_LIMIT, &mut ws_task)
            .await
            .is_err()
        {
            ws_task.abort();
        }
    });

    let (thumbs_ready_tx, thumbs_ready) = tokio::sync::watch::channel(false);
    let ui_addr = match cfg.ui {
        Some(std_listener) => {
            std_listener.set_nonblocking(true)?;
            let bound = std_listener.local_addr()?;
            let listener = tokio::net::TcpListener::from_std(std_listener)?;
            let rest = RestClient::new(cfg.resolume.rest_base());
            let ctx = crate::server::ServerCtx {
                commands: cmd_tx.clone(),
                structure: outputs.structure.clone(),
                runtime: outputs.runtime.clone(),
                thumbs: std::sync::Arc::new(crate::thumbs::ThumbCache::new(
                    thumbs_dir,
                    rest.clone(),
                )),
                rest,
                ui_port: bound.port(),
                shell: cfg.shell.clone(),
                quit: cfg.quit.clone(),
                data_dir: data_dir.clone(),
                ffgl_dir: cfg.ffgl_dir.clone(),
                thumbs_ready: std::sync::Arc::new(thumbs_ready_tx),
                clock: clock.clone(),
            };
            tokio::spawn(async move {
                if let Err(e) = crate::server::serve(listener, ctx).await {
                    log::error!("UI server: {e}");
                }
            });
            Some(bound)
        }
        None => None,
    };

    let runtime_file = ui_addr.map(|addr| {
        let path = data_dir.join("runtime.json");
        if let Err(e) = write_runtime_file(&path, addr) {
            log::warn!("{}: {e}", path.display());
        }
        path
    });

    Ok(App {
        ui_addr,
        thumbs_ready,
        runtime_file,
        commands: cmd_tx,
        outputs,
        engine_task,
        saver_task,
        shutdown: shutdown_tx,
        clock,
    })
}

/// `{ "pid", "ui_url", "started_at" }`, so other tools can find the running instance.
fn write_runtime_file(path: &std::path::Path, addr: std::net::SocketAddr) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::json!({
        "pid": std::process::id(),
        "ui_url": format!("http://{addr}"),
        "started_at": crate::logging::utc_iso8601(std::time::SystemTime::now(), false),
    });
    std::fs::write(path, serde_json::to_vec_pretty(&body)?)
}
