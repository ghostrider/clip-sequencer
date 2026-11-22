//! LUT library (SPEC §14): named LUTs in `<data-dir>/luts/`, each as `<name>.lut.json` (the controls, versioned)
//! and `<name>.cube` (the baked table that Resolume's own LUT effect reads, notes §18).
//!
//! The library lives in memory behind a mutex; the UI server changes it and every change bumps a revision that the
//! UI sessions watch. Files are written by one writer task, never on the engine task: always a temp file renamed
//! over the old one (Resolume reloads a `.cube` by itself when it changes, also on a rename, and never sees a
//! half-written file), at most every [`WRITE_INTERVAL`] per LUT while a control is dragged, the last value always.
//! Pointing a Resolume LUT effect at a file is a `set` on its `LUT 3D` parameter, sent by the engine.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lut_core::{parse_cube, valid_lut_name, write_cube, Curves, Grade, LutDoc, Table};
use resolume_client::composition::LutEffect;
use serde::Serialize;
use tokio::sync::{mpsc, watch};

/// Shortest time between two `.cube` writes of the same LUT while it changes.
pub const WRITE_INTERVAL: Duration = Duration::from_millis(100);
/// A renamed LUT's old file is removed this long after the LUT effects were pointed at the new one, so Resolume
/// never looks at a missing file.
const OLD_FILE_GRACE: Duration = Duration::from_secs(2);
/// At most this many LUTs in the library.
pub const MAX_LUTS: usize = 200;

struct Entry {
    doc: LutDoc,
    table: Arc<Table>,
    /// Bumped on every change of this LUT (the UI fetches the table when it changes).
    rev: u64,
}

struct Inner {
    entries: BTreeMap<String, Entry>,
    next_rev: u64,
}

/// The library, shared by the UI server's handlers.
pub struct LutLibrary {
    dir: PathBuf,
    inner: Mutex<Inner>,
    changed: watch::Sender<u64>,
    writer: mpsc::UnboundedSender<Write>,
    /// Sets a LUT effect's `LUT 3D` parameter in Resolume (parameter id, path); `None` without Resolume.
    set_file: Option<SetFile>,
}

/// Sets a LUT effect's `LUT 3D` parameter: (parameter id, path).
pub type SetFile = Box<dyn Fn(u64, &str) + Send + Sync>;

