//! The app side of the in-app update (SPEC §15): the check at start-up and by hand, the question
//! (Download and install / Skip this version / Later), the download with progress and the hand-over to the helper
//! (`update_apply`). Its [`UpdateView`] goes to every UI (`update` message); nothing here is on the timing path.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::engine::view::RuntimeState;
use crate::server::ShellAction;
use crate::update::{self, CheckResult, Release};

/// Where an install stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Phase {
    /// Bytes downloaded of `total` (0 = unknown); verifying follows the last byte.
    Downloading {
        done: u64,
        total: u64,
    },
    /// The helper runs; the app quits now.
    Restarting,
    Failed {
        message: String,
    },
}

/// What the UI shows (Help → Check for updates…, and the question after the start-up check).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateView {
    pub current: String,
    pub checking: bool,
    /// The last check's result (also the start-up check's when it found something to offer).
    pub result: Option<CheckResult>,
    /// The start-up check found a version to offer: the UI asks Download and install / Skip / Later.
    pub ask: bool,
    pub phase: Option<Phase>,
    /// Why this copy can't update itself (not a release folder, folder not writable); `None` = it can.
    /// Running or waiting sequences also block an install; the UI knows those from the runtime state.
    pub blocked: Option<String>,
    /// Preferences → Check for updates at start-up.
    pub startup_check: bool,
    pub releases_page: String,
}

pub struct UpdateCtl {
    data_dir: PathBuf,
    view: watch::Sender<UpdateView>,
    runtime: watch::Receiver<RuntimeState>,
    shell: Option<mpsc::UnboundedSender<ShellAction>>,
    quit: Option<CancellationToken>,
}

/// The program folder: where the running exe is.
pub fn program_folder() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_owned)
}

/// A folder unpacked from the release zip (not a build folder such as `target\debug`).
pub fn is_release_folder(dir: &Path) -> bool {
    [
        "seq-app.exe",
        "seq-app-cli.exe",
        "LICENSE",
        "THIRD_PARTY_LICENSES.md",
    ]
    .iter()
    .all(|f| dir.join(f).is_file())
}

/// Why this copy can't be updated in place, if it can't.
pub fn folder_blocked() -> Option<String> {
    let Some(dir) = program_folder() else {
        return Some("the program folder is unknown".into());
    };
    if !is_release_folder(&dir) {
        return Some(
            "this copy was not unpacked from a release zip (a development build); download the release instead"
                .into(),
        );
    }
    if !update::folder_writable(&dir) {
        return Some(format!(
            "the program folder {} is not writable; download the release and unpack it yourself",
            dir.display()
        ));
    }
    None
}

/// The arguments to start the app with again, without the update hand-over ones.
fn restart_args() -> Vec<String> {
    let mut out = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--after-update" || a == "--update-failed" {
            args.next();
            continue;
        }
        if a.starts_with("--after-update=") || a.starts_with("--update-failed=") {
            continue;
        }
        out.push(a);
    }
    out
}

