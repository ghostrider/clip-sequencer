//! Resolume client: REST, WebSocket (reconnect, resubscribe), OSC sender and the composition
//! index builder. The API surface it may use is recorded in `docs/resolume-api-notes.md`.
//!
//! - [`composition`]: composition JSON (REST `GET /composition` or a WebSocket push) →
//!   [`seq_core::index::CompositionIndex`] plus clip states.

pub mod composition;
