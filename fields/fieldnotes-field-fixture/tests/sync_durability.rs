//! Release gate R1 evidence for `sync`'s crash safety and untrusted-output
//! containment, driven through the fixture Field as a real child process.
//!
//! R1 items proved here:
//!
//! - kill-and-resume at every checkpoint boundary: a crash **before** the
//!   checkpoint covering a durable record, a crash **after** a committed
//!   checkpoint, and a non-zero exit after a durable write but before its
//!   checkpoint — with replay from each boundary changing no notebook byte;
//! - malformed child output containment: every malformed shape, every hostile
//!   artifact reference, and every declared-property and capability violation
//!   leaves the notebook byte-identical and reports the specified code;
//! - authoritative deletion: a declared tombstone removes a Note, an
//!   undeclared one is rejected, a complete snapshot removes only inside its
//!   declared scope, and a partial one removes nothing;
//! - duplicate-source handling within a run, and in-run divergence rejected as
//!   a Field defect;
//! - a manifest migration blocking sync rather than retyping notebook data.

mod sync_notebook;

use fieldnotes_app::{FieldRunOutcome, SyncOptions};
use sync_notebook::{Case, LOCAL_SCOPE, list_of, require_complete, require_rejection, text_of};

const README_KEY: &str = "file/projects/rollout/readme.md";
const AGREEMENT_KEY: &str = "document/contracts/2026-08-msa.md";
const TIMELINE_KEY: &str = "file/projects/rollout/timeline.md";

#[test]
fn an_incremental_run_installs_notes_artifacts_and_a_committed_cursor() {
    let case = Case::new("incremental");
    let report = case.run("incremental");
    require_complete(&report);
    assert_eq!(report.counts.created, 2);
    assert_eq!(report.counts.artifacts_stored, 1);
    assert!(report.cursor_committed);
    assert_eq!(report.cursor_coverage, Some(2));
    assert_eq!(case.last_sync_outcome().as_deref(), Some("complete"));

    // The declared prefixed properties arrive with their declared types: a
    // date stays a date, an opaque source flag stays text, and a set-like list
    // is deduplicated and sorted by core.
    let agreement = case.note_for(AGREEMENT_KEY);
    assert_eq!(
        text_of(&agreement, "local_document_flag").as_deref(),
        Some("true"),
        "an opaque source flag is never coerced to a boolean"
    );
    assert_eq!(
        list_of(&agreement, "local_tags"),
        vec!["contracts".to_owned(), "legal".to_owned()]
    );
    assert!(agreement.get("local_document_date").is_some());
}

#[test]
fn a_crash_before_the_covering_checkpoint_keeps_committed_state_and_replays_clean() {
    let case = Case::new("crash-before");
    // The scenario emits a record, a checkpoint covering it, a second record,
    // and then aborts: the second record is durable but no checkpoint covers it.
    let crashed = case.run("crash-before-checkpoint");
    assert_eq!(crashed.outcome, FieldRunOutcome::Failed);
    let cursor = case
        .cursor()
        .unwrap_or_else(|| panic!("the committed checkpoint must have advanced the cursor"));
    assert_eq!(
        cursor.covers_record_seq_through, 1,
        "the cursor lags the durable record it does not cover"
    );
    assert!(case.has_note_for(README_KEY));
    assert!(
        case.has_note_for(AGREEMENT_KEY),
        "a durable write before the crash stands"
    );

    let after_crash = case.notebook_state();

    // The next run replays from the committed cursor. The Field re-emits the
    // object the cursor did not cover, and reconciliation through the portable
    // exact-source key makes the replay a no-op.
    let resumed = case.run("resume-after-crash");
    require_complete(&resumed);
    assert_eq!(resumed.counts.records_accepted, 1);
    assert_eq!(resumed.counts.unchanged, 1);
    assert_eq!(resumed.counts.updated, 0);
    assert_eq!(
        case.notebook_state(),
        after_crash,
        "replay from a checkpoint boundary changes no notebook byte"
    );
}

