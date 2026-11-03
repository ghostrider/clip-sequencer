//! Resolume clip sequencer application: configuration, storage and (from M6 on) the engine and
//! UI server. The binary in `main.rs` only wires these together.

pub mod app;
pub mod cli;
pub mod clock_debug;
pub mod config;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod engine;
pub mod logging;
pub mod plugin_view;
pub mod runner;
pub mod saver;
pub mod server;
pub mod settings;
pub mod storage;
pub mod thumbs;
pub mod ui_port;

/// Product name shown in the window title, splash and `/api/version`. A placeholder until the
/// open-source release names the project (docs/M11-open-source-release.md, Phase A).
/// Source: `productName` in `tauri.conf.json` (also the exe's ProductName; build.rs).
pub const APP_NAME: &str = env!("SEQ_APP_NAME");
/// Stable identifier in `/api/version`; a second instance recognises a running one by it.
pub const APP_ID: &str = "seq-app";
/// From `[workspace.package]`, the only version source.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Git commit count (build.rs); the same for a commit in every checkout.
pub const BUILD_NUMBER: &str = env!("SEQ_BUILD_NUMBER");
/// Short commit hash, `-dirty` with uncommitted changes (build.rs).
pub const GIT_COMMIT: &str = env!("SEQ_GIT_COMMIT");
/// UTC build time, ISO 8601 (build.rs; `SOURCE_DATE_EPOCH` if set).
pub const BUILD_DATE: &str = env!("SEQ_BUILD_DATE");
/// Target triple of this build, e.g. `x86_64-pc-windows-msvc`.
pub const TARGET: &str = env!("SEQ_TARGET");
/// `0.1.0 (build 42, abc1234, 2026-11-03)`: `--version`, the first log line, error dialogs.
pub const LONG_VERSION: &str = env!("SEQ_LONG_VERSION");
/// Copyright line (M12 Phase A; same as the LICENSE of the open-source release).
/// Source: `bundle.copyright` in `tauri.conf.json` (also the exe's LegalCopyright; build.rs).
pub const COPYRIGHT: &str = env!("SEQ_COPYRIGHT");
