//! User settings from the Preferences dialog, saved as `<data-dir>/settings.json`. Command-line
//! flags win over them for the run they are given in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// At most this many saved themes.
pub const MAX_CUSTOM_THEMES: usize = 50;

/// A colour theme the user saved in Preferences (SPEC §9.7). The UI owns the meaning: `base` names a
/// built-in theme (its corners and label style), `colors` maps token names (`bg`, `accent`, …) to
/// `#RRGGBB`/`#RRGGBBAA`, `fonts` maps text roles (`ui`, `label`, `head`, `mono`) to font ids. The app
/// only checks the shape, so a newer UI can add tokens without an app change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub base: String,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub fonts: BTreeMap<String, String>,
}

/// A lower-case letter, then lower-case letters, digits and `-`; 1..=max characters.
fn is_key(s: &str, max: usize) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_hex_color(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() == 7 || b.len() == 9) && b[0] == b'#' && b[1..].iter().all(u8::is_ascii_hexdigit)
}

impl CustomTheme {
    /// Checks the shape and trims the name. Saved themes' ids start with `custom-`, so they never
    /// collide with a built-in theme.
    pub fn validated(mut self) -> Result<CustomTheme, String> {
        if !is_key(&self.id, 64) || !self.id.starts_with("custom-") {
            return Err(format!("invalid theme id {:?}", self.id));
        }
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() || self.name.chars().count() > 64 {
            return Err("a theme name needs 1 to 64 characters".into());
        }
        if !is_key(&self.base, 32) {
            return Err(format!("invalid base theme {:?}", self.base));
        }
        if self.colors.len() > 128 || self.fonts.len() > 16 {
            return Err("too many theme entries".into());
        }
        for (k, v) in &self.colors {
            if !is_key(k, 32) || !is_hex_color(v) {
                return Err(format!("invalid colour {k:?} = {v:?}"));
            }
        }
        for (k, v) in &self.fonts {
            if !is_key(k, 32) || !is_key(v, 64) {
                return Err(format!("invalid font {k:?} = {v:?}"));
            }
        }
        Ok(self)
    }
}

/// Checks a theme id sent by the UI (built-in or saved).
pub fn valid_theme_id(id: &str) -> bool {
    is_key(id, 64)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// MIDI input carrying the clock (substring of the port name, like `--midi-port`).
    #[serde(default)]
    pub midi_port: Option<String>,
    /// Resolume's plugin folder for the FFGL plugin, chosen in the install dialog (`None` = the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffgl_dir: Option<PathBuf>,
    /// Colour theme chosen in Preferences (`None` = the default theme).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Themes the user saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_themes: Vec<CustomTheme>,
}

/// Serialises read-modify-write of the settings file (Preferences, FFGL dialog, theme commands).
static UPDATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    /// Loads, changes and saves the settings under a lock, so concurrent changes do not undo each
    /// other. Blocking: call it from `spawn_blocking`. Returns what was saved.
    pub fn update<E: From<std::io::Error>>(
        data_dir: &Path,
        change: impl FnOnce(&mut Settings) -> Result<(), E>,
    ) -> Result<Settings, E> {
        let _guard = UPDATE.lock().unwrap_or_else(|e| e.into_inner());
        let mut settings = Settings::load(data_dir);
        change(&mut settings)?;
        settings.save(data_dir)?;
        Ok(settings)
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
            theme: Some("custom-1a2b".into()),
            custom_themes: vec![theme("custom-1a2b")],
        };
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), s);
        std::fs::write(Settings::path(dir.path()), "not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    fn theme(id: &str) -> CustomTheme {
        CustomTheme {
            id: id.into(),
            name: " Club Night ".into(),
            base: "rack".into(),
            colors: [
                ("accent".to_owned(), "#2FD8F0".to_owned()),
                ("scrim".to_owned(), "#000000AA".to_owned()),
            ]
            .into(),
            fonts: [("ui".to_owned(), "manrope".to_owned())].into(),
        }
    }

    #[test]
    fn older_settings_files_still_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            Settings::path(dir.path()),
            r#"{"midi_port":"Clock Shifter"}"#,
        )
        .unwrap();
        let s = Settings::load(dir.path());
        assert_eq!(s.midi_port.as_deref(), Some("Clock Shifter"));
        assert_eq!(s.theme, None);
        assert!(s.custom_themes.is_empty());
    }

    #[test]
    fn update_keeps_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        Settings::update::<std::io::Error>(dir.path(), |s| {
            s.midi_port = Some("A".into());
            Ok(())
        })
        .unwrap();
        let saved = Settings::update::<std::io::Error>(dir.path(), |s| {
            s.theme = Some("ink".into());
            Ok(())
        })
        .unwrap();
        assert_eq!(saved.midi_port.as_deref(), Some("A"));
        assert_eq!(Settings::load(dir.path()), saved);
    }

    #[test]
    fn theme_validation() {
        let t = theme("custom-1a2b").validated().unwrap();
        assert_eq!(t.name, "Club Night");
        let bad = |f: fn(&mut CustomTheme)| {
            let mut t = theme("custom-1a2b");
            f(&mut t);
            t.validated().is_err()
        };
        assert!(bad(|t| t.id = "Custom 1".into()));
        assert!(bad(|t| t.id = String::new()));
        assert!(bad(|t| t.id = "rack".into())); // built-in ids are not for saved themes
        assert!(bad(|t| t.name = "   ".into()));
        assert!(bad(|t| t.name = "x".repeat(65)));
        assert!(bad(|t| t.base = "../rack".into()));
        assert!(bad(|t| {
            t.colors.insert("accent".into(), "red".into());
        }));
        assert!(bad(|t| {
            t.colors.insert("accent".into(), "#12345".into());
        }));
        assert!(bad(|t| {
            t.colors.insert("--accent".into(), "#123456".into());
        }));
        assert!(bad(|t| {
            t.fonts.insert("ui".into(), "Comic Sans".into());
        }));
        assert!(valid_theme_id("rack") && valid_theme_id("custom-0f"));
        assert!(!valid_theme_id("") && !valid_theme_id("a b"));
    }
}
