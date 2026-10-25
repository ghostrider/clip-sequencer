//! Debounced autosave (SPEC §6: "Autosave after edits, debounced ~1 s").
//!
//! The engine hands every edited [`SeqFile`] to a [`SaverHandle`] and moves on; the saver task
//! writes the latest one once edits pause for `quiet`, or at the latest `max_wait` after the
//! first unsaved edit, so a steady stream of edits cannot postpone saving forever. Disk IO runs
//! on the blocking pool, never on the engine task.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use seq_core::SeqFile;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::storage::Storage;

#[derive(Debug, Clone, Copy)]
pub struct DebounceConfig {
    pub quiet: Duration,
    pub max_wait: Duration,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self {
            quiet: Duration::from_secs(1),
            max_wait: Duration::from_secs(5),
        }
    }
}

/// Pure debounce bookkeeping, separate from the task so it can be tested without a runtime.
#[derive(Debug)]
pub struct Debouncer {
    cfg: DebounceConfig,
    first_change: Option<Instant>,
    last_change: Option<Instant>,
}

impl Debouncer {
    pub fn new(cfg: DebounceConfig) -> Self {
        Self {
            cfg,
            first_change: None,
            last_change: None,
        }
    }

    pub fn changed(&mut self, now: Instant) {
        self.first_change.get_or_insert(now);
        self.last_change = Some(now);
    }

    /// When the pending change should be saved, or `None` if nothing is pending.
    pub fn deadline(&self) -> Option<Instant> {
        let first = self.first_change?;
        let last = self.last_change?;
        Some((last + self.cfg.quiet).min(first + self.cfg.max_wait))
    }

    /// Call after saving.
    pub fn reset(&mut self) {
        self.first_change = None;
        self.last_change = None;
    }
}

/// Reported after every save attempt, for the UI status bar and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveEvent {
    Saved(PathBuf),
    Failed(String),
}

enum Msg {
    Changed(Box<SeqFile>),
    Flush(oneshot::Sender<Result<(), String>>),
}

#[derive(Clone)]
pub struct SaverHandle {
    tx: mpsc::UnboundedSender<Msg>,
}

impl SaverHandle {
    /// Queues `file` for saving. Never blocks.
    pub fn changed(&self, file: SeqFile) {
        let _ = self.tx.send(Msg::Changed(Box::new(file)));
    }

    /// Saves any pending change now and waits for the result.
    pub async fn flush(&self) -> Result<(), String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Msg::Flush(reply))
            .map_err(|_| "saver task has stopped".to_owned())?;
        rx.await.map_err(|_| "saver task has stopped".to_owned())?
    }
}

/// Starts the saver task. When every handle is dropped it saves what is pending and exits;
/// await the returned `JoinHandle` on shutdown.
pub fn spawn(
    storage: Storage,
    cfg: DebounceConfig,
    events: Option<mpsc::UnboundedSender<SaveEvent>>,
) -> (SaverHandle, JoinHandle<()>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let mut debouncer = Debouncer::new(cfg);
        let mut pending: Option<SeqFile> = None;
        loop {
            let deadline = debouncer.deadline();
            let sleep = tokio::time::sleep_until(
                deadline
                    .map(tokio::time::Instant::from_std)
                    .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(3600)),
            );
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(Msg::Changed(file)) => {
                        pending = Some(*file);
                        debouncer.changed(Instant::now());
                    }
                    Some(Msg::Flush(reply)) => {
                        debouncer.reset();
                        let result = save(&storage, pending.take(), &events).await;
                        let _ = reply.send(result);
                    }
                    None => {
                        let _ = save(&storage, pending.take(), &events).await;
                        break;
                    }
                },
                () = sleep, if deadline.is_some() => {
                    debouncer.reset();
                    let _ = save(&storage, pending.take(), &events).await;
                }
            }
        }
    });
    (SaverHandle { tx }, task)
}

async fn save(
    storage: &Storage,
    file: Option<SeqFile>,
    events: &Option<mpsc::UnboundedSender<SaveEvent>>,
) -> Result<(), String> {
    let Some(file) = file else { return Ok(()) };
    let storage = storage.clone();
    let result = tokio::task::spawn_blocking(move || storage.save(&file))
        .await
        .map_err(|e| format!("save task failed: {e}"))
        .and_then(|r| r.map_err(|e| e.to_string()));
    let event = match &result {
        Ok(path) => {
            log::debug!("saved {}", path.display());
            SaveEvent::Saved(path.clone())
        }
        Err(e) => {
            log::error!("autosave failed: {e}");
            SaveEvent::Failed(e.clone())
        }
    };
    if let Some(tx) = events {
        let _ = tx.send(event);
    }
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: DebounceConfig = DebounceConfig {
        quiet: Duration::from_millis(1000),
        max_wait: Duration::from_millis(5000),
    };

    #[test]
    fn nothing_pending_means_no_deadline() {
        assert_eq!(Debouncer::new(CFG).deadline(), None);
    }

    #[test]
    fn deadline_moves_with_each_change() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(CFG);
        d.changed(t0);
        assert_eq!(d.deadline(), Some(t0 + CFG.quiet));
        d.changed(t0 + Duration::from_millis(600));
        assert_eq!(d.deadline(), Some(t0 + Duration::from_millis(1600)));
    }

    #[test]
    fn max_wait_caps_a_stream_of_edits() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(CFG);
        for ms in (0..=4800).step_by(300) {
            d.changed(t0 + Duration::from_millis(ms));
        }
        assert_eq!(d.deadline(), Some(t0 + CFG.max_wait));
    }

    #[test]
    fn reset_clears_pending() {
        let mut d = Debouncer::new(CFG);
        d.changed(Instant::now());
        d.reset();
        assert_eq!(d.deadline(), None);
    }
}
