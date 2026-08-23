//! Executable conformance cases for the Outlook Mail Field, driven against the
//! real compiled binary as a child process through the reusable protocol
//! conformance kit -- the same harness that validates the `local` and fixture
//! Fields.
//!
//! Every Graph response these cases see comes from a sanitized recording in
//! `tests/fixtures/graph/`. There is no tenant, no network, and no credential.

mod support;

use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit, RunOutcome};
use fieldnotes_field_protocol::conformance::CoreObservation;
use fieldnotes_field_protocol::grammar::{MediaTypeMatcher, SourceIdentity, SourceScope};
use fieldnotes_field_protocol::message::{
    ArtifactKind, Change, CollectionMode, RecollectTarget, Severity, SnapshotAuthority,
    TombstoneAuthority,
};

use support::{Case, diagnostics, has_severity, record_events, source_keys, windowed, with_cursor};

/// The beta's bounded window: one week ending at the fixture's collection day.
const WINDOW_FROM: &str = "2026-08-16T00:00:00+00:00";
const WINDOW_TO: &str = "2026-08-23T00:00:00+00:00";

#[test]
fn describe_reports_a_complete_self_declaration() {
    let case = Case::new("window");
    let manifest = case.manifest();

    assert_eq!(manifest.field_stem.as_str(), "outlook_mail");
    assert_eq!(
        manifest
            .property_prefix
            .as_ref()
            .map(|prefix| prefix.as_str()),
        Some("outlook_mail_")
    );
    assert_eq!(manifest.declared_properties.len(), 6);
    assert_eq!(manifest.capabilities.len(), 1);
    assert_eq!(
        manifest.capabilities[0].object_kind.as_str(),
        "mail-message"
    );
    assert_eq!(manifest.capabilities[0].note_type.as_str(), "mail");
    assert!(manifest.capabilities[0].emits_artifacts);
    assert!(manifest.capabilities[0].emits_identity_anchors);

    assert_eq!(
        manifest.collection.deletion.tombstones,
        TombstoneAuthority::Authoritative,
        "a Graph delta reports removals explicitly, which is genuine tombstone authority"
    );
    assert_eq!(
        manifest.collection.deletion.snapshot,
        SnapshotAuthority::Unsupported,
        "this Field never removes a Note by absence"
    );
    assert_eq!(
        manifest.collection.supported_modes,
        vec![CollectionMode::Incremental]
    );
    assert!(manifest.collection.window_supported);
    assert_eq!(manifest.collection.cursor_format_version, 1);
    assert_eq!(
        manifest.auth.scopes,
        Some(vec!["Mail.Read".to_owned()]),
        "the declared scope must stay least-privilege and read-only"
    );
    assert!(manifest.auth.protected_channel_required);
}

#[test]
fn a_windowed_collection_emits_records_and_a_checkpoint_without_advancing_the_cursor() {
    let case = Case::new("window");
    let manifest = case.manifest();
    let plan = windowed(case.plan(support::COLLECT_RUN), WINDOW_FROM, WINDOW_TO);
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "rejection: {:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 2);
    assert_eq!(
        run.last_cursor(),
        Some("outlook-mail/v1"),
        "a windowed run is bounded evidence, so it must not adopt a delta token"
    );
    assert!(
        run.actions
            .iter()
            .any(|action| matches!(action, CoreObservation::CommittedCheckpoint { .. })),
        "a successful windowed run must commit a checkpoint: {:?}",
        run.actions
    );
    let records = record_events(&run);
    assert_eq!(records[0].source.scope.as_str(), support::SOURCE_SCOPE);
    assert_eq!(
        records[0].source.identity.as_str(),
        "mail-message/AAMkAGI2TQABAAAA"
    );
    assert_eq!(
        records[0].note_type.as_ref().map(|note| note.as_str()),
        Some("mail")
    );
    assert!(
        !run.deletion().is_authorized(),
        "a windowed run can never authorize deletion: {:?}",
        run.deletion()
    );
}

