//! Mapping one Graph contact onto a normalized source envelope.
//!
//! Every value this module supplies is post-mapping and pre-serialization,
//! exactly as A2 section 6 requires. Nothing here computes a record ID, a
//! capture time, a content hash, a canonical key order, or an artifact
//! path -- the record and artifact types this module builds structurally
//! exclude all of them.
//!
//! # What `occurred_at` means for a contact
//!
//! A contact is not an event: nothing "happens" at a stated moment the way a
//! message is sent or a meeting starts. This Field uses the contact's own
//! `lastModifiedDateTime` -- the instant its current stated facts became
//! current -- falling back to `createdDateTime` when Graph did not return a
//! modification instant at all. This is the same choice
//! `fields/fieldnotes-field-local` makes for a file, which is not an event
//! either: a file's `occurred_at` is its own last-modified instant, because
//! that is the one instant a record asserting "this is the object's current
//! state" can honestly attach to that state, and because it advances every
//! time the state a Field re-collects has actually changed, unlike
//! `createdDateTime`, which never moves again once set.

use fieldnotes_field_protocol::grammar::{
    AttachmentRef, MarkdownTag, MediaType, MediaTypeMatcher, NoteTypeToken, ObjectKind,
    OffsetDatetime, ProtocolV1, RecordTag, RunId, Sha256Hex, SourceScope, TombstoneTag,
};
use fieldnotes_field_protocol::limits::{Limits, artifact_media_type_included};
use fieldnotes_field_protocol::message::{
    ArtifactKind, ArtifactRef, ArtifactRole, Body, Change, IdentityAnchor, Integrity, RecordEvent,
    SourceRef,
};
use fieldnotes_field_protocol::value::{PropertyValue, RecordProperties};

use crate::graph::GraphContact;
use crate::photo::PhotoTransport;
use crate::{identity, scope};

/// Why one contact could not be turned into a record.
///
/// Every reason here is treated as a per-contact skip, not a run failure: the
/// caller reports it as a diagnostic and continues with the rest of the
/// page, since one malformed contact must not cost the whole run.
#[derive(Debug)]
pub(crate) struct RecordError(pub(crate) String);

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordError {}

/// What building one record needs beyond the contact itself.
pub(crate) struct RecordContext<'a> {
    /// Core's identifier for this run.
    pub(crate) run_id: RunId,
    /// The portable exact-source scope every record in this run shares.
    pub(crate) source_scope: SourceScope,
    /// The per-run staging directory core created and named.
    pub(crate) staging_dir: &'a std::path::Path,
    /// The effective bounds for this run.
    pub(crate) limits: Limits,
    /// The effective media-type retention include set.
    pub(crate) media_policy: &'a [MediaTypeMatcher],
    /// Where to fetch a contact's photo bytes from (production: a real
    /// `GET`; test: an in-memory double). See [`crate::photo`].
    pub(crate) photo_transport: &'a dyn PhotoTransport,
    /// The bearer token to authenticate the photo fetch with. Never logged,
    /// never placed in a property, body, or cursor.
    pub(crate) bearer_token: &'a str,
    /// The Graph service root, for building the photo URL.
    pub(crate) graph_base_url: &'a str,
    /// The signed-in user's Graph resource segment, `/me`, for building the
    /// photo URL. Always `/me` in this release; see
    /// [`crate::config::ResolvedConfig::mailbox_resource`].
    pub(crate) mailbox_resource: &'a str,
}

/// The outcome of building one upsert record: the record itself, plus any
/// non-fatal issue encountered along the way (for example, a photo that
/// could not be fetched), which the caller reports as a diagnostic rather
/// than failing the record over.
pub(crate) struct BuildOutcome {
    pub(crate) record: RecordEvent,
    pub(crate) warnings: Vec<String>,
}

fn occurred_at_from(contact: &GraphContact) -> Result<OffsetDatetime, RecordError> {
    let stated = contact
        .last_modified_date_time
        .as_deref()
        .or(contact.created_date_time.as_deref())
        .ok_or_else(|| {
            RecordError(
                "the contact carries neither lastModifiedDateTime nor createdDateTime, so no \
                 event instant can be mapped"
                    .to_owned(),
            )
        })?;
    let explicit_offset = normalize_offset(stated);
    OffsetDatetime::parse(&explicit_offset).map_err(|error| {
        RecordError(format!(
            "the contact's timestamp {stated:?} could not be mapped: {error}"
        ))
    })
}

