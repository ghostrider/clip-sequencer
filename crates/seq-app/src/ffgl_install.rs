//! Installs the bundled "1000A Sequence" FFGL plugin (`SeqView.dll`) into Resolume's plugin folder, updates
//! and removes it (SPEC §12.1, M11 Phase G3).
//!
//! - The bundled DLL ships next to the executables as `ffgl/SeqView.dll` (release zip).
//! - The target folder is `--ffgl-dir`, else the one saved in the settings, else
//!   `<Documents>\Resolume Arena\Extra Effects` (Documents through the Known Folder API: `dirs`).
//! - Versions come from the DLL's Windows version resource (`VS_FIXEDFILEINFO`), parsed here from the file's
//!   bytes, so the logic is the same (and tested) on every platform.
//! - Only our own `SeqView.dll` is ever replaced or removed; nothing changes while Resolume runs (it locks
//!   loaded DLLs); a failed copy restores the previous DLL from `SeqView.dll.bak`.

use std::cmp::Ordering;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// File name of the plugin, bundled and installed.
pub const PLUGIN_FILE: &str = "SeqView.dll";
/// Display name of the plugin in Resolume.
pub const PLUGIN_NAME: &str = "1000A Sequence";

/// `FILEVERSION` of a DLL: major, minor, patch, build number (SPEC §12.1, Phase E).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DllVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: u16,
}

impl fmt::Display for DllVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{} (build {})",
            self.major, self.minor, self.patch, self.build
        )
    }
}

/// Reads `FILEVERSION` from a PE file's version resource: the `VS_FIXEDFILEINFO` block, found by its
/// signature `0xFEEF04BD` right after the UTF-16 key `VS_VERSION_INFO`. `None` without a version resource.
pub fn dll_version(bytes: &[u8]) -> Option<DllVersion> {
    let key: Vec<u8> = "VS_VERSION_INFO\0"
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let at = find(bytes, &key)?;
    // The fixed info follows the key, aligned to 4 bytes; look for its signature within a few bytes.
    let from = at + key.len();
    let sig = 0xFEEF_04BDu32.to_le_bytes();
    let rel = find(bytes.get(from..(from + 16).min(bytes.len()))?, &sig)?;
    let fixed = from + rel;
    let u32_at = |off: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(fixed + off..fixed + off + 4)?.try_into().ok()?,
        ))
    };
    // dwSignature, dwStrucVersion, dwFileVersionMS, dwFileVersionLS
    let ms = u32_at(8)?;
    let ls = u32_at(12)?;
    Some(DllVersion {
        major: (ms >> 16) as u16,
        minor: ms as u16,
        patch: (ls >> 16) as u16,
        build: ls as u16,
    })
}

/// Whether `bytes` is our plugin: its version resource names the product (UTF-16), or (early builds without a
/// version resource) it contains the plugin id `VJSQ` and the name `SeqView`.
pub fn is_our_plugin(bytes: &[u8]) -> bool {
    let product: Vec<u8> = "Clip Sequencer for Resolume"
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    find(bytes, &product).is_some()
        || (find(bytes, b"VJSQ").is_some() && find(bytes, b"SeqView").is_some())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Resolume's per-user plugin folder: `<Documents>\Resolume Arena\Extra Effects`.
pub fn default_dir() -> Option<PathBuf> {
    dirs::document_dir().map(|d| default_dir_in(&d))
}

/// The default folder under a given Documents folder (separate for tests).
pub fn default_dir_in(documents: &Path) -> PathBuf {
    documents.join("Resolume Arena").join("Extra Effects")
}

/// Where to install: `--ffgl-dir`, else the folder saved in the settings, else the default.
pub fn target_dir(
    cli: Option<&Path>,
    saved: Option<&Path>,
    default: Option<PathBuf>,
) -> Option<PathBuf> {
    cli.or(saved).map(Path::to_path_buf).or(default)
}

/// The bundled plugin: `<exe folder>/ffgl/SeqView.dll` (release zip). Debug builds also find a local
/// `ffgl/bin/x64/Release/SeqView.dll` in the repository.
pub fn bundled_plugin() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let mut candidates = vec![exe_dir.join("ffgl").join(PLUGIN_FILE)];
    if cfg!(debug_assertions) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ffgl/bin/x64/Release");
        candidates.push(repo.join(PLUGIN_FILE));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// What the primary button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Nothing installed yet.
    Install,
    /// The bundled plugin is newer (or the installed one has no version).
    Update,
    /// The same version.
    Reinstall,
    /// The bundled plugin is older: only after a confirmation.
    Downgrade,
}