enum Write {
    /// Write this LUT's files (throttled).
    Dirty(String),
    /// Delete this LUT's files now.
    Delete(String),
    /// Write the files of `to` at once, then delete those of `from` after [`OLD_FILE_GRACE`].
    Renamed { from: String, to: String },
    /// Write every pending file now; answers when done.
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// What the UI gets in a `luts` message.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LutsView {
    /// The folder of the `.cube` files (to tell which LUT effects use one of ours).
    pub dir: String,
    pub luts: Vec<LutView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LutView {
    pub name: String,
    pub rev: u64,
    pub grade: Grade,
    pub curves: Curves,
    /// An imported table is the base under the controls.
    pub has_base: bool,
    /// Full path of the `.cube` file.
    pub path: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LutError {
    #[error("{0:?} can't be a LUT name (no \\ / : * ? \" < > |, no leading or trailing dot or space, at most 64 characters)")]
    BadName(String),
    #[error("there is already a LUT called {0:?}")]
    Exists(String),
    #[error("there is no LUT called {0:?}")]
    NotFound(String),
    #[error("the library is full ({MAX_LUTS} LUTs)")]
    Full,
    #[error("not a .cube file: {0}")]
    BadCube(String),
    #[error("not connected to Resolume")]
    NoResolume,
}

impl LutLibrary {
    /// Opens the library in `<data_dir>/luts` (created if missing), loads every `.lut.json` and starts the writer.
    /// `.cube` files that are missing or differ from the baked table are rewritten. `set_file` points a LUT effect at
    /// a file in Resolume (`None` without Resolume).
    pub fn open(data_dir: &Path, set_file: Option<SetFile>) -> Arc<LutLibrary> {
        let dir =
            std::path::absolute(data_dir.join("luts")).unwrap_or_else(|_| data_dir.join("luts"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("LUT folder {}: {e}", dir.display());
        }
        let mut entries = BTreeMap::new();
        let mut next_rev = 1;
        for (name, doc) in load_all(&dir) {
            let table = Arc::new(doc.bake());
            entries.insert(
                name,
                Entry {
                    doc,
                    table,
                    rev: next_rev,
                },
            );
            next_rev += 1;
        }
        log::info!("LUT library {}: {} LUT(s)", dir.display(), entries.len());
        let (writer, rx) = mpsc::unbounded_channel();
        let lib = Arc::new(LutLibrary {
            dir,
            inner: Mutex::new(Inner { entries, next_rev }),
            changed: watch::channel(0).0,
            writer,
            set_file,
        });
        let names: Vec<String> = lib.inner.lock().unwrap().entries.keys().cloned().collect();
        tokio::spawn(writer_task(Arc::downgrade(&lib), rx));
        for name in names {
            // Only rewrites a .cube that differs (see `write_files`), so a restart doesn't make Resolume reload.
            let _ = lib.writer.send(Write::Dirty(name));
        }
        lib
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Bumped on every change: UI sessions watch it and send a new [`LutsView`].
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    pub fn cube_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.cube"))
    }

    pub fn view(&self) -> LutsView {
        let inner = self.inner.lock().unwrap();
        LutsView {
            dir: self.dir.display().to_string(),
            luts: inner
                .entries
                .iter()
                .map(|(name, e)| LutView {
                    name: name.clone(),
                    rev: e.rev,
                    grade: e.doc.grade.clone(),
                    curves: e.doc.curves.clone(),
                    has_base: e.doc.base.is_some(),
                    path: self.cube_path(name).display().to_string(),
                })
                .collect(),
        }
    }

    /// The baked table and its revision.
    pub fn table(&self, name: &str) -> Option<(u64, Arc<Table>)> {
        let inner = self.inner.lock().unwrap();
        inner.entries.get(name).map(|e| (e.rev, e.table.clone()))
    }

    /// The `.cube` text of a LUT (export).
    pub fn cube_text(&self, name: &str) -> Option<String> {
        self.table(name).map(|(_, t)| write_cube(name, &t))
    }

    fn insert(&self, inner: &mut Inner, name: String, doc: LutDoc) {
        let table = Arc::new(doc.bake());
        let rev = inner.next_rev;
        inner.next_rev += 1;
        inner
            .entries
            .insert(name.clone(), Entry { doc, table, rev });
        let _ = self.writer.send(Write::Dirty(name));
    }

    fn notify(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    fn check_new_name(inner: &Inner, name: &str) -> Result<(), LutError> {
        if !valid_lut_name(name) {
            return Err(LutError::BadName(name.to_owned()));
        }
        if inner.entries.keys().any(|n| n.eq_ignore_ascii_case(name)) {
            // Windows file names ignore case: "Warm" and "warm" would be one file.
            return Err(LutError::Exists(name.to_owned()));
        }
        if inner.entries.len() >= MAX_LUTS {
            return Err(LutError::Full);
        }
        Ok(())
    }

    /// `base` + " 2", " 3", … : the first free name.
    pub fn free_name(&self, base: &str) -> String {
        let inner = self.inner.lock().unwrap();
        let base: String = base
            .trim()
            .trim_matches('.')
            .chars()
            .filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_control())
            .take(56)
            .collect();
        let base = if base.trim().is_empty() {
            "LUT".to_owned()
        } else {
            base.trim().to_owned()
        };
        let taken =
            |n: &str| inner.entries.keys().any(|k| k.eq_ignore_ascii_case(n)) || !valid_lut_name(n);
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .expect("a free name")
    }

    /// A new LUT: a copy of `from`, or one that changes nothing.
    pub fn create(&self, name: &str, from: Option<&str>) -> Result<(), LutError> {
        let name = name.trim();
        let mut inner = self.inner.lock().unwrap();
        Self::check_new_name(&inner, name)?;
        let doc = match from {
            Some(f) => inner
                .entries
                .get(f)
                .ok_or_else(|| LutError::NotFound(f.to_owned()))?
                .doc
                .clone(),
            None => LutDoc::default(),
        };
        self.insert(&mut inner, name.to_owned(), doc);
        drop(inner);
        log::info!(
            "LUT {name:?} created{}",
            from.map(|f| format!(" from {f:?}")).unwrap_or_default()
        );
        self.notify();
        Ok(())
    }

    /// Imports a `.cube` file as a new LUT (the table becomes its base).
    pub fn import(&self, name: &str, cube: &str) -> Result<(), LutError> {
        let (_, table) = parse_cube(cube).map_err(|e| LutError::BadCube(e.to_string()))?;
        let name = name.trim();
        let mut inner = self.inner.lock().unwrap();
        Self::check_new_name(&inner, name)?;
        self.insert(&mut inner, name.to_owned(), LutDoc::from_base(table));
        drop(inner);
        log::info!("LUT {name:?} imported");
        self.notify();
        Ok(())
    }

    /// New controls for a LUT (live while dragging). The imported base stays.
    pub fn update(&self, name: &str, grade: Grade, curves: Curves) -> Result<(), LutError> {
        let mut inner = self.inner.lock().unwrap();
        let mut doc = inner
            .entries
            .get(name)
            .ok_or_else(|| LutError::NotFound(name.to_owned()))?
            .doc
            .clone();
        if doc.grade == grade && doc.curves == curves {
            return Ok(());
        }
        doc.grade = grade;
        doc.curves = curves;
        self.insert(&mut inner, name.to_owned(), doc);
        drop(inner);
        self.notify();
        Ok(())
    }

    /// Drops the imported base table (the LUT then is the controls alone).
    pub fn clear_base(&self, name: &str) -> Result<(), LutError> {
        let mut inner = self.inner.lock().unwrap();
        let mut doc = inner
            .entries
            .get(name)
            .ok_or_else(|| LutError::NotFound(name.to_owned()))?
            .doc
            .clone();
        if doc.base.take().is_none() {
            return Ok(());
        }
        self.insert(&mut inner, name.to_owned(), doc);
        drop(inner);
        self.notify();
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<(), LutError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .entries
            .remove(name)
            .ok_or_else(|| LutError::NotFound(name.to_owned()))?;
        let _ = self.writer.send(Write::Delete(name.to_owned()));
        drop(inner);
        log::info!("LUT {name:?} deleted");
        self.notify();
        Ok(())
    }

    /// Renames a LUT and points the LUT effects that use its file (`effects`, from the composition) at the new
    /// file. The old file is removed a little later.
    pub fn rename(&self, from: &str, to: &str, effects: &[LutEffect]) -> Result<usize, LutError> {
        let to = to.trim();
        if from == to {
            return Ok(0);
        }
        let mut inner = self.inner.lock().unwrap();
        if !inner.entries.contains_key(from) {
            return Err(LutError::NotFound(from.to_owned()));
        }
        let case_only = from.eq_ignore_ascii_case(to);
        if !case_only {
            Self::check_new_name(&inner, to)?;
        } else if !valid_lut_name(to) {
            return Err(LutError::BadName(to.to_owned()));
        }
        let mut e = inner.entries.remove(from).expect("checked");
        e.rev = inner.next_rev;
        inner.next_rev += 1;
        inner.entries.insert(to.to_owned(), e);
        drop(inner);
        let old = self.cube_path(from);
        let users: Vec<&LutEffect> = effects
            .iter()
            .filter(|e| same_path(&e.path, &old))
            .collect();
        if case_only {
            // One file on Windows: write it under the new spelling, nothing to delete.
            let _ = self.writer.send(Write::Dirty(to.to_owned()));
        } else {
            let _ = self.writer.send(Write::Renamed {
                from: from.to_owned(),
                to: to.to_owned(),
            });
        }
        let new = self.cube_path(to).display().to_string();
        for e in &users {
            self.set_file(e.file_param, &new);
        }
        log::info!(
            "LUT {from:?} renamed to {to:?} ({} LUT effect(s) pointed at the new file)",
            users.len()
        );
        self.notify();
        Ok(users.len())
    }

    /// Points one of Resolume's LUT effects at a library LUT. The file is written first.
    pub async fn apply(&self, name: &str, file_param: u64) -> Result<(), LutError> {
        if self.set_file.is_none() {
            return Err(LutError::NoResolume);
        }
        if self.table(name).is_none() {
            return Err(LutError::NotFound(name.to_owned()));
        }
        self.flush().await;
        let path = self.cube_path(name).display().to_string();
        log::info!("LUT effect (parameter {file_param}) set to {path}");
        self.set_file(file_param, &path);
        Ok(())
    }

    fn set_file(&self, file_param: u64, path: &str) {
        if let Some(f) = &self.set_file {
            f(file_param, path);
        }
    }

    /// Waits until every pending file is written.
    pub async fn flush(&self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if self.writer.send(Write::Flush(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

/// Whether a LUT effect's path names `file` (Windows: case and slash direction don't matter).
pub fn same_path(path: &str, file: &Path) -> bool {
    let norm = |s: &str| s.replace('/', "\\").to_lowercase();
    !path.is_empty() && norm(path) == norm(&file.display().to_string())
}

fn load_all(dir: &Path) -> Vec<(String, LutDoc)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".lut.json"))
        else {
            continue;
        };
        if !valid_lut_name(name) {
            log::warn!("{}: not a valid LUT name, skipped", path.display());
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| LutDoc::from_json(&t).map_err(|e| e.to_string()))
        {
            Ok(doc) => out.push((name.to_owned(), doc)),
            Err(e) => log::warn!("{}: {e} (skipped)", path.display()),
        }
    }
    out
}

/// Writes `text` to `path` through `<path>.tmp` and a rename; skipped when the file already holds exactly that.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == text.as_bytes()) {
        return Ok(false);
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, text)?;
    if let Err(e) = replace_file(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(true)
}

/// Tries to rename for this long while another program holds the target open.
const RENAME_TRIES: u32 = 10;
const RENAME_PAUSE: Duration = Duration::from_millis(15);

/// Renames `tmp` over `path`. Windows refuses to replace a file another program has open without delete sharing
/// ("access denied", os error 5, or a sharing violation, 32): Resolume does that for a moment while it reloads a
/// `.cube`. Such errors are retried for about 150 ms.
fn replace_file(tmp: &Path, path: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if is_busy(&e) && tries + 1 < RENAME_TRIES => {
                tries += 1;
                std::thread::sleep(RENAME_PAUSE);
            }
            Err(e) => return Err(e),
        }
    }
}