/// Graph renders every instant with a trailing `Z` (UTC). A1's `occurred_at`
/// requires an explicit numeric offset, so `Z`/`z` is rewritten to `+00:00`
/// rather than parsed as a distinct case: the rest of the RFC 3339 grammar is
/// unchanged, so no date arithmetic is needed to make the value acceptable.
fn normalize_offset(stated: &str) -> String {
    if let Some(prefix) = stated
        .strip_suffix('Z')
        .or_else(|| stated.strip_suffix('z'))
    {
        format!("{prefix}+00:00")
    } else {
        stated.to_owned()
    }
}

fn title_of(contact: &GraphContact) -> String {
    if let Some(display_name) = contact
        .display_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        return display_name.to_owned();
    }
    let combined = [contact.given_name.as_deref(), contact.surname.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if !combined.trim().is_empty() {
        return combined;
    }
    contact
        .company_name
        .clone()
        .unwrap_or_else(|| "Unnamed contact".to_owned())
}

/// This Field's own source-observable heuristic distinguishing a person from
/// an organization contact: a contact with a stated employer and no stated
/// personal name is an organization, everything else is a person.
///
/// This is deliberately a **prefixed** property
/// ([`crate::constants::PROPERTY_CONTACT_KIND`]), matching the frozen
/// `outlook_contacts_work` fixture, and not a shared A1 property: the graph
/// layer does not read vendor-prefixed properties, and no registered shared
/// property yet distinguishes a person from an organization. See this
/// crate's final report.
fn contact_kind_of(contact: &GraphContact) -> &'static str {
    let has_personal_name = contact
        .given_name
        .as_deref()
        .is_some_and(|name| !name.trim().is_empty())
        || contact
            .surname
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty());
    let trimmed_company = contact
        .company_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let Some(company) = trimmed_company else {
        return "person";
    };
    if has_personal_name {
        return "person";
    }
    // No stated personal name at all. This is an organization only when the
    // display name states nothing beyond the company itself -- absent, or
    // the same text -- rather than whenever a company happens to be filled
    // in alongside an unrelated display name.
    let display_states_only_the_company = contact
        .display_name
        .as_deref()
        .map(str::trim)
        .is_none_or(|display_name| display_name.eq_ignore_ascii_case(company));
    if display_states_only_the_company {
        "organization"
    } else {
        "person"
    }
}

fn identity_anchors_of(contact: &GraphContact) -> Vec<IdentityAnchor> {
    let mut anchors = Vec::new();
    for email in &contact.email_addresses {
        if let Some(address) = email.address.as_deref()
            && let Some(anchor) = identity::email_anchor(address)
        {
            anchors.push(anchor);
        }
    }
    for phone in contact.phone_numbers() {
        if let Some(anchor) = identity::phone_anchor(phone) {
            anchors.push(anchor);
        }
    }
    anchors
}

fn body_of(contact: &GraphContact, title: &str) -> String {
    let mut lines = vec![format!("# {title}"), String::new()];
    for email in &contact.email_addresses {
        if let Some(address) = email.address.as_deref().filter(|a| !a.trim().is_empty()) {
            lines.push(format!("- Email: {address}"));
        }
    }
    for phone in contact.phone_numbers() {
        lines.push(format!("- Phone: {phone}"));
    }
    if let Some(company) = contact
        .company_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        lines.push(format!("- Organization: {company}"));
    }
    if let Some(title) = contact
        .job_title
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        lines.push(format!("- Role: {title}"));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn insert(
    properties: &mut RecordProperties,
    name: &str,
    value: PropertyValue,
) -> Result<(), RecordError> {
    properties
        .insert(name, value)
        .map_err(|reason| RecordError(format!("property {name}: {reason}")))
}

fn photo_url(context: &RecordContext<'_>, graph_contact_id: &str) -> String {
    format!(
        "{}{}/contacts/{graph_contact_id}/photo/$value",
        context.graph_base_url.trim_end_matches('/'),
        context.mailbox_resource
    )
}

