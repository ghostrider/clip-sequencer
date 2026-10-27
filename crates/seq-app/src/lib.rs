//! Resolume clip sequencer application: configuration, storage and (from M6 on) the engine and
//! UI server. The binary in `main.rs` only wires these together.

pub mod app;
pub mod cli;
pub mod clock_debug;
pub mod config;
#[cfg(feature = "desktop")]
pub mod desktop;
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
/// Git commit count (build.rs); the same for a commit in every checkout.
pub const BUILD_NUMBER: &str = env!("SEQ_BUILD_NUMBER");
/// Short commit hash, `-dirty` with uncommitted changes (build.rs).
pub const GIT_COMMIT: &str = env!("SEQ_GIT_COMMIT");
/// Copyright line (M12 Phase A; same as the LICENSE of the open-source release).
pub const COPYRIGHT: &str = "Copyright (c) 2026 Ulrich Staudinger";