#[test]
fn a_crash_after_a_committed_checkpoint_commits_that_cursor_and_nothing_beyond() {
    let case = Case::new("crash-after");
    let crashed = case.run("crash-after-checkpoint");
    assert_eq!(crashed.outcome, FieldRunOutcome::Failed);
    let cursor = case
        .cursor()
        .unwrap_or_else(|| panic!("the checkpoint committed before the crash stands"));
    assert_eq!(cursor.covers_record_seq_through, 1);
    assert_eq!(case.validated_notes().len(), 1);

    let before = case.notebook_state();
    let resumed = case.run("resume-after-crash");
    require_complete(&resumed);
    assert_eq!(
        resumed.counts.created, 1,
        "the object the cursor did not cover"
    );
    assert_ne!(case.notebook_state(), before);
    // And replaying that resume is itself a no-op.
    let stable = case.notebook_state();
    let again = case.run("resume-after-crash");
    require_complete(&again);
    assert_eq!(again.counts.unchanged, 1);
    assert_eq!(case.notebook_state(), stable);
}

#[test]
fn a_non_zero_exit_after_a_durable_write_keeps_the_lagging_cursor() {
    let case = Case::new("exit-before");
    let report = case.run("exit-before-checkpoint");
    // Exit 1 with durable work is a partial run, not a failed one.
    assert_eq!(report.outcome, FieldRunOutcome::Partial);
    assert_eq!(report.exit, "exited 1");
    let cursor = case
        .cursor()
        .unwrap_or_else(|| panic!("the committed checkpoint stands"));
    assert_eq!(cursor.covers_record_seq_through, 1);
    assert_eq!(case.validated_notes().len(), 2);
    assert!(
        report.deletion.authorized_scope.is_none(),
        "a non-zero exit independently disqualifies deletion by absence"
    );
}

#[test]
fn a_duplicate_record_within_one_run_writes_exactly_one_note() {
    let case = Case::new("duplicate-replay");
    let report = case.run("duplicate-replay");
    require_complete(&report);
    assert_eq!(report.counts.records_accepted, 2);
    assert_eq!(report.counts.created, 1);
    assert_eq!(report.counts.unchanged, 1);
    assert_eq!(case.validated_notes().len(), 1);
}

#[test]
fn in_run_divergence_for_one_source_key_is_rejected_as_a_field_defect() {
    let case = Case::new("duplicate-divergent");
    let report = case.run("duplicate-divergent");
    require_rejection(&report, "record.duplicate_divergent_in_run");
    // The first assertion was durable before the second arrived, and durable
    // work before a failure stands.
    assert_eq!(case.validated_notes().len(), 1);
    assert_eq!(case.cursor(), None, "a rejected run commits no checkpoint");
}

/// Every malformed, hostile, or contract-violating shape whose **first** frame
/// is the violation, so nothing durable precedes it, with the code core must
/// reject it with.
const CONTAINED_VIOLATIONS: [(&str, &str); 18] = [
    ("malformed-unknown-event", "protocol.unknown_event"),
    ("malformed-invalid-utf8", "protocol.invalid_utf8"),
    ("malformed-oversized-frame", "protocol.oversized_frame"),
    ("malformed-truncated-frame", "protocol.truncated_frame"),
    ("artifact-traversal-handle", "artifact.invalid_handle"),
    ("artifact-absolute-handle", "artifact.invalid_handle"),
    ("artifact-device-name-handle", "artifact.invalid_handle"),
    ("artifact-symlink-escape", "artifact.not_regular_file"),
    ("artifact-digest-mismatch", "artifact.digest_mismatch"),
    ("artifact-unknown-digest", "artifact.unknown_digest"),
    (
        "artifact-missing-staged-file",
        "artifact.missing_staged_file",
    ),
    ("artifact-oversized", "artifact.oversized"),
    ("artifact-length-mismatch", "artifact.length_mismatch"),
    ("property-undeclared", "record.undeclared_property"),
    ("property-foreign-prefix", "record.foreign_prefix"),
    ("property-unknown", "record.unknown_property"),
    ("note-type-invalid", "record.invalid_note_type"),
    ("capability-undeclared", "manifest.undeclared_capability"),
];