fn attachment_ref_of(graph_contact_id: &str) -> Result<AttachmentRef, RecordError> {
    AttachmentRef::parse(&format!("contact/{graph_contact_id}/photo"))
        .map_err(|error| RecordError(format!("attachment reference guard: {error}")))
}

/// Fetches, and evaluates the run's retention policy against, one contact's
/// photo.
///
/// Returns `Ok(None)` when the contact has no photo at all. A photo that
/// cannot be fetched at all (a transport failure) is also `Ok(None)` --
/// paired with a warning the caller reports as a diagnostic -- because one
/// contact's photo endpoint misbehaving must not fail the whole record.
/// [`fieldnotes_msgraph`]'s `GraphClient` cannot make this call at all: see
/// [`crate::photo`] for why.
fn build_photo_artifact(
    context: &RecordContext<'_>,
    seq: u64,
    graph_contact_id: &str,
) -> (Option<ArtifactRef>, Vec<String>) {
    let url = photo_url(context, graph_contact_id);
    let fetched = match context.photo_transport.fetch(&url, context.bearer_token) {
        Ok(Some(photo)) => photo,
        Ok(None) => return (None, Vec::new()),
        Err(error) => {
            return (
                None,
                vec![format!(
                    "the photo for contact {graph_contact_id} could not be fetched: {error}"
                )],
            );
        }
    };

    let byte_length = u64::try_from(fetched.bytes.len()).unwrap_or(u64::MAX);
    let essence = fetched
        .media_type
        .as_deref()
        .map(fieldnotes_field_protocol::limits::media_type_essence);
    let policy_excluded = essence
        .as_deref()
        .is_some_and(|essence| !artifact_media_type_included(context.media_policy, essence));
    let oversized = byte_length > context.limits.max_artifact_bytes;

    let media_type = essence
        .as_deref()
        .and_then(|essence| MediaType::parse(essence).ok());

    if policy_excluded || oversized {
        let attachment_ref = match attachment_ref_of(graph_contact_id) {
            Ok(attachment_ref) => attachment_ref,
            Err(error) => return (None, vec![error.to_string()]),
        };
        return (
            Some(ArtifactRef {
                kind: ArtifactKind::NotRetained,
                handle: None,
                sha256: None,
                byte_length: Some(byte_length),
                media_type,
                role: ArtifactRole::Original,
                source_filename: None,
                attachment_ref: Some(attachment_ref),
            }),
            Vec::new(),
        );
    }

    let handle = fieldnotes_field_sdk::stage::handle_for_seq(seq);
    match fieldnotes_field_sdk::stage::stage_and_hash(context.staging_dir, &handle, &fetched.bytes)
    {
        Ok(digest) => {
            let sha256 = Sha256Hex::parse(&digest).ok();
            (
                Some(ArtifactRef {
                    kind: ArtifactKind::Staged,
                    handle: Some(handle),
                    sha256,
                    byte_length: Some(byte_length),
                    media_type,
                    role: ArtifactRole::Original,
                    source_filename: None,
                    attachment_ref: None,
                }),
                Vec::new(),
            )
        }
        Err(error) => (
            None,
            vec![format!(
                "the photo for contact {graph_contact_id} could not be staged: {error}"
            )],
        ),
    }
}

