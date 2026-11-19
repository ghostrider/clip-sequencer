//! Measures how late the Resolume tempo clock's ticks arrive (SPEC §3.1): runs the generator at a
//! tempo for a number of seconds and prints the delay between each tick's due time and its arrival.
//!
//! `cargo run --release -p clock-in --example resolume_clock_jitter -- [bpm] [seconds]`

use std::time::{Duration, Instant};

use clock_in::{ClockEvent, ClockMsg, ResolumeClock, Transport};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let bpm: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(128.0);
    let secs: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(10);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let clock = ResolumeClock::spawn(tx);
    clock.tempo(bpm);
    clock.transport(Transport::Play, Instant::now());
    let end = Instant::now() + Duration::from_secs(secs);
    let mut delays = Vec::new();
    while Instant::now() < end {
        if let Ok(Some(ClockEvent::Msg {
            msg: ClockMsg::Tick,
            at,
        })) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await
        {
            delays.push(Instant::now().saturating_duration_since(at).as_micros() as u64);
        }
    }
    drop(clock);
    delays.sort_unstable();
    let pct = |p: f64| delays[((delays.len() - 1) as f64 * p) as usize];
    println!(
        "{bpm} BPM, {} ticks in {secs} s (expected {:.0}): delay µs p50 {} p90 {} p99 {} max {}",
        delays.len(),
        bpm * 24.0 / 60.0 * secs as f64,
        pct(0.5),
        pct(0.9),
        pct(0.99),
        delays.last().copied().unwrap_or(0)
    );
}
