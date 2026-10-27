//! Resolume clip sequencer application: configuration, storage and (from M6 on) the engine and
//! UI server. The binary in `main.rs` only wires these together.

pub mod app;
pub mod clock_debug;
pub mod config;
pub mod engine;
pub mod plugin_view;
pub mod runner;
pub mod saver;
pub mod server;
pub mod storage;
pub mod thumbs;
pub mod ui_port;

/// Product name shown in the window title, splash and `/api/version`. A placeholder until the
/// open-source release names the project (docs/M11-open-source-release.md, Phase A).
pub const APP_NAME: &str = "Clip Sequencer";
/// Stable identifier in `/api/version`; a second instance recognises a running one by it.
pub const APP_ID: &str = "seq-app";
/// From `[workspace.package]`, the only version source.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
