//! Runs the mock Resolume standalone, e.g. for UI work without Resolume:
//! `cargo run -p mock-resolume -- --rest-port 8080 --osc-port 7000`

use std::time::Duration;

use clap::Parser;

#[derive(Parser)]
#[command(about = "Fake Resolume REST/WS/OSC server (see docs/resolume-api-notes.md)")]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 8080)]
    rest_port: u16,
    #[arg(long, default_value_t = 7000)]
    osc_port: u16,
    /// Live composition JSON file (default: docs/samples/composition.json, embedded).
    #[arg(long)]
    composition: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    let composition = match &args.composition {
        Some(p) => Some(serde_json::from_str(&std::fs::read_to_string(p)?)?),
        None => None,
    };
    let mock = mock_resolume::start(mock_resolume::MockConfig {
        rest_addr: format!("{}:{}", args.host, args.rest_port).parse()?,
        osc_addr: format!("{}:{}", args.host, args.osc_port).parse()?,
        composition,
        other_decks: None,
        push_delay: Duration::from_millis(55),
    })
    .await?;
    println!(
        "mock Resolume running: REST/WS http://{} OSC udp://{} (Ctrl+C to stop)",
        mock.rest_addr, mock.osc_addr
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}
