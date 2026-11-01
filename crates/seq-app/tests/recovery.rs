//! Recovery without restarting the sequencer (M10): Resolume quits and comes back, another
//! composition is opened, the clock stops and resumes.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use clock_in::{ClockEvent, ClockMsg};
use mock_resolume::{start as start_mock, Mock, MockConfig};
use resolume_client::ResolumeAddr;
use seq_app::app::{self, App, AppConfig, ClockSource};
use seq_app::engine::view::{RuntimeState, StructureState};
use seq_app::engine::{EngineCommand, EngineConfig};
use seq_app::saver::DebounceConfig;
use seq_app::storage::Storage;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

const TWISTED: &str = "Clip 13";
const LIQUID: &str = "Clip 08";
const LOOP: Uuid = Uuid::from_u128(0x100);
const OTHER: Uuid = Uuid::from_u128(0x200);

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn mock_at(rest: u16, osc: u16) -> Mock {
    let m = start_mock(MockConfig {
        rest_addr: SocketAddr::from(([127, 0, 0, 1], rest)),
        osc_addr: SocketAddr::from(([127, 0, 0, 1], osc)),
        ..MockConfig::default()
    })
    .await
    .unwrap();
    m.edit(|v| v["layers"][1]["name"]["value"] = "L2".into());
    m
}

fn seq_file(composition: &str, id: Uuid, name: &str) -> String {
    json!({"version": 1, "composition": composition, "decks": {"2132323": {"sequences": [{
        "id": id, "name": name, "start": "immediate", "repeat": {"mode": "infinite"},
        "after_last_pass": "clear",
        "lanes": [{"layer": "L2", "end": "loop_to_end", "steps": [
            {"clip": {"name": TWISTED}, "length": 1}, {"clip": {"name": LIQUID}, "length": 1}]}]
    }]}}})
    .to_string()
}

struct H {
    app: App,
    clock: mpsc::UnboundedSender<ClockEvent>,
    _dir: tempfile::TempDir,
}

fn start_app(rest: u16, osc: u16) -> H {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("dev.seq.json"),
        seq_file("dev", LOOP, "Loop"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("other.seq.json"),
        seq_file("other", OTHER, "Other"),
    )
    .unwrap();
    let (clock, clock_rx) = mpsc::unbounded_channel();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: rest,
            osc_port: osc,
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig {
            quiet: Duration::from_millis(50),
            max_wait: Duration::from_millis(200),
        },
        ui: None,
        shell: None,
    })
    .unwrap();
    H {
        app,
        clock,
        _dir: dir,
    }
}

impl H {
    async fn runtime(&self, what: &str, pred: impl Fn(&RuntimeState) -> bool) -> RuntimeState {
        let mut rx = self.app.outputs.runtime.clone();
        // 10 s: after Resolume closes the connection the client waits 5 s before reconnecting.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                {
                    let s = rx.borrow_and_update();
                    if pred(&s) {
                        return s.clone();
                    }
                }
                rx.changed().await.unwrap();
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "runtime never reached: {what}; last: {:?}",
                self.app.outputs.runtime.borrow()
            )
        })
    }

    async fn structure(
        &self,
        what: &str,
        pred: impl Fn(&StructureState) -> bool,
    ) -> StructureState {
        let mut rx = self.app.outputs.structure.clone();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                {
                    let s = rx.borrow_and_update();
                    if pred(&s) {
                        return s.clone();
                    }
                }
                rx.changed().await.unwrap();
            }
        })
        .await
        .unwrap_or_else(|_| panic!("structure never reached: {what}"))
    }

    fn ticks(&self, n: u64) {
        for _ in 0..n {
            self.tick_at(Instant::now());
        }
    }

    fn tick_at(&self, at: Instant) {
        self.clock
            .send(ClockEvent::Msg {
                msg: ClockMsg::Tick,
                at,
            })
            .unwrap();
    }
}

/// Ticks since the start, across passes.
fn total(r: &RuntimeState) -> u64 {
    r.running
        .first()
        .map_or(0, |s| u64::from(s.pass - 1) * s.pass_ticks + s.pos)
}

async fn osc_count(m: &Mock, n: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(3);
    while m.osc_log().len() < n && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    m.osc_log().len()
}

