//! Resolume clip sequencer application: configuration, storage and (from M6 on) the engine and
//! UI server. The binary in `main.rs` only wires these together.

pub mod app;
pub mod clock_debug;
pub mod config;
pub mod engine;
pub mod plugin_view;
pub mod saver;
pub mod server;
pub mod storage;
pub mod thumbs;