/// Builds one upsert record from a currently-stated Graph contact.
pub(crate) fn build_upsert(
    context: &RecordContext<'_>,
    seq: u64,
    contact: &GraphContact,
) -> Result<BuildOutcome, RecordError> {
    let identity = scope::identity_of(&contact.id)
        .map_err(|error| RecordError(format!("source identity: {error}")))?;
    let occurred_at = occurred_at_from(contact)?;
    let title = title_of(contact);

    let mut properties = RecordProperties::new();
    insert(&mut properties, "title", PropertyValue::Text(title.clone()))?;
    if let Some(company) = contact
        .company_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        insert(
            &mut properties,
            crate::constants::PROPERTY_COMPANY_NAME,
            PropertyValue::Text(company.to_owned()),
        )?;
    }
    if let Some(job_title) = contact
        .job_title
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        insert(
            &mut properties,
            crate::constants::PROPERTY_JOB_TITLE,
            PropertyValue::Text(job_title.to_owned()),
        )?;
    }
    insert(
        &mut properties,
        crate::constants::PROPERTY_CONTACT_KIND,
        PropertyValue::Text(contact_kind_of(contact).to_owned()),
    )?;

    let anchors = identity_anchors_of(contact);
    let (artifact, warnings) = build_photo_artifact(context, seq, &contact.id);

    let max_body_bytes = context.limits.max_body_bytes;
    let (body_text, lost_characters) =
        fieldnotes_field_sdk::truncate::truncate_utf8(&body_of(contact, &title), max_body_bytes);
    let integrity = Integrity {
        damaged: false,
        truncated: lost_characters > 0,
        lost_characters: (lost_characters > 0).then_some(lost_characters),
    };

    let record = RecordEvent {
        v: ProtocolV1,
        frame_type: RecordTag,
        run_id: context.run_id.clone(),
        seq,
        change: Change::Upsert,
        source: SourceRef {
            scope: context.source_scope.clone(),
            identity,
            version: contact
                .change_key
                .as_deref()
                .map(parse_source_version)
                .transpose()?,
            url: None,
            parent_identity: None,
        },
        object_kind: Some(parse_object_kind(crate::constants::OBJECT_KIND_CONTACT)?),
        note_type: Some(parse_note_type(crate::constants::OBJECT_KIND_CONTACT)?),
        occurred_at: Some(occurred_at),
        properties: Some(properties),
        body: Some(Body {
            format: MarkdownTag,
            text: body_text,
        }),
        artifacts: artifact.map(|artifact| vec![artifact]),
        identity_anchors: (!anchors.is_empty()).then_some(anchors),
        integrity: Some(integrity),
        authority: None,
        observed_at: None,
    };
    Ok(BuildOutcome { record, warnings })
}

/// Builds one authoritative tombstone record from a delta-feed removal
/// marker.
///
/// `observed_at` is supplied by the caller from an injected clock: a
/// deletion has no source-side timestamp (the object is gone), and library
/// code here never reads the wall clock itself (A2's own constraint, applied
/// to this Field the way it already applies to every Field).
pub(crate) fn build_delete(
    context: &RecordContext<'_>,
    seq: u64,
    contact: &GraphContact,
    observed_at: OffsetDatetime,
) -> Result<RecordEvent, RecordError> {
    let identity = scope::identity_of(&contact.id)
        .map_err(|error| RecordError(format!("source identity: {error}")))?;
    Ok(RecordEvent {
        v: ProtocolV1,
        frame_type: RecordTag,
        run_id: context.run_id.clone(),
        seq,
        change: Change::Delete,
        source: SourceRef {
            scope: context.source_scope.clone(),
            identity,
            version: None,
            url: None,
            parent_identity: None,
        },
        object_kind: Some(parse_object_kind(crate::constants::OBJECT_KIND_CONTACT)?),
        note_type: None,
        occurred_at: None,
        properties: None,
        body: None,
        artifacts: None,
        identity_anchors: None,
        integrity: None,
        authority: Some(TombstoneTag),
        observed_at: Some(observed_at),
    })
}

fn parse_object_kind(text: &str) -> Result<ObjectKind, RecordError> {
    ObjectKind::parse(text).map_err(|error| RecordError(format!("object kind guard: {error}")))
}

fn parse_note_type(text: &str) -> Result<NoteTypeToken, RecordError> {
    NoteTypeToken::parse(text).map_err(|error| RecordError(format!("Note type guard: {error}")))
}

