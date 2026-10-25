//! Storage and autosave against a real temp directory.

use std::fs;
use std::time::Duration;

use seq_app::saver::{self, DebounceConfig, SaveEvent};
use seq_app::storage::{Loaded, Storage, StorageError};
use seq_core::{from_json, SeqFile};
use tokio::sync::mpsc;

const FULL: &str = include_str!("../../seq-core/tests/fixtures/full.seq.json");

fn full() -> SeqFile {
    from_json(FULL).unwrap()
}

fn renamed(mut f: SeqFile, name: &str) -> SeqFile {
    f.decks.get_mut("2132323").unwrap().sequences[0].name = name.into();
    f
}

#[test]
fn missing_file_loads_as_new_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path().join("not-created-yet"));
    assert_eq!(
        storage.load("dev").unwrap(),
        Loaded::New(SeqFile::new("dev"))
    );
    assert!(
        !dir.path().join("not-created-yet").exists(),
        "load must not create anything"
    );
}

#[test]
fn save_then_load_round_trips_and_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path().join("nested/data"));
    let path = storage.save(&full()).unwrap();
    assert_eq!(path, dir.path().join("nested/data/dev.seq.json"));
    assert_eq!(storage.load("dev").unwrap(), Loaded::Existing(full()));

    // Overwriting an existing file works and replaces it completely.
    let edited = renamed(full(), "Renamed");
    storage.save(&edited).unwrap();
    assert_eq!(storage.load("dev").unwrap().into_file(), edited);

    let names: Vec<_> = fs::read_dir(dir.path().join("nested/data"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec!["dev.seq.json"]);
}

#[test]
fn invalid_file_on_disk_is_reported_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path());
    let path = storage.path_for("dev");
    let broken = FULL.replace("\"length\": 32", "\"length\": 0");
    fs::write(&path, &broken).unwrap();

    let err = storage.load("dev").unwrap_err();
    assert!(matches!(err, StorageError::Load { .. }), "{err}");
    assert!(err.to_string().contains("steps[0].length"), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
}

#[test]
fn saving_an_invalid_file_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path());
    let mut f = full();
    f.decks.get_mut("2132323").unwrap().sequences[0].lanes[0].steps[0].length = 0;
    assert!(matches!(storage.save(&f), Err(StorageError::Invalid(_))));
    assert!(!storage.path_for("dev").exists());
}

#[test]
fn colliding_file_names_are_detected() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path());
    storage.save(&SeqFile::new("A/B")).unwrap();
    assert_eq!(storage.path_for("A/B"), storage.path_for("A_B"));
    assert!(matches!(
        storage.load("A_B"),
        Err(StorageError::CompositionMismatch { .. })
    ));
}

const FAST: DebounceConfig = DebounceConfig {
    quiet: Duration::from_millis(60),
    max_wait: Duration::from_millis(400),
};

#[tokio::test]
async fn burst_of_edits_is_saved_once_with_the_latest_content() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path());
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let (handle, task) = saver::spawn(storage.clone(), FAST, Some(events_tx));

    for i in 0..5 {
        handle.changed(renamed(full(), &format!("edit {i}")));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !storage.path_for("dev").exists(),
        "saved before edits paused"
    );

    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event, SaveEvent::Saved(storage.path_for("dev")));
    assert_eq!(
        storage.load("dev").unwrap().into_file(),
        renamed(full(), "edit 4")
    );

    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(events.try_recv().is_err(), "expected exactly one save");
    drop(handle);
    task.await.unwrap();
}

#[tokio::test]
async fn continuous_edits_are_saved_by_max_wait() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path());
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let (handle, _task) = saver::spawn(storage.clone(), FAST, Some(events_tx));

    // An edit every 30 ms never leaves a 60 ms pause; max_wait (400 ms) must kick in.
    let started = std::time::Instant::now();
    let mut saved_at = None;
    for i in 0..40 {
        handle.changed(renamed(full(), &format!("edit {i}")));
        tokio::time::sleep(Duration::from_millis(30)).await;
        if events.try_recv().is_ok() {
            saved_at = Some(started.elapsed());
            break;
        }
    }
    let saved_at = saved_at.expect("never saved during continuous edits");
    assert!(
        saved_at < Duration::from_millis(1000),
        "saved after {saved_at:?}"
    );
}

#[tokio::test]
async fn flush_saves_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path());
    let slow = DebounceConfig {
        quiet: Duration::from_secs(60),
        max_wait: Duration::from_secs(60),
    };
    let (handle, _task) = saver::spawn(storage.clone(), slow, None);
    handle.changed(full());
    handle.flush().await.unwrap();
    assert_eq!(storage.load("dev").unwrap().into_file(), full());
    // Nothing pending: flush is a no-op success.
    handle.flush().await.unwrap();
}

#[tokio::test]
async fn dropping_the_handle_saves_pending_changes() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path());
    let slow = DebounceConfig {
        quiet: Duration::from_secs(60),
        max_wait: Duration::from_secs(60),
    };
    let (handle, task) = saver::spawn(storage.clone(), slow, None);
    handle.changed(full());
    drop(handle);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(storage.load("dev").unwrap().into_file(), full());
}

#[tokio::test]
async fn failed_save_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    // The data dir path is an existing *file*, so creating the directory fails.
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "x").unwrap();
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let (handle, _task) = saver::spawn(Storage::new(&blocker), FAST, Some(events_tx));
    handle.changed(full());
    assert!(handle.flush().await.is_err());
    assert!(matches!(events.recv().await, Some(SaveEvent::Failed(_))));
}
