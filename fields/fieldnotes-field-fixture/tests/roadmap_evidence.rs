//! The six behaviors the roadmap names as A2 approval evidence, plus the four
//! safety properties the package claims, asserted against the fixture Field
//! running as a **real child process** over real pipes.
//!
//! Roadmap evidence: resumption, duplicate records, malformed output, process
//! failure, log redaction, and crash boundaries around checkpoints.
//!
//! Safety properties: a cursor never advances past a failed durable write;
//! absence is never treated as deletion unless every declared authoritative
//! condition holds; an illegal or path-traversing artifact handle is rejected
//! with the code the package specifies; and no secret appears in arguments,
//! events, logs, or cursors.

mod support;

use std::time::Duration;

use fieldnotes_field_protocol::codes::{RejectionCode, RunOutcome};
use fieldnotes_field_protocol::conformance::{CoreObservation, DurabilityPolicy};
use fieldnotes_field_protocol::session::{
    DeletionAuthorization, DeletionRefusal, ExitObservation, RecordDisposition,
};

use support::{
    COLLECT_RUN, Case, LOCAL_FIELD, LOCAL_SCOPE, MAIL_FIELD, RESUMED_RUN,
    limits_with_frame_ceiling, with_cursor,
};

// ---------------------------------------------------------------------------
// Roadmap evidence 1: a successful incremental run, as the baseline
// ---------------------------------------------------------------------------

#[test]
fn a_successful_incremental_run_installs_two_notes_one_artifact_and_one_cursor() {
    let case = Case::new("incremental");
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(run.report.outcome, RunOutcome::Complete);
    assert_eq!(run.exit, ExitObservation::Exited(0));
    assert_eq!(run.report.records_accepted, 2);
    assert_eq!(run.events.len(), 3, "two records and one checkpoint");

    // Core derived the artifact identity from its own digest over the staged
    // bytes, which are the frozen A1 vector.
    let installed: Vec<&String> = run
        .actions
        .iter()
        .filter_map(|action| match action {
            CoreObservation::InstalledArtifact { digest } => Some(digest),
            _ => None,
        })
        .collect();
    assert_eq!(installed.len(), 1);
    assert_eq!(
        installed[0], "449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17",
        "core computes its own digest rather than trusting the declared one"
    );

    assert_eq!(run.committed_cursors.len(), 1);
    assert_eq!(
        run.last_cursor(),
        Some("walk:v1:seq=2;mtime=2026-08-22T09:45:00Z")
    );
    assert_eq!(
        run.committed_cursors[0].covers_record_seq_through, 2,
        "the committed cursor covers both durable records"
    );
    assert!(run.rejection.is_none());
}

#[test]
fn two_runs_of_one_scenario_produce_identical_events() {
    // Determinism: the fixture has no wall clock and no randomness, so replaying
    // a completed run must be byte-identical.
    let first = Case::new("incremental").incremental(LOCAL_FIELD);
    let second = Case::new("incremental").incremental(LOCAL_FIELD);
    let encode = |run: &fieldnotes_field_protocol::conformance::CollectRun| {
        run.events
            .iter()
            .filter_map(|event| event.to_json().ok())
            .map(|value| serde_json::to_string(&value).unwrap_or_default())
            .collect::<Vec<_>>()
    };
    assert_eq!(encode(&first), encode(&second));
}

// ---------------------------------------------------------------------------
// Roadmap evidence 2: resumption
// ---------------------------------------------------------------------------

