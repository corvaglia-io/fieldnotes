//! Mapping one Graph mail message onto a normalized source envelope.
//!
//! Every value this module supplies is post-mapping and pre-serialization,
//! exactly as A2 section 6 requires: it maps vendor structure onto Fieldnotes
//! vocabulary and does none of the work only core may do. Nothing here
//! computes a record ID, a capture time, a content hash, a canonical key
//! order, a filename, or an artifact path -- the record and artifact types
//! this module builds structurally exclude all of them.
//!
//! # The vocabulary this Field uses, and nothing else
//!
//! Unprefixed names come from the Note-applicable subset of A1's closed shared
//! registry (`from`, `to`, `cc`, `bcc`, `reply_to`, `subject`, `title`,
//! `conversation_id`, `thread_id`, `participants`). Prefixed names come from
//! this Field's own `outlook_mail_` declarations in [`crate::manifest`]. No
//! name is invented on either side: an unprefixed name outside the registry
//! and a prefixed name the manifest does not declare are both rejected by
//! core, and inventing a shared name would need registry review this Field
//! cannot grant itself.

use std::fmt;

use fieldnotes_field_protocol::grammar::{
    MarkdownTag, NoteTypeToken, ObjectKind, OffsetDatetime, ProtocolV1, RecordTag, RunId,
    SourceIdentity, SourceScope, SourceVersion, TombstoneTag,
};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{
    ArtifactRef, Body, Change, IdentityAnchor, IdentityScopeClass, Integrity, RecordEvent,
    SourceRef,
};
use fieldnotes_field_protocol::value::{PropertyValue, RecordProperties};

use crate::api::GraphMessage;
use crate::body::AttachmentLine;
use crate::manifest::{ADDRESS_NAMESPACE, ADDRESS_RULE, ADDRESS_RULE_VERSION};

/// Why one message could not be turned into a record.
///
/// Every reason here is a per-message skip, not a run failure: the caller
/// reports it as a diagnostic and keeps collecting, since one unusable message
/// must not cost the whole run. It does, however, stop the run from advancing
/// its cursor, because the message it could not map is a message it has not
/// collected.
#[derive(Debug)]
pub(crate) struct RecordError(pub(crate) String);

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordError {}

/// What every record in one run shares.
pub(crate) struct RecordContext {
    /// Core's identifier for this run.
    pub(crate) run_id: RunId,
    /// The portable exact-source scope every record in this run carries.
    pub(crate) source_scope: SourceScope,
    /// The effective bounds for this run.
    pub(crate) limits: Limits,
}

/// Builds the `mail-message/<id>` source identity.
pub(crate) fn identity_of(message_id: &str) -> Result<SourceIdentity, RecordError> {
    SourceIdentity::parse(&format!(
        "{}/{message_id}",
        crate::constants::OBJECT_KIND_MAIL_MESSAGE
    ))
    .map_err(|error| RecordError(format!("source identity guard: {error}")))
}

fn object_kind() -> Result<ObjectKind, RecordError> {
    ObjectKind::parse(crate::constants::OBJECT_KIND_MAIL_MESSAGE)
        .map_err(|error| RecordError(format!("object kind guard: {error}")))
}

fn note_type() -> Result<NoteTypeToken, RecordError> {
    NoteTypeToken::parse(crate::constants::NOTE_TYPE_MAIL)
        .map_err(|error| RecordError(format!("Note type guard: {error}")))
}

