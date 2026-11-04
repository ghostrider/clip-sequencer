//! User settings from the Preferences dialog, saved as `<data-dir>/settings.json`. Command-line
//! flags win over them for the run they are given in.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// MIDI input carrying the clock (substring of the port name, like `--midi-port`).
    #[serde(default)]
    pub midi_port: Option<String>,
    /// Resolume's plugin folder for the FFGL plugin, chosen in the install dialog (`None` = the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffgl_dir: Option<PathBuf>,
}

impl Settings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("settings.json")
    }

    /// The saved settings; defaults if there are none (or they cannot be read, which is logged).
    pub fn load(data_dir: &Path) -> Settings {
        let path = Self::path(data_dir);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("{}: {e}; using default settings", path.display());
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    /// Writes atomically (temp file, then rename).
    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::path(data_dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let s = Settings {
            midi_port: Some("Default App Loopback (B)".into()),
            ffgl_dir: Some(PathBuf::from("D:/Resolume/plugins")),
        };
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), s);
        std::fs::write(Settings::path(dir.path()), "not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }
}
