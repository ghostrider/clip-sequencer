//! Command-line configuration (SPEC §11). `seq.toml` support is not implemented yet.

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "seq-app", about = "Resolume clip sequencer", version)]
pub struct Config {
    /// MIDI input port carrying the clock (substring of the port name).
    #[arg(long)]
    pub midi_port: Option<String>,
    #[arg(long, default_value = "127.0.0.1")]
    pub resolume_host: String,
    #[arg(long, default_value_t = 8080)]
    pub rest_port: u16,
    #[arg(long, default_value_t = 7000)]
    pub osc_port: u16,
    #[arg(long, default_value_t = 8090)]
    pub ui_port: u16,
    /// Name of the Resolume layer holding the `§` placeholder clips.
    #[arg(long, default_value = "SEQ")]
    pub seq_layer: String,
    /// Prefix that marks placeholder clip names.
    #[arg(long, default_value = "§ ")]
    pub seq_prefix: String,
    /// Where `<Composition>.seq.json` files live. Default: `%APPDATA%\resolume-sequencer`.
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Check a sequence file and exit (0 = valid, 1 = problems found).
    #[arg(long, value_name = "FILE")]
    pub validate: Option<PathBuf>,
    /// Print the MIDI input ports and exit.
    #[arg(long)]
    pub list_midi_ports: bool,
    /// Print the incoming clock live (bar.beat.tick, BPM) instead of running the sequencer.
    #[arg(long)]
    pub clock_debug: bool,
    /// Start this sequence (by name, live deck preferred) once connected. For testing.
    #[arg(long, value_name = "NAME")]
    pub start: Option<String>,
    /// Exit when no sequence is running any more (after --start). For testing.
    #[arg(long)]
    pub exit_when_done: bool,
}

impl Config {
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(default_data_dir)
    }
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|appdata| PathBuf::from(appdata).join("resolume-sequencer"))
        .unwrap_or_else(|| PathBuf::from("data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::parse_from(["seq-app"]);
        assert_eq!(c.resolume_host, "127.0.0.1");
        assert_eq!((c.rest_port, c.osc_port, c.ui_port), (8080, 7000, 8090));
        assert_eq!(c.seq_layer, "SEQ");
        assert_eq!(c.seq_prefix, "§ ");
    }

    #[test]
    fn data_dir_override() {
        let c = Config::parse_from(["seq-app", "--data-dir", "x/y"]);
        assert_eq!(c.data_dir(), PathBuf::from("x/y"));
    }
}
