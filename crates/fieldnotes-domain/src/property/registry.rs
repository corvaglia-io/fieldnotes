//! The approved v1 property registry: shared property names, scalar types,
//! and list semantics.
//!
//! A property name has one meaning and one type everywhere in a notebook.
//! Set-like lists are deduplicated and sorted by normalized text value;
//! ordered lists preserve their registered role/generator order.
//!
//! This is shared vocabulary, not byte form: names, scalar types, and list
//! semantics, with no serialization of its own. The `fieldnotes-format` crate
//! is what turns a looked-up [`PropertyType`] into RFC 8785 number spelling,
//! the plain-versus-quoted text rule, and every other canonical byte; it
//! consumes this registry rather than defining a second one. See
//! [ADR 0010](../../../../docs/decisions/0010-property-registry-relocation.md)
//! for why the registry lives here instead of alongside the emitter: a Field
//! binary needs it to enforce the declared-property and Note-applicable-name
//! rules, but must never need the canonical serializer, and this crate is the
//! one dependency both sides can share without one dragging in the other.
//!
//! # `skipped_attachments`
//!
//! Approved by [ADR 0007](../../../../docs/decisions/0007-attachment-retention-policy.md)
//! as a Note-applicable, set-like `list[text]`: a Note may have several
//! attachments with some retained and some skipped, and A1 forbids both a
//! second boolean-shaped property per attachment (there is no per-attachment
//! slot to hang it on) and an array of objects. Two index-correlated parallel
//! lists were also rejected, because A1 sorts and deduplicates set-like lists,
//! which would destroy the index correlation between a references list and a
//! sizes list. One flat set-like list of stable connector-namespaced
//! attachment references is the only shape that survives canonicalization.
//!
//! Deliberately absent: a stored byte size or a stored reason. Re-collection
//! (see the ADR) re-evaluates each reference against the *current* retention
//! policy and refetches metadata from the source at that time, so a stored
//! size would only be a stale copy of something the source already knows, and
//! a stored reason would only be a stale copy of a policy decision that may
//! since have changed. Per-attachment human detail — names, sizes, why a
//! particular attachment was skipped — belongs in the Markdown body as
//! deterministic evidence, which A1's flat-frontmatter rule does not
//! constrain.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::ScalarKind;

/// Whether a registered list's order carries meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSemantics {
    /// Deduplicated and sorted by normalized text value at serialization.
    Set,
    /// Source/role/generator order is preserved.
    Ordered,
}

/// The registered shape of one property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyType {
    /// A single scalar of the given kind.
    Scalar(ScalarKind),
    /// A homogeneous list of the given kind with the given order semantics.
    List(ScalarKind, ListSemantics),
}

/// The frozen A1 registry of shared and derived-record property types.
#[derive(Debug)]
pub struct PropertyRegistry {
    map: BTreeMap<&'static str, PropertyType>,
}

impl PropertyRegistry {
    /// Looks up the registered type of a property name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<PropertyType> {
        self.map.get(name).copied()
    }

    /// The frozen v1 registry.
    #[must_use]
    pub fn v1() -> &'static PropertyRegistry {
        static REGISTRY: LazyLock<PropertyRegistry> = LazyLock::new(build_v1);
        &REGISTRY
    }
}

fn build_v1() -> PropertyRegistry {
    use ListSemantics::{Ordered, Set};
    use PropertyType::{List, Scalar};
    use ScalarKind::{Bool, Datetime, Number, Text};

    let mut map: BTreeMap<&'static str, PropertyType> = BTreeMap::new();
    // Required Note properties (id/type are shared with every public record).
    map.insert("id", Scalar(Text));
    map.insert("instance_id", Scalar(Text));
    map.insert("field_id", Scalar(Text));
    map.insert("type", Scalar(Text));
    map.insert("occurred_at", Scalar(Datetime));
    // Shared Note properties.
    map.insert("captured_at", Scalar(Datetime));
    map.insert("started_at", Scalar(Datetime));
    map.insert("ended_at", Scalar(Datetime));
    map.insert("duration_seconds", Scalar(Number));
    map.insert("source_scope", Scalar(Text));
    map.insert("source_identity", Scalar(Text));
    map.insert("source_parent_id", Scalar(Text));
    map.insert("source_url", Scalar(Text));
    map.insert("source_version", Scalar(Text));
    map.insert("collected_by", List(Text, Set));
    map.insert("content_hash", Scalar(Text));
    map.insert("from", Scalar(Text));
    map.insert("to", List(Text, Ordered));
    map.insert("cc", List(Text, Ordered));
    map.insert("bcc", List(Text, Ordered));
    map.insert("organizer", Scalar(Text));
    map.insert("participants", List(Text, Ordered));
    map.insert("subject", Scalar(Text));
    map.insert("title", Scalar(Text));
    map.insert("thread_id", Scalar(Text));
    map.insert("conversation_id", Scalar(Text));
    map.insert("reply_to", Scalar(Text));
    map.insert("related", List(Text, Set));
    map.insert("attachments", List(Text, Ordered));
    map.insert("artifacts", List(Text, Set));
    map.insert("audio_duration_seconds", Scalar(Number));
    map.insert("audio_media_type", Scalar(Text));
    map.insert("identities", List(Text, Set));
    map.insert("entities", List(Text, Set));
    map.insert("damaged", Scalar(Bool));
    map.insert("truncated", Scalar(Bool));
    map.insert("lost_characters", Scalar(Number));
    map.insert("skipped_attachments", List(Text, Set));
    // Derived-record properties.
    map.insert("generated_at", Scalar(Datetime));
    map.insert("generator_version", Scalar(Text));
    map.insert("source_note_id", Scalar(Text));
    map.insert("evidence_spans", List(Text, Ordered));
    map.insert("supported_by", List(Text, Set));
    map.insert("confidence", Scalar(Number));
    map.insert("subject_entity_id", Scalar(Text));
    map.insert("from_entity_id", Scalar(Text));
    map.insert("to_entity_id", Scalar(Text));
    map.insert("first_seen", Scalar(Datetime));
    map.insert("last_seen", Scalar(Datetime));
    map.insert("channels", List(Text, Set));
    map.insert("evidence_count", Scalar(Number));
    map.insert("evidence", List(Text, Ordered));
    map.insert("binding_status", Scalar(Text));
    map.insert("entity_id", Scalar(Text));
    map.insert("subject_identity", Scalar(Text));
    map.insert("target_field_id", Scalar(Text));
    map.insert("target_source_id", Scalar(Text));
    map.insert("status", Scalar(Text));
    map.insert("detected_at", Scalar(Datetime));
    map.insert("candidate_fingerprints", List(Text, Set));
    map.insert("involved_note_ids", List(Text, Set));
    map.insert("producer_references", List(Text, Set));
    map.insert("source_identities", List(Text, Set));
    map.insert("source_scopes", List(Text, Set));
    PropertyRegistry { map }
}

