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

    let storage = Storage::new(config.data_dir());
    log::info!("sequence files in {}", storage.dir().display());
    log::error!("the engine is not implemented yet (Milestone 6); only --validate works");
    ExitCode::FAILURE
}
