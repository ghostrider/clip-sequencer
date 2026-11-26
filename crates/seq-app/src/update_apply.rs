//! The update helper (SPEC §15): the **new** `seq-app-cli.exe`, started from the temp stage folder with
//! `--apply-update`, swaps the program folder's files after the old app has quit, starts the new version and waits for
//! its ok file; otherwise it puts the old files back and starts the old version.
//!
//! Only the entries the new release itself has (the exes, `ffgl\`, the texts) are moved and replaced; anything else a
//! user keeps in that folder stays where it is. Processes are behind [`Procs`] so the swap and the rollback are
//! tested with temp folders.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Folder inside the program folder that holds the old files during an update.
pub const BACKUP_DIR: &str = ".update-backup";
/// The helper's log inside [`BACKUP_DIR`]; the version that starts afterwards copies it to its log folder.
pub const LOG_FILE: &str = "update.log";
/// The file the new version writes when its start-up is done.
pub const OK_FILE: &str = "ok";

/// How long the helper waits for the old app to quit, and for the new one's ok file (SPEC §15). A new version that
/// quits before writing it is rolled back at once; the full wait is only for one that hangs.
pub const WAIT_EXIT: Duration = Duration::from_secs(30);
pub const WAIT_OK: Duration = Duration::from_secs(30);

/// What the helper does to processes; real ones in [`SystemProcs`].
pub trait Procs {
    /// Waits until process `pid` has ended; false after `timeout`.
    fn wait_exit(&mut self, pid: u32, timeout: Duration) -> bool;
    /// Starts `exe` with `args` (detached); returns a handle for [`Procs::kill`].
    fn spawn(&mut self, exe: &Path, args: &[String]) -> std::io::Result<u32>;
    /// Ends a process started by [`Procs::spawn`] (best effort).
    fn kill(&mut self, handle: u32);
    /// Whether a process started by [`Procs::spawn`] has already ended, and with which exit code.
    fn exited(&mut self, handle: u32) -> Option<Option<i32>>;
    fn sleep(&mut self, d: Duration);
}

pub struct Plan {
    /// The program folder (`clip-sequencer`) being updated.
    pub target: PathBuf,
    /// The unpacked new `clip-sequencer` folder (the helper runs from it).
    pub source: PathBuf,
    /// The old app's process id.
    pub wait_pid: u32,
    /// File name of the exe to start again (`seq-app.exe` or `seq-app-cli.exe`) and its arguments.
    pub restart: String,
    pub restart_args: Vec<String>,
    pub from: String,
    pub to: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The new version runs and reported ok; the backup is gone.
    Updated,
    /// Something failed after files were touched; the old files are back and the old version was started.
    RolledBack(String),
    /// Nothing was changed (the old app didn't quit, or the program folder isn't ours).
    NotStarted(String),
}

struct Log(Option<std::fs::File>);