/// The bookkeeping properties excluded from `fn-record-v1` semantic encoding,
/// in ascending ASCII order.
pub const SEMANTIC_EXCLUSIONS: [&str; 9] = [
    "captured_at",
    "collected_by",
    "content_hash",
    "entities",
    "field_id",
    "id",
    "instance_id",
    "related",
    "source_version",
];

/// Shared registry names that exist only for a **derived record** — a Note
/// generated from other Notes rather than collected from a source, such as a
/// summary, an entity, or a conflict bundle.
///
/// A Field collects a Note. It maps a source object onto A1 vocabulary; it
/// never derives one Note from others, and it has no evidence span, no
/// confidence score, and no binding status to report. These names are
/// registered A1 property types — [`PropertyRegistry::lookup`] resolves them —
/// but they are not part of the subset a Field's record may use. Listed in
/// ascending ASCII order.
pub const DERIVED_RECORD_ONLY: [&str; 26] = [
    "binding_status",
    "candidate_fingerprints",
    "channels",
    "confidence",
    "detected_at",
    "entity_id",
    "evidence",
    "evidence_count",
    "evidence_spans",
    "first_seen",
    "from_entity_id",
    "generated_at",
    "generator_version",
    "involved_note_ids",
    "last_seen",
    "producer_references",
    "source_identities",
    "source_note_id",
    "source_scopes",
    "status",
    "subject_entity_id",
    "subject_identity",
    "supported_by",
    "target_field_id",
    "target_source_id",
    "to_entity_id",
];

/// Whether `name` is in the Note-applicable subset of the shared registry.
///
/// A name outside the registry entirely is not covered by this check at all;
/// callers first look the name up with [`PropertyRegistry::lookup`] and then
/// ask this question only for a name that lookup found. `false` means the name
/// is a registered shared property, but one that exists only for a derived
/// record, never for a Field's collected Note.
#[must_use]
pub fn is_note_applicable(name: &str) -> bool {
    !DERIVED_RECORD_ONLY.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_types_and_semantics() {
        let registry = PropertyRegistry::v1();
        assert_eq!(
            registry.lookup("occurred_at"),
            Some(PropertyType::Scalar(ScalarKind::Datetime))
        );
        assert_eq!(
            registry.lookup("participants"),
            Some(PropertyType::List(ScalarKind::Text, ListSemantics::Ordered))
        );
        assert_eq!(
            registry.lookup("identities"),
            Some(PropertyType::List(ScalarKind::Text, ListSemantics::Set))
        );
        assert_eq!(registry.lookup("chat_id"), None);
    }

    #[test]
    fn derived_record_only_names_are_all_registered_and_sorted() {
        // Every excluded name must actually be in the registry: this list
        // narrows what a Field may use, it does not add new vocabulary.
        let registry = PropertyRegistry::v1();
        for name in DERIVED_RECORD_ONLY {
            assert!(
                registry.lookup(name).is_some(),
                "{name} is listed as derived-record-only but is not a registered property"
            );
            assert!(
                !is_note_applicable(name),
                "{name} must not be Note-applicable"
            );
        }
        let mut sorted = DERIVED_RECORD_ONLY;
        sorted.sort_unstable();
        assert_eq!(
            sorted, DERIVED_RECORD_ONLY,
            "the list is documented as ascending ASCII order"
        );
    }

    #[test]
    fn ordinary_note_properties_remain_note_applicable() {
        for name in [
            "title",
            "subject",
            "to",
            "cc",
            "participants",
            "occurred_at",
        ] {
            assert!(
                is_note_applicable(name),
                "{name} must remain Note-applicable"
            );
        }
    }

    #[test]
    fn skipped_attachments_is_a_note_applicable_set_like_text_list() {
        let registry = PropertyRegistry::v1();
        assert_eq!(
            registry.lookup("skipped_attachments"),
            Some(PropertyType::List(ScalarKind::Text, ListSemantics::Set)),
            "a Note may carry several attachments with some retained and some skipped, so this \
             must be a set-like list rather than a scalar"
        );
        assert!(
            is_note_applicable("skipped_attachments"),
            "a Field collects Notes, so this must remain usable on a collected Note"
        );
        assert!(!SEMANTIC_EXCLUSIONS.contains(&"skipped_attachments"));
    }
}