#[test]
fn resumption_from_a_committed_cursor_returns_only_newer_material() {
    let case = Case::new("resume");
    let manifest = case.manifest(LOCAL_FIELD);

    // Without a replayable cursor the Field starts unbounded.
    let unbounded = case.collect(&manifest, &case.plan(LOCAL_FIELD));
    assert_eq!(unbounded.report.outcome, RunOutcome::Complete);
    assert_eq!(unbounded.report.records_accepted, 3);

    // With one, it returns only what the cursor does not already account for.
    let resumed = case.collect(
        &manifest,
        &with_cursor(
            case.plan(LOCAL_FIELD),
            "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
            1,
        ),
    );
    assert_eq!(resumed.report.outcome, RunOutcome::Complete);
    assert_eq!(
        resumed.report.records_accepted, 1,
        "resumption is observable: the Field returned only the newer object"
    );
    assert_eq!(resumed.report.diagnostics_accepted, 1);
    assert_eq!(
        resumed.last_cursor(),
        Some("walk:v1:seq=3;mtime=2026-08-22T12:05:00Z"),
        "the cursor advanced again"
    );
}

#[test]
fn a_cursor_written_at_another_format_version_is_never_replayed_blindly() {
    // The manifest declares cursor_format_version 2, so a cursor stored at 1 is
    // not replayable and core must refuse to hand the Field a token it may
    // misread.
    let case = Case::new("describe-cursor-format-2");
    let current = case.manifest(LOCAL_FIELD);
    let previous = Case::new("describe-local").manifest(LOCAL_FIELD);

    let stored = fieldnotes_field_protocol::conformance::manifest_snapshot(&previous);
    let arriving = fieldnotes_field_protocol::conformance::manifest_snapshot(&current);
    match stored.check_against(&arriving) {
        Err(migration) => assert_eq!(
            migration.code,
            RejectionCode::ManifestCursorFormatChanged,
            "a changed cursor format blocks sync until an explicit migration"
        ),
        Ok(()) => panic!("a changed cursor format version must block the next sync"),
    }
}

// ---------------------------------------------------------------------------
// Roadmap evidence 3: duplicate records
// ---------------------------------------------------------------------------

#[test]
fn an_identical_duplicate_record_is_a_no_op_within_one_run() {
    let case = Case::new("duplicate-replay");
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(run.report.outcome, RunOutcome::Complete);
    assert_eq!(run.report.records_accepted, 2);
    let dispositions: Vec<&CoreObservation> = run
        .actions
        .iter()
        .filter(|action| {
            matches!(
                action,
                CoreObservation::WroteNote { .. } | CoreObservation::NoChange { .. }
            )
        })
        .collect();
    assert_eq!(dispositions.len(), 2);
    assert!(
        matches!(dispositions[1], CoreObservation::NoChange { seq: 2 }),
        "the second frame for one source key with an identical payload rewrites nothing, so \
         replay costs work and never correctness: {dispositions:?}"
    );
    assert_eq!(run.committed_cursors.len(), 1);
}

#[test]
fn a_divergent_duplicate_in_one_run_is_a_rejected_field_defect() {
    let case = Case::new("duplicate-divergent");
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::RecordDuplicateDivergentInRun),
        "one producer asserting two different current states for one object with no declared \
         version ordering is a bug, not a conflict"
    );
    assert_eq!(run.report.outcome, RunOutcome::Failed);
    assert!(
        run.committed_cursors.is_empty(),
        "a run in which core rejected a record commits no further checkpoint"
    );
}

// ---------------------------------------------------------------------------
// Roadmap evidence 4: malformed output
// ---------------------------------------------------------------------------

#[test]
fn a_non_json_line_on_standard_output_fails_the_run_but_not_the_committed_cursor() {
    let case = Case::new("malformed-not-json");
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(run.rejection_code(), Some(RejectionCode::ProtocolNotJson));
    assert_eq!(run.report.outcome, RunOutcome::Failed);
    assert_eq!(
        run.committed_cursors.len(),
        1,
        "checkpoints already committed stand, because they were durable before they were committed"
    );
    assert_eq!(run.committed_cursors[0].covers_record_seq_through, 1);
}