#[test]
fn malformed_and_hostile_child_output_leaves_the_notebook_byte_identical() {
    for (scenario, code) in CONTAINED_VIOLATIONS {
        let case = Case::new(&format!("contained-{scenario}"));
        // A seeded Note, so "byte-identical" is a claim about real content
        // rather than about an empty directory.
        require_complete(&case.run("incremental"));
        let before = case.notebook_state();
        let cursor_before = case.cursor();
        assert!(!before.is_empty());

        let report = case.run(scenario);
        require_rejection(&report, code);
        assert_eq!(
            case.notebook_state(),
            before,
            "{scenario} must leave the notebook byte-identical"
        );
        assert_eq!(
            case.cursor(),
            cursor_before,
            "{scenario} must commit no further checkpoint"
        );
    }
}

/// Sequence violations that arrive after one legitimate record, so a Note is
/// durable but no checkpoint ever covered it.
const SEQUENCE_VIOLATIONS_AFTER_A_RECORD: [(&str, &str); 2] = [
    ("malformed-duplicate-seq", "protocol.duplicate_seq"),
    ("malformed-seq-gap", "protocol.seq_gap"),
];

#[test]
fn a_sequence_violation_after_a_durable_record_advances_no_cursor() {
    for (scenario, code) in SEQUENCE_VIOLATIONS_AFTER_A_RECORD {
        let case = Case::new(&format!("sequence-{scenario}"));
        let report = case.run(scenario);
        require_rejection(&report, code);
        assert_eq!(
            case.validated_notes().len(),
            1,
            "{scenario}: the record accepted before the violation is durable"
        );
        assert_eq!(
            case.cursor(),
            None,
            "{scenario}: no checkpoint covered it, so no cursor advanced"
        );
    }
}

/// Violations that legitimately follow a committed checkpoint: the durable work
/// and the cursor that covered it stand, and only the run fails.
const VIOLATIONS_AFTER_A_COMMIT: [(&str, &str); 2] = [
    ("malformed-not-json", "protocol.not_json"),
    ("malformed-seq-regression", "protocol.seq_regression"),
];

#[test]
fn a_violation_after_a_committed_checkpoint_keeps_that_checkpoint() {
    for (scenario, code) in VIOLATIONS_AFTER_A_COMMIT {
        let case = Case::new(&format!("after-commit-{scenario}"));
        let report = case.run(scenario);
        require_rejection(&report, code);
        let cursor = case.cursor().unwrap_or_else(|| {
            panic!("{scenario} committed a checkpoint before the violation, which must stand")
        });
        assert_eq!(cursor.covers_record_seq_through, 1);
        assert_eq!(case.validated_notes().len(), 1);
    }
}

#[test]
fn a_property_type_mismatch_and_a_malformed_date_are_reported_with_their_own_codes() {
    for (scenario, code) in [
        ("property-type-mismatch", "record.property_type_mismatch"),
        ("property-invalid-date", "record.invalid_date"),
        ("property-derived-record-only", "record.unknown_property"),
        ("property-core-owned", "protocol.schema_invalid"),
        ("note-type-not-declared", "record.note_type_not_declared"),
    ] {
        let case = Case::new(&format!("typed-{scenario}"));
        let report = case.run(scenario);
        require_rejection(&report, code);
        assert!(
            case.validated_notes().is_empty(),
            "{scenario} must leave no partial Note"
        );
    }
}

#[test]
fn an_authoritative_tombstone_removes_the_note_and_writes_no_tombstone_record() {
    let case = Case::new("tombstone");
    // Seed three Notes, including the one the tombstone names.
    require_complete(&case.run("resume"));
    assert!(case.has_note_for(TIMELINE_KEY));
    let before = case.validated_notes().len();

    let report = case.run("tombstone-local");
    require_complete(&report);
    assert_eq!(report.counts.removed_by_tombstone, 1);
    assert!(!case.has_note_for(TIMELINE_KEY));
    assert_eq!(
        case.validated_notes().len(),
        before - 1,
        "no tombstone Note and no revision entry is written; deletion removes the Note"
    );
}

