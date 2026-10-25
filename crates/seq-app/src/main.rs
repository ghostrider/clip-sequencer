use std::process::ExitCode;

use clap::Parser;
use seq_app::config::Config;
use seq_app::storage::{load_text, Storage};

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
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

    let storage = Storage::new(config.data_dir());
    log::info!("sequence files in {}", storage.dir().display());
    log::error!("the engine is not implemented yet (Milestone 6); only --validate works");
    ExitCode::FAILURE
}
