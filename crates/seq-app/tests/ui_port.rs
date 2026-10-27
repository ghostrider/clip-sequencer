//! UI port selection and the single-instance check (docs/M12-desktop-shell.md, Phase B).

use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::process::Command;
use std::time::{Duration, Instant};

use seq_app::ui_port::{bind_ui, BindError, UiBind, FALLBACK_PORTS};

const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// A base port whose whole fallback range is free right now (tests run in parallel, so each
/// test gets its own range).
fn free_range() -> u16 {
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    let offset = NEXT.fetch_add(20, std::sync::atomic::Ordering::SeqCst);
    let mut base = 20_000 + (std::process::id() % 2_000) as u16 * 20 + offset;
    loop {
        let all_free =
            (0..=FALLBACK_PORTS).all(|i| TcpListener::bind((LOCALHOST, base + i)).is_ok());
        if all_free {
            return base;
        }
        base += 20;
    }
}

fn port_of(bind: &UiBind) -> u16 {
    match bind {
        UiBind::Bound { listener, .. } => listener.local_addr().unwrap().port(),
        other => panic!("{other:?}"),
    }
}

/// A sequencer stand-in that answers `/api/version` like the real app.
async fn fake_sequencer(listener: TcpListener) {
    listener.set_nonblocking(true).unwrap();
    let app = axum::Router::new().route(
        "/api/version",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!({"app": seq_app::APP_ID, "name": seq_app::APP_NAME}))
        }),
    );
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
}

#[tokio::test]
async fn free_port_is_used_as_configured() {
    let base = free_range();
    let bind = bind_ui(LOCALHOST, base, false).await.unwrap();
    assert_eq!(port_of(&bind), base);
}

#[tokio::test]
async fn taken_port_falls_back_to_the_next_one() {
    let base = free_range();
    let _other_program = TcpListener::bind((LOCALHOST, base)).unwrap();
    let bind = bind_ui(LOCALHOST, base, false).await.unwrap();
    assert_eq!(port_of(&bind), base + 1);
    assert!(matches!(bind, UiBind::Bound { requested, .. } if requested == base));
}

#[tokio::test]
async fn whole_range_taken_falls_back_to_an_os_assigned_port() {
    let base = free_range();
    let _taken: Vec<_> = (0..=FALLBACK_PORTS)
        .map(|i| TcpListener::bind((LOCALHOST, base + i)).unwrap())
        .collect();
    let port = port_of(&bind_ui(LOCALHOST, base, false).await.unwrap());
    assert!(port != 0 && !(base..=base + FALLBACK_PORTS).contains(&port));
}

#[tokio::test]
async fn running_sequencer_is_detected_instead_of_falling_back() {
    let base = free_range();
    fake_sequencer(TcpListener::bind((LOCALHOST, base)).unwrap()).await;
    match bind_ui(LOCALHOST, base, false).await.unwrap() {
        UiBind::AlreadyRunning { url } => assert_eq!(url, format!("http://127.0.0.1:{base}")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn strict_port_fails_instead_of_falling_back() {
    let base = free_range();
    let _other_program = TcpListener::bind((LOCALHOST, base)).unwrap();
    let started = Instant::now();
    match bind_ui(LOCALHOST, base, true).await {
        Err(BindError::InUse(p)) => assert_eq!(p, base),
        other => panic!("{other:?}"),
    }
    // The single-instance probe of a silent port gives up quickly.
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// Runs the real binary with its own data dir and no Resolume, and returns (success, stderr).
fn run_app(ui_port: u16, extra: &[&str]) -> (bool, String) {
    let dir = tempfile::tempdir().unwrap();
    let dead = free_range(); // nothing listens there: Resolume "not running"
    let mut child = Command::new(env!("CARGO_BIN_EXE_seq-app"))
        .args([
            "--ui-port",
            &ui_port.to_string(),
            "--rest-port",
            &dead.to_string(),
        ])
        .args(["--osc-port", &(dead + 1).to_string(), "--data-dir"])
        .arg(dir.path())
        .args(extra)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err).unwrap();
            return (status.success(), err);
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("seq-app did not exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn second_instance_exits_without_starting_an_engine() {
    let base = free_range();
    fake_sequencer(TcpListener::bind((LOCALHOST, base)).unwrap()).await;
    let (ok, err) = tokio::task::spawn_blocking(move || run_app(base, &[]))
        .await
        .unwrap();
    assert!(!ok, "must exit with an error");
    assert!(err.contains("already running"), "stderr: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn strict_port_taken_is_a_clean_error_exit() {
    let base = free_range();
    let _other_program = TcpListener::bind((LOCALHOST, base)).unwrap();
    let (ok, err) = tokio::task::spawn_blocking(move || run_app(base, &["--strict-port"]))
        .await
        .unwrap();
    assert!(!ok);
    assert!(
        err.contains(&format!("Port {base} is in use")),
        "stderr: {err}"
    );
}
