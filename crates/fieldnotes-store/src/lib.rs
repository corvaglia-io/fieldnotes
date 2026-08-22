//! Atomic persistence for Fieldnotes notebooks and operational state.
//!
//! This crate owns filesystem transactions, artifact storage, reconciliation,
//! and merge mechanics without source-vendor logic.
//!
//! Two rules shape everything here.
//!
//! **Only validated bytes are written.** A durable writer accepts a
//! [`fieldnotes_format::CanonicalRecord`], which cannot exist unless the format
//! crate has already emitted canonical bytes, re-parsed them, and validated
//! them with the same validator the conformance suite runs. This crate never
//! formats frontmatter.
//!
//! **Every install is a same-directory rename.** Bytes are staged in the
//! destination directory, made durable, and renamed onto their final name; see
//! [`atomic`] for the per-platform durability details. A crash therefore leaves
//! either the previous complete file or the new complete file, never a
//! valid-looking partial Note.

pub mod artifact;
pub mod atomic;
pub mod error;
pub mod instance;
pub mod layout;
pub mod note;
pub mod scan;

pub use artifact::{StoredArtifact, store_artifact};
pub use error::StoreError;
pub use instance::{read_instance, write_instance};
pub use layout::{InitState, Notebook};
pub use note::{NoteWrite, replace_note, write_note};
pub use scan::{NotebookScan, Problem, ScanOptions, ScannedArtifact, ScannedNote, scan};