/// Bundled vs. installed version → the action. `installed: None` = nothing there; `Some(None)` = a DLL
/// without a version resource (counts as older).
pub fn action_for(bundled: DllVersion, installed: Option<Option<DllVersion>>) -> Action {
    match installed {
        None => Action::Install,
        Some(None) => Action::Update,
        Some(Some(v)) => match bundled.cmp(&v) {
            Ordering::Greater => Action::Update,
            Ordering::Equal => Action::Reinstall,
            Ordering::Less => Action::Downgrade,
        },
    }
}

/// The installed DLL in a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    pub version: Option<DllVersion>,
    /// Our plugin (only then may it be replaced or removed).
    pub ours: bool,
}

/// What the dialog shows (`GET /api/ffgl`).
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub plugin_name: &'static str,
    /// The bundled DLL and its version; `None` if this build has none (development builds).
    pub bundled_path: Option<PathBuf>,
    pub bundled_version: Option<DllVersion>,
    /// The target folder, and whether it is the default one.
    pub dir: Option<PathBuf>,
    pub dir_is_default: bool,
    pub dir_exists: bool,
    /// `None` = no `SeqView.dll` in the folder.
    pub installed: Option<Installed>,
    /// The primary action, if one is possible.
    pub action: Option<Action>,
    pub resolume_running: bool,
}

/// Reads the state of `dir` against the bundled plugin.
pub fn status(
    bundled: Option<&Path>,
    dir: Option<&Path>,
    default: Option<&Path>,
    resolume_running: bool,
) -> Status {
    let bundled_version = bundled
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| dll_version(&b));
    let installed = dir
        .and_then(|d| std::fs::read(d.join(PLUGIN_FILE)).ok())
        .map(|b| Installed {
            version: dll_version(&b),
            ours: is_our_plugin(&b),
        });
    let action = bundled_version.map(|v| action_for(v, installed.as_ref().map(|i| i.version)));
    Status {
        plugin_name: PLUGIN_NAME,
        bundled_path: bundled.map(Path::to_path_buf),
        bundled_version,
        dir: dir.map(Path::to_path_buf),
        dir_is_default: dir.is_some() && dir == default,
        dir_exists: dir.is_some_and(Path::is_dir),
        installed,
        action,
        resolume_running,
    }
}

/// Why an install or removal didn't happen.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("This build contains no FFGL plugin to install.")]
    NoBundledPlugin,
    #[error(
        "The bundled plugin has no version information; it is not a release build of the plugin."
    )]
    BundledWithoutVersion,
    #[error("No plugin folder: Windows reports no Documents folder. Choose a folder.")]
    NoFolder,
    #[error("Quit Resolume Arena to install the plugin: Resolume locks loaded plugins.")]
    ResolumeRunning,
    #[error("{0} is not the {PLUGIN_NAME} plugin; it is left alone.")]
    NotOurs(PathBuf),
    #[error("The installed plugin ({installed}) is newer than the bundled one ({bundled}); confirm the downgrade.")]
    DowngradeNotConfirmed {
        installed: DllVersion,
        bundled: DllVersion,
    },
    #[error("Nothing to remove: there is no {PLUGIN_FILE} in {0}.")]
    NotInstalled(PathBuf),
    #[error("{what}: {source}")]
    Io {
        what: String,
        #[source]
        source: io::Error,
    },
}

fn io_err(what: impl Into<String>) -> impl FnOnce(io::Error) -> InstallError {
    let what = what.into();
    move |source| InstallError::Io { what, source }
}

/// File operations of an install, so tests can make one fail like a locked DLL does on Windows.
pub trait FileOps {
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove(&self, path: &Path) -> io::Result<()>;
}

/// The real file system.
pub struct RealFs;

