//! Command-line configuration (SPEC §11). `seq.toml` support is not implemented yet.

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "seq-app", about = "Resolume clip sequencer", version = crate::LONG_VERSION)]
pub struct Config {
    /// MIDI input port carrying the clock (substring of the port name).
    #[arg(long)]
    pub midi_port: Option<String>,
    #[arg(long, default_value = "127.0.0.1")]
    pub resolume_host: String,
    #[arg(long, default_value_t = 8080)]
    pub rest_port: u16,
    #[arg(long, default_value_t = 7000)]
    pub osc_port: u16,
    /// UI server port; if taken, the next 9 ports, then one the OS assigns (see --strict-port).
    #[arg(long, default_value_t = 8090)]
    pub ui_port: u16,
    /// No window: log to the console, UI in a browser (always on in seq-app-cli; the only mode
    /// without the `desktop` build feature).
    #[arg(long)]
    pub headless: bool,
    /// Desktop app: no splash screen; the window appears when the UI is ready.
    #[arg(long)]
    pub no_splash: bool,
    /// Exit with an error instead of falling back when --ui-port is taken.
    #[arg(long)]
    pub strict_port: bool,
    /// Name of the Resolume layer holding the `§` placeholder clips.
    #[arg(long, default_value = "SEQ")]
    pub seq_layer: String,
    /// Prefix that marks placeholder clip names.
    #[arg(long, default_value = "§ ")]
    pub seq_prefix: String,
    /// Where `<Composition>.seq.json` files live. Default: `%APPDATA%\clip-sequencer` (moved there
    /// from the older `%APPDATA%\resolume-sequencer` on first start, see [`migrate_data_dir`]).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Check a sequence file and exit (0 = valid, 1 = problems found).
    #[arg(long, value_name = "FILE")]
    pub validate: Option<PathBuf>,
    /// Print the MIDI input ports and exit.
    #[arg(long)]
    pub list_midi_ports: bool,
    /// Print the incoming clock live (bar.beat.tick, BPM) instead of running the sequencer.
    #[arg(long)]
    pub clock_debug: bool,
    /// Start this sequence (by name, live deck preferred) once connected. For testing.
    #[arg(long, value_name = "NAME")]
    pub start: Option<String>,
    /// Exit when no sequence is running any more (after --start). For testing.
    #[arg(long)]
    pub exit_when_done: bool,
    /// Resolume's plugin folder for the FFGL plugin. Default: the folder chosen in the app, else
    /// `<Documents>\Resolume Arena\Extra Effects`.
    #[arg(long, value_name = "DIR")]
    pub ffgl_dir: Option<PathBuf>,
    /// Install or update the bundled FFGL plugin in Resolume's plugin folder and exit (0 = done).
    #[arg(long)]
    pub install_ffgl: bool,
}

impl Config {
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(default_data_dir)
    }
}

/// Folder name of the data directory under `%APPDATA%`.
pub const DATA_DIR_NAME: &str = "clip-sequencer";
/// What it was called before the rename to Clip Sequencer.
pub const LEGACY_DATA_DIR_NAME: &str = "resolume-sequencer";

fn default_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|appdata| PathBuf::from(appdata).join(DATA_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from("data"))
}

/// Outcome of [`migrate_data_dir`], logged once logging runs.
#[derive(Debug)]
pub enum DataDirMigration {
    /// Nothing to do: no old folder, or `--data-dir` was given.
    None,
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// Both folders exist; the new one is used and the old one left alone.
    BothExist {
        old: PathBuf,
        new: PathBuf,
    },
    /// The rename failed (e.g. an older build still runs from the old folder); this run keeps
    /// using the old folder.
    Failed {
        from: PathBuf,
        to: PathBuf,
        error: std::io::Error,
    },
}