#[test]
fn a_delta_collection_stages_one_attachment_and_declines_the_rest_with_their_references() {
    let case = Case::new("delta-two-pages");
    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.plan(support::COLLECT_RUN));

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "rejection: {:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 2, "both pages were collected");

    let installed: Vec<&String> = run
        .actions
        .iter()
        .filter_map(|action| match action {
            CoreObservation::InstalledArtifact { digest } => Some(digest),
            _ => None,
        })
        .collect();
    assert_eq!(
        installed.len(),
        1,
        "exactly the one included, in-threshold attachment is staged: {:?}",
        run.actions
    );
    assert_eq!(
        installed[0], "5fe823fadb8e51207c5cc98aef9e49c038d0f49a04475e3eb9bc495898ebe90f",
        "core's own digest over the staged bytes"
    );

    let declined: Vec<&String> = run
        .actions
        .iter()
        .filter_map(|action| match action {
            CoreObservation::DeclinedArtifact { attachment_ref, .. } => Some(attachment_ref),
            _ => None,
        })
        .collect();
    assert_eq!(
        declined,
        vec![
            "mail-attachment/AAMkAGI2TQABAAACattach02",
            "mail-attachment/AAMkAGI2TQABAAACattach03",
            "mail-attachment/AAMkAGI2TQABAAACattach04",
        ],
        "every declined attachment carries its own stable reference: {:?}",
        run.actions
    );

    let attachment_record = record_events(&run)
        .into_iter()
        .find(|record| record.source.identity.as_str() == "mail-message/AAMkAGI2TQABAAAF")
        .unwrap_or_else(|| panic!("the attachment-bearing message must be collected"));
    let artifacts = attachment_record
        .artifacts
        .as_ref()
        .unwrap_or_else(|| panic!("artifacts must be present"));
    assert_eq!(artifacts.len(), 4);
    assert_eq!(artifacts[0].kind, ArtifactKind::Staged);
    for declined in &artifacts[1..] {
        assert_eq!(declined.kind, ArtifactKind::NotRetained);
        assert!(
            declined.attachment_ref.is_some(),
            "a declined artifact's reference is its only stable identity"
        );
        assert!(declined.handle.is_none() && declined.sha256.is_none());
    }
    let body = attachment_record
        .body
        .as_ref()
        .unwrap_or_else(|| panic!("a body is required"));
    assert!(
        body.text
            .contains("video/mp4 is outside this run's retention include set"),
        "the media-type decline must be reviewable in the body: {}",
        body.text
    );
    assert!(
        body.text.contains("over this run's retention threshold"),
        "the size decline must be reviewable in the body: {}",
        body.text
    );

    assert!(
        !has_severity(&run, Severity::Error),
        "declining an attachment is a policy decision, never a failure: {:?}",
        diagnostics(&run)
    );
    let cursor = run
        .last_cursor()
        .unwrap_or_else(|| panic!("a complete delta collection must commit a cursor"));
    assert!(
        cursor.starts_with("outlook-mail/v1;dt="),
        "a complete delta collection adopts Graph's delta token: {cursor}"
    );
    assert!(
        cursor.contains("FIXTURE_DELTA_1"),
        "the adopted token is the one the final page carried: {cursor}"
    );
}

#[test]
fn resumption_from_a_committed_delta_cursor_does_not_re_emit_a_settled_message() {
    let first = Case::new("delta-two-pages");
    let manifest = first.manifest();
    let baseline = first.collect(&manifest, &first.plan(support::COLLECT_RUN));
    assert_eq!(baseline.report.records_accepted, 2);
    let cursor = baseline
        .last_cursor()
        .unwrap_or_else(|| panic!("a cursor must be committed"))
        .to_owned();

    // The resume script answers **only** a request carrying the delta token
    // the first run committed, so this case fails loudly if the second run
    // starts over instead of resuming.
    let second = Case::new("delta-resume");
    let plan = with_cursor(second.plan(support::RESUME_RUN), &cursor, 1);
    let run = second.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "rejection: {:?}",
        run.rejection
    );
    assert_eq!(
        run.report.records_accepted, 0,
        "a delta resume with nothing new must re-emit nothing"
    );
    let resumed = run
        .last_cursor()
        .unwrap_or_else(|| panic!("a cursor must be committed"));
    assert!(
        resumed.contains("FIXTURE_DELTA_2"),
        "the cursor advances to the token the resumed page returned: {resumed}"
    );
}

#[test]
fn a_changed_message_produces_an_updated_record_under_the_same_source_key() {
    let first = Case::new("delta-two-pages");
    let manifest = first.manifest();
    let baseline = first.collect(&manifest, &first.plan(support::COLLECT_RUN));
    let cursor = baseline
        .last_cursor()
        .unwrap_or_else(|| panic!("a cursor must be committed"))
        .to_owned();
    let baseline_key = source_keys(&baseline)
        .into_iter()
        .find(|(_, identity)| identity == "mail-message/AAMkAGI2TQABAAAA")
        .unwrap_or_else(|| panic!("the message must be collected first"));

    let second = Case::new("delta-changed");
    let plan = with_cursor(second.plan(support::RESUME_RUN), &cursor, 1);
    let run = second.collect(&manifest, &plan);

    assert_eq!(run.report.records_accepted, 1);
    let records = record_events(&run);
    assert_eq!(
        (
            records[0].source.scope.as_str().to_owned(),
            records[0].source.identity.as_str().to_owned()
        ),
        baseline_key,
        "the same upstream message keeps the same portable exact-source key"
    );
    assert_eq!(records[0].change, Change::Upsert);
    assert!(
        run.actions
            .iter()
            .any(|action| matches!(action, CoreObservation::WroteNote { .. })),
        "a changed payload must rewrite the current Note, not be treated as unchanged: {:?}",
        run.actions
    );
    assert_eq!(
        records[0]
            .source
            .version
            .as_ref()
            .map(|value| value.as_str()),
        Some("CQAAABYAAADD"),
        "the new change key crosses as the source version"
    );
}

