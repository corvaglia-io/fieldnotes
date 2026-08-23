//! The A1 shared property registry, re-exported from [`fieldnotes_domain`].
//!
//! A property name, its scalar type, and its list semantics are shared
//! vocabulary, not byte form, so the authoritative definition lives in
//! [`fieldnotes_domain::property::registry`]. This crate consumes it to
//! decide the byte form — RFC 8785 number spelling, the plain-versus-quoted
//! text rule, canonical key order — which is this crate's own job and stays
//! here. Re-exported under this module path so [`crate::build`],
//! [`crate::emit`], and [`crate::record`] need no changes to keep consuming
//! it as `crate::registry::*`.
//!
//! See [ADR 0010](../../../docs/decisions/0010-property-registry-relocation.md)
//! for why the registry moved: a Field process binary transitively depended
//! on this crate's canonical serializer purely to reach the registry, which
//! is exactly the notebook-byte-work coupling A2 section 6's normalized
//! source envelope decision forbids a Field from carrying.

pub use fieldnotes_domain::property::registry::{
    DERIVED_RECORD_ONLY, ListSemantics, PropertyRegistry, PropertyType, SEMANTIC_EXCLUSIONS,
    is_note_applicable,
};
