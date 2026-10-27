//! Command line of both executables: `seq-app.exe` (desktop app) and `seq-app-cli.exe`
//! (console, headless). Each `main` only calls [`main`] with its [`Exe`].

use std::process::ExitCode;

use crate::config::Config;
use crate::runner;
use crate::storage::load_text;
use clap::{CommandFactory, FromArgMatches};
use tokio_util::sync::CancellationToken;

/// Which of the two executables is running (docs/M12-desktop-shell.md, Phase C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exe {
    /// `seq-app.exe`: the desktop app. Release builds have no console (GUI subsystem).
    Desktop,
    /// `seq-app-cli.exe`: console program; always headless.
    Console,
}

impl Exe {
    pub fn name(self) -> &'static str {
        match self {
            Exe::Desktop => "seq-app",
            Exe::Console => "seq-app-cli",
        }
    }
}

/// Whether this build of `seq-app.exe` is a GUI-subsystem program (see `main.rs`).
const DESKTOP_WITHOUT_CONSOLE: bool = cfg!(all(not(debug_assertions), feature = "desktop"));

/// Shared `main` of both executables.
pub fn main(exe: Exe) -> ExitCode {
    let gui_exe = exe == Exe::Desktop && DESKTOP_WITHOUT_CONSOLE;
    if gui_exe {
        // --help, --version and the hint below should still reach a terminal that started us.
        attach_parent_console();
    }
    let matches = Config::command().name(exe.name()).get_matches();
    let mut config = match Config::from_arg_matches(&matches) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };
    if exe == Exe::Console {
        config.headless = true;
    }
    let console_only = config.headless
        || config.validate.is_some()
        || config.list_midi_ports
        || config.clock_debug;
    if gui_exe && console_only {
        // A GUI program started from a terminal runs in the background and gets no Ctrl+C.
        eprintln!(
            "{}.exe is the desktop app. For --headless, --validate, --list-midi-ports and \
             --clock-debug use {}.exe.",
            exe.name(),
            Exe::Console.name()
        );
        return ExitCode::from(2);
    }
    // Console commands log to stderr only; the sequencer also to daily files (Console tab, M12 E).
    let runs = config.validate.is_none() && !config.list_midi_ports && !config.clock_debug;
    crate::logging::init(crate::logging::LogConfig {
        stderr: config.headless || !runs || cfg!(debug_assertions),
        dir: runs.then(|| config.data_dir().join("logs")),
    });
    crate::logging::log_panics();
    if runs {
        log::info!(
            "{} {} (build {}, {})",
            crate::APP_NAME,
            crate::VERSION,
            crate::BUILD_NUMBER,
            crate::GIT_COMMIT
        );
    }

    if let Some(path) = &config.validate {
        return match std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|text| load_text(path, &text).map_err(|e| e.to_string()))
        {
            Ok(file) => {
                let sequences: usize = file.decks.values().map(|d| d.sequences.len()).sum();
                println!(
                    "{}: valid ({} deck(s), {sequences} sequence(s))",
                    path.display(),
                    file.decks.len()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if config.list_midi_ports {
        return match clock_in::list_ports() {
            Ok(ports) if ports.is_empty() => {
                println!("no MIDI input ports");
                ExitCode::SUCCESS
            }
            Ok(ports) => {
                ports.iter().for_each(|p| println!("{p}"));
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if config.clock_debug {
        let Some(port) = config.midi_port.clone() else {
            eprintln!("--clock-debug needs --midi-port <name> (see --list-midi-ports)");
            return ExitCode::FAILURE;
        };
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        return match rt.block_on(crate::clock_debug::run(port)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    #[cfg(feature = "desktop")]
    if !config.headless {
        return crate::desktop::run(config);
    }
    run_headless(config)
}

/// Headless mode: console output, UI in a browser, Ctrl+C stops.
fn run_headless(config: Config) -> ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let shutdown = CancellationToken::new();
        // The startup steps the desktop app shows on its splash screen (M12 D2).
        let (progress, mut steps) = tokio::sync::mpsc::unbounded_channel::<runner::StartupStep>();
        let printer = tokio::spawn(async move {
            while let Some(s) = steps.recv().await {
                if s.warning {
                    println!("        ! {}", s.text);
                } else {
                    println!("  [{}/{}] {}", s.index, s.total, s.text);
                }
            }
        });
        let result = runner::run(config, shutdown.clone(), None, Some(progress)).await;
        let _ = printer.await;
        let running = match result {
            Ok(r) => r,
            Err(e) => {
                log::error!("{e}");
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        println!(
            "
    Sequencer UI:  {}
",
            running.ui_url
        );
        let mut done = running.done;
        tokio::select! {
            _ = stop_requested() => {
                shutdown.cancel();
                let _ = done.await;
            }
            _ = &mut done => {}
        }
        ExitCode::SUCCESS
    })
}

/// Ctrl+C, Ctrl+Break or closing the console window: each ends the sequencer cleanly (sequences
/// end per `after_last_pass`, `runtime.json` is removed). For a closing window Windows waits
/// about 5 s (tokio holds its handler); the shutdown takes at most 2 s.
async fn stop_requested() {
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
        if let (Ok(mut c), Ok(mut b), Ok(mut x)) = (ctrl_c(), ctrl_break(), ctrl_close()) {
            let what = tokio::select! {
                _ = c.recv() => "Ctrl+C",
                _ = b.recv() => "Ctrl+Break",
                _ = x.recv() => "console window closed",
            };
            log::info!("{what}: stopping");
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
    log::info!("Ctrl+C: stopping");
}

/// A GUI-subsystem exe has no console, so its output would vanish. Attach to the parent's
/// console unless the standard handles were redirected (pipe, file).
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };
    // SAFETY: plain Win32 calls without pointers.
    unsafe {
        let redirected =
            !GetStdHandle(STD_OUTPUT_HANDLE).is_null() || !GetStdHandle(STD_ERROR_HANDLE).is_null();
        if !redirected {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
