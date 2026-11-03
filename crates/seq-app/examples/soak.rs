//! Long-run test (M10): the real MIDI path (clock port → app → OSC) against an in-process mock
//! Resolume. An infinite sequence re-triggers the only clip of L1 every beat (24 ticks) and
//! alternates two clips of L2 every bar (96 ticks); every clip lives on its lane's layer, so OSC
//! messages per layer count the lane's triggers. Every 10 s the trigger counts are compared with what the elapsed tick count
//! requires; at the end, trigger timing jitter and drift are reported.
//!
//! Drive a clock first, e.g. through the Windows MIDI loopback:
//!   cargo run --release -p clock-in --example clock_gen -- "Default App Loopback (A)" 128 7300
//!   cargo run --release -p seq-app --example soak -- "Default App Loopback (B)" 120

use std::time::{Duration, Instant};

use mock_resolume::{start as start_mock, MockConfig};
use resolume_client::ResolumeAddr;
use seq_app::app::{self, AppConfig, ClockSource};
use seq_app::engine::{EngineCommand, EngineConfig};
use seq_app::saver::DebounceConfig;
use seq_app::storage::Storage;
use serde_json::json;

const TT: &str = "Clip 07";
const TW: &str = "Clip 13";
const LQ: &str = "Clip 08";

/// Keeps Windows from going to sleep while this process runs (a standby invalidates the run).
#[cfg(windows)]
fn keep_awake() {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };
    // SAFETY: plain Win32 call without pointers; the request ends when the process exits.
    unsafe {
        SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
    }
}

#[cfg(not(windows))]
fn keep_awake() {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    keep_awake();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = args
        .first()
        .cloned()
        .unwrap_or_else(|| "Default App Loopback (B)".into());
    let minutes: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);

    let mock = start_mock(MockConfig::default()).await?;
    mock.edit(|v| {
        v["layers"][0]["name"]["value"] = "L1".into();
        v["layers"][1]["name"]["value"] = "L2".into();
    });
    let dir = tempfile::tempdir()?;
    let file = json!({"version": 1, "composition": "dev", "decks": {"2132323": {"sequences": [{
    "id": "50a50a50-0000-0000-0000-000000000001", "name": "Soak", "start": "immediate",
    "repeat": {"mode": "infinite"}, "after_last_pass": "clear",
    "lanes": [
        {"layer": "L1", "end": "loop_to_end", "steps": [{"clip": {"name": TT}, "length": 2}]},
        {"layer": "L2", "end": "loop_to_end", "steps": [
            {"clip": {"name": LQ}, "length": 8}, {"clip": {"name": TW}, "length": 8}]}
    ]}]}}});
    std::fs::write(
        dir.path().join("dev.seq.json"),
        serde_json::to_string(&file)?,
    )?;

    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Midi(port.clone()),
        debounce: DebounceConfig::default(),
        ui: None,
        shell: None,
        quit: None,
    })?;
    let mut rt = app.outputs.runtime.clone();
    while !rt.borrow_and_update().ws_synced {
        rt.changed().await?;
    }
    // Start on the first clock tick so tick 0 and trigger 0 coincide.
    while !rt.borrow_and_update().clock.running {
        rt.changed().await?;
    }
    app.commands.send(EngineCommand::StartByName {
        name: "Soak".into(),
    })?;
    while rt.borrow_and_update().running.is_empty() {
        rt.changed().await?;
    }
    let started = Instant::now();
    println!("soak: {minutes} min on {port:?}");

    let total_ticks = |rt: &seq_app::engine::view::RuntimeState| {
        rt.running
            .first()
            .map_or(0, |r| u64::from(r.pass - 1) * r.pass_ticks + r.pos)
    };
    let connects = |layer: usize| {
        let prefix = format!("/composition/layers/{layer}/clips/");
        mock.osc_log()
            .into_iter()
            .filter(|r| r.addr.starts_with(&prefix))
            .map(|r| r.at)
            .collect::<Vec<_>>()
    };

    let mut problems = 0;
    let end = started + Duration::from_secs_f64(minutes * 60.0);
    while Instant::now() < end {
        tokio::time::sleep(
            Duration::from_secs(10).min(end.saturating_duration_since(Instant::now())),
        )
        .await;
        // Let in-flight OSC land, then compare with the tick count at the same moment.
        let state = rt.borrow().clone();
        tokio::time::sleep(Duration::from_millis(30)).await;
        let ticks = total_ticks(&state);
        let (l1, l2) = (connects(1).len() as u64, connects(2).len() as u64);
        // Triggers that must exist by `ticks` (tick 0 included); a trigger on the very next tick
        // may already have been sent, so allow one extra.
        let (e1, e2) = (ticks / 24 + 1, ticks / 96 + 1);
        let ok = (e1..=e1 + 1).contains(&l1) && (e2..=e2 + 1).contains(&l2);
        if !ok {
            problems += 1;
        }
        println!(
            "{:7.1} s  ticks {ticks:7}  bpm {:6.2}  L1 {l1}/{e1}  L2 {l2}/{e2}  clock {}  {}",
            started.elapsed().as_secs_f64(),
            state.clock.bpm.unwrap_or(0.0),
            if state.clock.running { "ok" } else { "LOST" },
            if ok { "ok" } else { "MISMATCH" },
        );
    }

    // Timing of the beat lane: intervals and drift against a straight line fit. The first trigger
    // is the start itself (when the command was processed, not on a tick), so it is left out.
    let times = connects(1)[1..].to_vec();
    let t0 = times[0];
    let xs: Vec<f64> = times
        .iter()
        .map(|t| t.duration_since(t0).as_secs_f64())
        .collect();
    let n = xs.len() as f64;
    let mean_iv = (xs[xs.len() - 1] - xs[0]) / (n - 1.0);
    let ivs: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    let (min_iv, max_iv) = ivs
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let sd = (ivs.iter().map(|x| (x - mean_iv).powi(2)).sum::<f64>() / ivs.len() as f64).sqrt();
    let max_resid = xs
        .iter()
        .enumerate()
        .map(|(k, &x)| (x - k as f64 * mean_iv).abs())
        .fold(0.0, f64::max);
    println!("---");
    println!(
        "beats triggered: {}  mean interval {:.4} ms (= {:.3} BPM)",
        times.len(),
        mean_iv * 1000.0,
        60.0 / mean_iv
    );
    println!(
        "interval min/max {:.3}/{:.3} ms, std dev {:.3} ms",
        min_iv * 1000.0,
        max_iv * 1000.0,
        sd * 1000.0
    );
    println!(
        "max deviation from a steady grid: {:.3} ms",
        max_resid * 1000.0
    );
    println!("count checks failed: {problems}");
    app.shutdown().await;
    if problems > 0 {
        anyhow::bail!("{problems} count mismatches");
    }
    Ok(())
}
