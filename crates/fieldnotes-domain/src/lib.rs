//! Core Fieldnotes domain concepts and invariants.
//!
//! This crate is independent of filesystems, processes, networks, vendors, and
//! model runtimes. It owns the A1 identifier grammars, datetime value rules,
//! the closed primary Note-type vocabulary, and the scalar value model shared
//! with the format crate.

pub mod datetime;
pub mod field;
pub mod ids;
pub mod note_type;
pub mod property;
pub mod value;

pub use datetime::{Date, Datetime, DatetimeError};
pub use field::{FieldId, FieldIdError, FieldStemRegistry};
pub use ids::{
    ArtifactId, Clock, IdError, RandomSource, RecordId, RecordIdGenerator, RecordKind, Uuid7,
};
pub use note_type::NoteType;
pub use value::{Scalar, ScalarKind, Value};
