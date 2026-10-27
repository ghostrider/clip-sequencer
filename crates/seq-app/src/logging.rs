//! Log capture (docs/M12-desktop-shell.md, Phase E). One `log` logger feeds:
//!
//! - a ring buffer of the last [`RING`] records (the Console tab's snapshot),
//! - a broadcast channel for live subscribers (UI sessions with the Console tab open),
//! - a writer thread: stderr (headless) and `<data-dir>/logs/seq-app-YYYY-MM-DD.log`, one file per
//!   UTC day, files older than [`KEEP_DAYS`] days deleted.
//!
//! Logging never blocks the caller (the engine logs on its timing path): the ring is a short
//! mutex push, the broadcast never waits, and the writer queue is `try_send`. Records that do not
//! fit are dropped and counted ([`stats`]). Filtering follows `RUST_LOG` (default `info`).
//!
//! Live subscribers get DEBUG/TRACE records only from the app's own crates ([`is_live`]): the
//! WebSocket and HTTP crates log every frame at TRACE, so forwarding theirs to the Console would
//! log the forwarding itself, endlessly. Those records still reach the ring and the file.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tokio::sync::broadcast;

/// Records kept for the Console tab's snapshot.
pub const RING: usize = 5000;
/// Live subscribers lagging further than this lose records (counted as dropped).
const BROADCAST: usize = 2048;
/// Lines waiting for the writer thread.
const WRITE_QUEUE: usize = 8192;
pub const KEEP_DAYS: i64 = 7;
const FILE_PREFIX: &str = "seq-app-";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogRecord {
    /// Increases by one per record; lets the UI merge snapshot and live records.
    pub seq: u64,
    /// UTC, `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub ts: String,
    /// `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`.
    pub level: &'static str,
    pub target: String,
    pub message: String,
}

