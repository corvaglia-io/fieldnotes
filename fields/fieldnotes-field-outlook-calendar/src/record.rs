//! Mapping one Graph calendar event onto a normalized source envelope.
//!
//! Every value this module supplies is post-mapping and pre-serialization,
//! exactly as A2 section 6 requires: it maps Graph's `event` resource onto
//! Fieldnotes vocabulary and does none of the work only core may do. Nothing
//! here computes a record ID, a capture time, a content hash, a canonical
//! key order, a filename, or an artifact path.
//!
//! # Recurrence
//!
//! This Field collects exclusively through Graph's `calendarView`, which
//! expands a recurring series into its instances server-side (see
//! [`crate::graph`]). Every item this module ever maps is therefore already
//! a `singleInstance`, `occurrence`, or `exception` with its own immutable
//! Graph `id`, distinct from its series master's `seriesMasterId`. The
//! portable source identity this module builds (`calendar-event/<id>`) uses
//! that per-instance `id`, never the series master's, so two occurrences of
//! the same series never collide and a series master is never collected as
//! a second, competing object for the same series -- satisfying gate R4's
//! "a recurring series must not duplicate instances" structurally rather
//! than by a runtime check. An occurrence's or exception's `seriesMasterId`
//! is carried instead as `source.parent_identity`, under a distinct
//! `calendar-event-series` namespace, purely as traceability evidence.
//!
//! # Intervals, all-day events, and offsets
//!
//! An event's start and end are mapped as a genuine interval
//! (`started_at`/`ended_at`, both registered shared properties) rather than
//! collapsed into one instant: `occurred_at` -- the envelope's own required
//! instant -- is set to the start. Graph reports every instant in UTC by
//! default (this Field never sends a `Prefer: outlook.timezone` header), so
//! every mapped instant carries the explicit `+00:00` offset A1 requires. An
//! all-day event's start and end are still full UTC-midnight-bounded
//! instants from Graph, not date-only values, so the same mapping handles
//! both cases uniformly; [`crate::constants`]'s `outlook_calendar_all_day`
//! property is what lets a reader distinguish the two rather than the
//! interval shape itself.

use fieldnotes_field_protocol::grammar::{
    IdentityNamespace, MarkdownTag, NoteTypeToken, ObjectKind, OffsetDatetime, ProtocolV1,
    RecordTag, RuleName, RunId, SourceIdentity, SourceScope, TombstoneTag,
};
use fieldnotes_field_protocol::message::{
    AnchorRole, Body, Change, IdentityAnchor, IdentityScopeClass, RecordEvent, SourceRef,
};
use fieldnotes_field_protocol::value::{PropertyValue, RecordProperties};

use crate::graph::GraphEvent;

/// Why one Graph event could not be turned into a record.
#[derive(Debug)]
pub(crate) struct RecordError(pub(crate) String);

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordError {}

/// What every record built in one run shares.
pub(crate) struct RecordContext {
    /// Core's identifier for this run.
    pub(crate) run_id: RunId,
    /// The portable exact-source scope every record in this run shares.
    pub(crate) source_scope: SourceScope,
}

fn identity_namespace(text: &str) -> IdentityNamespace {
    IdentityNamespace::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be an identity namespace: {error}"))
}

fn rule(text: &str) -> RuleName {
    RuleName::parse(text).unwrap_or_else(|error| panic!("{text:?} must be a rule name: {error}"))
}

fn identity_of(event_id: &str) -> Result<SourceIdentity, RecordError> {
    SourceIdentity::parse(&format!(
        "{}/{event_id}",
        crate::constants::IDENTITY_NAMESPACE
    ))
    .map_err(|error| RecordError(format!("source identity guard: {error}")))
}

fn series_identity_of(series_master_id: &str) -> Result<SourceIdentity, RecordError> {
    SourceIdentity::parse(&format!(
        "{}/{series_master_id}",
        crate::constants::SERIES_IDENTITY_NAMESPACE
    ))
    .map_err(|error| RecordError(format!("series parent identity guard: {error}")))
}