#[test]
fn each_malformed_shape_is_rejected_with_its_own_closed_vocabulary_code() {
    // Each shape is its own run, because a real run stops at the first
    // violation. The codes are exactly the ones transcript 08 names.
    let expectations = [
        (
            "malformed-unknown-event",
            RejectionCode::ProtocolUnknownEvent,
        ),
        (
            "malformed-seq-regression",
            RejectionCode::ProtocolSeqRegression,
        ),
        (
            "malformed-duplicate-seq",
            RejectionCode::ProtocolDuplicateSeq,
        ),
        ("malformed-seq-gap", RejectionCode::ProtocolUnexpectedOrder),
        ("malformed-invalid-utf8", RejectionCode::ProtocolInvalidUtf8),
        (
            "malformed-truncated-frame",
            RejectionCode::ProtocolTruncatedFrame,
        ),
    ];
    for (scenario, expected) in expectations {
        let run = Case::new(scenario).incremental(LOCAL_FIELD);
        assert_eq!(
            run.rejection_code(),
            Some(expected),
            "{scenario} must be rejected as {expected}"
        );
        assert_eq!(run.report.outcome, RunOutcome::Failed, "{scenario}");
    }
}

#[test]
fn an_oversized_frame_is_refused_at_the_ceiling() {
    let case = Case::new("malformed-oversized-frame").with_limits(limits_with_frame_ceiling(8192));
    let run = case.incremental(LOCAL_FIELD);
    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::ProtocolOversizedFrame)
    );
    assert_eq!(run.report.outcome, RunOutcome::Failed);
}

#[test]
fn a_field_that_stops_making_progress_hits_the_idle_bound() {
    let case = Case::new("hang").with_timeouts(Duration::from_millis(400), Duration::from_secs(2));
    let run = case.incremental(LOCAL_FIELD);
    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::ProtocolIdleTimeout),
        "a hung connector cannot hold an unbounded core run"
    );
    assert_eq!(run.report.outcome, RunOutcome::Failed);
}

// ---------------------------------------------------------------------------
// Roadmap evidence 5: process failure
// ---------------------------------------------------------------------------

#[test]
fn a_non_zero_exit_leaves_durable_work_and_the_committed_cursor_in_place() {
    let case = Case::new("exit-before-checkpoint");
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(run.exit, ExitObservation::Exited(1));
    assert_eq!(
        run.report.outcome,
        RunOutcome::Partial,
        "durable work happened and the run did not complete"
    );
    assert!(
        run.rejection.is_none(),
        "a non-zero exit is not a violation"
    );
    assert_eq!(run.committed_cursors.len(), 1);
    assert_eq!(run.committed_cursors[0].covers_record_seq_through, 1);
    assert_eq!(
        run.report.records_accepted, 2,
        "the record after the checkpoint was accepted and made durable"
    );
}

#[test]
fn a_signal_termination_is_normalized_to_a_failed_run() {
    let case = Case::new("crash-before-checkpoint");
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Failed,
        "core treats any abnormal termination as a failed run rather than inventing a \
         Field-level meaning for it"
    );
    assert!(
        !run.exit.is_normal(),
        "the child did not end normally: {:?}",
        run.exit
    );
}

#[test]
fn a_version_negotiation_failure_aborts_before_any_credential_grant_exists() {
    let case = Case::new("describe-version-mismatch");
    let run = match case.describe(MAIL_FIELD) {
        Ok(run) => run,
        Err(error) => panic!("the describe run could not start: {error}"),
    };

    assert!(
        run.manifest.is_none(),
        "a Field that supports no version core offered emits no manifest at all"
    );
    assert_eq!(
        run.rejection,
        Some(RejectionCode::ProtocolVersionUnsupported)
    );
    assert_eq!(run.exit, ExitObservation::Exited(3));
    assert!(
        run.stderr.contains("core offered [1]") && run.stderr.contains("[2, 3]"),
        "the failure is actionable and names both version sets: {}",
        run.stderr
    );
    // Negotiation happens inside the describe run, so no grant, staging
    // directory, or collect run ever existed.
    assert!(run.argv.iter().all(|token| !token.contains("collect")));
}

