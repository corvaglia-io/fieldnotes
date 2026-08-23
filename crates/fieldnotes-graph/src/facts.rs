//! Reproducible derived facts: threads, artifact references, and portable
//! source-key collapses.
//!
//! These are objective aggregates over current Notes — membership, counts, and
//! time ranges — that a human or downstream agent interprets. None of them is
//! materialized as a public record: A1 reserves no thread or duplicate-artifact
//! record type, and inventing one would be an unapproved vocabulary addition.

use core::fmt;

use fieldnotes_domain::{ArtifactId, Datetime, RecordId};

use crate::identity::IdentityKey;

/// Which source-provided property supplied a thread key.
///
/// `thread_id` and `conversation_id` are separate registered properties with
/// separate meanings, so their values are never pooled into one key space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThreadKeyKind {
    /// From the `thread_id` property.
    Thread,
    /// From the `conversation_id` property.
    Conversation,
}

impl ThreadKeyKind {
    /// The property name this kind reads.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadKeyKind::Thread => "thread_id",
            ThreadKeyKind::Conversation => "conversation_id",
        }
    }
}

impl fmt::Display for ThreadKeyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A source thread or conversation identity, qualified by the portable source
/// scope it is exact within.
///
/// A thread ID is source-local, so two Notes join into one thread only when
/// their `source_scope` matches as well. A Note carrying a thread key with no
/// `source_scope` cannot be scoped safely and is reported as a gap instead.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadKey {
    /// Which property supplied the value.
    pub kind: ThreadKeyKind,
    /// The portable source scope the value is exact within.
    pub scope: String,
    /// The source-provided value, verbatim.
    pub value: String,
}

impl fmt::Display for ThreadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={} in scope {}", self.kind, self.value, self.scope)
    }
}

/// One derived thread: the Notes sharing a scoped source thread key, with the
/// participants derived from them.
///
/// Instances arrive already ordered by [`ThreadKey`]; the struct itself is not
/// ordered because a datetime is ordered by instant rather than by spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// The scoped key that groups the Notes.
    pub key: ThreadKey,
    /// The member Notes, ascending by ID.
    pub notes: Vec<RecordId>,
    /// The normalized participant anchors across the thread, ascending.
    pub participants: Vec<IdentityKey>,
    /// The entities those anchors resolved to, ascending by ID.
    pub entities: Vec<RecordId>,
    /// The earliest member event instant.
    pub first_seen: Option<Datetime>,
    /// The latest member event instant.
    pub last_seen: Option<Datetime>,
}

impl Thread {
    /// The number of member Notes.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }
}

/// Every current Note that references one original artifact.
///
/// Two Notes referencing the same artifact ID reference the same exact bytes,
/// which A1 makes an identity statement about the artifact — not about the
/// Notes. The Notes stay separate evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactReference {
    /// The content-addressed artifact ID.
    pub artifact: ArtifactId,
    /// Notes listing the artifact in `artifacts`, ascending by ID.
    pub notes: Vec<RecordId>,
    /// The subset that received it as an attachment, ascending by ID.
    pub attachment_notes: Vec<RecordId>,
    /// The earliest referencing event instant.
    pub first_seen: Option<Datetime>,
    /// The latest referencing event instant.
    pub last_seen: Option<Datetime>,
}

impl ArtifactReference {
    /// Whether more than one current Note carries these exact bytes.
    #[must_use]
    pub fn is_duplicated(&self) -> bool {
        self.notes.len() > 1
    }
}

/// Two or more input Notes that carried the same portable source key and the
/// same semantic payload, collapsed to one for derivation.
///
/// This is derivation-input reconciliation only. Nothing is written, no Note is
/// removed, and producer provenance from every collapsed copy is retained here
/// so a caller can union it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceKeyCollapse {
    /// The portable source scope.
    pub source_scope: String,
    /// The source identity inside that scope.
    pub source_identity: String,
    /// The Note used as the survivor: the lowest Note ID in the group.
    pub survivor: RecordId,
    /// The collapsed Notes, ascending by ID, including the survivor.
    pub notes: Vec<RecordId>,
    /// The `<instance_id>/<field_id>` producer references seen across the group,
    /// ascending.
    pub producers: Vec<String>,
    /// The shared `fn-record-v1-sha256` semantic fingerprint.
    pub fingerprint: String,
}