#[test]
fn a_tombstone_from_a_field_without_declared_authority_is_rejected() {
    let case = Case::new("tombstone-unauthorized");
    require_complete(&case.run("resume"));
    let before = case.notebook_state();

    let report = case.run("tombstone-local-unauthorized");
    require_rejection(&report, "deletion.unauthorized");
    assert_eq!(
        case.notebook_state(),
        before,
        "a connector cannot acquire deletion power by emitting a frame"
    );
}

#[test]
fn a_complete_authoritative_snapshot_removes_only_inside_its_declared_scope() {
    let case = Case::new("snapshot-complete");
    // Seed three Notes; the snapshot reports only two of them.
    require_complete(&case.run("resume"));
    assert!(case.has_note_for(TIMELINE_KEY));

    let report = case.run_snapshot("snapshot-complete", LOCAL_SCOPE);
    require_complete(&report);
    assert_eq!(
        report.deletion.authorized_scope.as_deref(),
        Some(LOCAL_SCOPE),
        "refusals were {:?}",
        report.deletion.refusals
    );
    assert_eq!(report.counts.removed_by_snapshot, 1);
    assert!(!case.has_note_for(TIMELINE_KEY));
    assert!(case.has_note_for(README_KEY));
    assert!(case.has_note_for(AGREEMENT_KEY));
}

#[test]
fn a_partial_snapshot_removes_nothing() {
    let case = Case::new("snapshot-partial");
    require_complete(&case.run("resume"));
    let before = case.validated_notes().len();
    assert_eq!(before, 3);

    let report = case.run_snapshot("snapshot-partial", LOCAL_SCOPE);
    assert_ne!(report.outcome, FieldRunOutcome::Complete);
    assert!(report.deletion.authorized_scope.is_none());
    assert!(
        report
            .deletion
            .refusals
            .iter()
            .any(|reason| reason.contains("partial")),
        "refusals were {:?}",
        report.deletion.refusals
    );
    assert_eq!(report.counts.removed_by_snapshot, 0);
    assert_eq!(
        case.validated_notes().len(),
        before,
        "absence from a partial result is never evidence of deletion"
    );
}

#[test]
fn a_snapshot_claim_wider_than_the_requested_scope_is_rejected() {
    let case = Case::new("snapshot-widened");
    require_complete(&case.run("resume"));
    let before = case.notebook_state();

    let report = case.run_snapshot("snapshot-scope-widened", LOCAL_SCOPE);
    require_rejection(&report, "snapshot.scope_widened");
    assert_eq!(case.notebook_state(), before);
}

#[test]
fn a_declined_attachment_lands_in_skipped_attachments_and_retains_no_bytes() {
    let case = Case::new("not-retained");
    let report = case.run("artifact-not-retained");
    require_complete(&report);
    assert_eq!(report.counts.attachments_skipped, 1);
    assert_eq!(report.counts.artifacts_stored, 0);

    let record = case.note_for(README_KEY);
    assert_eq!(
        list_of(&record, "skipped_attachments"),
        vec!["file-attachment/full-export-zip-01".to_owned()]
    );
    assert!(list_of(&record, "artifacts").is_empty());
    assert!(record.body().contains("not retained; stays at its source"));
}

#[test]
fn a_hostile_field_staging_a_type_excluded_attachment_is_rejected() {
    // The mail-flavor manifest requires authentication, which `0.1.1` refuses,
    // so the hostile-type case is exercised against the local flavor by
    // narrowing the run's include set instead: `text/markdown` is then outside
    // it, and the Field stages it anyway.
    let case = Case::new("type-excluded-hostile");
    let report = case.run_with(
        "incremental",
        SyncOptions {
            artifact_media_types: Some(vec!["application/pdf".to_owned()]),
            ..SyncOptions::default()
        },
    );
    require_rejection(&report, "artifact.type_excluded");
    assert!(
        case.validated_notes().is_empty(),
        "a rejected record leaves no partial Note"
    );
}

#[test]
fn a_version_mismatch_fails_closed_and_names_both_version_sets() {
    let case = Case::new("version-mismatch");
    let report = case.run("describe-version-mismatch");
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    let failure = report
        .failure
        .unwrap_or_else(|| panic!("the mismatch must be reported"));
    assert!(
        failure.contains("no manifest"),
        "unexpected failure: {failure}"
    );
    assert!(
        failure.contains("core offered") && failure.contains("[2, 3]"),
        "the message must name both version sets: {failure}"
    );
    assert!(case.validated_notes().is_empty());
    assert_eq!(case.cursor(), None);
}

