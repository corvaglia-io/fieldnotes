//! Derived entity projections.
//!
//! An entity is the current evidence-backed projection that several normalized
//! identity anchors refer to one real-world thing. Its ID is a locator for this
//! projection, not durable proof of real-world identity: a rebuild whose prior
//! entity files were deleted may mint different IDs for the same person, so
//! consumers follow the anchors and cited evidence.

use core::fmt;

use fieldnotes_domain::{Datetime, RecordId};

use crate::evidence::Explanation;
use crate::identity::IdentityKey;

/// The kind of thing an entity projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityKind {
    /// A person.
    Person,
    /// An organization.
    Organization,
    /// An immutable original artifact, identified by its exact bytes.
    Artifact,
}

impl EntityKind {
    /// The A1 record type token for this kind.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Person => "person",
            EntityKind::Organization => "organization",
            EntityKind::Artifact => "artifact",
        }
    }

    /// Whether v0.1 materializes this kind as a public `entities/` record.
    ///
    /// `person` and `organization` have frozen or reserved envelopes. An
    /// artifact's exact identity already lives in its content-addressed
    /// artifact ID and in the Notes that cite it, and no artifact-entity fixture
    /// is frozen, so artifact evidence is reported through
    /// [`crate::facts::ArtifactReference`] instead of a projected record.
    #[must_use]
    pub fn is_materialized(self) -> bool {
        matches!(self, EntityKind::Person | EntityKind::Organization)
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One derived entity projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    /// The projection ID (`ent_<uuidv7>`).
    pub id: RecordId,
    /// The kind of thing projected.
    pub kind: EntityKind,
    /// The normalized anchors this entity rests on, in ascending key order.
    pub identities: Vec<IdentityKey>,
    /// The display name, present only when current evidence supplies exactly
    /// one. Contradictory names leave this `None` and are reported as a
    /// conflict, because picking one would be last-writer-wins.
    pub title: Option<String>,
    /// The registered Field stems the entity was observed through, ascending.
    pub channels: Vec<String>,
    /// The cited Notes, ascending by ID. Bounded by
    /// [`crate::GraphConfig::evidence_limit`] when one is configured.
    pub evidence: Vec<RecordId>,
    /// The number of distinct current Notes supporting the entity, even when
    /// [`Entity::evidence`] is a bounded representative list.
    pub interaction_count: usize,
    /// The earliest supporting event instant.
    pub first_seen: Option<Datetime>,
    /// The latest supporting event instant.
    pub last_seen: Option<Datetime>,
    /// Why this entity exists, in full.
    pub explanation: Explanation,
}

impl Entity {
    /// The lowest-ordered anchor, used as the entity's stable sort key and as
    /// the subject identity a proposal can rebind through.
    #[must_use]
    pub fn primary_identity(&self) -> Option<&IdentityKey> {
        self.identities.first()
    }

    /// Whether `key` is one of this entity's anchors.
    #[must_use]
    pub fn has_identity(&self, key: &IdentityKey) -> bool {
        self.identities.contains(key)
    }

    /// The anchors that may be written into a public `identities` list.
    pub fn publishable_identities(&self) -> impl Iterator<Item = &IdentityKey> {
        self.identities.iter().filter(|key| key.is_publishable())
    }
}
