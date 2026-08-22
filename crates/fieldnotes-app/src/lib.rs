//! Fieldnotes application use cases and orchestration.
//!
//! This crate coordinates domain, storage, and deterministic graph services;
//! user interfaces and source-vendor adapters remain outside it.
//!
//! `0.1.0` provides the four local-notebook use cases — [`init()`],
//! [`create_note`], [`status()`], and [`inspect()`] — over an injected
//! [`Kernel`] holding the clock,
//! the ID generator, and the numeric UTC offset that generated datetimes carry.
//! Nothing here reads the wall clock, an operating-system random source, or the
//! network, so the same inputs always produce the same bytes.

pub mod error;
pub mod init;
pub mod inspect;
pub mod kernel;
pub mod note;
pub mod status;

pub use error::AppError;
pub use init::{InitOutcome, init};
pub use inspect::{InspectReport, InspectedArtifact, InspectedRecord, ReportedProblem, inspect};
pub use kernel::{Kernel, SELF_FIELD};
pub use note::{NoteOutcome, NoteRequest, NoteSource, create_note};
pub use status::{StatusReport, status};