#[test]
fn replaying_the_same_messages_after_a_crash_changes_nothing() {
    // A crash between a durable record and its checkpoint commit leaves the
    // cursor lagging, so the next run re-collects what it already has. Seeding
    // the state the first run left behind is how the kit models "locate the
    // current Note by portable source key" without a notebook.
    let first = Case::new("delta-two-pages");
    let manifest = first.manifest();
    let baseline = first.collect(&manifest, &first.plan(support::COLLECT_RUN));
    assert_eq!(baseline.report.records_accepted, 2);

    let replay = Case::new("delta-two-pages");
    let plan = replay
        .plan(support::RESUME_RUN)
        .resuming_state(baseline.current_state.clone());
    let run = replay.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "rejection: {:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 2, "both are re-offered");
    let rewrites = run
        .actions
        .iter()
        .filter(|action| matches!(action, CoreObservation::WroteNote { .. }))
        .count();
    assert_eq!(
        rewrites, 0,
        "an unchanged replay must rewrite nothing: {:?}",
        run.actions
    );
    assert_eq!(
        run.actions
            .iter()
            .filter(|action| matches!(action, CoreObservation::NoChange { .. }))
            .count(),
        2,
        "both replayed messages are recognised as the same current state: {:?}",
        run.actions
    );
}

#[test]
fn a_removal_reported_by_the_delta_feed_produces_an_authoritative_tombstone() {
    let case = Case::new("delta-removal");
    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.plan(support::COLLECT_RUN));

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "rejection: {:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 1);
    let records = record_events(&run);
    let tombstone = records[0];
    assert_eq!(tombstone.change, Change::Delete);
    assert_eq!(
        tombstone.source.identity.as_str(),
        "mail-message/AAMkAGI2TQABAAAA"
    );
    assert!(
        tombstone.authority.is_some(),
        "a delete carries its declared authority"
    );
    assert!(
        tombstone.observed_at.is_some(),
        "a delete carries the instant the removal was observed"
    );
    // A delete carries no content at all, which is what stops a deletion from
    // ever being confused with an empty or partial result.
    assert!(tombstone.note_type.is_none());
    assert!(tombstone.occurred_at.is_none());
    assert!(tombstone.properties.is_none());
    assert!(tombstone.body.is_none());
    assert!(tombstone.artifacts.is_none());
    assert!(tombstone.identity_anchors.is_none());
    assert!(tombstone.integrity.is_none());

    assert!(
        run.actions
            .iter()
            .any(|action| matches!(action, CoreObservation::RemovedNote { .. })),
        "core must act on the tombstone under the declared authority: {:?}",
        run.actions
    );
}

#[test]
fn a_recollection_re_evaluates_a_declined_attachment_against_the_current_policy() {
    // ADR 0007's motivating case, and the reason this Field declares
    // `refetch: supported`: widening the retention include set must be able to
    // reach an attachment an earlier run already reported and declined, which
    // ordinary forward-only delta collection never revisits.
    let case = Case::new("recollect");
    let manifest = case.manifest();
    let widened = ["text/plain", "video/mp4"]
        .into_iter()
        .map(|matcher| {
            MediaTypeMatcher::parse(matcher)
                .unwrap_or_else(|error| panic!("{matcher} must parse: {error}"))
        })
        .collect();
    let target = RecollectTarget {
        scope: SourceScope::parse(support::SOURCE_SCOPE)
            .unwrap_or_else(|error| panic!("must parse: {error}")),
        identity: SourceIdentity::parse("mail-message/AAMkAGI2TQABAAAF")
            .unwrap_or_else(|error| panic!("must parse: {error}")),
    };
    let plan = case
        .plan(support::RESUME_RUN)
        .with_artifact_media_types(widened)
        .recollecting(vec![target]);
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "rejection: {:?} diagnostics: {:?}",
        run.rejection,
        diagnostics(&run)
    );
    assert_eq!(run.report.records_accepted, 1);
    let installed = run
        .actions
        .iter()
        .filter(|action| matches!(action, CoreObservation::InstalledArtifact { .. }))
        .count();
    assert_eq!(
        installed, 2,
        "the newly-included video is retained this time alongside the text: {:?}",
        run.actions
    );
    assert_eq!(
        run.last_cursor(),
        Some("outlook-mail/v1"),
        "a recollection is scoped to its targets, so it never advances the delta cursor"
    );
}