impl Log {
    fn line(&mut self, text: impl AsRef<str>) {
        let line = format!(
            "{} {}\n",
            crate::logging::utc_iso8601(std::time::SystemTime::now(), true),
            text.as_ref()
        );
        if let Some(f) = &mut self.0 {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Runs the update. Never panics on IO: every failure ends in [`Outcome::RolledBack`] or [`Outcome::NotStarted`].
pub fn apply(plan: &Plan, procs: &mut dyn Procs, wait_ok: Duration) -> Outcome {
    let backup_root = plan.target.join(BACKUP_DIR);
    let backup = backup_root.join(&plan.from);
    let _ = std::fs::create_dir_all(&backup_root);
    let mut log = Log(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(backup_root.join(LOG_FILE))
        .ok());
    log.line(format!(
        "update {} -> {} in {}",
        plan.from,
        plan.to,
        plan.target.display()
    ));

    if !plan.target.join("seq-app-cli.exe").is_file() {
        let why = "the program folder has no seq-app-cli.exe".to_owned();
        log.line(&why);
        return Outcome::NotStarted(why);
    }
    // The new release's top-level entries are what gets replaced.
    let entries: Vec<String> = match std::fs::read_dir(&plan.source) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != BACKUP_DIR)
            .collect(),
        Err(e) => {
            let why = format!("cannot read the new files: {e}");
            log.line(&why);
            return Outcome::NotStarted(why);
        }
    };
    if !procs.wait_exit(plan.wait_pid, WAIT_EXIT) {
        let why = "the app did not quit".to_owned();
        log.line(&why);
        return Outcome::NotStarted(why);
    }
    log.line("the old app has quit");

    // From here on, every failure rolls back.
    let mut moved: Vec<String> = Vec::new();
    let mut started: Option<u32> = None;
    let result = (|| -> Result<(), String> {
        let _ = std::fs::remove_dir_all(&backup);
        std::fs::create_dir_all(&backup).map_err(|e| format!("cannot create the backup: {e}"))?;
        for name in &entries {
            let from = plan.target.join(name);
            if from.exists() {
                std::fs::rename(&from, backup.join(name))
                    .map_err(|e| format!("cannot move {name} aside (in use?): {e}"))?;
                moved.push(name.clone());
            }
        }
        log.line(format!("moved aside: {}", moved.join(", ")));
        for name in &entries {
            copy_tree(&plan.source.join(name), &plan.target.join(name))
                .map_err(|e| format!("cannot copy {name}: {e}"))?;
        }
        log.line("new files in place");
        let ok = backup_root.join(OK_FILE);
        let _ = std::fs::remove_file(&ok);
        let mut args = plan.restart_args.clone();
        args.push("--after-update".into());
        args.push(ok.to_string_lossy().into_owned());
        let exe = plan.target.join(&plan.restart);
        started = Some(
            procs
                .spawn(&exe, &args)
                .map_err(|e| format!("cannot start {}: {e}", plan.restart))?,
        );
        log.line(format!("started {} {}", plan.restart, plan.to));
        let deadline = Instant::now() + wait_ok;
        while !ok.is_file() {
            // Quit before reporting ok (a crash at start-up): no point waiting for the deadline.
            if let Some(code) = started.and_then(|h| procs.exited(h)) {
                if ok.is_file() {
                    break;
                }
                return Err(format!(
                    "{} quit right after starting ({})",
                    plan.to,
                    code.map_or("no exit code".to_owned(), |c| format!("exit code {c}"))
                ));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "{} did not start within {} s",
                    plan.to,
                    wait_ok.as_secs()
                ));
            }
            procs.sleep(Duration::from_millis(200));
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            log.line("the new version reported ok");
            drop(log);
            let _ = std::fs::remove_dir_all(&backup_root);
            Outcome::Updated
        }
        Err(why) => {
            log.line(format!("failed: {why}; rolling back"));
            if let Some(h) = started {
                procs.kill(h);
                procs.sleep(Duration::from_millis(500));
            }
            for name in &entries {
                if moved.contains(name) || !backup.join(name).exists() {
                    remove_any(&plan.target.join(name));
                }
            }
            for name in &moved {
                if let Err(e) = std::fs::rename(backup.join(name), plan.target.join(name)) {
                    log.line(format!("could not put back {name}: {e}"));
                }
            }
            let mut args = plan.restart_args.clone();
            args.push("--update-failed".into());
            args.push(why.clone());
            match procs.spawn(&plan.target.join(&plan.restart), &args) {
                Ok(_) => log.line(format!("old version {} started again", plan.from)),
                Err(e) => log.line(format!("could not start the old version: {e}")),
            }
            Outcome::RolledBack(why)
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn remove_any(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

/// Real processes (Windows).
#[derive(Default)]
pub struct SystemProcs {
    children: Vec<std::process::Child>,
}

impl Procs for SystemProcs {
    fn wait_exit(&mut self, pid: u32, timeout: Duration) -> bool {
        wait_pid(pid, timeout)
    }

    fn spawn(&mut self, exe: &Path, args: &[String]) -> std::io::Result<u32> {
        let mut cmd = std::process::Command::new(exe);
        cmd.args(args);
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // The console program gets its own console window again; the desktop app needs none.
            const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            let console = exe
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("seq-app-cli.exe"));
            cmd.creation_flags(if console {
                CREATE_NEW_CONSOLE
            } else {
                DETACHED_PROCESS
            });
        }
        let child = cmd.spawn()?;
        self.children.push(child);
        Ok((self.children.len() - 1) as u32)
    }

    fn kill(&mut self, handle: u32) {
        if let Some(c) = self.children.get_mut(handle as usize) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn exited(&mut self, handle: u32) -> Option<Option<i32>> {
        let c = self.children.get_mut(handle as usize)?;
        match c.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            _ => None,
        }
    }

    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[cfg(windows)]
fn wait_pid(pid: u32, timeout: Duration) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    // SAFETY: plain Win32 calls on a handle we open and close here.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return true; // already gone
        }
        let r = WaitForSingleObject(h, timeout.as_millis().min(u32::MAX as u128) as u32);
        CloseHandle(h);
        r == WAIT_OBJECT_0
    }
}