/// Renders a Graph instant as an A1 datetime with an explicit numeric offset.
///
/// Graph reports mail instants in UTC, spelled with a trailing `Z`, which A1
/// section 3 does not admit: it requires an explicit **numeric** offset. This
/// re-renders the same instant as `+00:00`, which is the honest offset for a
/// value whose source states no other one. A mail message carries no
/// sender-local offset anywhere in the Graph mail resource, so inventing one
/// from the notebook's own timezone would attribute a client-side setting to
/// the source.
pub(crate) fn occurred_at_of(graph_instant: &str) -> Result<OffsetDatetime, RecordError> {
    let text = graph_instant.trim();
    // Already-numeric offsets (which Graph does use elsewhere) pass straight
    // through their own guard.
    if let Ok(parsed) = fieldnotes_domain::Datetime::parse(text) {
        return OffsetDatetime::parse(&parsed.to_string()).map_err(|error| {
            RecordError(format!("rendered instant failed its own guard: {error}"))
        });
    }
    let utc = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix('z'))
        .ok_or_else(|| {
            RecordError(
                "the source instant carries neither a numeric UTC offset nor a 'Z'".to_owned(),
            )
        })?;
    let parsed = fieldnotes_domain::Datetime::parse(&format!("{utc}+00:00"))
        .map_err(|error| RecordError(format!("source instant: {error}")))?;
    OffsetDatetime::parse(&parsed.to_string())
        .map_err(|error| RecordError(format!("rendered instant failed its own guard: {error}")))
}

/// The instant this run observed a removal, from the injected wall clock.
pub(crate) fn observed_at_of(unix_millis: u64) -> Result<OffsetDatetime, RecordError> {
    let millis = i64::try_from(unix_millis)
        .map_err(|_| RecordError("the observation instant is out of range".to_owned()))?;
    let datetime = fieldnotes_domain::Datetime::from_unix_millis(millis, 0)
        .map_err(|error| RecordError(format!("observation instant: {error}")))?;
    OffsetDatetime::parse(&datetime.to_string())
        .map_err(|error| RecordError(format!("rendered instant failed its own guard: {error}")))
}

/// Builds one record's property candidates, self-policing every value against
/// the run's declared per-value byte bound and list-member bound.
///
/// A2 section 14's bounds are echoed to the Field precisely so a well-behaved
/// connector can stay inside them rather than discovering one by being
/// rejected: a mail subject can be pathologically long, and a distribution
/// list can name more recipients than the run's list bound admits.
struct Inserter<'a> {
    properties: RecordProperties,
    limits: &'a Limits,
}

impl<'a> Inserter<'a> {
    fn new(limits: &'a Limits) -> Self {
        Inserter {
            properties: RecordProperties::new(),
            limits,
        }
    }

    fn bounded(&self, text: &str) -> String {
        fieldnotes_field_sdk::truncate::truncate_utf8(text, self.limits.max_property_value_bytes).0
    }

    fn insert(&mut self, name: &str, value: PropertyValue) -> Result<(), RecordError> {
        self.properties
            .insert(name, value)
            .map_err(|reason| RecordError(format!("property {name}: {reason}")))
    }

    fn text(&mut self, name: &str, value: &str) -> Result<(), RecordError> {
        let bounded = self.bounded(value);
        if bounded.is_empty() {
            return Ok(());
        }
        self.insert(name, PropertyValue::Text(bounded))
    }

    fn list(&mut self, name: &str, values: Vec<String>) -> Result<(), RecordError> {
        let max_members = usize::try_from(self.limits.max_list_members).unwrap_or(usize::MAX);
        let bounded: Vec<String> = values
            .iter()
            .take(max_members)
            .map(|value| self.bounded(value))
            .filter(|value| !value.is_empty())
            .collect();
        if bounded.is_empty() {
            return Ok(());
        }
        self.insert(name, PropertyValue::TextList(bounded))
    }

    fn boolean(&mut self, name: &str, value: bool) -> Result<(), RecordError> {
        self.insert(name, PropertyValue::Boolean(value))
    }

    fn finish(self) -> RecordProperties {
        self.properties
    }
}

fn addresses(recipients: Option<&Vec<crate::api::Recipient>>) -> Vec<String> {
    let Some(recipients) = recipients else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for recipient in recipients {
        if let Some(address) = recipient.normalized_address()
            && !out.contains(&address)
        {
            out.push(address);
        }
    }
    out
}