#[test]
fn a_manifest_answering_with_an_unoffered_version_is_rejected_not_interpreted() {
    let case = Case::new("describe-future-version");
    let run = match case.describe(MAIL_FIELD) {
        Ok(run) => run,
        Err(error) => panic!("the describe run could not start: {error}"),
    };
    assert!(run.manifest.is_none());
    assert_eq!(
        run.rejection,
        Some(RejectionCode::ProtocolVersionUnsupported),
        "core reports the version mismatch, not a schema-internal detail"
    );
}

// ---------------------------------------------------------------------------
// Roadmap evidence 6: crash boundaries around checkpoints
// ---------------------------------------------------------------------------

#[test]
fn a_crash_after_a_checkpoint_commit_leaves_the_cursor_at_that_checkpoint() {
    let case = Case::new("crash-after-checkpoint");
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(run.report.outcome, RunOutcome::Failed);
    assert_eq!(run.committed_cursors.len(), 1);
    assert_eq!(run.committed_cursors[0].covers_record_seq_through, 1);
    assert_eq!(
        run.last_cursor(),
        Some("walk:v1:seq=2;mtime=2026-08-22T09:45:00Z")
    );
}

#[test]
fn a_crash_before_a_checkpoint_leaves_the_cursor_lagging_and_replay_changes_nothing() {
    // The whole design in one test. A run makes one record durable, commits a
    // checkpoint, makes a second record durable, and dies before the checkpoint
    // covering it. The committed cursor therefore lags durable state.
    let crashing = Case::new("crash-before-checkpoint");
    let crashed = crashing.incremental(LOCAL_FIELD);

    assert_eq!(crashed.report.outcome, RunOutcome::Failed);
    assert_eq!(crashed.report.records_accepted, 2);
    assert_eq!(
        crashed.committed_cursors.len(),
        1,
        "no checkpoint covered the second durable record"
    );
    assert_eq!(
        crashed.committed_cursors[0].covers_record_seq_through, 1,
        "a lagging cursor costs work; an advanced cursor would lose an object forever"
    );

    // The next run resumes from the lagging cursor. The Field re-emits the
    // object, and reconciliation through the portable exact-source key makes the
    // replay a no-op.
    let resuming = Case::new("resume-after-crash");
    let manifest = resuming.manifest(LOCAL_FIELD);
    let mut plan = with_cursor(
        resuming.plan(LOCAL_FIELD),
        "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
        1,
    )
    // Core locates the current Note by portable exact-source key, so the second
    // run starts from the state the crashed run left durable.
    .resuming_state(crashed.current_state.clone());
    plan.run_id = match fieldnotes_field_protocol::grammar::RunId::parse(RESUMED_RUN) {
        Ok(run_id) => run_id,
        Err(error) => panic!("the resumed run identifier is invalid: {error}"),
    };
    let replay = resuming.collect(&manifest, &plan);

    assert_eq!(replay.report.outcome, RunOutcome::Complete);
    assert_eq!(replay.report.records_accepted, 1);
    assert!(
        replay
            .actions
            .iter()
            .any(|action| matches!(action, CoreObservation::NoChange { seq: 1 })),
        "the replayed object reconciles to the same Note with no byte change: {:?}",
        replay.actions
    );
    assert_eq!(
        replay.last_cursor(),
        Some("walk:v1:seq=3;mtime=2026-08-22T20:05:00Z"),
        "the cursor finally passes the object that was durable before the crash"
    );
}

// ---------------------------------------------------------------------------
// Safety property 1: a cursor never advances past a failed durable write
// ---------------------------------------------------------------------------