fn parse_source_version(
    text: &str,
) -> Result<fieldnotes_field_protocol::grammar::SourceVersion, RecordError> {
    fieldnotes_field_protocol::grammar::SourceVersion::parse(text)
        .map_err(|error| RecordError(format!("source version guard: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo::testing::{Scripted, ScriptedPhotoTransport};
    use fieldnotes_field_protocol::grammar::SourceScope;
    use fieldnotes_test_support::TempDir;

    fn context<'a>(
        staging: &'a std::path::Path,
        transport: &'a dyn PhotoTransport,
        media_policy: &'a [MediaTypeMatcher],
    ) -> RecordContext<'a> {
        RecordContext {
            run_id: RunId::parse("1a4c9f2e-0000-4000-8000-000000000001")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            source_scope: SourceScope::parse(
                "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001",
            )
            .unwrap_or_else(|error| panic!("must parse: {error}")),
            staging_dir: staging,
            limits: Limits::ceilings(),
            media_policy,
            photo_transport: transport,
            bearer_token: "FIXTURE-NOT-A-REAL-TOKEN-canary",
            graph_base_url: "https://graph.microsoft.com/v1.0",
            mailbox_resource: "/me",
        }
    }

    fn full_contact() -> GraphContact {
        serde_json::from_str(
            r#"{
                "id": "AAMkAGI2CONTACT01",
                "displayName": "Alice Müller",
                "companyName": "Example AG",
                "jobTitle": "Head of Operations",
                "emailAddresses": [{"address": "alice@example.com"}],
                "businessPhones": ["+41 44 123 45 67"],
                "lastModifiedDateTime": "2026-08-22T08:15:00Z",
                "changeKey": "contact-version-3"
            }"#,
        )
        .unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    #[test]
    fn occurred_at_prefers_last_modified_and_maps_the_z_offset() {
        let contact = full_contact();
        let occurred_at =
            occurred_at_from(&contact).unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(occurred_at.to_string(), "2026-08-22T08:15:00+00:00");
    }

    #[test]
    fn occurred_at_falls_back_to_created_when_last_modified_is_absent() {
        let contact: GraphContact =
            serde_json::from_str(r#"{"id":"x","createdDateTime":"2026-01-01T00:00:00Z"}"#)
                .unwrap_or_else(|error| panic!("must parse: {error}"));
        let occurred_at =
            occurred_at_from(&contact).unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(occurred_at.to_string(), "2026-01-01T00:00:00+00:00");
    }

    #[test]
    fn a_contact_with_neither_timestamp_is_reported_not_panicked() {
        let contact: GraphContact = serde_json::from_str(r#"{"id":"x"}"#)
            .unwrap_or_else(|error| panic!("must parse: {error}"));
        assert!(occurred_at_from(&contact).is_err());
    }

    #[test]
    fn a_company_only_contact_is_classified_an_organization() {
        let contact: GraphContact = serde_json::from_str(
            r#"{"id":"x","companyName":"Acme Corp","displayName":"Acme Corp"}"#,
        )
        .unwrap_or_else(|error| panic!("must parse: {error}"));
        assert_eq!(contact_kind_of(&contact), "organization");
    }

    #[test]
    fn a_named_contact_is_classified_a_person_even_with_a_company() {
        let contact = full_contact();
        assert_eq!(contact_kind_of(&contact), "person");
    }

    #[test]
    fn several_emails_and_phones_all_become_anchors() {
        let contact: GraphContact = serde_json::from_str(
            r#"{
                "id": "x",
                "emailAddresses": [{"address": "alice@example.com"}, {"address": "a.other@example.com"}],
                "businessPhones": ["+41 44 123 45 67"],
                "homePhones": ["+41 44 999 00 00"],
                "mobilePhone": "+41 79 111 22 33"
            }"#,
        )
        .unwrap_or_else(|error| panic!("must parse: {error}"));
        let anchors = identity_anchors_of(&contact);
        assert_eq!(anchors.len(), 5);
        assert!(
            anchors
                .iter()
                .any(|a| a.namespace.as_str() == "email" && a.value == "alice@example.com")
        );
        assert!(
            anchors
                .iter()
                .any(|a| a.namespace.as_str() == "phone" && a.value == "+41441234567")
        );
    }

    #[test]
    fn a_contact_with_no_anchors_at_all_maps_cleanly() -> Result<(), Box<dyn std::error::Error>> {
        let staging = TempDir::new("record-no-anchors")?;
        let transport = ScriptedPhotoTransport::new(vec![Scripted::None]);
        let policy = fieldnotes_field_protocol::limits::default_artifact_media_types();
        let contact: GraphContact = serde_json::from_str(
            r#"{"id":"x","displayName":"No Anchors","lastModifiedDateTime":"2026-08-22T08:15:00Z"}"#,
        )?;
        let outcome = build_upsert(&context(staging.path(), &transport, &policy), 1, &contact)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        assert!(outcome.record.identity_anchors.is_none());
        assert!(outcome.record.artifacts.is_none());
        Ok(())
    }

    #[test]
    fn a_retained_photo_is_staged_under_the_seq_handle() -> Result<(), Box<dyn std::error::Error>> {
        let staging = TempDir::new("record-photo-staged")?;
        let transport = ScriptedPhotoTransport::new(vec![Scripted::Photo {
            bytes: b"\xff\xd8\xff-fake-jpeg-bytes".to_vec(),
            media_type: Some("image/jpeg".to_owned()),
        }]);
        let policy = fieldnotes_field_protocol::limits::default_artifact_media_types();
        let contact = full_contact();
        let outcome = build_upsert(&context(staging.path(), &transport, &policy), 7, &contact)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        let artifacts = outcome.record.artifacts.unwrap_or_default();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].kind, ArtifactKind::Staged);
        assert_eq!(artifacts[0].handle.as_deref(), Some("a0000007"));
        assert!(staging.path().join("a0000007").exists());
        assert!(outcome.warnings.is_empty());
        assert_eq!(
            transport.requested_urls(),
            vec![
                "https://graph.microsoft.com/v1.0/me/contacts/AAMkAGI2CONTACT01/photo/$value"
                    .to_owned()
            ]
        );
        Ok(())
    }

    #[test]
    fn an_oversized_photo_is_declined_as_not_retained() -> Result<(), Box<dyn std::error::Error>> {
        let staging = TempDir::new("record-photo-oversized")?;
        let transport = ScriptedPhotoTransport::new(vec![Scripted::Photo {
            bytes: vec![0_u8; 1024],
            media_type: Some("image/jpeg".to_owned()),
        }]);
        let policy = fieldnotes_field_protocol::limits::default_artifact_media_types();
        let contact = full_contact();
        let mut context = context(staging.path(), &transport, &policy);
        context.limits.max_artifact_bytes = 10;
        let outcome = build_upsert(&context, 3, &contact)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        let artifacts = outcome.record.artifacts.unwrap_or_default();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].kind, ArtifactKind::NotRetained);
        assert!(artifacts[0].attachment_ref.is_some());
        assert_eq!(
            artifacts[0].attachment_ref.as_ref().map(|r| r.as_str()),
            Some("contact/AAMkAGI2CONTACT01/photo")
        );
        assert!(
            std::fs::read_dir(staging.path())?.next().is_none(),
            "a declined photo must never be staged"
        );
        Ok(())
    }

    #[test]
    fn a_photo_fetch_failure_is_a_warning_not_a_record_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let staging = TempDir::new("record-photo-error")?;
        let transport = ScriptedPhotoTransport::new(vec![Scripted::Err("timed out".to_owned())]);
        let policy = fieldnotes_field_protocol::limits::default_artifact_media_types();
        let contact = full_contact();
        let outcome = build_upsert(&context(staging.path(), &transport, &policy), 1, &contact)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        assert!(outcome.record.artifacts.is_none());
        assert_eq!(outcome.warnings.len(), 1);
        Ok(())
    }

    #[test]
    fn a_delete_record_carries_no_content_at_all() {
        let staging_holder =
            TempDir::new("record-delete").unwrap_or_else(|error| panic!("temp dir: {error}"));
        let transport = ScriptedPhotoTransport::new(vec![]);
        let policy = fieldnotes_field_protocol::limits::default_artifact_media_types();
        let contact: GraphContact =
            serde_json::from_str(r#"{"id":"AAMkAGI2CONTACT01","@removed":{"reason":"deleted"}}"#)
                .unwrap_or_else(|error| panic!("must parse: {error}"));
        let observed_at = OffsetDatetime::parse("2026-08-22T10:20:00+00:00")
            .unwrap_or_else(|error| panic!("must parse: {error}"));
        let record = build_delete(
            &context(staging_holder.path(), &transport, &policy),
            1,
            &contact,
            observed_at,
        )
        .unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(record.change, Change::Delete);
        assert!(record.note_type.is_none());
        assert!(record.occurred_at.is_none());
        assert!(record.properties.is_none());
        assert!(record.body.is_none());
        assert!(record.artifacts.is_none());
        assert!(record.identity_anchors.is_none());
        assert!(record.integrity.is_none());
        assert_eq!(record.source.identity.as_str(), "contact/AAMkAGI2CONTACT01");
    }
}
