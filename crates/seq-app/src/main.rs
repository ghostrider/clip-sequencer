// Release builds of the desktop app have no console window; `--headless` attaches to the
// console of the terminal that started it (see `attach_parent_console`).
#![cfg_attr(
    all(not(debug_assertions), feature = "desktop"),
    windows_subsystem = "windows"
)]

use std::process::ExitCode;

use clap::Parser;
use seq_app::config::Config;
use seq_app::runner;
use seq_app::storage::load_text;
use tokio_util::sync::CancellationToken;

fn main() -> ExitCode {
    attach_parent_console();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_micros()
        .init();
    let config = Config::parse();

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
        return match rt.block_on(seq_app::clock_debug::run(port)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if config.headless || !cfg!(feature = "desktop") {
        return run_headless(config);
    }
    // The desktop shell (Tauri) arrives in M12 Phase D; until then desktop mode runs headless.
    run_headless(config)
}

/// Headless mode: console output, UI in a browser, Ctrl+C stops.
fn run_headless(config: Config) -> ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let shutdown = CancellationToken::new();
        let running = match runner::run(config, shutdown.clone()).await {
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
            _ = tokio::signal::ctrl_c() => {
                shutdown.cancel();
                let _ = done.await;
            }
            _ = &mut done => {}
        }
        ExitCode::SUCCESS
    })
}

/// A GUI-subsystem exe has no console: output started from a terminal would vanish. Attach to
/// the parent's console unless the standard handles were redirected (pipe, file).
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