#[test]
fn a_cursor_never_advances_past_a_failed_durable_write() {
    let case = Case::new("incremental");
    let manifest = case.manifest(LOCAL_FIELD);
    // Core's durable write for the second record does not complete, so the
    // checkpoint covering it is not eligible for commit.
    let plan = case
        .plan(LOCAL_FIELD)
        .with_durability(DurabilityPolicy::FailAt(2));
    let run = case.collect(&manifest, &plan);

    assert!(
        run.actions
            .iter()
            .any(|action| matches!(action, CoreObservation::DurableWriteFailed { seq: 2 })),
        "the harness must actually fail the write: {:?}",
        run.actions
    );
    assert!(
        run.committed_cursors.is_empty(),
        "the checkpoint covers records through 2, and 2 is not durable"
    );
    let withheld = run
        .actions
        .iter()
        .find_map(|action| match action {
            CoreObservation::WithheldCheckpoint { reason } => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        withheld.contains("never advances past an undurable write"),
        "core says why it refused: {withheld}"
    );
    assert_eq!(run.report.committed_cursor, None);
}

#[test]
fn a_cursor_may_still_advance_to_the_durable_position_when_a_later_write_fails() {
    // Failing the write for record 2 must not unmake a checkpoint that covers
    // only record 1.
    let case = Case::new("exit-before-checkpoint");
    let manifest = case.manifest(LOCAL_FIELD);
    let plan = case
        .plan(LOCAL_FIELD)
        .with_durability(DurabilityPolicy::FailAt(3));
    let run = case.collect(&manifest, &plan);

    assert_eq!(run.committed_cursors.len(), 1);
    assert_eq!(run.committed_cursors[0].covers_record_seq_through, 1);
}

// ---------------------------------------------------------------------------
// Safety property 2: absence is never deletion unless every condition holds
// ---------------------------------------------------------------------------

#[test]
fn a_complete_authoritative_snapshot_is_the_only_path_by_which_absence_removes_a_note() {
    let case = Case::new("snapshot-complete");
    let manifest = case.manifest(LOCAL_FIELD);
    let plan = case.snapshot_plan(LOCAL_FIELD, LOCAL_SCOPE);
    let run = case.collect(&manifest, &plan);

    assert_eq!(run.report.outcome, RunOutcome::Complete);
    match run.deletion() {
        DeletionAuthorization::Authorized { scope } => assert_eq!(
            scope, LOCAL_SCOPE,
            "removal is authorized for exactly the requested scope and nothing wider"
        ),
        DeletionAuthorization::Refused { reasons } => {
            panic!("a complete authoritative snapshot must authorize removal, refused: {reasons:?}")
        }
    }
}

#[test]
fn every_other_shape_of_run_refuses_deletion_by_absence_and_says_why() {
    // A partial claim, an error diagnostic, and a non-zero exit each
    // independently disqualify removal; any one is sufficient.
    let partial = Case::new("snapshot-partial");
    let manifest = partial.manifest(LOCAL_FIELD);
    let run = partial.collect(&manifest, &partial.snapshot_plan(LOCAL_FIELD, LOCAL_SCOPE));
    assert_eq!(run.report.outcome, RunOutcome::Partial);
    assert_eq!(run.exit, ExitObservation::Exited(6));
    for reason in [
        DeletionRefusal::ClaimIsPartial,
        DeletionRefusal::ErrorDiagnostic,
        DeletionRefusal::NonZeroExit,
    ] {
        assert!(
            run.deletion().refused_because(reason),
            "a partial snapshot run must refuse deletion because {reason}: {:?}",
            run.deletion()
        );
    }
    assert!(
        run.committed_cursors.len() == 1,
        "the cursor may still advance to the durable position, because a cursor records what has \
         been durably collected, not what has been proven absent"
    );

    // An incremental run is not a snapshot run at all.
    let incremental = Case::new("incremental");
    let run = incremental.incremental(LOCAL_FIELD);
    assert!(
        run.deletion()
            .refused_because(DeletionRefusal::NotSnapshotMode)
    );

    // A cancelled snapshot run reaches the same protection by a different route.
    let cancelled = Case::new("cancel");
    let manifest = cancelled.manifest(LOCAL_FIELD);
    let plan = cancelled
        .snapshot_plan(LOCAL_FIELD, LOCAL_SCOPE)
        .cancelling_after(1);
    let run = cancelled.collect(&manifest, &plan);
    assert_eq!(run.exit, ExitObservation::Exited(8));
    assert_eq!(run.report.outcome, RunOutcome::Partial);
    assert!(run.actions.contains(&CoreObservation::Cancelled));
    assert!(
        run.deletion()
            .refused_because(DeletionRefusal::ClaimIsPartial)
    );
    assert!(run.deletion().refused_because(DeletionRefusal::NonZeroExit));
}

#[test]
fn a_snapshot_claim_wider_than_the_requested_scope_is_rejected() {
    let case = Case::new("snapshot-scope-widened");
    let manifest = case.manifest(LOCAL_FIELD);
    let run = case.collect(&manifest, &case.snapshot_plan(LOCAL_FIELD, LOCAL_SCOPE));
    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::SnapshotScopeWidened)
    );
    assert!(!run.deletion().is_authorized());
}

