//! Test MIDI clock generator: sends Start then 0xF8 at a steady tempo to an output port, e.g. a
//! Windows loopback port, so the sequencer can be tested without the Clock Shifter.
//!
//! `cargo run --release -p clock-in --example clock_gen -- "<output port>" <bpm> [seconds] [--no-start]`
//! With no arguments it lists the output ports.

use std::time::{Duration, Instant};

use midir::MidiOutput;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = MidiOutput::new("clock-gen").expect("MIDI init");
    let ports = out.ports();
    let names: Vec<String> = ports.iter().filter_map(|p| out.port_name(p).ok()).collect();
    let Some(pattern) = args.first() else {
        println!("output ports:");
        names.iter().for_each(|n| println!("  {n}"));
        return;
    };
    let bpm: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let seconds: f64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(f64::INFINITY);
    let send_start = !args.iter().any(|a| a == "--no-start");
    let idx = names
        .iter()
        .position(|n| n.to_lowercase().contains(&pattern.to_lowercase()))
        .unwrap_or_else(|| panic!("no output port matches {pattern:?}; have {names:?}"));
    let mut conn = out.connect(&ports[idx], "clock-gen").expect("connect");
    println!("sending {bpm} BPM clock to {:?}", names[idx]);
    if send_start {
        conn.send(&[0xFA]).unwrap();
    }
    let interval = Duration::from_secs_f64(60.0 / (bpm * 24.0));
    let begin = Instant::now();
    let mut n: u64 = 0;
    loop {
        let due = begin + interval * n as u32;
        if due.duration_since(begin).as_secs_f64() > seconds {
            break;
        }
        // Sleep coarsely, then spin for the last 2 ms for accurate spacing.
        loop {
            let now = Instant::now();
            if now >= due {
                break;
            }
            let left = due - now;
            if left > Duration::from_millis(2) {
                std::thread::sleep(left - Duration::from_millis(2));
            } else {
                std::hint::spin_loop();
            }
        }
        if let Err(e) = conn.send(&[0xF8]) {
            // E.g. the MIDI driver reset after system standby.
            eprintln!("sending tick {n} failed: {e}; stopping");
            std::process::exit(1);
        }
        n += 1;
    }
    conn.send(&[0xFC]).unwrap();
    println!("sent {n} ticks");
}
