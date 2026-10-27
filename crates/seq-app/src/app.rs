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
}

pub struct App {
    pub commands: mpsc::UnboundedSender<EngineCommand>,
    pub outputs: Outputs,
    pub engine_task: JoinHandle<()>,
    pub saver_task: JoinHandle<()>,
    /// Bound address of the UI server.
    pub ui_addr: Option<std::net::SocketAddr>,
    shutdown: tokio::sync::oneshot::Sender<()>,
    /// `<data-dir>/runtime.json`, written while the UI server runs.
    runtime_file: Option<std::path::PathBuf>,
    _clock: Option<ClockIn>,
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

    let (clock_rx, clock_in, port) = match cfg.clock {
        ClockSource::Midi(port) => {
            let (tx, rx) = mpsc::unbounded_channel();
            (rx, Some(ClockIn::spawn(port.clone(), tx)), Some(port))
        }
        ClockSource::Channel(rx) => (rx, None, None),
        ClockSource::None => {
            log::warn!("no MIDI clock port configured (--midi-port): sequences will not advance");
            (mpsc::unbounded_channel().1, None, None)
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

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
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
        runtime_file,
        commands: cmd_tx,
        outputs,
        engine_task,
        saver_task,
        shutdown: shutdown_tx,
        _clock: clock_in,
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
        "started_at": utc_now_iso8601(),
    });
    std::fs::write(path, serde_json::to_vec_pretty(&body)?)
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ` (days-from-civil, Howard Hinnant).
fn utc_now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