#[test]
fn a_declared_tombstone_removes_a_note_and_an_undeclared_one_is_refused() {
    let authorized = Case::new("tombstone");
    let manifest = authorized.manifest(MAIL_FIELD);
    let run = authorized.collect(&manifest, &authorized.plan(MAIL_FIELD));
    assert_eq!(run.report.outcome, RunOutcome::Complete);
    assert!(
        run.actions
            .iter()
            .any(|action| matches!(action, CoreObservation::RemovedNote { seq: 1 })),
        "declared authority plus an explicit tombstone removes the current Note: {:?}",
        run.actions
    );
    assert_eq!(run.committed_cursors.len(), 1);

    let unauthorized = Case::new("tombstone-unauthorized");
    let manifest = unauthorized.manifest(MAIL_FIELD);
    let run = unauthorized.collect(&manifest, &unauthorized.plan(MAIL_FIELD));
    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::DeletionUnauthorized),
        "a connector cannot acquire deletion power by emitting a frame"
    );
    assert!(run.committed_cursors.is_empty());
}

// ---------------------------------------------------------------------------
// Safety property 3: an illegal artifact handle is rejected, with its own code
// ---------------------------------------------------------------------------

#[test]
fn every_hostile_artifact_reference_is_rejected_with_the_code_the_package_specifies() {
    let expectations = [
        (
            "artifact-traversal-handle",
            RejectionCode::ArtifactInvalidHandle,
        ),
        (
            "artifact-absolute-handle",
            RejectionCode::ArtifactInvalidHandle,
        ),
        (
            "artifact-device-name-handle",
            RejectionCode::ArtifactInvalidHandle,
        ),
        (
            "artifact-symlink-escape",
            RejectionCode::ArtifactInvalidHandle,
        ),
        (
            "artifact-digest-mismatch",
            RejectionCode::ArtifactDigestMismatch,
        ),
        (
            "artifact-unknown-digest",
            RejectionCode::ArtifactUnknownDigest,
        ),
        (
            "artifact-missing-staged-file",
            RejectionCode::ArtifactMissingStagedFile,
        ),
        (
            "artifact-length-mismatch",
            RejectionCode::ArtifactLengthMismatch,
        ),
    ];
    for (scenario, expected) in expectations {
        let run = Case::new(scenario).incremental(LOCAL_FIELD);
        assert_eq!(
            run.rejection_code(),
            Some(expected),
            "{scenario} must be rejected as {expected}"
        );
        assert_eq!(run.report.outcome, RunOutcome::Failed, "{scenario}");
        assert!(
            run.committed_cursors.is_empty(),
            "{scenario}: a rejected record commits no further checkpoint"
        );
    }
}

#[test]
fn a_digest_only_reference_is_accepted_only_for_bytes_the_notebook_already_stores() {
    let digest = "449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17";

    let known = Case::new("artifact-digest-only");
    let manifest = known.manifest(LOCAL_FIELD);
    let run = known.collect(
        &manifest,
        &known.plan(LOCAL_FIELD).with_known_digest(digest),
    );
    assert_eq!(run.report.outcome, RunOutcome::Complete);
    assert!(
        run.actions.iter().any(|action| matches!(
            action,
            CoreObservation::ReusedArtifact { digest: reused } if reused == digest
        )),
        "core reuses bytes it already stores and transfers nothing: {:?}",
        run.actions
    );

    // The same reference with nothing stored is rejected so the Field retries
    // with bytes.
    let unknown = Case::new("artifact-digest-only");
    let manifest = unknown.manifest(LOCAL_FIELD);
    let run = unknown.collect(&manifest, &unknown.plan(LOCAL_FIELD));
    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::ArtifactUnknownDigest)
    );
}

