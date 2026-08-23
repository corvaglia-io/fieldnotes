//! Executable conformance cases for the `outlook_calendar` Field, driven
//! against the real compiled binary as a child process through the reusable
//! protocol conformance kit -- the same harness that validates the `local`
//! Field and the fixture Field. Every scenario is fixture-backed: Graph
//! itself never receives a real request (see `tests/support/mod.rs`).

mod support;

use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode, RunOutcome};
use fieldnotes_field_protocol::message::{Change, FieldEvent};
use fieldnotes_field_protocol::value::PropertyValue;

use support::{
    Case, all_day_event_json, event_json, occurrence_json, ok, page, record_events, removed_json,
    status,
};

const WINDOW_FROM: &str = "2026-08-24T00:00:00+00:00";
const WINDOW_TO: &str = "2026-08-31T00:00:00+00:00";
const DELTA_LINK_1: &str =
    "https://graph.microsoft.com/v1.0/me/calendarView/delta?$deltatoken=RUN1";
const DELTA_LINK_2: &str =
    "https://graph.microsoft.com/v1.0/me/calendarView/delta?$deltatoken=RUN2";

#[test]
fn describe_reports_a_complete_self_declaration() {
    let case = Case::new("describe", Vec::new());
    let manifest = case.manifest();

    assert_eq!(manifest.field_stem.as_str(), "outlook_calendar");
    assert_eq!(
        manifest
            .property_prefix
            .as_ref()
            .map(|prefix| prefix.as_str()),
        Some("outlook_calendar_")
    );
    assert_eq!(manifest.declared_properties.len(), 4);
    assert_eq!(manifest.capabilities.len(), 1);
    assert_eq!(
        manifest.capabilities[0].object_kind.as_str(),
        "calendar-event"
    );
    assert_eq!(manifest.capabilities[0].note_type.as_str(), "event");
    assert_eq!(
        manifest.collection.deletion.tombstones,
        fieldnotes_field_protocol::message::TombstoneAuthority::Authoritative
    );
    assert_eq!(
        manifest.collection.deletion.snapshot,
        fieldnotes_field_protocol::message::SnapshotAuthority::Unsupported
    );
    assert_eq!(
        manifest.collection.supported_modes,
        vec![fieldnotes_field_protocol::message::CollectionMode::Incremental]
    );
    assert!(manifest.collection.window_supported);
    assert_eq!(
        manifest.auth.kind,
        fieldnotes_field_protocol::message::AuthKind::OauthAuthorizationCode
    );
    assert_eq!(
        manifest.auth.scopes.as_deref(),
        Some(["Calendars.Read".to_owned()].as_slice())
    );
}

#[test]
fn a_windowed_collection_commits_a_delta_cursor_checkpoint() {
    let script = vec![ok(page(
        vec![
            event_json(
                "AAMkAGI2EVENT01",
                "Migration planning",
                "2026-08-24T09:00:00.0000000",
                "2026-08-24T09:45:00.0000000",
            ),
            event_json(
                "AAMkAGI2EVENT02",
                "Standup",
                "2026-08-25T09:00:00.0000000",
                "2026-08-25T09:15:00.0000000",
            ),
        ],
        None,
        Some(DELTA_LINK_1),
    ))];
    let case = Case::new("windowed", script);
    let manifest = case.manifest();
    let plan = case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 2);
    assert!(
        run.last_cursor().is_some(),
        "a fully-drained delta page must commit a resume cursor"
    );
    let records = record_events(&run);
    assert_eq!(
        records[0].source.identity.as_str(),
        "calendar-event/AAMkAGI2EVENT01"
    );
    assert_eq!(
        records[0].note_type.as_ref().map(|value| value.as_str()),
        Some("event")
    );
    assert_eq!(records[0].change, Change::Upsert);
}

#[test]
fn resumption_from_a_delta_token_does_not_re_emit_when_nothing_changed() {
    let first_script = vec![ok(page(
        vec![event_json(
            "AAMkAGI2EVENT01",
            "Migration planning",
            "2026-08-24T09:00:00.0000000",
            "2026-08-24T09:45:00.0000000",
        )],
        None,
        Some(DELTA_LINK_1),
    ))];
    let first_case = Case::new("resume-first", first_script);
    let first_manifest = first_case.manifest();
    let first_plan = first_case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let first = first_case.collect(&first_manifest, &first_plan);
    assert_eq!(first.report.records_accepted, 1);
    let cursor = first
        .last_cursor()
        .unwrap_or_else(|| panic!("a cursor must be committed"))
        .to_owned();

    // Graph's own delta semantics: resuming reports an empty page (nothing
    // changed since last time) with a fresh continuation.
    let second_script = vec![ok(page(vec![], None, Some(DELTA_LINK_2)))];
    let second_case = Case::new("resume-second", second_script);
    let second_manifest = second_case.manifest();
    let second_plan = second_case.resume_plan(support::RESUME_RUN, &cursor);
    let second = second_case.collect(&second_manifest, &second_plan);

    assert_eq!(
        second.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        second.rejection
    );
    assert_eq!(
        second.report.records_accepted, 0,
        "an unchanged delta page must not re-emit anything"
    );
    assert!(
        second.last_cursor().is_some(),
        "the run still advances its cursor"
    );
}

