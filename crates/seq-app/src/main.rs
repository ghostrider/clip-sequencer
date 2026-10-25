use std::process::ExitCode;

use clap::Parser;
use resolume_client::ResolumeAddr;
use seq_app::app;
use seq_app::config::Config;
use seq_app::engine::{EngineCommand, EngineConfig};
use seq_app::saver::DebounceConfig;
use seq_app::storage::{load_text, Storage};

fn main() -> ExitCode {
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

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    match rt.block_on(run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config) -> anyhow::Result<()> {
    let storage = Storage::new(config.data_dir());
    log::info!("sequence files in {}", storage.dir().display());
    let app = app::start(app::AppConfig {
        resolume: ResolumeAddr {
            host: config.resolume_host.clone(),
            rest_port: config.rest_port,
            osc_port: config.osc_port,
        },
        storage,
        engine: EngineConfig {
            seq_layer: config.seq_layer.clone(),
            seq_prefix: config.seq_prefix.clone(),
            ..EngineConfig::default()
        },
        clock: match &config.midi_port {
            Some(p) => app::ClockSource::Midi(p.clone()),
            None => app::ClockSource::None,
        },
        debounce: DebounceConfig::default(),
    })?;
    if let Some(name) = &config.start {
        app.commands
            .send(EngineCommand::StartByName { name: name.clone() })?;
    }
    let mut runtime = app.outputs.runtime.clone();
    let mut seen_running = false;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            changed = runtime.changed() => {
                if changed.is_err() {
                    break;
                }
                let state = runtime.borrow_and_update().clone();
                if !state.running.is_empty() {
                    seen_running = true;
                }
                if config.exit_when_done && seen_running && state.running.is_empty() {
                    log::info!("no sequence running any more; exiting");
                    break;
                }
            }
        }
    }
    app.shutdown().await;
    Ok(())
}