impl UpdateCtl {
    pub fn new(
        data_dir: PathBuf,
        runtime: watch::Receiver<RuntimeState>,
        shell: Option<mpsc::UnboundedSender<ShellAction>>,
        quit: Option<CancellationToken>,
    ) -> Arc<Self> {
        let settings = crate::settings::Settings::load(&data_dir);
        Arc::new(UpdateCtl {
            view: watch::channel(UpdateView {
                current: crate::VERSION.to_owned(),
                checking: false,
                result: None,
                ask: false,
                phase: None,
                blocked: None,
                startup_check: settings.update_check != Some(false),
                releases_page: update::RELEASES_PAGE.to_owned(),
            })
            .0,
            data_dir,
            runtime,
            shell,
            quit,
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<UpdateView> {
        self.view.subscribe()
    }

    /// Checks for a newer release in the background. `startup`: only ask when there is a version the user hasn't
    /// skipped, and say nothing on errors (they are logged).
    pub fn check(self: &Arc<Self>, startup: bool) {
        if self.view.borrow().checking {
            return;
        }
        self.view.send_modify(|v| {
            v.checking = true;
            if !startup {
                v.result = None;
                v.phase = None;
            }
        });
        let this = self.clone();
        tokio::spawn(async move {
            let client = update::client(crate::VERSION);
            let result = update::check(&client, update::source(), crate::VERSION).await;
            let blocked = tokio::task::spawn_blocking(folder_blocked)
                .await
                .unwrap_or(None);
            let skipped = crate::settings::Settings::load(&this.data_dir).update_skipped;
            match &result {
                CheckResult::Error { message, .. } => log::info!("update check: {message}"),
                CheckResult::UpToDate { .. } => log::info!("update check: up to date"),
                CheckResult::Available { release, .. } => {
                    log::info!("update check: version {} is available", release.version)
                }
            }
            this.view.send_modify(|v| {
                v.checking = false;
                v.blocked = blocked;
                if startup {
                    if let CheckResult::Available { release, .. } = &result {
                        if skipped.as_deref() != Some(release.version.as_str()) {
                            v.ask = true;
                            v.result = Some(result.clone());
                        }
                    }
                } else {
                    v.result = Some(result.clone());
                }
            });
        });
    }

    /// The answer to the start-up question: Skip this version (saved) or Later.
    pub fn answer(self: &Arc<Self>, skip: bool) {
        let version = match &self.view.borrow().result {
            Some(CheckResult::Available { release, .. }) => Some(release.version.clone()),
            _ => None,
        };
        self.view.send_modify(|v| v.ask = false);
        if let (true, Some(version)) = (skip, version) {
            let dir = self.data_dir.clone();
            tokio::task::spawn_blocking(move || {
                log::info!("update: version {version} skipped");
                if let Err(e) = crate::settings::Settings::update::<std::io::Error>(&dir, |s| {
                    s.update_skipped = Some(version);
                    Ok(())
                }) {
                    log::warn!("saving the skipped version failed: {e}");
                }
            });
        }
    }

    /// Preferences → Check for updates at start-up.
    pub fn set_startup_check(self: &Arc<Self>, on: bool) {
        self.view.send_modify(|v| v.startup_check = on);
        let dir = self.data_dir.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = crate::settings::Settings::update::<std::io::Error>(&dir, |s| {
                s.update_check = (!on).then_some(false);
                Ok(())
            }) {
                log::warn!("saving the update setting failed: {e}");
            }
        });
    }

    fn fail(&self, message: impl Into<String>) {
        let message = message.into();
        log::warn!("update: {message}");
        self.view.send_modify(|v| {
            v.phase = Some(Phase::Failed { message });
        });
    }

    /// Download and install: refused while sequences run or wait, or when the folder can't be updated; otherwise
    /// download, verify, start the helper and quit.
    pub fn install(self: &Arc<Self>) {
        if matches!(
            self.view.borrow().phase,
            Some(Phase::Downloading { .. } | Phase::Restarting)
        ) {
            return;
        }
        let release: Release = match &self.view.borrow().result {
            Some(CheckResult::Available { release, .. }) => release.clone(),
            _ => return self.fail("there is no update to install; check again"),
        };
        {
            let r = self.runtime.borrow();
            if !r.running.is_empty() || !r.pending.is_empty() {
                return self.fail("stop all sequences first");
            }
        }
        self.view.send_modify(|v| {
            v.ask = false;
            v.phase = Some(Phase::Downloading {
                done: 0,
                total: release.zip_size,
            });
        });
        let this = self.clone();
        tokio::spawn(async move {
            if let Some(why) = tokio::task::spawn_blocking(folder_blocked)
                .await
                .unwrap_or(None)
            {
                return this.fail(why);
            }
            let client = update::client(crate::VERSION);
            let stage = update::stage_dir(&release.version);
            let view = this.view.clone();
            let progress = move |done: u64, total: u64| {
                view.send_if_modified(|v| {
                    let next = Some(Phase::Downloading { done, total });
                    // At most one update per 64 KiB step keeps the UI messages few.
                    let changed = match &v.phase {
                        Some(Phase::Downloading { done: d, .. }) => {
                            done == total || done / 65_536 != *d / 65_536
                        }
                        _ => true,
                    };
                    if changed {
                        v.phase = next;
                    }
                    changed
                });
            };
            let folder = match update::download(&client, &release, &stage, &progress).await {
                Ok(f) => f,
                Err(e) => return this.fail(e),
            };
            if let Err(e) = this.start_helper(&folder, &release.version) {
                return this.fail(format!("cannot start the update helper: {e}"));
            }
            log::info!(
                "update: helper started, quitting for version {}",
                release.version
            );
            this.view.send_modify(|v| v.phase = Some(Phase::Restarting));
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            if let Some(shell) = &this.shell {
                let _ = shell.send(ShellAction::Quit);
            } else if let Some(quit) = &this.quit {
                quit.cancel();
            } else {
                this.fail("quit the app now to finish the update");
            }
        });
    }

    fn start_helper(&self, folder: &Path, version: &str) -> std::io::Result<()> {
        let exe = std::env::current_exe()?;
        let target = exe
            .parent()
            .ok_or_else(|| std::io::Error::other("no program folder"))?;
        let name = exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "seq-app.exe".into());
        let mut cmd = std::process::Command::new(folder.join("seq-app-cli.exe"));
        cmd.arg("--apply-update")
            .arg("--update-target")
            .arg(target)
            .arg("--update-wait-pid")
            .arg(std::process::id().to_string())
            .arg("--update-restart")
            .arg(&name)
            .arg("--update-from")
            .arg(crate::VERSION)
            .arg("--update-to")
            .arg(version);
        for a in restart_args() {
            cmd.arg(format!("--update-restart-arg={a}"));
        }
        cmd.current_dir(folder);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.spawn().map(|_| ())
    }
}

