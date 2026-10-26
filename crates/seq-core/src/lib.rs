//! Pure sequencer core. No IO, no async, no clock reads: everything time-related is driven by
//! ticks passed in by the caller, so all of it is unit-testable.
//!
//! - [`model`]: sequences, lanes, steps and the on-disk file format (SPEC §5, §6).
//! - [`validate`]: rules a sequence file must satisfy, with path-qualified errors.
//! - [`file`]: parse and serialize `.seq.json` text, including the version check.
//! - [`index`]: the live deck's layers and clips as a plain value (built by `resolume-client`).
//! - [`resolve`]: step clip and lane layer resolution against an index (SPEC §4.3, §4.4).

pub mod edit;
pub mod file;
pub mod index;
pub mod model;
pub mod resolve;
pub mod scheduler;
pub mod validate;

pub use file::{from_json, to_json, LoadError, FORMAT_VERSION};
pub use model::{
    AfterLastPass, ClipRef, Deck, Lane, LaneEnd, Repeat, SeqFile, Sequence, Start, Step,
    EIGHTHS_PER_BAR, TICKS_PER_EIGHTH,
};
pub use validate::{validate, ValidationError, ValidationErrorKind};
