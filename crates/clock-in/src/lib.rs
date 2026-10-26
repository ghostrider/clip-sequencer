//! MIDI clock input (SPEC §3).
//!
//! - [`ClockIn`] connects to an existing MIDI input port by name (on Windows `midir` cannot create
//!   virtual ports; the MIDI Clock Shifter provides `Clock Shifter Out`), timestamps realtime
//!   messages in the driver callback and forwards them as [`ClockEvent`]s. A supervisor thread
//!   reconnects when the port disappears and comes back.
//! - [`TempoEstimator`] is pure: tempo from tick intervals (display only) and clock-loss detection
//!   ("no tick for more than 2 beats at the last known tempo").

mod tempo;

pub use tempo::TempoEstimator;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputConnection};
use tokio::sync::mpsc::UnboundedSender;

/// MIDI realtime messages the sequencer cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockMsg {
    /// 0xF8, 24 per quarter note.
    Tick,
    /// 0xFA. Logged only: the Clock Shifter's resync sends Start; it must not restart sequences.
    Start,
    /// 0xFB.
    Continue,
    /// 0xFC.
    Stop,
}

/// Parses a MIDI message from the driver. Realtime messages are single status bytes.
pub fn parse(message: &[u8]) -> Option<ClockMsg> {
    match message.first()? {
        0xF8 => Some(ClockMsg::Tick),
        0xFA => Some(ClockMsg::Start),
        0xFB => Some(ClockMsg::Continue),
        0xFC => Some(ClockMsg::Stop),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockEvent {
    /// A clock message, stamped when the driver delivered it.
    Msg { msg: ClockMsg, at: Instant },
    /// Connected to this port.
    Connected { port: String },
    /// The port went away (or connecting failed); reconnecting is automatic.
    Disconnected { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ClockError {
    #[error("MIDI init failed: {0}")]
    Init(#[from] midir::InitError),
}

/// Names of all MIDI input ports.
pub fn list_ports() -> Result<Vec<String>, ClockError> {
    let input = MidiInput::new("resolume-sequencer list")?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect())
}

/// Picks the port for `pattern`: an exact (case-insensitive) name match wins, otherwise the first
/// name containing it.
pub fn match_port<'a>(names: &'a [String], pattern: &str) -> Option<&'a str> {
    let pat = pattern.to_lowercase();
    names
        .iter()
        .find(|n| n.to_lowercase() == pat)
        .or_else(|| names.iter().find(|n| n.to_lowercase().contains(&pat)))
        .map(String::as_str)
}

const POLL: Duration = Duration::from_millis(500);

/// Running MIDI clock input. Dropping it disconnects and stops the supervisor thread.
pub struct ClockIn {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ClockIn {
    /// Connects to the first port matching `pattern` (now or whenever it appears) and sends
    /// events to `tx`.
    pub fn spawn(pattern: String, tx: UnboundedSender<ClockEvent>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("clock-in".into())
                .spawn(move || supervise(&pattern, &tx, &stop))
                .expect("spawn clock-in thread")
        };
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for ClockIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn supervise(pattern: &str, tx: &UnboundedSender<ClockEvent>, stop: &AtomicBool) {
    supervise_with(
        pattern,
        tx,
        stop,
        POLL,
        || list_ports().unwrap_or_default(),
        connect,
        |c: MidiInputConnection<()>| {
            c.close();
        },
    );
}

/// The supervisor loop with the MIDI system injected (so port loss can be tested): connects
/// when a matching port exists, notices when it disappears, reconnects when it is back.
fn supervise_with<C>(
    pattern: &str,
    tx: &UnboundedSender<ClockEvent>,
    stop: &AtomicBool,
    poll: Duration,
    list: impl Fn() -> Vec<String>,
    mut connect: impl FnMut(&[String], &str, UnboundedSender<ClockEvent>) -> Result<(String, C), String>,
    close: impl Fn(C),
) {
    let mut conn: Option<(String, C)> = None;
    let mut last_error = String::new();
    while !stop.load(Ordering::Relaxed) && !tx.is_closed() {
        let names = list();
        match &conn {
            Some((name, _)) if !names.contains(name) => {
                let (name, c) = conn.take().expect("checked");
                close(c);
                log::warn!("MIDI port {name:?} disappeared");
                let _ = tx.send(ClockEvent::Disconnected {
                    reason: format!("port {name:?} disappeared"),
                });
            }
            Some(_) => {}
            None => match connect(&names, pattern, tx.clone()) {
                Ok(c) => {
                    log::info!("MIDI clock input connected to {:?}", c.0);
                    let _ = tx.send(ClockEvent::Connected { port: c.0.clone() });
                    last_error.clear();
                    conn = Some(c);
                }
                Err(e) => {
                    if e != last_error {
                        log::warn!("MIDI clock input: {e}");
                        let _ = tx.send(ClockEvent::Disconnected { reason: e.clone() });
                        last_error = e;
                    }
                }
            },
        }
        std::thread::sleep(poll);
    }
    if let Some((_, c)) = conn {
        close(c);
    }
}

fn connect(
    names: &[String],
    pattern: &str,
    tx: UnboundedSender<ClockEvent>,
) -> Result<(String, MidiInputConnection<()>), String> {
    let name = match_port(names, pattern)
        .ok_or_else(|| format!("no MIDI input port matches {pattern:?} (have: {names:?})"))?
        .to_owned();
    let mut input = MidiInput::new("resolume-sequencer").map_err(|e| e.to_string())?;
    // Timing messages are what we are here for; nothing may be filtered.
    input.ignore(Ignore::None);
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).as_deref() == Ok(name.as_str()))
        .ok_or_else(|| format!("port {name:?} vanished while connecting"))?;
    let conn = input
        .connect(
            &port,
            "resolume-sequencer-clock",
            move |_stamp, message, _| {
                let at = Instant::now();
                if let Some(msg) = parse(message) {
                    let _ = tx.send(ClockEvent::Msg { msg, at });
                }
            },
            (),
        )
        .map_err(|e| format!("connecting to {name:?}: {e}"))?;
    Ok((name, conn))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_realtime_messages() {
        assert_eq!(parse(&[0xF8]), Some(ClockMsg::Tick));
        assert_eq!(parse(&[0xFA]), Some(ClockMsg::Start));
        assert_eq!(parse(&[0xFB]), Some(ClockMsg::Continue));
        assert_eq!(parse(&[0xFC]), Some(ClockMsg::Stop));
        assert_eq!(parse(&[0x90, 60, 100]), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn port_loss_is_reported_and_the_port_reconnects_when_it_returns() {
        use std::sync::Mutex;
        use std::time::Duration;
        let ports = Arc::new(Mutex::new(Vec::<String>::new()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (ports, stop) = (ports.clone(), stop.clone());
            std::thread::spawn(move || {
                supervise_with(
                    "Clock Shifter Out",
                    &tx,
                    &stop,
                    Duration::from_millis(5),
                    || ports.lock().unwrap().clone(),
                    |names: &[String], pattern: &str, _tx| {
                        match_port(names, pattern)
                            .map(|n| (n.to_owned(), ()))
                            .ok_or_else(|| "no port".to_owned())
                    },
                    |()| {},
                )
            })
        };
        let mut next = || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                if let Ok(ev) = rx.try_recv() {
                    return ev;
                }
                assert!(std::time::Instant::now() < deadline, "no event");
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        assert!(
            matches!(next(), ClockEvent::Disconnected { .. }),
            "no port at start"
        );
        ports.lock().unwrap().push("Clock Shifter Out".into());
        assert_eq!(
            next(),
            ClockEvent::Connected {
                port: "Clock Shifter Out".into()
            }
        );
        ports.lock().unwrap().clear();
        assert!(
            matches!(next(), ClockEvent::Disconnected { reason } if reason.contains("disappeared"))
        );
        // The repeated "no port" error is reported once, then the port comes back.
        assert!(matches!(next(), ClockEvent::Disconnected { .. }));
        ports.lock().unwrap().push("Clock Shifter Out".into());
        assert_eq!(
            next(),
            ClockEvent::Connected {
                port: "Clock Shifter Out".into()
            }
        );
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
    }

    #[test]
    fn port_matching_prefers_exact_then_substring() {
        let names = vec![
            "Clock Shifter In".to_owned(),
            "Clock Shifter Out".to_owned(),
            "Arturia BeatStep Pro".to_owned(),
        ];
        assert_eq!(
            match_port(&names, "clock shifter out"),
            Some("Clock Shifter Out")
        );
        assert_eq!(
            match_port(&names, "Clock Shifter"),
            Some("Clock Shifter In")
        );
        assert_eq!(match_port(&names, "beatstep"), Some("Arturia BeatStep Pro"));
        assert_eq!(match_port(&names, "nope"), None);
    }
}
