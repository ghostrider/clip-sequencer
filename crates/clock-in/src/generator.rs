//! "Resolume tempo as clock" (SPEC §3.1): ticks at 24 PPQN made by the app at Resolume's tempo,
//! because Resolume reports its tempo, transport and resync but not its beat phase (notes §17).
//!
//! - [`TickPlanner`] is pure: when the next tick is due, given tempo, transport and resync. The
//!   caller passes instants in, so it is testable without a clock.
//! - [`ResolumeClock`] runs a planner on its own thread and sends the ticks as
//!   [`ClockEvent::Msg`] into the same channel as the MIDI input, so the engine sees no difference.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;

use crate::{ClockEvent, ClockMsg};

/// Ticks per quarter note.
const PPQN: f64 = 24.0;
/// Resolume's tempo range (notes §17: `tempo` is a `ParamRange` 20–500).
const MIN_BPM: f64 = 20.0;
const MAX_BPM: f64 = 500.0;
/// The tempo until Resolume has reported one.
pub const DEFAULT_BPM: f64 = 120.0;

/// Resolume's tempo controller transport (`…/tempocontroller/play`, notes §17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Play,
    Pause,
    Stop,
}

impl Transport {
    /// From the `play` parameter's value (`"Play"`, `"Pause"`, `"Stop"`).
    pub fn from_resolume(value: &str) -> Option<Self> {
        match value {
            "Play" => Some(Self::Play),
            "Pause" => Some(Self::Pause),
            "Stop" => Some(Self::Stop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Stopped,
    /// Paused with this much of the current tick interval left.
    Paused {
        remaining: Duration,
    },
    Running {
        next: Instant,
    },
}

/// When ticks are due. Pure.
#[derive(Debug, Clone)]
pub struct TickPlanner {
    interval: Duration,
    state: State,
}

/// A tick later than this is not caught up: the planner starts afresh from now (a stalled thread,
/// a suspended machine) instead of sending a burst.
const MAX_LAG: Duration = Duration::from_millis(250);

impl TickPlanner {
    /// Stopped, at `bpm`.
    pub fn new(bpm: f64) -> Self {
        Self {
            interval: interval(bpm),
            state: State::Stopped,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }

    /// A new tempo applies from the next tick: the part of the current interval that is left
    /// is scaled, so the phase moves on smoothly.
    pub fn set_bpm(&mut self, bpm: f64, now: Instant) {
        let new = interval(bpm);
        let ratio = new.as_secs_f64() / self.interval.as_secs_f64();
        match &mut self.state {
            State::Running { next } => {
                let left = next.saturating_duration_since(now);
                *next = now + left.mul_f64(ratio);
            }
            State::Paused { remaining } => *remaining = remaining.mul_f64(ratio),
            State::Stopped => {}
        }
        self.interval = new;
    }

    /// Play. Returns true when this restarts the phase (play after Stop counts as a resync).
    pub fn play(&mut self, now: Instant) -> bool {
        match self.state {
            State::Stopped => {
                self.state = State::Running {
                    next: now + self.interval,
                };
                true
            }
            State::Paused { remaining } => {
                self.state = State::Running {
                    next: now + remaining,
                };
                false
            }
            State::Running { .. } => false,
        }
    }

    pub fn pause(&mut self, now: Instant) {
        if let State::Running { next } = self.state {
            self.state = State::Paused {
                remaining: next.saturating_duration_since(now),
            };
        }
    }

    pub fn stop(&mut self) {
        self.state = State::Stopped;
    }

    /// Resync at `at`: that moment is beat 1 (the engine rewinds there), so the next tick comes
    /// one interval later. Returns false while stopped (nothing runs; the next Play resyncs).
    pub fn resync(&mut self, at: Instant) -> bool {
        match &mut self.state {
            State::Running { next } => {
                *next = at + self.interval;
                true
            }
            State::Paused { remaining } => {
                *remaining = self.interval;
                true
            }
            State::Stopped => false,
        }
    }

    pub fn next_due(&self) -> Option<Instant> {
        match self.state {
            State::Running { next } => Some(next),
            _ => None,
        }
    }

    /// The next tick if it is due at `now`, stamped with the time it was due. Call until `None`.
    pub fn pop_due(&mut self, now: Instant) -> Option<Instant> {
        let State::Running { next } = &mut self.state else {
            return None;
        };
        if *next > now {
            return None;
        }
        if now.saturating_duration_since(*next) > MAX_LAG {
            *next = now;
        }
        let at = *next;
        *next += self.interval;
        Some(at)
    }
}

fn interval(bpm: f64) -> Duration {
    let bpm = if bpm.is_finite() {
        bpm.clamp(MIN_BPM, MAX_BPM)
    } else {
        DEFAULT_BPM
    };
    Duration::from_secs_f64(60.0 / (bpm * PPQN))
}

enum Cmd {
    Tempo(f64),
    /// `true`: a change made in Resolume (play after stop resyncs); `false`: the state as reported
    /// on connect (the phase is unknown, nothing resyncs).
    Transport(Transport, Instant, bool),
    Resync(Instant),
}

/// Waits longer than this use a channel timeout (coarse on Windows, ~16 ms); shorter ones sleep in
/// small slices (`thread::sleep` uses a high-resolution timer), so commands are still seen quickly.
const COARSE_WAIT: Duration = Duration::from_millis(25);
const COARSE_MARGIN: Duration = Duration::from_millis(18);
const SLICE: Duration = Duration::from_millis(1);

/// The tick generator thread. Starts stopped at [`DEFAULT_BPM`]; dropping it stops the thread.
pub struct ResolumeClock {
    tx: Option<mpsc::Sender<Cmd>>,
    thread: Option<JoinHandle<()>>,
}

impl ResolumeClock {
    pub fn spawn(out: UnboundedSender<ClockEvent>) -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("resolume-clock".into())
            .spawn(move || generate(&rx, &out))
            .expect("spawn resolume-clock thread");
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    pub fn tempo(&self, bpm: f64) {
        self.send(Cmd::Tempo(bpm));
    }

    /// A transport change made in Resolume at `at`: Play after Stop is a resync (answered with
    /// [`ClockEvent::Resync`]).
    pub fn transport(&self, transport: Transport, at: Instant) {
        self.send(Cmd::Transport(transport, at, true));
    }

    /// The transport as Resolume reports it (on connect, after a reload): runs or stops the ticks
    /// without a resync, because the phase is unknown.
    pub fn set_state(&self, transport: Transport, at: Instant) {
        self.send(Cmd::Transport(transport, at, false));
    }

    /// Resync at `at` (when Resolume reported it). Answered with [`ClockEvent::Resync`] unless
    /// stopped.
    pub fn resync(&self, at: Instant) {
        self.send(Cmd::Resync(at));
    }

    fn send(&self, cmd: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }
}

impl Drop for ResolumeClock {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn generate(rx: &mpsc::Receiver<Cmd>, out: &UnboundedSender<ClockEvent>) {
    let mut planner = TickPlanner::new(DEFAULT_BPM);
    let apply = |planner: &mut TickPlanner, cmd: Cmd| {
        let resync_at = match cmd {
            Cmd::Tempo(bpm) => {
                planner.set_bpm(bpm, Instant::now());
                None
            }
            Cmd::Transport(Transport::Play, at, change) => {
                (planner.play(at) && change).then_some(at)
            }
            Cmd::Transport(Transport::Pause, at, _) => {
                planner.pause(at);
                None
            }
            Cmd::Transport(Transport::Stop, ..) => {
                planner.stop();
                None
            }
            Cmd::Resync(at) => planner.resync(at).then_some(at),
        };
        if let Some(at) = resync_at {
            let _ = out.send(ClockEvent::Resync { at });
        }
    };
    loop {
        if out.is_closed() {
            return;
        }
        let now = Instant::now();
        while let Some(at) = planner.pop_due(now) {
            let _ = out.send(ClockEvent::Msg {
                msg: ClockMsg::Tick,
                at,
            });
        }
        let cmd = match planner.next_due() {
            None => match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            },
            Some(due) => {
                let wait = due.saturating_duration_since(Instant::now());
                if wait > COARSE_WAIT {
                    match rx.recv_timeout(wait - COARSE_MARGIN) {
                        Ok(c) => Some(c),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                } else {
                    if !wait.is_zero() {
                        std::thread::sleep(wait.min(SLICE));
                    }
                    None
                }
            }
        };
        if let Some(c) = cmd {
            apply(&mut planner, c);
        }
        loop {
            match rx.try_recv() {
                Ok(c) => apply(&mut planner, c),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Ticks due in [t0, t0 + span], as offsets from t0 in µs.
    fn ticks(p: &mut TickPlanner, t0: Instant, span: Duration) -> Vec<u128> {
        let mut out = Vec::new();
        let end = t0 + span;
        while let Some(due) = p.next_due() {
            if due > end {
                break;
            }
            let at = p.pop_due(due).unwrap();
            out.push(at.duration_since(t0).as_micros());
        }
        out
    }

    #[test]
    fn interval_from_bpm_and_clamped() {
        assert_eq!(TickPlanner::new(125.0).interval(), ms(20));
        assert_eq!(
            TickPlanner::new(1000.0).interval(),
            TickPlanner::new(500.0).interval()
        );
        assert_eq!(
            TickPlanner::new(f64::NAN).interval(),
            TickPlanner::new(DEFAULT_BPM).interval()
        );
    }

    #[test]
    fn stopped_until_play_and_play_after_stop_resyncs() {
        let t0 = Instant::now();
        let mut p = TickPlanner::new(125.0);
        assert_eq!(p.next_due(), None);
        assert_eq!(p.pop_due(t0 + ms(100)), None);
        assert!(p.play(t0), "play after stop restarts the phase");
        assert_eq!(ticks(&mut p, t0, ms(65)), vec![20_000, 40_000, 60_000]);
        assert!(!p.play(t0 + ms(65)), "already playing");
    }

    #[test]
    fn pause_keeps_the_phase_and_stop_forgets_it() {
        let t0 = Instant::now();
        let mut p = TickPlanner::new(125.0);
        p.play(t0);
        assert_eq!(ticks(&mut p, t0, ms(20)), vec![20_000]);
        p.pause(t0 + ms(25)); // 15 ms left
        assert_eq!(p.next_due(), None);
        assert!(!p.play(t0 + ms(1000)), "resume is not a resync");
        assert_eq!(p.next_due(), Some(t0 + ms(1015)));
        p.stop();
        assert_eq!(p.next_due(), None);
        assert!(p.play(t0 + ms(2000)));
        assert_eq!(p.next_due(), Some(t0 + ms(2020)));
    }

    #[test]
    fn resync_makes_that_moment_beat_one() {
        let t0 = Instant::now();
        let mut p = TickPlanner::new(125.0);
        p.play(t0);
        ticks(&mut p, t0, ms(40));
        assert!(p.resync(t0 + ms(47)));
        assert_eq!(p.next_due(), Some(t0 + ms(67)));
        // Paused: resumes a full interval after play.
        p.pause(t0 + ms(50));
        assert!(p.resync(t0 + ms(55)));
        p.play(t0 + ms(100));
        assert_eq!(p.next_due(), Some(t0 + ms(120)));
        // Stopped: nothing to resync.
        p.stop();
        assert!(!p.resync(t0 + ms(200)));
    }

    #[test]
    fn tempo_change_scales_the_rest_of_the_interval() {
        let t0 = Instant::now();
        let mut p = TickPlanner::new(125.0); // 20 ms
        p.play(t0);
        p.set_bpm(250.0, t0 + ms(10)); // 10 ms left -> 5 ms, then every 10 ms
        assert_eq!(ticks(&mut p, t0, ms(36)), vec![15_000, 25_000, 35_000]);
        // While paused too.
        p.pause(t0 + ms(40)); // 5 ms left
        p.set_bpm(125.0, t0 + ms(50)); // -> 10 ms
        p.play(t0 + ms(100));
        assert_eq!(p.next_due(), Some(t0 + ms(110)));
    }

    #[test]
    fn late_ticks_are_caught_up_but_a_long_stall_is_not() {
        let t0 = Instant::now();
        let mut p = TickPlanner::new(125.0);
        p.play(t0);
        // 70 ms late: three ticks, each stamped when it was due.
        let mut got = Vec::new();
        while let Some(at) = p.pop_due(t0 + ms(70)) {
            got.push(at.duration_since(t0).as_millis());
        }
        assert_eq!(got, vec![20, 40, 60]);
        // A stall of a second: one tick now, then the grid continues from there.
        let late = t0 + ms(1080);
        assert_eq!(p.pop_due(late), Some(late));
        assert_eq!(p.pop_due(late), None);
        assert_eq!(p.next_due(), Some(late + ms(20)));
    }

    #[test]
    fn transport_from_resolume_values() {
        assert_eq!(Transport::from_resolume("Play"), Some(Transport::Play));
        assert_eq!(Transport::from_resolume("Pause"), Some(Transport::Pause));
        assert_eq!(Transport::from_resolume("Stop"), Some(Transport::Stop));
        assert_eq!(Transport::from_resolume("play"), None);
    }

    #[test]
    fn thread_sends_ticks_and_resync() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let clock = ResolumeClock::spawn(tx);
        clock.tempo(500.0); // 5 ms ticks
        let t0 = Instant::now();
        clock.transport(Transport::Play, t0);
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while events.len() < 12 && Instant::now() < deadline {
            match rx.try_recv() {
                Ok(e) => events.push(e),
                Err(_) => std::thread::sleep(ms(1)),
            }
        }
        assert_eq!(events[0], ClockEvent::Resync { at: t0 }, "play after stop");
        let ticks: Vec<_> = events[1..]
            .iter()
            .map(|e| match e {
                ClockEvent::Msg {
                    msg: ClockMsg::Tick,
                    at,
                } => at.duration_since(t0),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(ticks.len(), 11);
        for (i, d) in ticks.iter().enumerate() {
            assert_eq!(*d, ms(5 * (i as u64 + 1)), "stamped on the grid");
        }
        clock.transport(Transport::Stop, Instant::now());
        drop(clock); // joins the thread
    }

    #[test]
    fn reported_state_runs_without_a_resync() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let clock = ResolumeClock::spawn(tx);
        clock.tempo(500.0);
        clock.set_state(Transport::Play, Instant::now());
        let deadline = Instant::now() + Duration::from_secs(2);
        let first = loop {
            if let Ok(e) = rx.try_recv() {
                break e;
            }
            assert!(Instant::now() < deadline, "no tick");
            std::thread::sleep(ms(1));
        };
        assert!(matches!(
            first,
            ClockEvent::Msg {
                msg: ClockMsg::Tick,
                ..
            }
        ));
    }
}
