//! Review candidates: matches that weak evidence suggests and no rule may make.
//!
//! Medium or weak evidence produces a candidate rather than a silent union. A
//! candidate is modelled separately from a resolved [`crate::entity::Entity`] on
//! purpose: it is never emitted as an entity or relationship record, never joins
//! anchors, and never influences the derived graph. It exists so a human, or a
//! later release with contact evidence, can act on it explicitly.

use core::fmt;

use fieldnotes_domain::RecordId;

use crate::identity::IdentityKey;

/// Why a candidate was raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CandidateReason {
    /// Two distinct entities carry the same normalized display name. Display
    /// name equality is explicitly not identity evidence.
    DisplayNameEquality,
    /// A role property named a value that could not be normalized into any
    /// declared namespace, and that value equals a known entity's display name.
    UnresolvedValueMatchesName,
}

impl CandidateReason {
    /// The stable lowercase label used in reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CandidateReason::DisplayNameEquality => "display-name-equality",
            CandidateReason::UnresolvedValueMatchesName => "unresolved-value-matches-name",
        }
    }
}

impl fmt::Display for CandidateReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One suggested, deliberately unapplied match.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MergeCandidate {
    /// Why it was raised.
    pub reason: CandidateReason,
    /// The weak value the candidate rests on, such as a normalized display name.
    pub value: String,
    /// The entities involved, ascending by ID.
    pub entities: Vec<RecordId>,
    /// The distinct normalized anchors the candidate would have joined,
    /// ascending. Empty when one side has no anchor at all.
    pub identities: Vec<IdentityKey>,
    /// The Notes that supplied the weak evidence, ascending by ID.
    pub evidence: Vec<RecordId>,
    /// Why this is a candidate and not a merge.
    pub detail: String,
}

impl fmt::Display for MergeCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.value, self.reason, self.detail)
    }
}