impl FileOps for RealFs {
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::copy(from, to).map(|_| ())
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
}

/// What an install did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallReport {
    pub action: Action,
    pub path: PathBuf,
    pub version: DllVersion,
    pub previous: Option<DllVersion>,
}

/// Installs (or updates, reinstalls, downgrades) the bundled plugin into `dir`.
///
/// `create_dir`: create a missing folder. `allow_downgrade`: replace a newer installed version.
pub fn install(
    bundled: Option<&Path>,
    dir: Option<&Path>,
    create_dir: bool,
    allow_downgrade: bool,
    resolume_running: bool,
    fs: &dyn FileOps,
) -> Result<InstallReport, InstallError> {
    let result = install_inner(
        bundled,
        dir,
        create_dir,
        allow_downgrade,
        resolume_running,
        fs,
    );
    match &result {
        Ok(r) => log::info!(
            "FFGL plugin: {:?} {} -> {} (was {})",
            r.action,
            r.version,
            r.path.display(),
            r.previous.map_or("not installed".into(), |v| v.to_string())
        ),
        Err(e) => log::warn!(
            "FFGL plugin: install into {} failed: {e}",
            dir.map_or("?".into(), |d| d.display().to_string())
        ),
    }
    result
}

fn install_inner(
    bundled: Option<&Path>,
    dir: Option<&Path>,
    create_dir: bool,
    allow_downgrade: bool,
    resolume_running: bool,
    fs: &dyn FileOps,
) -> Result<InstallReport, InstallError> {
    let bundled = bundled.ok_or(InstallError::NoBundledPlugin)?;
    let dir = dir.ok_or(InstallError::NoFolder)?;
    let new = std::fs::read(bundled).map_err(io_err(format!("reading {}", bundled.display())))?;
    let version = dll_version(&new).ok_or(InstallError::BundledWithoutVersion)?;
    if resolume_running {
        return Err(InstallError::ResolumeRunning);
    }
    if !dir.is_dir() {
        if !create_dir {
            return Err(InstallError::Io {
                what: format!("{} does not exist", dir.display()),
                source: io::ErrorKind::NotFound.into(),
            });
        }
        std::fs::create_dir_all(dir).map_err(io_err(format!("creating {}", dir.display())))?;
    }
    let dest = dir.join(PLUGIN_FILE);
    let old = std::fs::read(&dest).ok();
    if let Some(old) = &old {
        if !is_our_plugin(old) {
            return Err(InstallError::NotOurs(dest));
        }
    }
    let previous = old.as_deref().and_then(dll_version);
    let action = action_for(version, old.as_ref().map(|_| previous));
    if action == Action::Downgrade && !allow_downgrade {
        return Err(InstallError::DowngradeNotConfirmed {
            installed: previous.expect("a downgrade has an installed version"),
            bundled: version,
        });
    }

    // Copy next to the target, move the old DLL aside, move the new one in; undo on any failure.
    let tmp = dir.join(format!("{PLUGIN_FILE}.tmp"));
    let bak = dir.join(format!("{PLUGIN_FILE}.bak"));
    fs.copy(bundled, &tmp)
        .map_err(io_err(format!("copying to {}", tmp.display())))?;
    if old.is_some() {
        if bak.exists() {
            let _ = fs.remove(&bak);
        }
        if let Err(e) = fs.rename(&dest, &bak) {
            let _ = fs.remove(&tmp);
            return Err(io_err(format!(
                "{} is in use (is Resolume running?)",
                dest.display()
            ))(e));
        }
    }
    if let Err(e) = fs.rename(&tmp, &dest) {
        let _ = fs.remove(&tmp);
        if old.is_some() {
            let _ = fs.rename(&bak, &dest);
        }
        return Err(io_err(format!("replacing {}", dest.display()))(e));
    }
    if old.is_some() {
        let _ = fs.remove(&bak);
    }
    Ok(InstallReport {
        action,
        path: dest,
        version,
        previous,
    })
}

