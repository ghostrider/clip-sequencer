//! Pure sequencer core. No IO, no async, no clock reads: everything time-related is driven by
//! ticks passed in by the caller, so all of it is unit-testable.
//!
//! - [`model`]: sequences, lanes, steps and the on-disk file format (SPEC §5, §6).
//! - [`validate`]: rules a sequence file must satisfy, with path-qualified errors.
//! - [`file`]: parse and serialize `.seq.json` text, including the version check.
//! - [`index`]: the live deck's layers and clips as a plain value (built by `resolume-client`).
//! - [`resolve`]: step clip and lane layer resolution against an index (SPEC §4.3, §4.4).
//! - [`record`]: a take recorded from Resolume, written into a sequence (SPEC §7.9).
//! - [`seq_sync`]: what "Sync SEQ layer" has to create, add and may clear (SPEC §7.6).
//! - [`transition`]: layer transition values, originals and who changed them (SPEC §7.7).

pub mod edit;
pub mod file;
pub mod index;
pub mod model;
pub mod record;
pub mod resolve;
pub mod scheduler;
pub mod seq_sync;
pub mod transition;
pub mod validate;

pub use file::{file_version, from_json, to_json, LoadError, FORMAT_VERSION};
pub use model::{
    AfterLastPass, ClipRef, Deck, Lane, LaneEnd, LaneMode, Repeat, SeqFile, Sequence, Start, Step,
    SIXTEENTHS_PER_BAR, TICKS_PER_SIXTEENTH,
};
pub use validate::{validate, ValidationError, ValidationErrorKind};
