//! Tempo estimate and clock-loss detection from tick timestamps. Pure: the caller passes
//! instants in, so it is testable without a clock.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Ticks per quarter note.
const PPQN: u32 = 24;
/// Moving average over two beats of intervals.
const WINDOW: usize = 48;
/// An interval longer than this is a gap (clock stopped), not a tempo (≈ 5 BPM).
const MAX_INTERVAL: Duration = Duration::from_millis(500);
/// Loss timeout before any tempo is known.
const LOSS_WITHOUT_TEMPO: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
pub struct TempoEstimator {
    last: Option<Instant>,
    intervals: VecDeque<Duration>,
    sum: Duration,
}

impl TempoEstimator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_tick(&mut self, at: Instant) {
        if let Some(last) = self.last {
            let d = at.saturating_duration_since(last);
            if d > MAX_INTERVAL {
                // Clock was stopped; start averaging afresh.
                self.intervals.clear();
                self.sum = Duration::ZERO;
            } else {
                self.intervals.push_back(d);
                self.sum += d;
                if self.intervals.len() > WINDOW {
                    self.sum -= self.intervals.pop_front().expect("non-empty");
                }
            }
        }
        self.last = Some(at);
    }

    /// Average tick interval, once at least a few intervals are known.
    pub fn tick_interval(&self) -> Option<Duration> {
        (self.intervals.len() >= 4).then(|| self.sum / self.intervals.len() as u32)
    }

    pub fn bpm(&self) -> Option<f64> {
        self.tick_interval()
            .map(|d| 60.0 / (d.as_secs_f64() * f64::from(PPQN)))
    }

    /// True when no tick arrived for more than 2 beats at the last known tempo (SPEC §3).
    /// Also true before the first tick.
    pub fn is_lost(&self, now: Instant) -> bool {
        let Some(last) = self.last else { return true };
        let timeout = self
            .tick_interval()
            .map_or(LOSS_WITHOUT_TEMPO, |d| d * (2 * PPQN));
        now.saturating_duration_since(last) > timeout
    }

    pub fn last_tick(&self) -> Option<Instant> {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(est: &mut TempoEstimator, start: Instant, bpm: f64, ticks: u32) -> Instant {
        let d = Duration::from_secs_f64(60.0 / (bpm * f64::from(PPQN)));
        let mut t = start;
        for _ in 0..ticks {
            est.on_tick(t);
            t += d;
        }
        t - d
    }

    #[test]
    fn steady_clock_gives_its_tempo() {
        let mut e = TempoEstimator::new();
        feed(&mut e, Instant::now(), 128.0, 200);
        assert!((e.bpm().unwrap() - 128.0).abs() < 0.01, "{:?}", e.bpm());
    }

    #[test]
    fn tempo_follows_changes_within_the_window() {
        let mut e = TempoEstimator::new();
        let t = feed(&mut e, Instant::now(), 120.0, 100);
        let d = Duration::from_secs_f64(60.0 / (140.0 * 24.0));
        feed(&mut e, t + d, 140.0, 100);
        assert!((e.bpm().unwrap() - 140.0).abs() < 0.01);
    }

    #[test]
    fn jitter_averages_out() {
        let mut e = TempoEstimator::new();
        let base = Duration::from_secs_f64(60.0 / (128.0 * 24.0));
        let mut t = Instant::now();
        for i in 0..200 {
            e.on_tick(t);
            // ±1 ms alternating jitter
            t += if i % 2 == 0 {
                base + Duration::from_millis(1)
            } else {
                base - Duration::from_millis(1)
            };
        }
        assert!((e.bpm().unwrap() - 128.0).abs() < 0.1);
    }

    #[test]
    fn loss_after_two_beats_at_last_tempo() {
        let mut e = TempoEstimator::new();
        let start = Instant::now();
        assert!(e.is_lost(start), "no clock yet");
        let last = feed(&mut e, start, 120.0, 100); // beat = 500 ms
        assert!(!e.is_lost(last + Duration::from_millis(990)));
        assert!(e.is_lost(last + Duration::from_millis(1010)));
        // A tick resumes it.
        e.on_tick(last + Duration::from_millis(1500));
        assert!(!e.is_lost(last + Duration::from_millis(1600)));
    }

    #[test]
    fn a_gap_resets_the_average_instead_of_skewing_it() {
        let mut e = TempoEstimator::new();
        let last = feed(&mut e, Instant::now(), 128.0, 100);
        feed(&mut e, last + Duration::from_secs(3), 128.0, 10);
        assert!((e.bpm().unwrap() - 128.0).abs() < 0.01);
    }
}
