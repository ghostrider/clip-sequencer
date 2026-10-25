//! Resolume client. Only the API surface recorded in `docs/resolume-api-notes.md` is used.
//!
//! - [`composition`]: composition JSON → [`seq_core::index::CompositionIndex`] plus clip states.
//! - [`rest`]: REST calls (composition dump, thumbnails, deck select, clip thumbnail upload).
//!   Async; never awaited on the timing path.
//! - [`osc`]: the timing-critical sender (clip connect, layer clear, deck select). Synchronous
//!   UDP send, safe to call from the engine task.
//! - [`ws`]: WebSocket task with reconnect and resubscribe; parses composition pushes itself so
//!   the engine never does.

pub mod composition;
pub mod osc;
pub mod rest;
pub mod ws;

/// Where Resolume listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolumeAddr {
    pub host: String,
    pub rest_port: u16,
    pub osc_port: u16,
}

impl ResolumeAddr {
    pub fn rest_base(&self) -> String {
        format!("http://{}:{}/api/v1", self.host, self.rest_port)
    }

    pub fn ws_url(&self) -> String {
        format!("ws://{}:{}/api/v1", self.host, self.rest_port)
    }

    pub fn osc_target(&self) -> String {
        format!("{}:{}", self.host, self.osc_port)
    }
}
