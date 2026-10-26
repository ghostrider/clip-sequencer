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
    /// Where the browser UI listens; `None` = no UI server.
    pub ui: Option<std::net::SocketAddr>,
}

pub struct App {
    pub commands: mpsc::UnboundedSender<EngineCommand>,
    pub outputs: Outputs,
    pub engine_task: JoinHandle<()>,
    pub saver_task: JoinHandle<()>,
    /// Bound address of the UI server.
    pub ui_addr: Option<std::net::SocketAddr>,
    shutdown: tokio::sync::oneshot::Sender<()>,
    _clock: Option<ClockIn>,
}

impl App {
    /// Stops the engine (no clears) and waits for pending saves.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(());
        drop(self.commands);
        let _ = self.engine_task.await;
        let _ = tokio::time::timeout(Duration::from_secs(5), self.saver_task).await;
    }
}

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
        drop(ws.commands);
        ws.task.abort();
    });

    let ui_addr = match cfg.ui {
        Some(addr) => {
            let std_listener = std::net::TcpListener::bind(addr)?;
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

    Ok(App {
        ui_addr,
        commands: cmd_tx,
        outputs,
        engine_task,
        saver_task,
        shutdown: shutdown_tx,
        _clock: clock_in,
    })
}
