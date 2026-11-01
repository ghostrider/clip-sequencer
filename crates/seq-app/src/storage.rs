//! Sequence files on disk: `<data-dir>/<Composition>.seq.json` (SPEC §6).
//!
//! Saves are atomic: the JSON goes to `<file>.tmp`, is flushed to disk, then renamed over the
//! old file, so a crash never leaves a half-written file. Loading never modifies anything.
//!
//! A version 1 file (lengths in eighths) is migrated in memory on load. The first save over it
//! copies it to `<file>.v1.bak` first, so the original stays readable by older builds.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use seq_core::{file_version, from_json, to_json, validate, LoadError, SeqFile, ValidationError};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Load { path: PathBuf, source: LoadError },
    /// Two composition names map to the same file name (e.g. `A/B` and `A_B`).
    #[error("{path} belongs to composition {found:?}, not {expected:?}")]
    CompositionMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },
    #[error("refusing to save an invalid sequence file:\n{}", list(.0))]
    Invalid(Vec<ValidationError>),
}

fn list(errors: &[ValidationError]) -> String {
    errors
        .iter()
        .map(|e| format!("  - {e}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result of [`Storage::load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// The file existed and is valid.
    Existing(SeqFile),
    /// No file yet; an empty one for this composition (not written until the first save).
    New(SeqFile),
}

impl Loaded {
    pub fn into_file(self) -> SeqFile {
        match self {
            Loaded::Existing(f) | Loaded::New(f) => f,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Storage {
    dir: PathBuf,
}

impl Storage {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, composition: &str) -> PathBuf {
        self.dir
            .join(format!("{}.seq.json", file_stem_for(composition)))
    }

    /// Loads the file for `composition`. A missing file is not an error. An unreadable or invalid
    /// file is, and is left untouched so nothing the user wrote is lost.
    pub fn load(&self, composition: &str) -> Result<Loaded, StorageError> {
        let path = self.path_for(composition);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Loaded::New(SeqFile::new(composition)))
            }
            Err(source) => return Err(StorageError::Io { path, source }),
        };
        let file = load_text(&path, &text)?;
        if file.composition != composition {
            return Err(StorageError::CompositionMismatch {
                path,
                expected: composition.to_owned(),
                found: file.composition,
            });
        }
        Ok(Loaded::Existing(file))
    }

    /// Validates and atomically writes `file`. Returns the path written.
    pub fn save(&self, file: &SeqFile) -> Result<PathBuf, StorageError> {
        validate(file).map_err(StorageError::Invalid)?;
        let path = self.path_for(&file.composition);
        let io = |path: &Path| {
            let path = path.to_owned();
            move |source| StorageError::Io { path, source }
        };
        fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        backup_v1(&path).map_err(io(&path))?;
        let tmp = tmp_path(&path);
        {
            let mut f = fs::File::create(&tmp).map_err(io(&tmp))?;
            f.write_all(to_json(file).as_bytes()).map_err(io(&tmp))?;
            f.sync_all().map_err(io(&tmp))?;
        }
        rename_with_retry(&tmp, &path).map_err(io(&path))?;
        Ok(path)
    }
}

/// Parses file content read from `path` (used by `load` and `seq-app --validate`).
pub fn load_text(path: &Path, text: &str) -> Result<SeqFile, StorageError> {
    from_json(text).map_err(|source| StorageError::Load {
        path: path.to_owned(),
        source,
    })
}

/// Copies a version 1 file at `path` to `<path>.v1.bak` before it is overwritten in the current
/// format. An existing backup is kept (it is the oldest original).
fn backup_v1(path: &Path) -> std::io::Result<()> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let backup = suffixed(path, ".v1.bak");
    if file_version(&text) == Some(1) && !backup.exists() {
        fs::write(&backup, text)?;
        log::info!("kept the version 1 sequence file as {}", backup.display());
    }
    Ok(())
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn tmp_path(path: &Path) -> PathBuf {
    suffixed(path, ".tmp")
}

/// On Windows a rename onto a file that another process (virus scanner, indexer, an editor) has
/// open briefly fails with a sharing violation; retry a few times before giving up.
fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    const ATTEMPTS: u32 = 5;
    let mut attempt = 1;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < ATTEMPTS => {
                std::thread::sleep(Duration::from_millis(20 * u64::from(attempt)));
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Turns a composition name into a file name stem that is valid on Windows.
pub fn file_stem_for(composition: &str) -> String {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let mut stem: String = composition
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    stem = stem.trim_end_matches(['.', ' ']).trim_start().to_owned();
    if stem.is_empty() {
        stem.push('_');
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&stem)) {
        stem.insert(0, '_');
    }
    stem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stems_are_windows_safe() {
        assert_eq!(file_stem_for("MyShow"), "MyShow");
        assert_eq!(
            file_stem_for("Act 1: Intro / Drop?"),
            "Act 1_ Intro _ Drop_"
        );
        assert_eq!(file_stem_for("show. "), "show");
        assert_eq!(file_stem_for(""), "_");
        assert_eq!(file_stem_for("con"), "_con");
        assert_eq!(file_stem_for("§ Ümlaut"), "§ Ümlaut");
    }

    #[test]
    fn tmp_path_appends_suffix() {
        assert_eq!(
            tmp_path(Path::new("d/MyShow.seq.json")),
            PathBuf::from("d/MyShow.seq.json.tmp")
        );
    }
}