impl LogRecord {
    /// One line of the log file / stderr.
    pub fn line(&self) -> String {
        format!(
            "{} {:<5} {}: {}",
            self.ts, self.level, self.target, self.message
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LogStats {
    /// Error records since start.
    pub errors: u64,
    /// Records that did not reach the file or a live subscriber.
    pub dropped: u64,
}

pub struct LogConfig {
    /// Also print to stderr (headless and debug builds).
    pub stderr: bool,
    /// Where the daily files go; `None` = no files.
    pub dir: Option<PathBuf>,
}

struct State {
    filter: env_filter::Filter,
    ring: Mutex<VecDeque<LogRecord>>,
    live: broadcast::Sender<LogRecord>,
    writer: SyncSender<LogRecord>,
    dir: Option<PathBuf>,
    seq: AtomicU64,
    errors: AtomicU64,
    dropped: AtomicU64,
}

static STATE: OnceLock<State> = OnceLock::new();

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        STATE.get().is_some_and(|s| s.filter.enabled(metadata))
    }

    fn log(&self, record: &log::Record<'_>) {
        let Some(state) = STATE.get() else { return };
        if !state.filter.matches(record) {
            return;
        }
        let rec = LogRecord {
            seq: state.seq.fetch_add(1, Ordering::Relaxed),
            ts: utc_iso8601(SystemTime::now(), true),
            level: record.level().as_str(),
            target: record.target().to_owned(),
            message: record.args().to_string(),
        };
        if record.level() == log::Level::Error {
            state.errors.fetch_add(1, Ordering::Relaxed);
        }
        {
            let mut ring = state.ring.lock().unwrap_or_else(|e| e.into_inner());
            if ring.len() == RING {
                ring.pop_front();
            }
            ring.push_back(rec.clone());
        }
        if is_live(record.level(), record.target()) {
            let _ = state.live.send(rec.clone()); // no subscriber: nothing to do
        }
        if let Err(TrySendError::Full(_)) = state.writer.try_send(rec) {
            state.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn flush(&self) {}
}

/// Installs the logger. Call once, before anything logs; later calls are ignored.
pub fn init(cfg: LogConfig) {
    let mut builder = env_filter::Builder::new();
    match std::env::var("RUST_LOG") {
        Ok(spec) if !spec.trim().is_empty() => builder.parse(&spec),
        _ => builder.filter_level(log::LevelFilter::Info),
    };
    let filter = builder.build();
    let max = filter.filter();
    let (writer, lines) = sync_channel(WRITE_QUEUE);
    if let Some(dir) = &cfg.dir {
        let _ = std::fs::create_dir_all(dir);
    }
    let state = State {
        filter,
        ring: Mutex::new(VecDeque::with_capacity(RING)),
        live: broadcast::channel(BROADCAST).0,
        writer,
        dir: cfg.dir.clone(),
        seq: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
    };
    if STATE.set(state).is_err() || log::set_logger(&Logger).is_err() {
        return;
    }
    log::set_max_level(max);
    let _ = std::thread::Builder::new()
        .name("log-writer".into())
        .spawn(move || write_loop(lines, cfg.stderr, cfg.dir));
}

/// The app's own crates; everything else is a dependency.
const OWN_CRATES: [&str; 4] = ["seq_app", "seq_core", "resolume_client", "clock_in"];

/// Whether a record goes to live subscribers (see the module docs).
pub fn is_live(level: log::Level, target: &str) -> bool {
    level <= log::Level::Info
        || OWN_CRATES
            .iter()
            .any(|c| target == *c || target.strip_prefix(c).is_some_and(|r| r.starts_with("::")))
}

/// The ring buffer, oldest first.
pub fn snapshot() -> Vec<LogRecord> {
    STATE
        .get()
        .map(|s| {
            s.ring
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Live records from now on (`None` before `init`).
pub fn subscribe() -> Option<broadcast::Receiver<LogRecord>> {
    STATE.get().map(|s| s.live.subscribe())
}

pub fn stats() -> LogStats {
    STATE
        .get()
        .map(|s| LogStats {
            errors: s.errors.load(Ordering::Relaxed),
            dropped: s.dropped.load(Ordering::Relaxed),
        })
        .unwrap_or_default()
}

/// A live subscriber fell behind by `n` records.
pub fn note_dropped(n: u64) {
    if let Some(s) = STATE.get() {
        s.dropped.fetch_add(n, Ordering::Relaxed);
    }
}

/// An error the process may not outlive (a panic, a window that cannot open): kept in the ring
/// and appended to today's file right away instead of through the writer thread, which a dying
/// process may not give the time. stderr is left to the caller (the default panic hook prints).
pub fn error_now(target: &str, message: &str) {
    let Some(state) = STATE.get() else { return };
    let rec = LogRecord {
        seq: state.seq.fetch_add(1, Ordering::Relaxed),
        ts: utc_iso8601(SystemTime::now(), true),
        level: "ERROR",
        target: target.to_owned(),
        message: message.to_owned(),
    };
    state.errors.fetch_add(1, Ordering::Relaxed);
    {
        let mut ring = state.ring.lock().unwrap_or_else(|e| e.into_inner());
        if ring.len() == RING {
            ring.pop_front();
        }
        ring.push_back(rec.clone());
    }
    let _ = state.live.send(rec.clone());
    if let Some(path) = current_file() {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{}", rec.line());
        }
    }
}

/// Panics go to the log file too (the desktop app has no console to print them to).
pub fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        error_now("panic", &format!("thread '{name}' {info}"));
        default(info);
    }));
}

/// `<data-dir>/logs`, if logging to files.
pub fn logs_dir() -> Option<PathBuf> {
    STATE.get().and_then(|s| s.dir.clone())
}

/// Today's log file (it may not exist yet).
pub fn current_file() -> Option<PathBuf> {
    logs_dir().map(|d| d.join(file_name(&utc_iso8601(SystemTime::now(), false)[..10])))
}

fn file_name(day: &str) -> String {
    format!("{FILE_PREFIX}{day}.log")
}

fn write_loop(lines: Receiver<LogRecord>, stderr: bool, dir: Option<PathBuf>) {
    let mut day = String::new();
    let mut file: Option<std::io::BufWriter<File>> = None;
    while let Ok(first) = lines.recv() {
        let mut batch = vec![first];
        batch.extend(lines.try_iter().take(1024));
        for rec in batch {
            let line = rec.line();
            if stderr {
                eprintln!("{line}");
            }
            let Some(dir) = &dir else { continue };
            let rec_day = &rec.ts[..10];
            if rec_day != day {
                day = rec_day.to_owned();
                file = open_day(dir, &day);
            }
            if let Some(f) = &mut file {
                let _ = writeln!(f, "{line}");
            }
        }
        if let Some(f) = &mut file {
            let _ = f.flush();
        }
    }
}

/// Opens (appends to) the day's file and deletes files older than `KEEP_DAYS`.
fn open_day(dir: &Path, day: &str) -> Option<std::io::BufWriter<File>> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let names: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        for stale in stale_logs(&names, day) {
            let _ = std::fs::remove_file(dir.join(stale));
        }
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(file_name(day)))
        .ok()
        .map(std::io::BufWriter::new)
}

