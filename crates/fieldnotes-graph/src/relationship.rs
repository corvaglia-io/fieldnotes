//! Derived relationship projections.
//!
//! A relationship is an evidence-backed connection between two entities. v0.1
//! derives only the connection A1 reserved a type for: two person entities the
//! source itself recorded in the same current object. It reports observed
//! interaction — cited Notes, counts, and a time range — and never a strength,
//! trust, importance, or sentiment judgement.

use core::fmt;

use fieldnotes_domain::{Datetime, RecordId};

use crate::evidence::Explanation;

/// The kind of connection a relationship projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationshipKind {
    /// Two person entities appearing in the same current source object.
    PersonPerson,
}

impl RelationshipKind {
    /// The A1 record type token for this kind.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RelationshipKind::PersonPerson => "person_person",
        }
    }
}

impl fmt::Display for RelationshipKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One derived relationship projection.
///
/// `person_person` is undirected. `from_entity_id` and `to_entity_id` are the
/// canonical orientation — the pair ordered by primary identity anchor — so the
/// same evidence yields the same record whatever order the input arrived in.
/// Neither side implies an initiator or a direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// The projection ID (`rel_<uuidv7>`).
    pub id: RecordId,
    /// The kind of connection.
    pub kind: RelationshipKind,
    /// The lower-ordered entity of the canonical orientation.
    pub from_entity_id: RecordId,
    /// The higher-ordered entity of the canonical orientation.
    pub to_entity_id: RecordId,
    /// The registered Field stems the connection was observed through.
    pub channels: Vec<String>,
    /// The cited Notes, ascending by ID. Bounded by
    /// [`crate::GraphConfig::evidence_limit`] when one is configured.
    pub evidence: Vec<RecordId>,
    /// The number of distinct current Notes recording both parties together,
    /// even when [`Relationship::evidence`] is a bounded representative list.
    pub interaction_count: usize,
    /// The earliest supporting event instant.
    pub first_seen: Option<Datetime>,
    /// The latest supporting event instant.
    pub last_seen: Option<Datetime>,
    /// Why this relationship exists, in full.
    pub explanation: Explanation,
}