#[test]
fn an_artifact_past_the_run_bound_is_refused_before_anything_is_read() {
    let mut limits = fieldnotes_field_protocol::limits::Limits::ceilings();
    limits.max_artifact_bytes = 1024;
    let case = Case::new("artifact-oversized").with_limits(limits);
    let run = case.incremental(LOCAL_FIELD);
    assert_eq!(run.rejection_code(), Some(RejectionCode::ArtifactOversized));
}

// ---------------------------------------------------------------------------
// Roadmap evidence 5 continued, and safety property 4: redaction and canaries
// ---------------------------------------------------------------------------

#[test]
fn a_diagnostic_requiring_redaction_arrives_already_sanitized_and_names_what_was_removed() {
    let case = Case::new("redacted-diagnostic");
    let manifest = case.manifest(MAIL_FIELD);
    let run = case.collect(&manifest, &case.plan(MAIL_FIELD));

    assert_eq!(run.exit, ExitObservation::Exited(4));
    assert_eq!(
        run.report.outcome,
        RunOutcome::Partial,
        "an error-severity diagnostic means the run cannot be reported complete"
    );
    assert_eq!(
        run.committed_cursors.len(),
        1,
        "durable work before the failure stands"
    );

    let diagnostic = run
        .events
        .iter()
        .find_map(|event| match event {
            fieldnotes_field_protocol::message::FieldEvent::Diagnostic(frame) => {
                Some(frame.as_ref().clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the run emitted a diagnostic"));
    let named = diagnostic.redacted.unwrap_or_default();
    assert_eq!(
        named,
        vec![
            "authorization".to_owned(),
            "continuation_url".to_owned(),
            "www_authenticate_realm".to_owned()
        ],
        "the Field names what it removed so a reviewer can see that redaction happened"
    );
    let detail = diagnostic
        .detail
        .map(|detail| {
            detail
                .iter()
                .map(|(name, value)| format!("{name}={value:?}"))
                .collect::<Vec<_>>()
                .join(";")
        })
        .unwrap_or_default();
    assert!(
        detail.matches("[redacted]").count() == 3,
        "each removed value is replaced by the exact marker: {detail}"
    );
    assert!(
        detail.contains("http_status"),
        "redaction replaces values, never structure: {detail}"
    );

    // Standard error carries logs, never protocol data, and core captured it.
    assert!(
        run.raw_stderr.contains("token refresh failed"),
        "captured standard error: {}",
        run.raw_stderr
    );
}

#[test]
fn no_secret_appears_in_arguments_events_logs_or_cursors() {
    let canary = "FIXTURE-NOT-A-REAL-TOKEN-canary-9f14c0a3";
    let case = Case::new("incremental").with_secret(canary);
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(
        run.secret_locations(canary),
        Vec::<&str>::new(),
        "a canary core holds must appear in no argument, no environment entry, no event, no log \
         line, and no cursor"
    );
    // And nothing that looks like credential material reached the child at all.
    assert!(
        run.argv.iter().all(|token| !token.contains("TOKEN")),
        "argv is world-readable on common systems: {:?}",
        run.argv
    );
    assert!(
        run.environment.values().all(|value| value != canary),
        "an inherited environment leaks to grandchild processes: {:?}",
        run.environment
    );
}

#[test]
fn the_secret_canary_scan_is_not_vacuous() {
    // A canary test that cannot fail proves nothing, so two scenarios leak on
    // purpose and the scan must catch both.
    let canary = "FIXTURE-NOT-A-REAL-TOKEN-canary-negative-control";

    let in_diagnostic = Case::new("leak-secret-in-diagnostic")
        .with_secret(canary)
        .with_env(support::LEAK_VARIABLE, canary);
    let run = in_diagnostic.incremental(LOCAL_FIELD);
    let locations = run.secret_locations(canary);
    assert!(
        locations.contains(&"events"),
        "the scan must find a secret leaked into a diagnostic: {locations:?}"
    );

    let in_cursor = Case::new("leak-secret-in-cursor")
        .with_secret(canary)
        .with_env(support::LEAK_VARIABLE, canary);
    let run = in_cursor.incremental(LOCAL_FIELD);
    let locations = run.secret_locations(canary);
    assert!(
        locations.contains(&"cursors"),
        "the scan must find a secret leaked into a committed cursor: {locations:?}"
    );
}

#[test]
fn core_never_persists_raw_standard_error_and_the_ring_buffer_is_bounded() {
    let mut limits = fieldnotes_field_protocol::limits::Limits::ceilings();
    limits.max_stderr_bytes = 4096;
    let case = Case::new("stderr-flood").with_limits(limits);
    let run = case.incremental(LOCAL_FIELD);

    assert_eq!(run.report.outcome, RunOutcome::Complete);
    assert!(
        run.stderr_truncated,
        "a noisy connector cannot hold an unbounded core run"
    );
    assert!(
        run.raw_stderr.len() <= 4096,
        "captured standard error is ring-buffered at the bound, got {} bytes",
        run.raw_stderr.len()
    );
}

// ---------------------------------------------------------------------------
// The record envelope stays a normalized source envelope
// ---------------------------------------------------------------------------

#[test]
fn a_record_never_carries_a_note_id_a_hash_or_a_path_core_would_trust() {
    let case = Case::new("incremental");
    let run = case.incremental(LOCAL_FIELD);
    for event in &run.events {
        let Ok(value) = event.to_json() else {
            continue;
        };
        let text = serde_json::to_string(&value).unwrap_or_default();
        for forbidden in [
            "\"id\":",
            "\"instance_id\":",
            "\"captured_at\":",
            "\"collected_by\":",
            "\"content_hash\":",
            "\"filename\":",
        ] {
            assert!(
                !text.contains(forbidden),
                "a record is post-mapping and pre-serialization; {forbidden} is core's: {text}"
            );
        }
    }
    // The one path-shaped value a record does carry is display evidence only.
    let record = run
        .events
        .iter()
        .find_map(|event| match event {
            fieldnotes_field_protocol::message::FieldEvent::Record(frame) => Some(frame.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the run emitted a record"));
    let properties = record.properties.unwrap_or_default();
    assert!(
        properties.get("local_relative_path").is_some(),
        "the connector's own path property is retained as evidence"
    );
    let artifacts = record.artifacts.unwrap_or_default();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(
        artifacts[0].source_filename.as_deref(),
        Some("readme.md"),
        "source_filename is display metadata only: core derives the notebook path and the \
         canonical extension itself"
    );
}

#[test]
fn a_record_disposition_is_reported_for_every_accepted_record() {
    // Guards against a silent no-op path: every accepted record has to produce
    // exactly one durable-write decision.
    let case = Case::new("incremental");
    let run = case.incremental(LOCAL_FIELD);
    let decisions = run
        .actions
        .iter()
        .filter(|action| {
            matches!(
                action,
                CoreObservation::WroteNote { .. }
                    | CoreObservation::NoChange { .. }
                    | CoreObservation::RemovedNote { .. }
                    | CoreObservation::DurableWriteFailed { .. }
            )
        })
        .count();
    assert_eq!(
        u64::try_from(decisions).unwrap_or(0),
        run.report.records_accepted
    );
    // And the disposition vocabulary is the one the session exposes.
    assert_ne!(RecordDisposition::Upsert, RecordDisposition::NoChange);
    assert_eq!(COLLECT_RUN.len(), 36);
}