/// At "Ready" (SPEC §15): after an update, keep the helper's log, report ok and say so; after a rollback, say why.
/// Returns the notice to show, if any.
pub fn after_start(config: &crate::config::Config, data_dir: &Path) -> Option<String> {
    let logs = data_dir.join("logs");
    let today = &crate::logging::utc_iso8601(std::time::SystemTime::now(), false)[..10];
    let keep_log = |backup_root: &Path| {
        let from = backup_root.join(crate::update_apply::LOG_FILE);
        if let Ok(text) = std::fs::read_to_string(&from) {
            let _ = std::fs::create_dir_all(&logs);
            let to = logs.join(format!("update-{today}.log"));
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(to)
                .ok()?;
            std::io::Write::write_all(&mut f, text.as_bytes()).ok()?;
        }
        Some(())
    };
    if let Some(ok) = &config.after_update {
        if let Some(root) = ok.parent() {
            keep_log(root);
        }
        if let Err(e) = std::fs::write(ok, b"ok") {
            log::error!("update: cannot report ok ({}): {e}", ok.display());
        }
        log::info!("updated to {}", crate::VERSION);
        // The helper may still run from its temp folder: clean up old ones a bit later.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(120));
            remove_stage_dirs();
        });
        return Some(format!("Updated to {}", crate::VERSION));
    }
    if let Some(why) = &config.update_failed {
        if let Some(root) = program_folder().map(|d| d.join(crate::update_apply::BACKUP_DIR)) {
            keep_log(&root);
            let _ = std::fs::remove_dir_all(&root);
        }
        remove_stage_dirs();
        log::warn!("update failed: {why}");
        return Some(format!("Update failed: {why}. Still {}.", crate::VERSION));
    }
    None
}

/// Removes `%TEMP%\clip-sequencer-update-*` folders (best effort).
fn remove_stage_dirs() {
    if cfg!(test) {
        return; // never the real temp folder from a unit test
    }
    let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for e in rd.flatten() {
        if e.file_name()
            .to_string_lossy()
            .starts_with("clip-sequencer-update-")
        {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn release_folders_only() {
        let tmp = tempfile::tempdir().unwrap();
        for f in ["seq-app.exe", "seq-app-cli.exe", "LICENSE"] {
            std::fs::write(tmp.path().join(f), b"x").unwrap();
        }
        assert!(
            !is_release_folder(tmp.path()),
            "a build folder has no licence list"
        );
        std::fs::write(tmp.path().join("THIRD_PARTY_LICENSES.md"), b"x").unwrap();
        assert!(is_release_folder(tmp.path()));
    }

    #[test]
    fn after_an_update_the_log_is_kept_and_ok_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let backup = tmp
            .path()
            .join("clip-sequencer")
            .join(crate::update_apply::BACKUP_DIR);
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join(crate::update_apply::LOG_FILE), "helper log\n").unwrap();
        let ok = backup.join(crate::update_apply::OK_FILE);
        let config = crate::config::Config::parse_from([
            "seq-app".as_ref(),
            "--after-update".as_ref(),
            ok.as_os_str(),
        ]);
        let notice = after_start(&config, &data).unwrap();
        assert!(notice.starts_with("Updated to"), "{notice}");
        assert!(ok.is_file());
        let logs: Vec<_> = std::fs::read_dir(data.join("logs"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(logs.len(), 1);
        assert_eq!(
            std::fs::read_to_string(logs[0].path()).unwrap(),
            "helper log\n"
        );
        // A normal start: nothing to say.
        let plain = crate::config::Config::parse_from(["seq-app"]);
        assert_eq!(after_start(&plain, &data), None);
    }

    #[test]
    fn a_rollback_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let config = crate::config::Config::parse_from([
            "seq-app",
            "--update-failed",
            "the new version did not start",
        ]);
        let notice = after_start(&config, tmp.path()).unwrap();
        assert!(notice.contains("the new version did not start"), "{notice}");
        assert!(notice.contains(crate::VERSION));
    }
}