/// The file is in use by another program (it may work a moment later).
fn is_busy(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(32)
}

/// Writes both files of a LUT; the error of the first that failed, if any.
fn write_files(dir: &Path, name: &str, doc: &LutDoc, table: &Table) -> Result<(), String> {
    let json = dir.join(format!("{name}.lut.json"));
    let cube = dir.join(format!("{name}.cube"));
    let a = write_atomic(&json, &(doc.to_json() + "\n"))
        .map_err(|e| format!("{}: {e}", json.display()));
    let b = write_atomic(&cube, &write_cube(name, table))
        .map_err(|e| format!("{}: {e}", cube.display()));
    a.and(b).map(|_| ())
}

fn delete_files(dir: &Path, name: &str) {
    for ext in ["lut.json", "cube"] {
        let p = dir.join(format!("{name}.{ext}"));
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("{}: {e}", p.display()),
        }
    }
}

/// Writes and deletes the library's files, off the UI handlers and the engine.
async fn writer_task(lib: std::sync::Weak<LutLibrary>, mut rx: mpsc::UnboundedReceiver<Write>) {
    // Per LUT: when its files are due, and when they were last written (the throttle).
    let mut due: HashMap<String, Instant> = HashMap::new();
    let mut last: HashMap<String, Instant> = HashMap::new();
    let mut flushes: Vec<tokio::sync::oneshot::Sender<()>> = Vec::new();
    // LUTs whose last write failed (file in use): retried until it works, logged once per streak.
    let mut failing: HashMap<String, u32> = HashMap::new();
    loop {
        let msg = if !flushes.is_empty() {
            rx.try_recv().ok() // a flush waits: no sleeping
        } else if let Some(at) = due.values().min().copied() {
            tokio::select! {
                m = rx.recv() => match m { Some(m) => Some(m), None => return },
                _ = tokio::time::sleep_until(at.into()) => None,
            }
        } else {
            match rx.recv().await {
                Some(m) => Some(m),
                None => return,
            }
        };
        let mut write_now: Vec<String> = Vec::new();
        let mut delete_now: Vec<String> = Vec::new();
        match msg {
            Some(Write::Dirty(name)) => {
                let now = Instant::now();
                let at = last
                    .get(&name)
                    .map_or(now, |t| (*t + WRITE_INTERVAL).max(now));
                due.entry(name).or_insert(at);
            }
            Some(Write::Delete(name)) => {
                due.remove(&name);
                delete_now.push(name);
            }
            Some(Write::Renamed { from, to }) => {
                due.remove(&from);
                due.remove(&to);
                write_now.push(to);
                tokio::spawn(delete_later(lib.clone(), from));
            }
            Some(Write::Flush(tx)) => flushes.push(tx),
            None => {}
        }
        let now = Instant::now();
        let flushing = !flushes.is_empty();
        due.retain(|n, t| {
            let ready = flushing || *t <= now;
            if ready {
                write_now.push(n.clone());
            }
            !ready
        });
        let Some(l) = lib.upgrade() else { return };
        let dir = l.dir.clone();
        let jobs: Vec<(String, LutDoc, Arc<Table>)> = {
            let inner = l.inner.lock().unwrap();
            write_now
                .into_iter()
                .filter_map(|n| {
                    let e = inner.entries.get(&n)?;
                    Some((n, e.doc.clone(), e.table.clone()))
                })
                .collect()
        };
        drop(l);
        let results: Vec<(String, Result<(), String>)> = tokio::task::spawn_blocking(move || {
            for n in delete_now {
                delete_files(&dir, &n);
            }
            jobs.into_iter()
                .map(|(n, doc, table)| {
                    let r = write_files(&dir, &n, &doc, &table);
                    (n, r)
                })
                .collect()
        })
        .await
        .unwrap_or_default();
        let t = Instant::now();
        for (n, r) in results {
            last.insert(n.clone(), t);
            match r {
                Ok(()) => {
                    if let Some(tries) = failing.remove(&n) {
                        log::info!("LUT {n:?} written after {tries} failed attempt(s)");
                    }
                }
                Err(e) => {
                    let tries = failing.entry(n.clone()).or_insert(0);
                    if *tries == 0 {
                        log::warn!("{e} (the file is in use, probably by Resolume; trying again)");
                    }
                    *tries += 1;
                    // Not given up: the newest values are written as soon as the file is free.
                    due.entry(n).or_insert(t + WRITE_INTERVAL);
                }
            }
        }
        if flushing && rx.is_empty() {
            for tx in flushes.drain(..) {
                let _ = tx.send(());
            }
        }
    }
}

/// Removes a renamed LUT's old files after [`OLD_FILE_GRACE`], unless a LUT of that name exists again by then.
async fn delete_later(lib: std::sync::Weak<LutLibrary>, name: String) {
    tokio::time::sleep(OLD_FILE_GRACE).await;
    let Some(l) = lib.upgrade() else { return };
    if l.inner
        .lock()
        .unwrap()
        .entries
        .keys()
        .any(|k| k.eq_ignore_ascii_case(&name))
    {
        return;
    }
    let dir = l.dir.clone();
    drop(l);
    let _ = tokio::task::spawn_blocking(move || delete_files(&dir, &name)).await;
}