#[cfg(not(windows))]
fn wait_pid(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Path::new(&format!("/proc/{pid}")).exists() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake processes: the old app has quit (or not); a started exe writes the ok file (or not, or can't start).
    struct Fake {
        old_quits: bool,
        new_reports_ok: bool,
        /// The new version quits at once without reporting ok (exit code 1).
        new_quits: bool,
        spawn_fails_for: Option<String>,
        spawned: Vec<(PathBuf, Vec<String>)>,
        killed: Vec<u32>,
    }

    impl Fake {
        fn new() -> Self {
            Fake {
                old_quits: true,
                new_reports_ok: true,
                new_quits: false,
                spawn_fails_for: None,
                spawned: Vec::new(),
                killed: Vec::new(),
            }
        }
    }

    impl Procs for Fake {
        fn wait_exit(&mut self, _pid: u32, _t: Duration) -> bool {
            self.old_quits
        }
        fn spawn(&mut self, exe: &Path, args: &[String]) -> std::io::Result<u32> {
            self.spawned.push((exe.to_owned(), args.to_vec()));
            if self
                .spawn_fails_for
                .as_deref()
                .is_some_and(|v| std::fs::read_to_string(exe).unwrap_or_default().contains(v))
            {
                return Err(std::io::Error::other("bad exe"));
            }
            if self.new_reports_ok {
                if let Some(i) = args.iter().position(|a| a == "--after-update") {
                    std::fs::write(&args[i + 1], b"ok").unwrap();
                }
            }
            Ok(self.spawned.len() as u32 - 1)
        }
        fn kill(&mut self, h: u32) {
            self.killed.push(h);
        }
        fn exited(&mut self, _h: u32) -> Option<Option<i32>> {
            self.new_quits.then_some(Some(1))
        }
        fn sleep(&mut self, _d: Duration) {}
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A program folder at 0.5.0 (plus a file of the user's), and the unpacked 0.5.1.
    fn setup() -> (tempfile::TempDir, Plan) {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("clip-sequencer");
        let source = tmp.path().join("stage/clip-sequencer");
        for (dir, v) in [(&target, "0.5.0"), (&source, "0.5.1")] {
            write(&dir.join("seq-app.exe"), &format!("gui {v}"));
            write(&dir.join("seq-app-cli.exe"), &format!("cli {v}"));
            write(&dir.join("ffgl/SeqView.dll"), &format!("dll {v}"));
            write(&dir.join("README.md"), &format!("readme {v}"));
        }
        write(&target.join("my notes.txt"), "mine");
        let plan = Plan {
            target,
            source,
            wait_pid: 1234,
            restart: "seq-app.exe".into(),
            restart_args: vec!["--no-splash".into()],
            from: "0.5.0".into(),
            to: "0.5.1".into(),
        };
        (tmp, plan)
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn a_successful_update_replaces_the_release_files_only() {
        let (_tmp, plan) = setup();
        let mut fake = Fake::new();
        assert_eq!(apply(&plan, &mut fake, WAIT_OK), Outcome::Updated);
        assert_eq!(read(&plan.target.join("seq-app.exe")), "gui 0.5.1");
        assert_eq!(read(&plan.target.join("ffgl/SeqView.dll")), "dll 0.5.1");
        assert_eq!(
            read(&plan.target.join("my notes.txt")),
            "mine",
            "not ours: kept"
        );
        assert!(!plan.target.join(BACKUP_DIR).exists(), "backup removed");
        let (exe, args) = &fake.spawned[0];
        assert_eq!(exe, &plan.target.join("seq-app.exe"));
        assert_eq!(args[0], "--no-splash");
        assert_eq!(args[1], "--after-update");
    }

    /// A new version that quits at start-up is rolled back at once, not after the whole wait.
    #[test]
    fn a_new_version_that_quits_at_once_is_rolled_back_at_once() {
        let (_tmp, plan) = setup();
        let mut fake = Fake::new();
        fake.new_reports_ok = false;
        fake.new_quits = true;
        let t = Instant::now();
        let out = apply(&plan, &mut fake, Duration::from_secs(3600));
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "no waiting for the deadline"
        );
        assert!(
            matches!(&out, Outcome::RolledBack(w) if w.contains("quit right after starting (exit code 1)")),
            "{out:?}"
        );
        assert_eq!(read(&plan.target.join("seq-app.exe")), "gui 0.5.0");
        assert!(fake.spawned[1].1.iter().any(|a| a == "--update-failed"));
    }

    #[test]
    fn no_ok_from_the_new_version_rolls_back() {
        let (_tmp, plan) = setup();
        let mut fake = Fake::new();
        fake.new_reports_ok = false;
        let out = apply(&plan, &mut fake, Duration::ZERO);
        assert!(
            matches!(&out, Outcome::RolledBack(w) if w.contains("did not start")),
            "{out:?}"
        );
        assert_eq!(read(&plan.target.join("seq-app.exe")), "gui 0.5.0");
        assert_eq!(read(&plan.target.join("ffgl/SeqView.dll")), "dll 0.5.0");
        assert_eq!(read(&plan.target.join("my notes.txt")), "mine");
        assert_eq!(fake.killed, [0], "the new version is stopped");
        let (exe, args) = &fake.spawned[1];
        assert_eq!(exe, &plan.target.join("seq-app.exe"));
        assert!(args.iter().any(|a| a == "--update-failed"));
        assert!(
            read(&plan.target.join(BACKUP_DIR).join(LOG_FILE)).contains("rolling back"),
            "the log stays for the old version to pick up"
        );
    }

    #[test]
    fn a_new_version_that_cannot_start_rolls_back() {
        let (_tmp, plan) = setup();
        let mut fake = Fake::new();
        fake.spawn_fails_for = Some("0.5.1".into());
        let out = apply(&plan, &mut fake, WAIT_OK);
        assert!(
            matches!(&out, Outcome::RolledBack(w) if w.contains("cannot start")),
            "{out:?}"
        );
        assert_eq!(read(&plan.target.join("seq-app-cli.exe")), "cli 0.5.0");
        assert_eq!(fake.spawned.len(), 2, "the old version was started again");
    }

    #[test]
    fn nothing_changes_when_the_old_app_does_not_quit_or_the_folder_is_not_ours() {
        let (_tmp, plan) = setup();
        let mut fake = Fake::new();
        fake.old_quits = false;
        assert_eq!(
            apply(&plan, &mut fake, WAIT_OK),
            Outcome::NotStarted("the app did not quit".into())
        );
        assert_eq!(read(&plan.target.join("seq-app.exe")), "gui 0.5.0");
        assert!(fake.spawned.is_empty());

        let (_tmp2, mut plan) = setup();
        std::fs::remove_file(plan.target.join("seq-app-cli.exe")).unwrap();
        plan.restart = "seq-app.exe".into();
        assert!(matches!(
            apply(&plan, &mut Fake::new(), WAIT_OK),
            Outcome::NotStarted(_)
        ));
        assert_eq!(read(&plan.target.join("seq-app.exe")), "gui 0.5.0");
    }

    #[cfg(windows)]
    #[test]
    fn a_file_in_use_rolls_back_what_was_moved() {
        // Windows: a file opened without FILE_SHARE_DELETE can't be renamed.
        use std::os::windows::fs::OpenOptionsExt;
        let (_tmp, plan) = setup();
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(plan.target.join("seq-app.exe"))
            .unwrap();
        let mut fake = Fake::new();
        let out = apply(&plan, &mut fake, WAIT_OK);
        assert!(
            matches!(&out, Outcome::RolledBack(w) if w.contains("in use")),
            "{out:?}"
        );
        drop(_held);
        for f in [
            "seq-app-cli.exe",
            "README.md",
            "ffgl/SeqView.dll",
            "seq-app.exe",
        ] {
            assert!(read(&plan.target.join(f)).ends_with("0.5.0"), "{f}");
        }
    }
}
