//! What the graph could not resolve, and the contradictions it preserved.
//!
//! A gap is evidence the resolver deliberately declined to use, with the reason
//! attached. A reported conflict is contradictory current evidence the resolver
//! preserved instead of picking a winner. Neither is written to the notebook by
//! this crate: cross-notebook merge and conflict-bundle creation are a separate
//! pass, so a condition that would open a bundle is surfaced here for the caller
//! to act on.

use core::fmt;

use fieldnotes_domain::RecordId;

/// Why some input could not be resolved into the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum GapKind {
    /// An `identities` anchor could not be normalized into a declared namespace.
    UnresolvedIdentityAnchor,
    /// A role property value (`from`, `to`, `cc`, `bcc`, `organizer`,
    /// `participants`) could not be normalized into a declared namespace.
    UnresolvedRoleValue,
    /// A Note carries a thread or conversation ID but no `source_scope`, so the
    /// key cannot be scoped and is never joined with another Note's.
    UnscopableThreadKey,
    /// A Note supplied no resolvable identity anchor, so it supports no entity.
    NoIdentityAnchors,
    /// A source contact record carried anchors of different entity classes, so
    /// the co-identity rule refused to join them.
    MixedClassContactRecord,
    /// An entity rests only on scoped anchors, which A1 froze no public flat
    /// spelling for, so its `identities` list would be empty.
    UnpublishableIdentities,
    /// A Note listed an artifact reference that is not a valid artifact ID.
    MalformedArtifactReference,
    /// An input record was not a Note and contributed no evidence.
    NonNoteInput,
    /// A record carried a Note ID but not the typed properties a Note needs.
    MalformedNote,
    /// A Note was excluded from derivation because it is part of a preserved
    /// conflict, so using it would silently declare one side current.
    ExcludedByConflict,
}

impl GapKind {
    /// The stable lowercase label used in reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GapKind::UnresolvedIdentityAnchor => "unresolved-identity-anchor",
            GapKind::UnresolvedRoleValue => "unresolved-role-value",
            GapKind::UnscopableThreadKey => "unscopable-thread-key",
            GapKind::NoIdentityAnchors => "no-identity-anchors",
            GapKind::MixedClassContactRecord => "mixed-class-contact-record",
            GapKind::UnpublishableIdentities => "unpublishable-identities",
            GapKind::MalformedArtifactReference => "malformed-artifact-reference",
            GapKind::NonNoteInput => "non-note-input",
            GapKind::MalformedNote => "malformed-note",
            GapKind::ExcludedByConflict => "excluded-by-conflict",
        }
    }
}

impl fmt::Display for GapKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One thing the graph could not resolve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gap {
    /// What kind of gap this is.
    pub kind: GapKind,
    /// The exact input value involved, when there is one.
    pub value: Option<String>,
    /// The property the value came from, when it came from one.
    pub property: Option<String>,
    /// The records involved, ascending by ID.
    pub records: Vec<RecordId>,
    /// Why the resolver declined, in one line.
    pub detail: String,
}

impl fmt::Display for Gap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(property) = &self.property {
            write!(f, " {property}")?;
        }
        if let Some(value) = &self.value {
            write!(f, " {value:?}")?;
        }
        write!(f, ": {}", self.detail)
    }
}

/// The class of a preserved contradiction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ConflictKind {
    /// The same Note ID arrived with divergent semantic content.
    SameNoteIdDivergence,
    /// The same portable source key arrived with divergent current state and no
    /// reliable ordering between the versions.
    SourceKeyDivergence,
    /// Current evidence supplies contradictory display names for one entity.
    ContradictoryName,
    /// More than one prior entity projection shares anchors with one newly
    /// derived entity, so no prior projection ID can be reused unambiguously.
    AmbiguousProjectionRebind,
}

impl ConflictKind {
    /// The stable lowercase label used in reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictKind::SameNoteIdDivergence => "same-note-id-divergence",
            ConflictKind::SourceKeyDivergence => "source-key-divergence",
            ConflictKind::ContradictoryName => "contradictory-name",
            ConflictKind::AmbiguousProjectionRebind => "ambiguous-projection-rebind",
        }
    }
}

impl fmt::Display for ConflictKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One preserved contradiction in current evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReportedConflict {
    /// The class of contradiction.
    pub kind: ConflictKind,
    /// The Notes involved, ascending by ID.
    pub notes: Vec<RecordId>,
    /// The projections involved, ascending by ID.
    pub entities: Vec<RecordId>,
    /// The competing `fn-record-v1-sha256` fingerprints, ascending.
    pub fingerprints: Vec<String>,
    /// The competing values, in ascending order, when the contradiction is
    /// about a value rather than a whole record.
    pub values: Vec<String>,
    /// What contradicts what, in one line.
    pub detail: String,
}

impl fmt::Display for ReportedConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.detail)
    }
}