fn sender_address(message: &GraphMessage) -> Option<String> {
    message
        .from
        .as_ref()
        .and_then(crate::api::Recipient::normalized_address)
        .or_else(|| {
            message
                .sender
                .as_ref()
                .and_then(crate::api::Recipient::normalized_address)
        })
}

/// Every mail address the message names, deduplicated and sorted.
///
/// `participants` is a registered **ordered** list, so whatever order this
/// Field emits is the order core preserves. Sorting is therefore a deliberate
/// choice rather than an accident of the source: a mail message has no
/// meaningful participant order to preserve -- `to` and `cc` already carry
/// their own header order separately -- and a stable, source-independent order
/// keeps the same message rendering the same bytes whichever run collected it.
fn participants(message: &GraphMessage) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    for address in sender_address(message)
        .into_iter()
        .chain(addresses(message.to_recipients.as_ref()))
        .chain(addresses(message.cc_recipients.as_ref()))
        .chain(addresses(message.bcc_recipients.as_ref()))
    {
        if !all.contains(&address) {
            all.push(address);
        }
    }
    all.sort_unstable();
    all
}

/// The body text this Field derives from the message, deterministically.
fn body_text(message: &GraphMessage) -> String {
    if let Some(body) = &message.body
        && let Some(content) = &body.content
    {
        let is_html = body
            .content_type
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("html"));
        if is_html {
            return crate::body::html_to_text(content);
        }
        return content.trim_end().to_owned();
    }
    message
        .body_preview
        .as_deref()
        .unwrap_or_default()
        .trim_end()
        .to_owned()
}

fn subject_of(message: &GraphMessage) -> Option<String> {
    let subject = message.subject.as_ref()?.trim();
    if subject.is_empty() {
        None
    } else {
        Some(subject.to_owned())
    }
}

fn source_url_of(message_id: &str) -> String {
    format!(
        "https://outlook.office.com/mail/id/{}",
        crate::mail::encode_path_segment(message_id)
    )
}

fn anchors(message: &GraphMessage) -> Vec<IdentityAnchor> {
    let rule = fieldnotes_field_protocol::grammar::RuleName::parse(ADDRESS_RULE).ok();
    let namespace =
        fieldnotes_field_protocol::grammar::IdentityNamespace::parse(ADDRESS_NAMESPACE).ok();
    let Some(namespace) = namespace else {
        return Vec::new();
    };
    participants(message)
        .into_iter()
        .map(|address| IdentityAnchor {
            namespace: namespace.clone(),
            value: address,
            // A mail address is a normalized channel identity: exact enough to
            // relate graph entities, never an upstream object's identity.
            scope_class: IdentityScopeClass::NormalizedChannel,
            scope: None,
            normalization_rule: rule.clone(),
            normalization_version: Some(ADDRESS_RULE_VERSION),
            role: None,
        })
        .collect()
}