#[tokio::test]
async fn resolume_restart_reconnects_and_running_sequences_keep_going() {
    let (rest, osc) = (free_port(), free_udp_port());
    let mock = mock_at(rest, osc).await;
    let h = start_app(rest, osc);
    h.runtime("synced", |r| r.ws_synced).await;
    h.structure("model loaded", |s| {
        s.decks.iter().any(|d| !d.sequences.is_empty())
    })
    .await;
    h.app
        .commands
        .send(EngineCommand::Start { seq: LOOP })
        .unwrap();
    h.runtime("running", |r| r.running.len() == 1).await;
    assert_eq!(osc_count(&mock, 1).await, 1);

    // Resolume quits.
    mock.shutdown().await;
    h.runtime("ws down", |r| !r.ws).await;
    // The sequence keeps counting and sending; nobody confirms.
    h.ticks(12);
    let r = h
        .runtime("not confirmed", |r| {
            r.running
                .first()
                .is_some_and(|s| !s.lanes[0].not_confirmed.is_empty())
        })
        .await;
    assert_eq!(r.running[0].pos, 12);

    // Resolume comes back on the same ports: reconnect, resubscribe, confirmations again.
    let mock = mock_at(rest, osc).await;
    let r = h
        .runtime("ws back and synced", |r| r.ws && r.ws_synced)
        .await;
    let before = r.confirmations;
    h.ticks(12);
    assert_eq!(
        osc_count(&mock, 1).await,
        1,
        "trigger reaches the new Resolume"
    );
    h.runtime("confirmed after restart", |r| {
        r.confirmations > before && r.running.len() == 1
    })
    .await;
    assert_eq!(mock.clip_state(2, 1).as_deref(), Some("Connected"));
    h.app.shutdown().await;
    mock.shutdown().await;
}

#[tokio::test]
async fn opening_another_composition_switches_sequence_files() {
    let (rest, osc) = (free_port(), free_udp_port());
    let mock = mock_at(rest, osc).await;
    let h = start_app(rest, osc);
    h.runtime("synced", |r| r.ws_synced).await;
    h.structure("dev loaded", |s| {
        s.composition.as_deref() == Some("dev")
            && s.decks
                .iter()
                .any(|d| d.sequences.iter().any(|v| v.sequence.id == LOOP))
    })
    .await;
    h.app
        .commands
        .send(EngineCommand::Start { seq: LOOP })
        .unwrap();
    h.runtime("running", |r| r.running.len() == 1).await;

    mock.edit(|v| v["name"]["value"] = "other".into());
    h.structure("other loaded", |s| {
        s.composition.as_deref() == Some("other")
            && s.decks
                .iter()
                .any(|d| d.sequences.iter().any(|v| v.sequence.id == OTHER))
    })
    .await;
    h.runtime("stopped on composition change", |r| r.running.is_empty())
        .await;

    mock.edit(|v| v["name"]["value"] = "dev".into());
    h.structure("dev again", |s| {
        s.decks
            .iter()
            .any(|d| d.sequences.iter().any(|v| v.sequence.id == LOOP))
    })
    .await;
    h.app.shutdown().await;
    mock.shutdown().await;
}

#[tokio::test]
async fn clock_loss_freezes_and_ticks_resume_where_it_stopped() {
    let (rest, osc) = (free_port(), free_udp_port());
    let mock = mock_at(rest, osc).await;
    let h = start_app(rest, osc);
    h.runtime("synced", |r| r.ws_synced).await;
    h.structure("model", |s| s.decks.iter().any(|d| !d.sequences.is_empty()))
        .await;
    h.app
        .commands
        .send(EngineCommand::Start { seq: LOOP })
        .unwrap();
    h.runtime("running", |r| r.running.len() == 1).await;

    // 30 ticks at 120 BPM (20.8 ms apart), the last one now.
    let iv = Duration::from_secs_f64(60.0 / (120.0 * 24.0));
    let now = Instant::now();
    for i in (0..30u32).rev() {
        h.tick_at(now - iv * i);
    }
    let r = h
        .runtime("clock running", |r| r.clock.bpm.is_some() && total(r) == 30)
        .await;
    assert!(
        (r.clock.bpm.unwrap() - 120.0).abs() < 1.0,
        "{:?}",
        r.clock.bpm
    );
    // No tick for more than 2 beats (1 s at 120 BPM): frozen.
    let r = h.runtime("no clock", |r| !r.clock.running).await;
    assert!(r.notices.iter().any(|n| n.contains("no clock")));
    assert_eq!(total(&r), 30, "position frozen");
    h.ticks(6);
    let r = h
        .runtime("resumed", |r| r.clock.running && total(r) == 36)
        .await;
    assert_eq!(r.running.len(), 1);
    h.app.shutdown().await;
    mock.shutdown().await;
}