fn parse_object_kind(text: &str) -> Result<ObjectKind, RecordError> {
    ObjectKind::parse(text).map_err(|error| RecordError(format!("object kind guard: {error}")))
}

fn parse_note_type(text: &str) -> Result<NoteTypeToken, RecordError> {
    NoteTypeToken::parse(text).map_err(|error| RecordError(format!("Note type guard: {error}")))
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

/// Parses one of Graph's UTC event instants into an explicit-offset
/// [`OffsetDatetime`].
///
/// Refuses anything other than `"UTC"`: this Field never requests a
/// `Prefer: outlook.timezone` header, so a differently named zone would mean
/// either an upstream change this Field has not reviewed or a malformed
/// fixture, and guessing an offset for a named zone this Field has no
/// timezone database for would silently mismap the instant.
fn parse_utc_datetime(
    raw: &crate::graph::GraphDateTimeTimeZone,
) -> Result<OffsetDatetime, RecordError> {
    if !raw.time_zone.eq_ignore_ascii_case("utc") {
        return Err(RecordError(format!(
            "event instant {:?} carries timeZone {:?}; this Field only supports UTC, which is \
             what Graph returns by default",
            raw.date_time, raw.time_zone
        )));
    }
    let trimmed = raw.date_time.trim_end_matches(['Z', 'z']);
    OffsetDatetime::parse(&format!("{trimmed}+00:00")).map_err(|error| {
        RecordError(format!(
            "event instant {:?} failed its own guard: {error}",
            raw.date_time
        ))
    })
}

/// Whole seconds between two instants, matching the registered
/// `duration_seconds` shared property.
fn duration_seconds(start: OffsetDatetime, end: OffsetDatetime) -> i64 {
    let (start_seconds, _) = start.datetime().instant();
    let (end_seconds, _) = end.datetime().instant();
    end_seconds - start_seconds
}

/// Renders Graph's `event.type` as this Field's snake_cased
/// `outlook_calendar_event_kind`. Unrecognized values pass through verbatim
/// rather than being refused, so an upstream addition to Graph's own
/// vocabulary degrades to an unfamiliar-but-honest string instead of failing
/// the record.
fn event_kind_of(event_type: Option<&str>) -> String {
    match event_type {
        Some("singleInstance") => "single_instance".to_owned(),
        Some("occurrence") => "occurrence".to_owned(),
        Some("exception") => "exception".to_owned(),
        Some("seriesMaster") => "series_master".to_owned(),
        Some(other) => other.to_owned(),
        None => "single_instance".to_owned(),
    }
}

/// Trims and lowercases a mail address for both the `email_v1` identity
/// anchor and for deduplicating the ordered `participants` list. Returns
/// `None` for an address that is empty once trimmed.
fn normalize_email(address: &str) -> Option<String> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn email_anchor(address: &str, role: AnchorRole) -> Option<IdentityAnchor> {
    let normalized = normalize_email(address)?;
    Some(IdentityAnchor {
        namespace: identity_namespace("email"),
        value: normalized,
        scope_class: IdentityScopeClass::NormalizedChannel,
        scope: None,
        normalization_rule: Some(rule("email_v1")),
        normalization_version: Some(1),
        role: Some(role),
    })
}

fn address_of(recipient: Option<&crate::graph::GraphRecipient>) -> Option<&str> {
    recipient?
        .email_address
        .as_ref()?
        .address
        .as_deref()
        .map(str::trim)
        .filter(|address| !address.is_empty())
}

/// The organizer and participant addresses genuinely present on `event`,
/// alongside the identity anchors they back.
///
/// `participants` lists the organizer first (when present), then each
/// attendee address in Graph's own order, skipping an address already
/// listed under a different role -- most commonly an organizer who also
/// appears in their own attendee list. Every genuinely present address still
/// gets its own role-specific identity anchor even when the participants
/// list de-duplicated it, since a duplicate anchor for the same normalized
/// value is harmless (core's own `identities` projection is itself a
/// deduplicated set).
fn participants_and_anchors(
    event: &GraphEvent,
) -> (Option<String>, Vec<String>, Vec<IdentityAnchor>) {
    let organizer_address = address_of(event.organizer.as_ref());
    let mut participants = Vec::new();
    let mut anchors = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    if let Some(address) = organizer_address
        && let Some(normalized) = normalize_email(address)
    {
        if seen.insert(normalized) {
            participants.push(address.to_owned());
        }
        if let Some(anchor) = email_anchor(address, AnchorRole::Organizer) {
            anchors.push(anchor);
        }
    }
    for attendee in event.attendees.as_deref().unwrap_or(&[]) {
        let Some(address) = attendee
            .email_address
            .as_ref()
            .and_then(|email| email.address.as_deref())
            .map(str::trim)
            .filter(|address| !address.is_empty())
        else {
            continue;
        };
        if let Some(normalized) = normalize_email(address)
            && seen.insert(normalized)
        {
            participants.push(address.to_owned());
        }
        if let Some(anchor) = email_anchor(address, AnchorRole::Attendee) {
            anchors.push(anchor);
        }
    }
    (organizer_address.map(str::to_owned), participants, anchors)
}

fn body_text(
    title: &str,
    organizer: Option<&str>,
    participants: &[String],
    started_at: OffsetDatetime,
    ended_at: OffsetDatetime,
    all_day: bool,
    cancelled: bool,
) -> String {
    let mut text = format!("# {title}\n\n");
    if all_day {
        text.push_str(&format!("All day: {started_at} to {ended_at} (UTC).\n\n"));
    } else {
        text.push_str(&format!("{started_at} to {ended_at} (UTC).\n\n"));
    }
    if cancelled {
        text.push_str("This event is cancelled.\n\n");
    }
    if let Some(organizer) = organizer {
        text.push_str(&format!("Organizer: {organizer}\n"));
    }
    if !participants.is_empty() {
        text.push_str(&format!("Participants: {}\n", participants.join(", ")));
    }
    text
}

/// Builds one upsert record from a present Graph calendar event.
pub(crate) fn build_upsert(
    context: &RecordContext,
    seq: u64,
    event: &GraphEvent,
) -> Result<RecordEvent, RecordError> {
    let identity = identity_of(&event.id)?;
    let parent_identity = event
        .series_master_id
        .as_deref()
        .map(series_identity_of)
        .transpose()?;

    let start_raw = event
        .start
        .as_ref()
        .ok_or_else(|| RecordError("event carries no start instant".to_owned()))?;
    let end_raw = event
        .end
        .as_ref()
        .ok_or_else(|| RecordError("event carries no end instant".to_owned()))?;
    let started_at = parse_utc_datetime(start_raw)?;
    let ended_at = parse_utc_datetime(end_raw)?;

    let title = event
        .subject
        .as_deref()
        .map(str::trim)
        .filter(|subject| !subject.is_empty())
        .unwrap_or("Untitled event")
        .to_owned();
    let all_day = event.is_all_day.unwrap_or(false);
    let cancelled = event.is_cancelled.unwrap_or(false);
    let (organizer, participants, anchors) = participants_and_anchors(event);

    let body = body_text(
        &title,
        organizer.as_deref(),
        &participants,
        started_at,
        ended_at,
        all_day,
        cancelled,
    );

    let mut properties = RecordProperties::new();
    insert(&mut properties, "title", PropertyValue::Text(title))?;
    if let Some(organizer) = &organizer {
        insert(
            &mut properties,
            "organizer",
            PropertyValue::Text(organizer.clone()),
        )?;
    }
    if !participants.is_empty() {
        insert(
            &mut properties,
            "participants",
            PropertyValue::TextList(participants.clone()),
        )?;
    }
    insert(
        &mut properties,
        "started_at",
        PropertyValue::Text(started_at.to_string()),
    )?;
    insert(
        &mut properties,
        "ended_at",
        PropertyValue::Text(ended_at.to_string()),
    )?;
    insert(
        &mut properties,
        "duration_seconds",
        PropertyValue::Number(serde_json::Number::from(duration_seconds(
            started_at, ended_at,
        ))),
    )?;
    insert(
        &mut properties,
        "outlook_calendar_event_kind",
        PropertyValue::Text(event_kind_of(event.event_type.as_deref())),
    )?;
    if let Some(response) = event
        .response_status
        .as_ref()
        .and_then(|status| status.response.as_deref())
        .filter(|response| !response.is_empty())
    {
        insert(
            &mut properties,
            "outlook_calendar_response_status",
            PropertyValue::Text(response.to_owned()),
        )?;
    }
    insert(
        &mut properties,
        "outlook_calendar_all_day",
        PropertyValue::Boolean(all_day),
    )?;
    insert(
        &mut properties,
        "outlook_calendar_is_cancelled",
        PropertyValue::Boolean(cancelled),
    )?;

    Ok(RecordEvent {
        v: ProtocolV1,
        frame_type: RecordTag,
        run_id: context.run_id.clone(),
        seq,
        change: Change::Upsert,
        source: SourceRef {
            scope: context.source_scope.clone(),
            identity,
            version: event
                .change_key
                .as_deref()
                .map(parse_source_version)
                .transpose()?,
            url: event.web_link.clone(),
            parent_identity,
        },
        object_kind: Some(parse_object_kind(crate::constants::OBJECT_KIND_EVENT)?),
        note_type: Some(parse_note_type(crate::constants::NOTE_TYPE_EVENT)?),
        occurred_at: Some(started_at),
        properties: Some(properties),
        body: Some(Body {
            format: MarkdownTag,
            text: body,
        }),
        artifacts: None,
        identity_anchors: if anchors.is_empty() {
            None
        } else {
            Some(anchors)
        },
        integrity: Some(fieldnotes_field_protocol::message::Integrity {
            damaged: false,
            truncated: false,
            lost_characters: None,
        }),
        authority: None,
        observed_at: None,
    })
}

fn parse_source_version(
    change_key: &str,
) -> Result<fieldnotes_field_protocol::grammar::SourceVersion, RecordError> {
    fieldnotes_field_protocol::grammar::SourceVersion::parse(&format!("W/\"{change_key}\""))
        .map_err(|error| RecordError(format!("source version guard: {error}")))
}

/// Builds one authoritative tombstone record for a Graph delta `@removed`
/// item.
///
/// Carries the portable source key, the `tombstone` authority, and the
/// instant this Field observed the removal, and nothing else: A2 section 10
/// structurally forbids any content on a delete, so a deletion can never be
/// confused with an empty or partial collection result.
pub(crate) fn build_delete(
    context: &RecordContext,
    seq: u64,
    event_id: &str,
    observed_at: OffsetDatetime,
) -> Result<RecordEvent, RecordError> {
    let identity = identity_of(event_id)?;
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
        object_kind: Some(parse_object_kind(crate::constants::OBJECT_KIND_EVENT)?),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphDateTimeTimeZone, GraphEvent};

    fn utc(text: &str) -> GraphDateTimeTimeZone {
        GraphDateTimeTimeZone {
            date_time: text.to_owned(),
            time_zone: "UTC".to_owned(),
        }
    }

    fn minimal_event(id: &str) -> GraphEvent {
        GraphEvent {
            id: id.to_owned(),
            removed: None,
            subject: Some("Migration planning".to_owned()),
            start: Some(utc("2026-08-22T09:00:00.0000000")),
            end: Some(utc("2026-08-22T09:45:00.0000000")),
            is_all_day: Some(false),
            is_cancelled: Some(false),
            organizer: None,
            attendees: None,
            event_type: Some("singleInstance".to_owned()),
            series_master_id: None,
            web_link: None,
            change_key: None,
            response_status: None,
        }
    }

    fn context() -> RecordContext {
        RecordContext {
            run_id: fieldnotes_field_protocol::grammar::RunId::parse(
                "1a4c9f2e-0000-4000-8000-000000000002",
            )
            .unwrap_or_else(|error| panic!("must parse: {error}")),
            source_scope: SourceScope::parse("microsoft-graph:tenant/8d820000")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
        }
    }

    #[test]
    fn a_utc_instant_maps_to_an_explicit_zero_offset() {
        let raw = utc("2026-08-22T09:00:00.0000000");
        let parsed = parse_utc_datetime(&raw).unwrap_or_else(|error| panic!("must parse: {error}"));
        assert_eq!(parsed.to_string(), "2026-08-22T09:00:00+00:00");
    }

    #[test]
    fn a_non_utc_timezone_is_refused_rather_than_guessed() {
        let raw = GraphDateTimeTimeZone {
            date_time: "2026-08-22T09:00:00.0000000".to_owned(),
            time_zone: "Pacific Standard Time".to_owned(),
        };
        assert!(parse_utc_datetime(&raw).is_err());
    }

    #[test]
    fn an_upsert_maps_the_interval_organizer_and_participants() {
        let mut event = minimal_event("AAMkAGI2EVENT01");
        event.organizer = Some(crate::graph::GraphRecipient {
            email_address: Some(crate::graph::GraphEmailAddress {
                address: Some("sam@example.net".to_owned()),
            }),
        });
        event.attendees = Some(vec![crate::graph::GraphAttendee {
            email_address: Some(crate::graph::GraphEmailAddress {
                address: Some("alice@example.com".to_owned()),
            }),
        }]);

        let record = build_upsert(&context(), 1, &event)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(
            record.source.identity.as_str(),
            "calendar-event/AAMkAGI2EVENT01"
        );
        assert_eq!(
            record.occurred_at.map(|value| value.to_string()),
            Some("2026-08-22T09:00:00+00:00".to_owned())
        );
        let properties = record
            .properties
            .unwrap_or_else(|| panic!("properties required"));
        assert_eq!(
            properties.get("organizer"),
            Some(&PropertyValue::Text("sam@example.net".to_owned()))
        );
        assert_eq!(
            properties.get("participants"),
            Some(&PropertyValue::TextList(vec![
                "sam@example.net".to_owned(),
                "alice@example.com".to_owned()
            ]))
        );
        assert_eq!(
            properties.get("duration_seconds"),
            Some(&PropertyValue::Number(serde_json::Number::from(2700)))
        );
        let anchors = record
            .identity_anchors
            .unwrap_or_else(|| panic!("anchors required"));
        assert_eq!(anchors.len(), 2);
    }

    #[test]
    fn an_occurrence_carries_its_series_as_a_distinct_parent_identity() {
        let mut event = minimal_event("AAMkAGI2OCCURRENCE01");
        event.event_type = Some("occurrence".to_owned());
        event.series_master_id = Some("AAMkAGI2SERIES01".to_owned());

        let record = build_upsert(&context(), 1, &event)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(
            record.source.identity.as_str(),
            "calendar-event/AAMkAGI2OCCURRENCE01",
            "the instance's own id, never the series master's, is the portable identity"
        );
        assert_eq!(
            record.source.parent_identity.map(|value| value.to_string()),
            Some("calendar-event-series/AAMkAGI2SERIES01".to_owned())
        );
    }

    #[test]
    fn a_delete_record_carries_no_content_at_all() {
        let observed_at = OffsetDatetime::parse("2026-08-22T12:00:00+00:00")
            .unwrap_or_else(|error| panic!("must parse: {error}"));
        let record = build_delete(&context(), 1, "AAMkAGI2EVENT01", observed_at)
            .unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(record.change, Change::Delete);
        assert!(record.properties.is_none());
        assert!(record.body.is_none());
        assert_eq!(record.observed_at, Some(observed_at));
    }
}