/// Removes our plugin from `dir`.
pub fn remove(
    dir: Option<&Path>,
    resolume_running: bool,
    fs: &dyn FileOps,
) -> Result<PathBuf, InstallError> {
    let result = (|| {
        let dir = dir.ok_or(InstallError::NoFolder)?;
        let dest = dir.join(PLUGIN_FILE);
        let bytes =
            std::fs::read(&dest).map_err(|_| InstallError::NotInstalled(dir.to_path_buf()))?;
        if !is_our_plugin(&bytes) {
            return Err(InstallError::NotOurs(dest));
        }
        if resolume_running {
            return Err(InstallError::ResolumeRunning);
        }
        fs.remove(&dest).map_err(io_err(format!(
            "removing {} (is Resolume running?)",
            dest.display()
        )))?;
        Ok(dest)
    })();
    match &result {
        Ok(p) => log::info!("FFGL plugin: removed {}", p.display()),
        Err(e) => log::warn!("FFGL plugin: remove failed: {e}"),
    }
    result
}

/// Whether Resolume (Arena or Avenue) is running. Only Windows can tell; elsewhere `false`.
pub fn resolume_running() -> bool {
    #[cfg(windows)]
    {
        windows_procs::any_running(&["arena.exe", "avenue.exe"])
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod windows_procs {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// Any process whose executable name (case-insensitive) is in `names`.
    pub fn any_running(names: &[&str]) -> bool {
        // SAFETY: plain Win32 calls; the entry is zero-initialised with its size set, the handle is closed.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = false;
            let mut ok = Process32FirstW(snap, &mut entry) != 0;
            while ok {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                if names.contains(&name.as_str()) {
                    found = true;
                    break;
                }
                ok = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
            found
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A minimal stand-in for a DLL: some bytes, then a version resource (key, padding, VS_FIXEDFILEINFO).
    pub(crate) fn fake_dll(version: Option<(u16, u16, u16, u16)>, ours: bool) -> Vec<u8> {
        let mut b = b"MZ\x90\x00 fake PE ".to_vec();
        if ours {
            b.extend(
                "Clip Sequencer for Resolume"
                    .encode_utf16()
                    .flat_map(|u| u.to_le_bytes()),
            );
        }
        if let Some((ma, mi, pa, bu)) = version {
            b.extend([0u8; 6]); // wLength, wValueLength, wType
            b.extend(
                "VS_VERSION_INFO\0"
                    .encode_utf16()
                    .flat_map(|u| u.to_le_bytes()),
            );
            b.extend([0u8; 2]); // padding to 4 bytes
            b.extend(0xFEEF_04BDu32.to_le_bytes());
            b.extend(0x0001_0000u32.to_le_bytes());
            b.extend(((u32::from(ma) << 16) | u32::from(mi)).to_le_bytes());
            b.extend(((u32::from(pa) << 16) | u32::from(bu)).to_le_bytes());
            b.extend([0u8; 40]);
        }
        b
    }

    fn v(major: u16, minor: u16, patch: u16, build: u16) -> DllVersion {
        DllVersion {
            major,
            minor,
            patch,
            build,
        }
    }

    #[test]
    fn reads_the_file_version() {
        assert_eq!(
            dll_version(&fake_dll(Some((0, 1, 0, 57)), true)),
            Some(v(0, 1, 0, 57))
        );
        assert_eq!(
            dll_version(&fake_dll(Some((12, 345, 6, 65535)), true)),
            Some(v(12, 345, 6, 65535))
        );
        assert_eq!(dll_version(&fake_dll(None, true)), None);
        assert_eq!(dll_version(b""), None);
        // A truncated resource is no version, not a panic.
        let full = fake_dll(Some((1, 2, 3, 4)), true);
        assert_eq!(dll_version(&full[..full.len() - 50]), None);
    }

    #[test]
    fn recognises_only_our_plugin() {
        assert!(is_our_plugin(&fake_dll(Some((0, 1, 0, 1)), true)));
        assert!(is_our_plugin(b"...VJSQ...SeqView...")); // early builds without a version resource
        assert!(!is_our_plugin(&fake_dll(Some((0, 1, 0, 1)), false)));
        assert!(!is_our_plugin(b"some other plugin with VJSQ only"));
    }

    #[test]
    fn versions_decide_the_action() {
        let b = v(0, 2, 0, 80);
        assert_eq!(action_for(b, None), Action::Install);
        assert_eq!(action_for(b, Some(None)), Action::Update);
        assert_eq!(action_for(b, Some(Some(v(0, 1, 9, 900)))), Action::Update);
        assert_eq!(
            action_for(b, Some(Some(v(0, 2, 0, 79)))),
            Action::Update,
            "same version, newer build"
        );
        assert_eq!(action_for(b, Some(Some(b))), Action::Reinstall);
        assert_eq!(action_for(b, Some(Some(v(0, 2, 0, 81)))), Action::Downgrade);
        assert_eq!(action_for(b, Some(Some(v(1, 0, 0, 0)))), Action::Downgrade);
    }

    #[test]
    fn target_folder_order() {
        let docs = Path::new("/home/u/OneDrive/Documents");
        let def = default_dir_in(docs);
        assert_eq!(def, docs.join("Resolume Arena").join("Extra Effects"));
        let cli = Path::new("/cli");
        let saved = Path::new("/saved");
        assert_eq!(
            target_dir(Some(cli), Some(saved), Some(def.clone())),
            Some(cli.into())
        );
        assert_eq!(
            target_dir(None, Some(saved), Some(def.clone())),
            Some(saved.into())
        );
        assert_eq!(target_dir(None, None, Some(def.clone())), Some(def));
        assert_eq!(target_dir(None, None, None), None);
    }

    /// Real file system, but `rename` of one path fails (Windows refuses to rename a DLL Resolume has loaded).
    struct LockedFs {
        locked: PathBuf,
        renames: Cell<usize>,
    }
    impl FileOps for LockedFs {
        fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
            RealFs.copy(from, to)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.renames.set(self.renames.get() + 1);
            if from == self.locked {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"));
            }
            RealFs.rename(from, to)
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            RealFs.remove(path)
        }
    }

    /// Fails the final step (temp file → SeqView.dll), after the old DLL was moved aside.
    struct FailLastRename;
    impl FileOps for FailLastRename {
        fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
            RealFs.copy(from, to)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if from.extension().is_some_and(|e| e == "tmp") {
                return Err(io::Error::other("disk full"));
            }
            RealFs.rename(from, to)
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            RealFs.remove(path)
        }
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut f: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        f.sort();
        f
    }

    #[test]
    fn install_update_lock_restore_and_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled_v1 = tmp.path().join("bundle1.dll");
        let bundled_v2 = tmp.path().join("bundle2.dll");
        std::fs::write(&bundled_v1, fake_dll(Some((0, 1, 0, 10)), true)).unwrap();
        std::fs::write(&bundled_v2, fake_dll(Some((0, 1, 0, 20)), true)).unwrap();
        let dir = tmp
            .path()
            .join("Documents")
            .join("Resolume Arena")
            .join("Extra Effects");
        let dest = dir.join(PLUGIN_FILE);

        // Missing folder: only created when asked.
        assert!(install(Some(&bundled_v1), Some(&dir), false, false, false, &RealFs).is_err());
        let r = install(Some(&bundled_v1), Some(&dir), true, false, false, &RealFs).unwrap();
        assert_eq!(
            (r.action, r.version, r.previous),
            (Action::Install, v(0, 1, 0, 10), None)
        );
        assert_eq!(files(&dir), [PLUGIN_FILE]);

        // Resolume running: nothing happens.
        assert!(matches!(
            install(Some(&bundled_v2), Some(&dir), true, false, true, &RealFs),
            Err(InstallError::ResolumeRunning)
        ));
        assert_eq!(
            dll_version(&std::fs::read(&dest).unwrap()),
            Some(v(0, 1, 0, 10))
        );

        // A locked DLL: the update fails and the old version stays, no leftovers.
        let locked = LockedFs {
            locked: dest.clone(),
            renames: Cell::new(0),
        };
        let e = install(Some(&bundled_v2), Some(&dir), true, false, false, &locked).unwrap_err();
        assert!(e.to_string().contains("in use"), "{e}");
        assert_eq!(
            dll_version(&std::fs::read(&dest).unwrap()),
            Some(v(0, 1, 0, 10))
        );
        assert_eq!(files(&dir), [PLUGIN_FILE]);

        // The last step fails after the old DLL was moved aside: it is restored from the .bak.
        let e = install(
            Some(&bundled_v2),
            Some(&dir),
            true,
            false,
            false,
            &FailLastRename,
        )
        .unwrap_err();
        assert!(e.to_string().contains("disk full"), "{e}");
        assert_eq!(
            dll_version(&std::fs::read(&dest).unwrap()),
            Some(v(0, 1, 0, 10))
        );
        assert_eq!(files(&dir), [PLUGIN_FILE]);

        // Update, then a downgrade only with confirmation, then a reinstall.
        let r = install(Some(&bundled_v2), Some(&dir), true, false, false, &RealFs).unwrap();
        assert_eq!(
            (r.action, r.previous),
            (Action::Update, Some(v(0, 1, 0, 10)))
        );
        assert_eq!(files(&dir), [PLUGIN_FILE], "no .bak or .tmp left");
        assert!(matches!(
            install(Some(&bundled_v1), Some(&dir), true, false, false, &RealFs),
            Err(InstallError::DowngradeNotConfirmed { .. })
        ));
        let r = install(Some(&bundled_v1), Some(&dir), true, true, false, &RealFs).unwrap();
        assert_eq!(r.action, Action::Downgrade);
        let r = install(Some(&bundled_v1), Some(&dir), true, false, false, &RealFs).unwrap();
        assert_eq!(r.action, Action::Reinstall);

        // Status as the dialog sees it.
        let s = status(Some(&bundled_v2), Some(&dir), Some(&dir), false);
        assert_eq!(
            s.installed,
            Some(Installed {
                version: Some(v(0, 1, 0, 10)),
                ours: true
            })
        );
        assert_eq!(
            (s.action, s.dir_is_default, s.dir_exists),
            (Some(Action::Update), true, true)
        );

        // Remove: not while Resolume runs; then gone.
        assert!(matches!(
            remove(Some(&dir), true, &RealFs),
            Err(InstallError::ResolumeRunning)
        ));
        assert_eq!(remove(Some(&dir), false, &RealFs).unwrap(), dest);
        assert!(files(&dir).is_empty());
        assert!(matches!(
            remove(Some(&dir), false, &RealFs),
            Err(InstallError::NotInstalled(_))
        ));
    }

    #[test]
    fn never_touches_another_dll_of_the_same_name() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundle.dll");
        std::fs::write(&bundled, fake_dll(Some((0, 1, 0, 1)), true)).unwrap();
        let dir = tmp.path().join("fx");
        std::fs::create_dir(&dir).unwrap();
        let foreign = fake_dll(Some((9, 9, 9, 9)), false);
        std::fs::write(dir.join(PLUGIN_FILE), &foreign).unwrap();
        assert!(matches!(
            install(Some(&bundled), Some(&dir), true, true, false, &RealFs),
            Err(InstallError::NotOurs(_))
        ));
        assert!(matches!(
            remove(Some(&dir), false, &RealFs),
            Err(InstallError::NotOurs(_))
        ));
        assert_eq!(std::fs::read(dir.join(PLUGIN_FILE)).unwrap(), foreign);
        let s = status(Some(&bundled), Some(&dir), None, false);
        assert_eq!(s.installed.map(|i| i.ours), Some(false));
    }

    #[test]
    fn errors_without_a_bundled_plugin_or_folder() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            install(None, Some(tmp.path()), true, false, false, &RealFs),
            Err(InstallError::NoBundledPlugin)
        ));
        let unversioned = tmp.path().join("dev.dll");
        std::fs::write(&unversioned, fake_dll(None, true)).unwrap();
        assert!(matches!(
            install(
                Some(&unversioned),
                Some(tmp.path()),
                true,
                false,
                false,
                &RealFs
            ),
            Err(InstallError::BundledWithoutVersion)
        ));
        let bundled = tmp.path().join("b.dll");
        std::fs::write(&bundled, fake_dll(Some((0, 1, 0, 1)), true)).unwrap();
        assert!(matches!(
            install(Some(&bundled), None, true, false, false, &RealFs),
            Err(InstallError::NoFolder)
        ));
    }
}