/// Builds the `upsert` record for one message.
///
/// `artifacts` and `evidence` come from [`crate::attachment`], already policed
/// against the run's retention policy.
pub(crate) fn build_upsert(
    context: &RecordContext,
    seq: u64,
    message: &GraphMessage,
    artifacts: Vec<ArtifactRef>,
    evidence: &[AttachmentLine],
) -> Result<RecordEvent, RecordError> {
    let message_id = message
        .id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| RecordError("a message arrived with no identifier".to_owned()))?;
    let identity = identity_of(message_id)?;
    let instant = message
        .received_date_time
        .as_deref()
        .or(message.sent_date_time.as_deref())
        .ok_or_else(|| {
            RecordError(format!(
                "message {} carries neither a received nor a sent instant, so it has no event \
                 instant to map",
                identity.as_str()
            ))
        })?;
    let occurred_at = occurred_at_of(instant)?;

    let mut properties = Inserter::new(&context.limits);
    let subject = subject_of(message);
    if let Some(subject) = &subject {
        properties.text("subject", subject)?;
        properties.text("title", subject)?;
    }
    if let Some(from) = sender_address(message) {
        properties.text("from", &from)?;
    }
    for (name, list) in [
        ("to", addresses(message.to_recipients.as_ref())),
        ("cc", addresses(message.cc_recipients.as_ref())),
        ("bcc", addresses(message.bcc_recipients.as_ref())),
    ] {
        properties.list(name, list)?;
    }
    properties.list("participants", participants(message))?;
    if let Some(reply_to) = addresses(message.reply_to.as_ref()).into_iter().next() {
        properties.text("reply_to", &reply_to)?;
    }
    if let Some(conversation) = message
        .conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        properties.text("conversation_id", conversation)?;
        properties.text(
            "thread_id",
            &format!("{}/{conversation}", crate::constants::THREAD_NAMESPACE),
        )?;
    }

    // This Field's own declared prefixed properties.
    if let Some(importance) = message
        .importance
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        properties.text("outlook_mail_importance", importance)?;
    }
    if let Some(internet_id) = message
        .internet_message_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        properties.text("outlook_mail_internet_message_id", internet_id)?;
    }
    if let Some(is_read) = message.is_read {
        properties.boolean("outlook_mail_is_read", is_read)?;
    }
    if let Some(is_draft) = message.is_draft {
        properties.boolean("outlook_mail_is_draft", is_draft)?;
    }
    if let Some(folder) = message
        .parent_folder_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        properties.text("outlook_mail_parent_folder_id", folder)?;
    }
    let categories: Vec<String> = message
        .categories
        .as_ref()
        .map(|values| {
            let mut unique: Vec<String> = Vec::new();
            for value in values {
                let trimmed = value.trim();
                if !trimmed.is_empty() && !unique.iter().any(|kept| kept == trimmed) {
                    unique.push(trimmed.to_owned());
                }
            }
            unique
        })
        .unwrap_or_default();
    properties.list("outlook_mail_categories", categories)?;

    let heading = subject.unwrap_or_else(|| "(no subject)".to_owned());
    let rendered = crate::body::render(&heading, &body_text(message), evidence);
    // The body is bounded by the run's body limit *and* by half its frame
    // limit, so a body that fits `max_body_bytes` can never be the reason a
    // record frame exceeds `max_frame_bytes` and stops the run.
    let body_bound = context
        .limits
        .max_body_bytes
        .min(context.limits.max_frame_bytes / 2);
    let (text, lost_characters) =
        fieldnotes_field_sdk::truncate::truncate_utf8(&rendered, body_bound);

    Ok(RecordEvent {
        v: ProtocolV1,
        frame_type: RecordTag,
        run_id: context.run_id.clone(),
        seq,
        change: Change::Upsert,
        source: SourceRef {
            scope: context.source_scope.clone(),
            identity,
            version: message
                .change_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(SourceVersion::parse)
                .transpose()
                .map_err(|error| RecordError(format!("source version guard: {error}")))?,
            url: Some(source_url_of(message_id)),
            parent_identity: None,
        },
        object_kind: Some(object_kind()?),
        note_type: Some(note_type()?),
        occurred_at: Some(occurred_at),
        properties: Some(properties.finish()),
        body: Some(Body {
            format: MarkdownTag,
            text,
        }),
        artifacts: (!artifacts.is_empty()).then_some(artifacts),
        identity_anchors: Some(anchors(message)),
        integrity: Some(Integrity {
            damaged: false,
            truncated: lost_characters > 0,
            lost_characters: (lost_characters > 0).then_some(lost_characters),
        }),
        authority: None,
        observed_at: None,
    })
}