#[test]
fn a_changed_event_updates_one_record_under_the_same_source_key() {
    let first_script = vec![ok(page(
        vec![event_json(
            "AAMkAGI2EVENT01",
            "Migration planning",
            "2026-08-24T09:00:00.0000000",
            "2026-08-24T09:45:00.0000000",
        )],
        None,
        Some(DELTA_LINK_1),
    ))];
    let first_case = Case::new("changed-first", first_script);
    let first_manifest = first_case.manifest();
    let first_plan = first_case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let first = first_case.collect(&first_manifest, &first_plan);
    let first_records = record_events(&first);
    let first_key = (
        first_records[0].source.scope.as_str().to_owned(),
        first_records[0].source.identity.as_str().to_owned(),
    );
    let cursor = first
        .last_cursor()
        .unwrap_or_else(|| panic!("a cursor must be committed"))
        .to_owned();

    let second_script = vec![ok(page(
        vec![event_json(
            "AAMkAGI2EVENT01",
            "Migration planning (rescheduled)",
            "2026-08-24T10:00:00.0000000",
            "2026-08-24T10:45:00.0000000",
        )],
        None,
        Some(DELTA_LINK_2),
    ))];
    let second_case = Case::new("changed-second", second_script);
    let second_manifest = second_case.manifest();
    let second_plan = second_case.resume_plan(support::RESUME_RUN, &cursor);
    let second = second_case.collect(&second_manifest, &second_plan);

    assert_eq!(
        second.report.records_accepted, 1,
        "the changed event must be re-emitted"
    );
    let second_records = record_events(&second);
    let second_key = (
        second_records[0].source.scope.as_str().to_owned(),
        second_records[0].source.identity.as_str().to_owned(),
    );
    assert_eq!(
        first_key, second_key,
        "the same source object must keep the same portable exact-source key"
    );
    let properties = second_records[0]
        .properties
        .as_ref()
        .unwrap_or_else(|| panic!("properties required"));
    assert_eq!(
        properties.get("title"),
        Some(&PropertyValue::Text(
            "Migration planning (rescheduled)".to_owned()
        ))
    );
}

#[test]
fn a_removal_produces_an_authoritative_tombstone() {
    let script = vec![ok(page(
        vec![removed_json("AAMkAGI2EVENT09")],
        None,
        Some(DELTA_LINK_1),
    ))];
    let case = Case::new("tombstone", script);
    let manifest = case.manifest();
    let plan = case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        run.rejection
    );
    let records = record_events(&run);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].change, Change::Delete);
    assert_eq!(
        records[0].source.identity.as_str(),
        "calendar-event/AAMkAGI2EVENT09"
    );
    assert!(records[0].properties.is_none());
    assert!(records[0].body.is_none());
}

#[test]
fn a_recurring_series_does_not_duplicate_instances() {
    let script = vec![ok(page(
        vec![
            occurrence_json(
                "AAMkAGI2OCC01",
                "AAMkAGI2SERIES01",
                "Daily standup",
                "2026-08-24T09:00:00.0000000",
                "2026-08-24T09:15:00.0000000",
            ),
            occurrence_json(
                "AAMkAGI2OCC02",
                "AAMkAGI2SERIES01",
                "Daily standup",
                "2026-08-25T09:00:00.0000000",
                "2026-08-25T09:15:00.0000000",
            ),
            occurrence_json(
                "AAMkAGI2OCC03",
                "AAMkAGI2SERIES01",
                "Daily standup",
                "2026-08-26T09:00:00.0000000",
                "2026-08-26T09:15:00.0000000",
            ),
        ],
        None,
        Some(DELTA_LINK_1),
    ))];
    let case = Case::new("recurring", script);
    let manifest = case.manifest();
    let plan = case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        run.rejection
    );
    let records = record_events(&run);
    assert_eq!(
        records.len(),
        3,
        "every occurrence is collected exactly once"
    );
    let mut identities: Vec<&str> = records
        .iter()
        .map(|record| record.source.identity.as_str())
        .collect();
    identities.sort_unstable();
    identities.dedup();
    assert_eq!(
        identities.len(),
        3,
        "no two occurrences collapse to one identity"
    );
    assert!(
        identities
            .iter()
            .all(|identity| *identity != "calendar-event/AAMkAGI2SERIES01"),
        "the series master itself is never collected as a competing object"
    );
    for record in &records {
        assert_eq!(
            record
                .source
                .parent_identity
                .as_ref()
                .map(|value| value.as_str()),
            Some("calendar-event-series/AAMkAGI2SERIES01")
        );
    }
}

