//! Deterministic Fieldnotes identity and relationship derivation.
//!
//! This crate derives an explainable identity and relationship graph from
//! validated notebook records. It performs no inference, no enhancement, and no
//! I/O: the caller reads and writes files, and passes in parsed records.
//!
//! # What a caller does
//!
//! ```no_run
//! use fieldnotes_domain::{RecordIdGenerator, RecordKind};
//! use fieldnotes_format::{ParsedRecord, parse_record};
//! use fieldnotes_graph::{GraphConfig, derive_graph};
//!
//! # fn run<C: fieldnotes_domain::Clock + Copy, R: fieldnotes_domain::RandomSource>(
//! #     files: &[Vec<u8>], clock: C, random: R,
//! # ) -> Result<(), Box<dyn std::error::Error>> {
//! let records: Vec<ParsedRecord> = files
//!     .iter()
//!     .map(|bytes| parse_record(bytes))
//!     .collect::<Result<_, _>>()?;
//! let mut ids = RecordIdGenerator::new(clock, random);
//! let graph = derive_graph(&records, &GraphConfig::default(), &clock, &mut ids)?;
//!
//! for entity in graph.entities() {
//!     // Every projection can name the Notes and anchors that produced it.
//!     let explanation = graph.explain(&entity.id);
//!     assert!(explanation.is_some());
//! }
//! for projected in graph.projected_records()? {
//!     // Validated canonical bytes; the caller owns the write.
//!     let _ = (projected.relative_path, projected.record.bytes());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Determinism
//!
//! Derivation reads no wall clock, no environment, no filesystem, and no random
//! source of its own. The generation instant comes from an injected
//! [`fieldnotes_domain::Clock`] and projection IDs from an injected
//! [`ProjectionIds`] generator. Every internal collection is ordered, and every
//! output is sorted by a key that is a function of the evidence — anchors,
//! record IDs, and instants — never of input order. The same records in a
//! different order therefore produce the same graph, byte for byte.
//!
//! # What it refuses
//!
//! Identities join only through an approved deterministic rule: the same
//! normalized channel anchor, or the anchors a source contact record states
//! belong to one subject. Display-name equality, timestamp proximity, subject
//! similarity, organization labels, and equal `content_hash` values never merge
//! anything. Weak evidence becomes a [`MergeCandidate`], which is modelled apart
//! from resolved entities and never emitted as a record.
//!
//! # What it does not do
//!
//! It writes no files, creates no conflict bundles, and generates no Extractions
//! or Observations. A condition that would open a conflict bundle is reported as
//! a [`ReportedConflict`] for the caller to act on. Threads, artifact duplicates,
//! and candidates are returned as derived facts rather than records, because A1
//! reserves no record type for them.

pub mod candidate;
pub mod derive;
pub mod emit;
pub mod entity;
pub mod evidence;
pub mod facts;
pub mod gap;
pub mod identity;
pub mod relationship;

pub use candidate::{CandidateReason, MergeCandidate};
pub use derive::{DerivedGraph, GraphConfig, GraphError, ProjectionIds, derive_graph};
pub use emit::{ProjectedRecord, entity_record, relationship_record};
pub use entity::{Entity, EntityKind};
pub use evidence::{
    CompetingEvidence, ENTITY_GENERATOR, Explanation, IdentityJoin, Origin, RELATIONSHIP_GENERATOR,
    ResolvedIdentity, RuleId,
};
pub use facts::{ArtifactReference, SourceKeyCollapse, Thread, ThreadKey, ThreadKeyKind};
pub use gap::{ConflictKind, Gap, GapKind, ReportedConflict};
pub use identity::{
    IdentityKey, NamespacePolicy, NamespaceRegistry, NormalizationRule, Refusal, RefusalReason,
    ScopeClass, Strength,
};
pub use relationship::{Relationship, RelationshipKind};