/// Builds the authoritative tombstone for one removed message.
///
/// A delete carries the portable source key, its declared authority, and the
/// observation instant -- and **no content at all**. That is a structural
/// property of the frame this function builds, not a convention it follows:
/// every content member below is `None`, and the schema rejects a delete that
/// carries any of them, so a deletion can never be confused with an empty or
/// partial collection result.
pub(crate) fn build_tombstone(
    context: &RecordContext,
    seq: u64,
    message_id: &str,
    observed_at_unix_millis: u64,
) -> Result<RecordEvent, RecordError> {
    Ok(RecordEvent {
        v: ProtocolV1,
        frame_type: RecordTag,
        run_id: context.run_id.clone(),
        seq,
        change: Change::Delete,
        source: SourceRef {
            scope: context.source_scope.clone(),
            identity: identity_of(message_id)?,
            version: None,
            url: None,
            parent_identity: None,
        },
        object_kind: Some(object_kind()?),
        note_type: None,
        occurred_at: None,
        properties: None,
        body: None,
        artifacts: None,
        identity_anchors: None,
        integrity: None,
        authority: Some(TombstoneTag),
        observed_at: Some(observed_at_of(observed_at_unix_millis)?),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        RecordContext, build_tombstone, build_upsert, occurred_at_of, participants, source_url_of,
    };
    use crate::api::GraphMessage;
    use fieldnotes_field_protocol::grammar::{RunId, SourceScope};
    use fieldnotes_field_protocol::limits::Limits;
    use fieldnotes_field_protocol::message::{Change, Validate as _};
    use fieldnotes_field_protocol::value::PropertyValue;

    fn context() -> RecordContext {
        RecordContext {
            run_id: RunId::parse("1a4c9f2e-0000-4000-8000-000000000002")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            source_scope: SourceScope::parse(&crate::scope::compute(
                "8d820000-0000-7000-8000-000000000001",
            ))
            .unwrap_or_else(|error| panic!("must parse: {error}")),
            limits: Limits::defaults(),
        }
    }

    fn message() -> GraphMessage {
        serde_json::from_str(include_str!(
            "../tests/fixtures/graph/message-migration-thursday.json"
        ))
        .unwrap_or_else(|error| panic!("the fixture must deserialize: {error}"))
    }

    fn text_of(record: &fieldnotes_field_protocol::message::RecordEvent, name: &str) -> String {
        match record
            .properties
            .as_ref()
            .and_then(|properties| properties.get(name))
        {
            Some(PropertyValue::Text(text)) => text.clone(),
            other => panic!("{name} must be a text scalar, got {other:?}"),
        }
    }

    fn list_of(
        record: &fieldnotes_field_protocol::message::RecordEvent,
        name: &str,
    ) -> Vec<String> {
        match record
            .properties
            .as_ref()
            .and_then(|properties| properties.get(name))
        {
            Some(PropertyValue::TextList(list)) => list.clone(),
            other => panic!("{name} must be a text list, got {other:?}"),
        }
    }

    #[test]
    fn a_graph_utc_instant_becomes_an_explicit_numeric_offset() {
        let rendered = occurred_at_of("2026-08-22T08:00:00Z")
            .unwrap_or_else(|error| panic!("must render: {error}"));
        assert_eq!(rendered.to_string(), "2026-08-22T08:00:00+00:00");
    }

    #[test]
    fn a_graph_instant_that_already_carries_an_offset_passes_through() {
        let rendered = occurred_at_of("2026-08-22T10:00:00+02:00")
            .unwrap_or_else(|error| panic!("must render: {error}"));
        assert_eq!(rendered.to_string(), "2026-08-22T10:00:00+02:00");
    }

    #[test]
    fn an_instant_with_no_offset_at_all_is_refused_rather_than_assumed() {
        assert!(occurred_at_of("2026-08-22T08:00:00").is_err());
    }

    #[test]
    fn the_source_key_matches_the_frozen_fixture() {
        let record = build_upsert(&context(), 1, &message(), Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        assert_eq!(
            record.source.scope.as_str(),
            "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001"
        );
        assert_eq!(
            record.source.identity.as_str(),
            "mail-message/AAMkAGI2TQABAAAA"
        );
        assert_eq!(
            record.source.version.as_ref().map(|value| value.as_str()),
            Some("CQAAABYAAAC1")
        );
        assert_eq!(
            record.source.url.as_deref(),
            Some("https://outlook.office.com/mail/id/AAMkAGI2TQABAAAA")
        );
    }

    #[test]
    fn the_mapped_vocabulary_matches_the_frozen_fixture() {
        let record = build_upsert(&context(), 1, &message(), Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        assert_eq!(record.note_type.as_ref().map(|t| t.as_str()), Some("mail"));
        assert_eq!(
            record.object_kind.as_ref().map(|k| k.as_str()),
            Some("mail-message")
        );
        assert_eq!(
            record.occurred_at.as_ref().map(ToString::to_string),
            Some("2026-08-22T08:00:00+00:00".to_owned())
        );
        assert_eq!(text_of(&record, "subject"), "Migration Thursday");
        assert_eq!(text_of(&record, "from"), "alice@example.com");
        assert_eq!(list_of(&record, "to"), vec!["sam@example.net".to_owned()]);
        assert_eq!(list_of(&record, "cc"), vec!["bob@example.net".to_owned()]);
        assert_eq!(
            list_of(&record, "participants"),
            vec![
                "alice@example.com".to_owned(),
                "bob@example.net".to_owned(),
                "sam@example.net".to_owned()
            ]
        );
        assert_eq!(text_of(&record, "conversation_id"), "AAQkAGI2TQ");
        assert_eq!(text_of(&record, "thread_id"), "outlook-thread/AAQkAGI2TQ");
        assert_eq!(text_of(&record, "outlook_mail_importance"), "normal");
        assert_eq!(
            text_of(&record, "outlook_mail_internet_message_id"),
            "<migration-thursday@example.com>"
        );
    }

    #[test]
    fn every_participant_becomes_a_normalized_channel_anchor() {
        let record = build_upsert(&context(), 1, &message(), Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        let anchors = record
            .identity_anchors
            .unwrap_or_else(|| panic!("anchors must be present"));
        assert_eq!(anchors.len(), 3);
        for anchor in &anchors {
            assert_eq!(anchor.namespace.as_str(), "email");
            assert_eq!(
                anchor.scope_class,
                fieldnotes_field_protocol::message::IdentityScopeClass::NormalizedChannel
            );
            assert_eq!(
                anchor.normalization_rule.as_ref().map(|rule| rule.as_str()),
                Some("mail_address_lowercase")
            );
            assert_eq!(anchor.normalization_version, Some(1));
        }
    }

    #[test]
    fn a_mapped_record_satisfies_its_own_schema() {
        let record = build_upsert(&context(), 1, &message(), Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        if let Err(error) = record.validate() {
            panic!("the record must validate: {error:?}");
        }
    }

    #[test]
    fn a_record_never_carries_a_core_owned_name() {
        let record = build_upsert(&context(), 1, &message(), Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        let properties = record
            .properties
            .unwrap_or_else(|| panic!("properties must be present"));
        for (name, _) in properties.iter() {
            assert!(
                !fieldnotes_field_protocol::value::is_core_owned_property(name),
                "{name} is core-owned and must never be emitted by a Field"
            );
        }
    }

    #[test]
    fn every_prefixed_name_emitted_is_declared_by_the_manifest() {
        let record = build_upsert(&context(), 1, &message(), Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        let manifest = crate::manifest::build(
            RunId::parse("1a4c9f2e-0000-4000-8000-000000000001")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
        );
        let properties = record
            .properties
            .unwrap_or_else(|| panic!("properties must be present"));
        for (name, _) in properties.iter() {
            if name.starts_with("outlook_mail_") {
                assert!(
                    manifest
                        .declared_properties
                        .iter()
                        .any(|declared| declared.name.as_str() == name),
                    "{name} is emitted but not declared"
                );
            }
        }
    }

    #[test]
    fn a_message_with_no_identifier_is_a_per_message_skip() {
        let mut message = message();
        message.id = None;
        assert!(build_upsert(&context(), 1, &message, Vec::new(), &[]).is_err());
    }

    #[test]
    fn a_message_with_no_instant_at_all_is_a_per_message_skip() {
        let mut message = message();
        message.received_date_time = None;
        message.sent_date_time = None;
        assert!(build_upsert(&context(), 1, &message, Vec::new(), &[]).is_err());
    }

    #[test]
    fn a_tombstone_carries_the_source_key_the_instant_and_nothing_else() {
        let record = build_tombstone(&context(), 7, "AAMkAGI2GONE01", 1_787_000_000_000)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(record.change, Change::Delete);
        assert_eq!(
            record.source.identity.as_str(),
            "mail-message/AAMkAGI2GONE01"
        );
        assert!(record.authority.is_some());
        assert!(record.observed_at.is_some());
        assert!(record.note_type.is_none());
        assert!(record.occurred_at.is_none());
        assert!(record.properties.is_none());
        assert!(record.body.is_none());
        assert!(record.artifacts.is_none());
        assert!(record.identity_anchors.is_none());
        assert!(record.integrity.is_none());
        assert!(record.source.version.is_none());
        if let Err(error) = record.validate() {
            panic!("the tombstone must validate: {error:?}");
        }
    }

    #[test]
    fn participants_are_deduplicated_and_stably_ordered() {
        let json = r#"{
            "id":"AAMkAGI2DUPES","receivedDateTime":"2026-08-22T08:00:00Z",
            "from":{"emailAddress":{"address":"Sam@example.net"}},
            "toRecipients":[{"emailAddress":{"address":"sam@EXAMPLE.net"}},
                            {"emailAddress":{"address":"alice@example.com"}}],
            "ccRecipients":[{"emailAddress":{"address":"alice@example.com"}}]
        }"#;
        let message: GraphMessage =
            serde_json::from_str(json).unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert_eq!(
            participants(&message),
            vec!["alice@example.com".to_owned(), "sam@example.net".to_owned()]
        );
    }

    #[test]
    fn a_source_url_can_never_be_widened_by_an_identifier() {
        let url = source_url_of("../../../etc/passwd");
        assert_eq!(
            url,
            "https://outlook.office.com/mail/id/..%2F..%2F..%2Fetc%2Fpasswd"
        );
    }

    #[test]
    fn an_html_body_is_reduced_deterministically_under_the_body_bound() {
        let record = build_upsert(&context(), 1, &message(), Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        let body = record.body.unwrap_or_else(|| panic!("a body is required"));
        assert!(body.text.starts_with("# Migration Thursday\n\nHi Sam,"));
        assert!(
            !body.text.contains('<'),
            "no markup may survive into the body: {}",
            body.text
        );
    }

    #[test]
    fn a_body_over_the_run_bound_is_truncated_and_says_so() {
        let mut message = message();
        message.body = Some(crate::api::ItemBody {
            content_type: Some("text".to_owned()),
            content: Some("x".repeat(4096)),
        });
        let context = RecordContext {
            limits: Limits {
                max_body_bytes: 1024,
                ..Limits::defaults()
            },
            ..context()
        };
        let record = build_upsert(&context, 1, &message, Vec::new(), &[])
            .unwrap_or_else(|error| panic!("must map: {error}"));
        let integrity = record
            .integrity
            .unwrap_or_else(|| panic!("integrity must be present"));
        assert!(integrity.truncated);
        assert!(integrity.lost_characters.unwrap_or_default() > 0);
        assert!(!integrity.damaged);
    }
}