#[test]
fn an_all_day_event_and_an_offset_bearing_event_both_map_correctly() {
    let script = vec![ok(page(
        vec![
            all_day_event_json(
                "AAMkAGI2ALLDAY01",
                "Company holiday",
                "2026-08-24",
                "2026-08-25",
            ),
            event_json(
                "AAMkAGI2EVENT01",
                "Migration planning",
                "2026-08-24T09:00:00.0000000",
                "2026-08-24T09:45:00.0000000",
            ),
        ],
        None,
        Some(DELTA_LINK_1),
    ))];
    let case = Case::new("all-day", script);
    let manifest = case.manifest();
    let plan = case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        run.rejection
    );
    let records = record_events(&run);

    let all_day = records
        .iter()
        .find(|record| record.source.identity.as_str() == "calendar-event/AAMkAGI2ALLDAY01")
        .unwrap_or_else(|| panic!("the all-day event must be collected"));
    let all_day_properties = all_day
        .properties
        .as_ref()
        .unwrap_or_else(|| panic!("properties required"));
    assert_eq!(
        all_day_properties.get("outlook_calendar_all_day"),
        Some(&PropertyValue::Boolean(true))
    );
    assert_eq!(
        all_day_properties.get("started_at"),
        Some(&PropertyValue::Text("2026-08-24T00:00:00+00:00".to_owned()))
    );
    assert_eq!(
        all_day_properties.get("ended_at"),
        Some(&PropertyValue::Text("2026-08-25T00:00:00+00:00".to_owned()))
    );
    assert_eq!(
        all_day_properties.get("duration_seconds"),
        Some(&PropertyValue::Number(serde_json::Number::from(86_400)))
    );

    let timed = records
        .iter()
        .find(|record| record.source.identity.as_str() == "calendar-event/AAMkAGI2EVENT01")
        .unwrap_or_else(|| panic!("the timed event must be collected"));
    let timed_properties = timed
        .properties
        .as_ref()
        .unwrap_or_else(|| panic!("properties required"));
    assert_eq!(
        timed_properties.get("outlook_calendar_all_day"),
        Some(&PropertyValue::Boolean(false))
    );
    assert_eq!(
        timed.occurred_at.map(|value| value.to_string()),
        Some("2026-08-24T09:00:00+00:00".to_owned()),
        "the interval start carries an explicit numeric offset"
    );
}

#[test]
fn a_throttled_response_is_retried_and_the_run_still_completes() {
    let script = vec![
        status(
            429,
            Some(0),
            serde_json::json!({"error": {"code": "TooManyRequests", "message": "throttled"}}),
        ),
        ok(page(
            vec![event_json(
                "AAMkAGI2EVENT01",
                "Migration planning",
                "2026-08-24T09:00:00.0000000",
                "2026-08-24T09:45:00.0000000",
            )],
            None,
            Some(DELTA_LINK_1),
        )),
    ];
    let case = Case::new("throttled", script);
    let manifest = case.manifest();
    let plan = case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 1);
}

#[test]
fn an_expired_token_surfaces_actionably() {
    let script = vec![status(
        401,
        None,
        serde_json::json!({"error": {"code": "InvalidAuthenticationToken", "message": "expired"}}),
    )];
    let case = Case::new("expired-token", script);
    let manifest = case.manifest();
    let plan = case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_ne!(run.report.outcome, RunOutcome::Complete);
    assert_eq!(run.exit.exit_code(), Some(ExitCode::Authentication));
    let diagnostics: Vec<_> = run
        .events
        .iter()
        .filter_map(|event| match event {
            FieldEvent::Diagnostic(diagnostic) => Some(diagnostic.as_ref()),
            _ => None,
        })
        .collect();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::AuthReauthRequired),
        "an expired token must surface as a distinct, actionable diagnostic: {diagnostics:?}"
    );
    assert!(
        run.last_cursor().is_none(),
        "a run that never reached Graph successfully commits no cursor"
    );
}

#[test]
fn a_partial_result_is_never_read_as_deletion() {
    let script = vec![
        ok(page(
            vec![
                event_json(
                    "AAMkAGI2EVENT01",
                    "Migration planning",
                    "2026-08-24T09:00:00.0000000",
                    "2026-08-24T09:45:00.0000000",
                ),
                removed_json("AAMkAGI2EVENT02"),
            ],
            Some("https://graph.microsoft.com/v1.0/me/calendarView/delta?$skip=next"),
            None,
        )),
        // A well-formed HTTP response whose body does not match the expected
        // collection-page shape: not retried (only status-based failures
        // are), so this ends the run immediately on the second page.
        ok(serde_json::json!("not-a-collection-envelope")),
    ];
    let case = Case::new("partial", script);
    let manifest = case.manifest();
    let plan = case.windowed_plan(support::COLLECT_RUN, WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_ne!(run.report.outcome, RunOutcome::Complete);
    assert!(
        run.report.records_accepted >= 2,
        "durable work from the first page remains: {:?}",
        run.report
    );
    assert!(
        !run.deletion().is_authorized(),
        "this Field never claims snapshot authority, so absence can never authorize removal"
    );
    assert!(
        run.last_cursor().is_none(),
        "a first run that did not finish offers no cursor, so no forward progress is silently \
         claimed"
    );
}
