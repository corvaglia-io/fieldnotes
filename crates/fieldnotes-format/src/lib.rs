//! Strict parsing and canonical serialization for public Fieldnotes files.
//!
//! This crate owns the portable notebook representation frozen at A1: the
//! flat-YAML-subset frontmatter grammar, the canonical emitter, body
//! normalization, the `fn-content-v1` and `fn-record-v1` hash domains,
//! artifact identity and paths, Note filenames, instance metadata, and record
//! validation. It does not own persistence or source collection.
//!
//! The A1 contract defines the byte grammar itself, so the frontmatter parser
//! is hand-written for that small subset instead of delegating to a general
//! YAML library whose accepted language is wider than the contract.

pub mod build;
pub mod emit;
pub mod error;
pub mod extension;
pub mod filename;
pub mod hash;
pub mod instance;
pub mod jcs;
pub mod normalize;
pub mod record;
pub mod registry;
mod yaml;

pub use build::{CanonicalRecord, RecordBuilder};
pub use emit::{canonical_record_string, plain_style_allowed, semantic_record_string};
pub use error::ValidationError;
pub use extension::{canonical_extension, detect_media_type};
pub use filename::{expected_note_filename, validate_note_filename};
pub use hash::{
    artifact_id_for_bytes, artifact_relative_path, content_hash_value, record_fingerprint,
    sha256_hex,
};
pub use instance::{InstanceMetadata, instance_yaml_string, parse_instance_yaml};
pub use normalize::{normalize_body_bytes, normalize_body_str};
pub use record::{ParsedRecord, parse_record, validate_record};
pub use registry::{ListSemantics, PropertyRegistry, PropertyType};
pub use yaml::core_schema_resolves_string;