#[test]
fn a_changed_cursor_format_version_is_a_migration_that_blocks_sync() {
    let case = Case::new("migration-cursor");
    require_complete(&case.run("incremental"));
    let before = case.notebook_state();

    let report = case.run("describe-cursor-format-2");
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    let failure = report
        .failure
        .unwrap_or_else(|| panic!("the migration must be reported"));
    assert!(
        failure.contains("cursor_format_changed"),
        "unexpected failure: {failure}"
    );
    assert_eq!(case.notebook_state(), before);
}

#[test]
fn a_retyped_declared_property_is_a_migration_that_blocks_sync() {
    let case = Case::new("migration-retype");
    require_complete(&case.run("incremental"));
    let before = case.notebook_state();

    let report = case.run("describe-retyped-property");
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    let failure = report
        .failure
        .unwrap_or_else(|| panic!("the migration must be reported"));
    assert!(
        failure.contains("property_type_changed"),
        "unexpected failure: {failure}"
    );
    assert_eq!(case.notebook_state(), before);
}

#[test]
fn a_field_that_needs_authentication_is_refused_actionably() {
    // The mail-flavor manifest declares an OAuth authorization-code flow and
    // requires both a credential profile and the protected channel, none of
    // which exists before the `0.1.3` authentication gate.
    let case = Case::with_field("auth-refused", "outlook_mail", "work");
    let report = case.run("redacted-diagnostic");
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    let failure = report
        .failure
        .unwrap_or_else(|| panic!("the refusal must be reported"));
    assert!(
        failure.contains("needs authentication") && failure.contains("0.1.3"),
        "unexpected refusal: {failure}"
    );
    assert!(case.validated_notes().is_empty());
}

#[test]
fn a_hung_child_is_bounded_by_the_idle_timeout_and_writes_nothing() {
    let case = Case::new("hang");
    let report = case.run_with(
        "hang",
        SyncOptions {
            idle_seconds: Some(1),
            ..SyncOptions::default()
        },
    );
    require_rejection(&report, "protocol.idle_timeout");
    assert!(case.validated_notes().is_empty());
    assert_eq!(case.cursor(), None);
}

#[test]
fn a_standard_error_flood_is_captured_bounded_and_reported() {
    let case = Case::new("stderr-flood");
    let report = case.run("stderr-flood");
    require_complete(&report);
    let stderr = report
        .stderr
        .unwrap_or_else(|| panic!("the captured standard error must be reported"));
    assert!(
        stderr.contains("standard error was truncated"),
        "the ring buffer overflow must be surfaced"
    );
    assert_eq!(case.validated_notes().len(), 1);
}

#[test]
fn a_field_configured_under_a_mismatched_stem_is_refused() {
    // The fixture's local flavor declares stem `local`; configuring it under
    // `jira` would let its `local_`-prefixed properties land on a `jira_` Note.
    let case = Case::with_field("stem-mismatch", "jira", "acme");
    let report = case.run("incremental");
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    let failure = report
        .failure
        .unwrap_or_else(|| panic!("the mismatch must be reported"));
    assert!(
        failure.contains("declares stem `local`"),
        "unexpected refusal: {failure}"
    );
    assert!(case.validated_notes().is_empty());
}

#[test]
fn a_diagnostic_is_surfaced_with_its_severity_code_and_redaction_intact() {
    let case = Case::new("diagnostic");
    // The `resume` scenario reports a skipped object as an `info` diagnostic
    // before any record, which is exactly the shape that would defeat a naive
    // contiguous-`seq` durability watermark.
    require_complete(&case.run("incremental"));
    let report = case.run("resume");
    require_complete(&report);
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.severity, "info");
    assert_eq!(diagnostic.code, "content.skipped");
    assert!(
        report.cursor_committed,
        "a diagnostic before a record must not stop the cursor"
    );
    assert_eq!(report.cursor_coverage, Some(2));
}