#[test]
fn a_throttled_response_is_retried_transparently_and_the_run_still_completes() {
    let case = Case::new("throttled");
    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.plan(support::COLLECT_RUN));

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "throttling the transport already absorbed must not surface as a failure: {:?} {:?}",
        run.rejection,
        diagnostics(&run)
    );
    assert_eq!(run.report.records_accepted, 1);
    assert!(
        !has_severity(&run, Severity::Warning),
        "a transparently retried request needs no diagnostic at all: {:?}",
        diagnostics(&run)
    );
    assert!(
        run.last_cursor()
            .is_some_and(|cursor| cursor.contains("FIXTURE_DELTA_1")),
        "the retried run still reached its final page and advanced"
    );
}

#[test]
fn an_expired_token_surfaces_actionably_and_freezes_the_cursor() {
    let case = Case::new("expired-token");
    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.plan(support::COLLECT_RUN));

    let expired = diagnostics(&run)
        .into_iter()
        .find(|diagnostic| diagnostic.code == DiagnosticCode::AuthExpired)
        .unwrap_or_else(|| panic!("an expired token must be reported as one: {:?}", run.events));
    assert_eq!(expired.severity, Severity::Error);
    assert!(
        expired.message.as_str().contains("Re-authenticate"),
        "the message must say what to do: {}",
        expired.message
    );
    assert_eq!(
        run.exit.exit_code(),
        Some(ProtocolExit::Authentication),
        "an authentication failure exits with the authentication code"
    );
    assert_eq!(
        run.last_cursor(),
        Some("outlook-mail/v1"),
        "a failed run must not advance the cursor"
    );
    assert!(!run.deletion().is_authorized());
}

#[test]
fn a_partial_result_never_reads_as_deletion() {
    let case = Case::new("partial");
    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.plan(support::COLLECT_RUN));

    assert_eq!(
        run.report.outcome,
        RunOutcome::Partial,
        "durable work happened but the run did not complete: {:?}",
        run.rejection
    );
    assert_eq!(
        run.report.records_accepted, 1,
        "the page that did arrive is still collected"
    );
    assert!(
        has_severity(&run, Severity::Error),
        "the failed page must be reported: {:?}",
        diagnostics(&run)
    );
    assert!(
        !run.deletion().is_authorized(),
        "a partial result must never authorize deletion: {:?}",
        run.deletion()
    );
    assert_eq!(
        run.last_cursor(),
        Some("outlook-mail/v1"),
        "a run that did not see its final page must not adopt a delta token"
    );
}

#[test]
fn an_unreadable_cursor_is_reported_and_restarts_an_unbounded_delta_collection() {
    let case = Case::new("delta-two-pages");
    let manifest = case.manifest();
    // A cursor at this Field's own declared format version that this Field
    // cannot read: corrupt, not a version migration.
    let plan = with_cursor(case.plan(support::COLLECT_RUN), "local-walk/v1;hw=42", 1);
    let run = case.collect(&manifest, &plan);

    assert!(
        diagnostics(&run)
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::CursorResetRequired),
        "an unreadable cursor must be reported, not silently ignored: {:?}",
        diagnostics(&run)
    );
    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "restarting unbounded is a recovery, not a failure: {:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 2);
}

#[test]
fn a_missing_tenant_configuration_is_refused_actionably_before_any_read() {
    let case = Case::new("window");
    let manifest = case.manifest();
    let run = case.collect(
        &manifest,
        &case.plan_without_configuration(support::COLLECT_RUN),
    );

    let refusal = diagnostics(&run)
        .into_iter()
        .find(|diagnostic| diagnostic.code == DiagnosticCode::ConfigInvalid)
        .unwrap_or_else(|| panic!("a missing configuration must be reported: {:?}", run.events));
    assert_eq!(refusal.severity, Severity::Error);
    assert!(
        refusal.message.as_str().contains("tenant_id"),
        "the message must name the missing key: {}",
        refusal.message
    );
    assert_eq!(run.exit.exit_code(), Some(ProtocolExit::ConfigInvalid));
    assert_eq!(run.report.records_accepted, 0);
    assert!(
        run.last_cursor().is_none(),
        "a run that never began offers no cursor"
    );
}

#[test]
fn the_placeholder_token_never_appears_anywhere_in_the_childs_output() {
    let case = Case::new("delta-two-pages");
    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.plan(support::COLLECT_RUN));

    assert!(
        run.secret_locations(support::PLACEHOLDER_TOKEN).is_empty(),
        "the value presented to the transport must never reach a record, a cursor, a \
         diagnostic, an artifact, or standard error: {:?}",
        run.secret_locations(support::PLACEHOLDER_TOKEN)
    );
    assert!(
        !run.argv.iter().any(|argument| argument.contains("TOKEN")),
        "no token-shaped value may appear in argv: {:?}",
        run.argv
    );
}