/// Log files (by name) older than `KEEP_DAYS` days before `today` (`YYYY-MM-DD`).
pub fn stale_logs<'a>(names: &'a [String], today: &str) -> Vec<&'a str> {
    let Some(today_days) = days_from_iso(today) else {
        return Vec::new();
    };
    names
        .iter()
        .filter_map(|n| {
            let day = n.strip_prefix(FILE_PREFIX)?.strip_suffix(".log")?;
            let age = today_days - days_from_iso(day)?;
            (age >= KEEP_DAYS).then_some(n.as_str())
        })
        .collect()
}

/// `YYYY-MM-DD` → days since 1970-01-01 (Howard Hinnant's days_from_civil).
fn days_from_iso(day: &str) -> Option<i64> {
    let mut parts = day.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// UTC time as `YYYY-MM-DDTHH:MM:SS[.mmm]Z` (civil_from_days, Howard Hinnant).
pub fn utc_iso8601(t: SystemTime, millis: bool) -> String {
    let d = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    if millis {
        format!(
            "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{:03}Z",
            d.subsec_millis()
        )
    } else {
        format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(1_793_100_000_123);
        assert_eq!(utc_iso8601(t, true), "2026-10-27T11:20:00.123Z");
        assert_eq!(utc_iso8601(t, false), "2026-10-27T11:20:00Z");
        assert_eq!(days_from_iso("1970-01-01"), Some(0));
        assert_eq!(days_from_iso("2026-10-27"), Some(1_793_100_000 / 86_400));
        assert_eq!(days_from_iso("2026-13-01"), None);
    }

    #[test]
    fn only_old_log_files_are_stale() {
        let names: Vec<String> = [
            "seq-app-2026-10-27.log",
            "seq-app-2026-10-21.log",
            "seq-app-2026-10-20.log",
            "seq-app-2026-09-30.log",
            "other.log",
            "seq-app-garbage.log",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            stale_logs(&names, "2026-10-27"),
            ["seq-app-2026-10-20.log", "seq-app-2026-09-30.log"]
        );
    }

    #[test]
    fn dependencies_send_only_info_and_above_live() {
        use log::Level::*;
        assert!(is_live(Trace, "seq_app::engine"));
        assert!(is_live(Debug, "resolume_client"));
        assert!(!is_live(Trace, "tokio_tungstenite::compat"));
        assert!(!is_live(Debug, "seq_application")); // a prefix is not the crate
        assert!(is_live(Info, "tokio_tungstenite::compat"));
        assert!(is_live(Error, "hyper"));
    }

    #[test]
    fn line_format() {
        let r = LogRecord {
            seq: 0,
            ts: "2026-10-27T10:40:00.123Z".into(),
            level: "WARN",
            target: "seq_app::engine".into(),
            message: "hello".into(),
        };
        assert_eq!(
            r.line(),
            "2026-10-27T10:40:00.123Z WARN  seq_app::engine: hello"
        );
    }
}
