//! `seq-app --clock-debug`: prints the incoming MIDI clock live, to check the port and tempo
//! before a show. Position counts from the first tick (or the last Start message).

use std::time::{Duration, Instant};

use clock_in::{ClockEvent, ClockIn, ClockMsg, TempoEstimator};
use seq_core::scheduler::{TICKS_PER_BAR, TICKS_PER_BEAT};
use tokio::sync::mpsc;

pub async fn run(port: String) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _clock = ClockIn::spawn(port.clone(), tx);
    println!("waiting for MIDI clock on {port:?} (Ctrl+C to quit)");
    let mut tempo = TempoEstimator::new();
    let mut ticks: u64 = 0;
    let mut lost = true;
    let mut check = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                None => return Ok(()),
                Some(ClockEvent::Connected { port }) => println!("connected to {port:?}"),
                Some(ClockEvent::Disconnected { reason }) => println!("disconnected: {reason}"),
                Some(ClockEvent::Msg { msg: ClockMsg::Tick, at }) => {
                    tempo.on_tick(at);
                    if lost {
                        println!("clock running");
                        lost = false;
                    }
                    if ticks % TICKS_PER_BEAT == 0 {
                        println!("{}  {}", position(ticks), bpm(&tempo));
                    }
                    ticks += 1;
                }
                Some(ClockEvent::Msg { msg, .. }) => {
                    if msg == ClockMsg::Start {
                        println!("Start received (display position reset; sequences ignore it)");
                        ticks = 0;
                    } else {
                        println!("{msg:?} received (sequences ignore it)");
                    }
                }
            },
            _ = check.tick() => {
                if !lost && tempo.is_lost(Instant::now()) {
                    println!("NO CLOCK (no tick for 2 beats)");
                    lost = true;
                }
            }
        }
    }
}

fn position(ticks: u64) -> String {
    format!(
        "{:>4}.{}.{:02}",
        ticks / TICKS_PER_BAR + 1,
        ticks % TICKS_PER_BAR / TICKS_PER_BEAT + 1,
        ticks % TICKS_PER_BEAT
    )
}

fn bpm(tempo: &TempoEstimator) -> String {
    tempo
        .bpm()
        .map_or_else(|| "--- BPM".into(), |b| format!("{b:6.2} BPM"))
}
