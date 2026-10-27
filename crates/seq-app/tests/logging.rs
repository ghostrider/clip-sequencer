//! Log capture and the Console protocol (docs/M12-desktop-shell.md, Phase E). The logger is
//! process-global, so this test binary installs it once and runs one test.

use std::time::Duration;

use clock_in::ClockEvent;
use futures_util::{SinkExt, StreamExt};
use mock_resolume::{start as start_mock, MockConfig};
use resolume_client::ResolumeAddr;
use seq_app::app::{self, AppConfig, ClockSource};
use seq_app::engine::EngineConfig;
use seq_app::logging::{self, LogConfig};
use seq_app::saver::DebounceConfig;
use seq_app::storage::Storage;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

async fn next_of(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    kind: &str,
) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .unwrap_or_else(|_| panic!("no {kind} message"))
            .unwrap()
            .unwrap();
        if let Message::Text(t) = m {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["type"] == kind {
                return v["data"].clone();
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_reach_the_ring_the_file_and_the_console() {
    let dir = tempfile::tempdir().unwrap();
    let logs_dir = dir.path().join("logs");
    logging::init(LogConfig {
        stderr: false,
        dir: Some(logs_dir.clone()),
    });
    log::info!("first line");
    log::warn!(target: "test::target", "a warning");
    let snap = logging::snapshot();
    assert!(snap
        .iter()
        .any(|r| r.message == "first line" && r.level == "INFO"));
    let w = snap.iter().find(|r| r.message == "a warning").unwrap();
    assert_eq!((w.level, w.target.as_str()), ("WARN", "test::target"));
    assert!(snap.windows(2).all(|p| p[1].seq == p[0].seq + 1));

    // The writer thread puts them into today's file.
    let file = logging::current_file().unwrap();
    assert_eq!(file.parent(), Some(logs_dir.as_path()));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if text.contains("WARN  test::target: a warning") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "file: {text:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The Console protocol over /ws.
    let mock = start_mock(MockConfig::default()).await.unwrap();
    let (_clock, clock_rx) = mpsc::unbounded_channel::<ClockEvent>();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig::default(),
        ui: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        shell: None,
    })
    .unwrap();
    let ui = app.ui_addr.unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{ui}/ws"))
        .await
        .unwrap();
    let stats = next_of(&mut ws, "log_stats").await;
    let errors_before = stats["errors"].as_u64().unwrap();

    ws.send(Message::Text(json!({"cmd": "log_subscribe"}).to_string()))
        .await
        .unwrap();
    let snapshot = next_of(&mut ws, "log_snapshot").await;
    let records = snapshot["records"].as_array().unwrap();
    assert!(records.iter().any(|r| r["message"] == "a warning"));

    log::error!(target: "test::target", "live error");
    let batch = next_of(&mut ws, "log").await;
    assert!(
        batch
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["message"] == "live error" && r["level"] == "ERROR"),
        "{batch}"
    );
    assert_eq!(logging::stats().errors, errors_before + 1);

    // Unsubscribed: no more records, but the stats still update (error badge).
    ws.send(Message::Text(json!({"cmd": "log_unsubscribe"}).to_string()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    log::error!("second error");
    // (A stats message for the first error may still be queued: wait for the new count.)
    loop {
        let stats = next_of(&mut ws, "log_stats").await;
        if stats["errors"].as_u64().unwrap() == errors_before + 2 {
            break;
        }
    }

    // "Save log…" returns today's file.
    let resp = reqwest::get(format!("http://{ui}/api/logs/current"))
        .await
        .unwrap();
    let disposition = resp.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(disposition.contains("seq-app-"), "{disposition}");
    let body = resp.text().await.unwrap();
    assert!(body.contains("live error"), "{body}");

    app.shutdown().await;
    mock.shutdown().await;
}