impl DataDirMigration {
    pub fn log(&self) {
        match self {
            Self::None => {}
            Self::Moved { from, to } => {
                log::info!("data folder moved: {} -> {}", from.display(), to.display())
            }
            Self::BothExist { old, new } => log::warn!(
                "old data folder {} still exists next to {}; it is not used any more",
                old.display(),
                new.display()
            ),
            Self::Failed { from, to, error } => log::warn!(
                "could not move the data folder {} to {} ({error}); using the old folder for now",
                from.display(),
                to.display()
            ),
        }
    }
}

/// Moves the old default data folder (`%APPDATA%\resolume-sequencer`) to the new one, once, before
/// anything opens files in it. Only when no `--data-dir` is given. On failure the config is
/// pointed at the old folder so sequences are not lost from view.
pub fn migrate_data_dir(config: &mut Config) -> DataDirMigration {
    if config.data_dir.is_some() {
        return DataDirMigration::None;
    }
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        return DataDirMigration::None;
    };
    let result = migrate_dir(
        &appdata.join(LEGACY_DATA_DIR_NAME),
        &appdata.join(DATA_DIR_NAME),
    );
    if let DataDirMigration::Failed { from, .. } = &result {
        config.data_dir = Some(from.clone());
    }
    result
}

/// Renames `old` to `new` when only `old` exists. A rename keeps every file (sequences, their
/// `.v1.bak` copies, settings, logs, caches) and leaves no second copy to drift apart.
pub fn migrate_dir(old: &std::path::Path, new: &std::path::Path) -> DataDirMigration {
    match (old.is_dir(), new.exists()) {
        (false, _) => DataDirMigration::None,
        (true, true) => DataDirMigration::BothExist {
            old: old.to_path_buf(),
            new: new.to_path_buf(),
        },
        (true, false) => {
            let moved = new
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::rename(old, new));
            match moved {
                Ok(()) => DataDirMigration::Moved {
                    from: old.to_path_buf(),
                    to: new.to_path_buf(),
                },
                Err(error) => DataDirMigration::Failed {
                    from: old.to_path_buf(),
                    to: new.to_path_buf(),
                    error,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::parse_from(["seq-app"]);
        assert_eq!(c.resolume_host, "127.0.0.1");
        assert_eq!((c.rest_port, c.osc_port, c.ui_port), (8080, 7000, 8090));
        assert_eq!(c.seq_layer, "SEQ");
        assert_eq!(c.seq_prefix, "§ ");
    }

    #[test]
    fn old_data_dir_is_moved_once() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join(LEGACY_DATA_DIR_NAME);
        let new = tmp.path().join(DATA_DIR_NAME);
        assert!(matches!(migrate_dir(&old, &new), DataDirMigration::None));

        std::fs::create_dir_all(old.join("logs")).unwrap();
        std::fs::write(old.join("dev.seq.json"), "{}").unwrap();
        assert!(matches!(
            migrate_dir(&old, &new),
            DataDirMigration::Moved { .. }
        ));
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(new.join("dev.seq.json")).unwrap(),
            "{}"
        );
        assert!(new.join("logs").is_dir());

        // Second start: nothing left to move.
        assert!(matches!(migrate_dir(&old, &new), DataDirMigration::None));
        // An old folder reappearing next to the new one is left alone.
        std::fs::create_dir_all(&old).unwrap();
        assert!(matches!(
            migrate_dir(&old, &new),
            DataDirMigration::BothExist { .. }
        ));
        assert!(new.join("dev.seq.json").exists());
    }

    #[test]
    fn explicit_data_dir_is_never_migrated() {
        let mut c = Config::parse_from(["seq-app", "--data-dir", "x/y"]);
        assert!(matches!(migrate_data_dir(&mut c), DataDirMigration::None));
        assert_eq!(c.data_dir(), PathBuf::from("x/y"));
    }

    #[test]
    fn data_dir_override() {
        let c = Config::parse_from(["seq-app", "--data-dir", "x/y"]);
        assert_eq!(c.data_dir(), PathBuf::from("x/y"));
    }
}
