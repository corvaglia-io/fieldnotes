//! Release gate R1 evidence for `sync` against the real `local` Field.
//!
//! Every case here starts `fieldnotes-field-local` as a real child process and
//! asserts what landed in a real notebook. R1's items proved here:
//!
//! - syncing twice is idempotent and produces no duplicate Notes;
//! - a changed source object rewrites one Note under its original ID and leaves
//!   no second file;
//! - a resumed run does not re-ingest what a committed cursor already covered;
//! - an injected durable-write failure leaves the cursor where it was;
//! - a partial result — here, an incremental run in which a file has
//!   disappeared — never deletes;
//! - a complete authoritative snapshot does delete, and only inside its scope;
//! - an exact-current-state rewrite creates no history Notes;
//! - the final notebook validates through `fieldnotes-format`'s own validator.
//!
//! The crash and malformed-output halves of R1 live with the fixture Field,
//! which can be driven into those states on demand.

mod sync_notebook;

use fieldnotes_app::{DurabilityPolicy, FieldRunOutcome, SyncMode, SyncOptions};
use fieldnotes_store::{StoredCursor, write_cursor};
use sync_notebook::{Case, FIELD_ID, list_of, require_complete, text_of, touch_forward};

const README: &str = "projects/rollout/readme.md";
const NOTES: &str = "projects/rollout/notes.md";

fn incremental() -> SyncOptions {
    SyncOptions::default()
}

fn snapshot() -> SyncOptions {
    SyncOptions {
        mode: SyncMode::Snapshot,
        ..SyncOptions::default()
    }
}

#[test]
fn a_first_sync_collects_notes_artifacts_and_a_committed_cursor() {
    let case = Case::new("first");
    case.write_source(
        README,
        "# Rollout reference\n\nThe checklist has three steps.\n",
    );
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");

    let report = case.sync_one(&incremental());
    require_complete(&report);
    assert_eq!(report.counts.created, 2);
    assert_eq!(report.counts.updated, 0);
    assert_eq!(report.counts.artifacts_stored, 2);
    assert!(report.cursor_committed);

    // The Note carries the hoisted portable source key and core's own artifact
    // identity, and its body links to the derived notebook-relative path.
    let (_, record) = case.note_for("file/projects/rollout/readme.md");
    assert_eq!(text_of(&record, "field_id").as_deref(), Some(FIELD_ID));
    assert!(text_of(&record, "source_scope").is_some());
    assert_eq!(list_of(&record, "artifacts").len(), 1);
    assert!(
        record
            .body()
            .contains("retained at `../artifacts/artifact_sha256_")
    );

    // The cursor is recorded with the format version it was committed at.
    let cursor = case
        .cursor()
        .unwrap_or_else(|| panic!("a cursor must have been committed"));
    assert_eq!(cursor.cursor_format_version, 1);
    assert!(!cursor.cursor.is_empty());
}

#[test]
fn syncing_twice_is_idempotent_and_produces_no_duplicate_notes() {
    let case = Case::new("idempotent");
    case.write_source(
        README,
        "# Rollout reference\n\nThe checklist has three steps.\n",
    );
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");
    require_complete(&case.sync_one(&incremental()));

    let notes_before = case.notes();
    let artifacts_before = case.artifacts();
    assert_eq!(notes_before.len(), 2);

    // An incremental replay: the cursor filters everything, so nothing is even
    // re-emitted.
    let second = case.sync_one(&incremental());
    require_complete(&second);
    assert_eq!(second.counts.created, 0);
    assert_eq!(second.counts.updated, 0);

    // A snapshot replay: every object *is* re-emitted, and every one of them is
    // recognised as the notebook's current state, so nothing is rewritten. This
    // is the cross-run idempotence the store, not the protocol boundary,
    // guarantees.
    let third = case.sync_one(&snapshot());
    require_complete(&third);
    assert_eq!(third.counts.records_accepted, 2);
    assert_eq!(third.counts.unchanged, 2);
    assert_eq!(third.counts.updated, 0);
    assert_eq!(third.counts.created, 0);

    assert_eq!(
        case.notes(),
        notes_before,
        "a replay must leave the notebook byte-for-byte identical"
    );
    assert_eq!(case.artifacts(), artifacts_before);
}

#[test]
fn a_changed_source_object_rewrites_one_note_under_its_original_id() {
    let case = Case::new("changed");
    let path = case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");
    require_complete(&case.sync_one(&incremental()));

    let (first_name, first_record) = case.note_for("file/projects/rollout/readme.md");
    let original_id = first_record.id().to_string();

    // Change the object upstream, moving its event time forward so the
    // UTC event-time filename changes too.
    case.write_source(README, "# Rollout reference\n\nFour steps now.\n");
    touch_forward(&path, 3600);

    let report = case.sync_one(&incremental());
    require_complete(&report);
    assert_eq!(report.counts.updated, 1);
    assert_eq!(report.counts.created, 0);
    assert_eq!(report.counts.renamed, 1);

    // Exactly one Note still holds that portable source key, under the same ID,
    // and no history Note was left behind.
    let (second_name, second_record) = case.note_for("file/projects/rollout/readme.md");
    assert_eq!(second_record.id().to_string(), original_id);
    assert_ne!(second_name, first_name, "the event-time filename changed");
    assert_eq!(case.notes().len(), 2, "no second Note for the same object");
    assert!(
        !case.notes().iter().any(|(name, _)| *name == first_name),
        "the superseded filename must be gone once the replacement is durable"
    );
    assert!(second_record.body().contains("Four steps now."));
}

#[test]
fn a_resumed_run_does_not_re_ingest_what_a_committed_cursor_covered() {
    let case = Case::new("resume");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");
    let first = case.sync_one(&incremental());
    require_complete(&first);
    assert_eq!(first.counts.records_accepted, 2);

    let second = case.sync_one(&incremental());
    require_complete(&second);
    assert_eq!(
        second.counts.records_accepted, 0,
        "a run resuming from a committed cursor re-ingests nothing"
    );
    assert!(second.cursor_committed);
}

#[test]
fn an_injected_durable_write_failure_leaves_the_cursor_where_it_was() {
    let case = Case::new("durability");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");

    let failing = SyncOptions {
        durability: DurabilityPolicy::FailAt(1),
        ..SyncOptions::default()
    };
    let report = case.sync_one(&failing);
    assert_eq!(report.counts.durable_write_failures, 1);
    assert!(report.failure.is_some(), "the run must report why");
    assert!(
        !report.cursor_committed,
        "the cursor must not advance past an undurable write"
    );
    assert_eq!(case.cursor(), None);
    assert!(
        case.notes().is_empty(),
        "no Note was installed for the failed write"
    );

    // Nothing was lost: the next run collects the same objects from scratch.
    let recovered = case.sync_one(&incremental());
    require_complete(&recovered);
    assert_eq!(recovered.counts.created, 2);
    assert!(recovered.cursor_committed);
}

#[test]
fn a_durable_write_failure_after_a_successful_one_still_freezes_the_cursor() {
    let case = Case::new("durability-mid");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");

    // The second record's write does not complete, so the checkpoint covering
    // both records is never eligible and the cursor stays absent — even though
    // the first record's Note is durable and stays.
    let failing = SyncOptions {
        durability: DurabilityPolicy::FailAt(2),
        ..SyncOptions::default()
    };
    let report = case.sync_one(&failing);
    assert_eq!(report.counts.durable_write_failures, 1);
    assert_eq!(report.counts.created, 1);
    assert!(!report.cursor_committed);
    assert_eq!(case.cursor(), None);
    assert_eq!(
        case.notes().len(),
        1,
        "durable work before the failure stands"
    );
}

#[test]
fn an_incremental_run_never_deletes_a_note_whose_source_disappeared() {
    let case = Case::new("partial-no-delete");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");
    require_complete(&case.sync_one(&incremental()));
    assert_eq!(case.notes().len(), 2);

    case.remove_source(NOTES);
    let report = case.sync_one(&incremental());
    require_complete(&report);
    assert_eq!(report.counts.removed_by_snapshot, 0);
    assert_eq!(report.counts.removed_by_tombstone, 0);
    assert!(
        report.deletion.authorized_scope.is_none(),
        "an incremental run never authorizes deletion by absence"
    );
    assert!(
        report
            .deletion
            .refusals
            .iter()
            .any(|reason| reason.contains("not a snapshot run"))
    );
    assert_eq!(
        case.notes().len(),
        2,
        "absence in an incremental run is never evidence of deletion"
    );
}

#[test]
fn a_complete_authoritative_snapshot_removes_the_note_whose_source_is_gone() {
    let case = Case::new("snapshot-delete");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    case.write_source(NOTES, "# Notes\n\nMeeting notes.\n");
    require_complete(&case.sync_one(&incremental()));
    assert_eq!(case.notes().len(), 2);

    case.remove_source(NOTES);
    let report = case.sync_one(&snapshot());
    require_complete(&report);
    assert!(
        report.deletion.authorized_scope.is_some(),
        "a complete snapshot for the requested scope authorizes removal: {:?}",
        report.deletion.refusals
    );
    assert_eq!(report.counts.removed_by_snapshot, 1);
    assert_eq!(case.notes().len(), 1);
    // No tombstone Note and no revision entry: deletion removes the Note.
    let (_, record) = case.note_for("file/projects/rollout/readme.md");
    assert_eq!(text_of(&record, "type").as_deref(), Some("file"));
}

#[test]
fn a_snapshot_refuses_when_no_scope_can_be_named() {
    let case = Case::new("snapshot-no-scope");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");

    // No Note exists yet, so core has nothing to infer the Field's run-time
    // source scope from, and a snapshot claim it could not check is refused
    // rather than guessed.
    let report = case.sync_one(&snapshot());
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    let failure = report
        .failure
        .unwrap_or_else(|| panic!("the refusal must be reported"));
    assert!(
        failure.contains("must name the scope"),
        "unexpected refusal: {failure}"
    );
    assert!(case.notes().is_empty());
}

#[test]
fn an_oversize_attachment_lands_in_skipped_attachments_and_links_to_its_source() {
    let case = Case::new("skipped-size");
    case.write_source(
        README,
        "# Rollout reference\n\nThis body is well past ten bytes.\n",
    );

    let tiny = SyncOptions {
        max_artifact_bytes: Some(10),
        ..SyncOptions::default()
    };
    let report = case.sync_one(&tiny);
    require_complete(&report);
    assert_eq!(report.counts.attachments_skipped, 1);
    assert_eq!(report.counts.artifacts_stored, 0);

    let (_, record) = case.note_for("file/projects/rollout/readme.md");
    assert_eq!(
        list_of(&record, "skipped_attachments"),
        vec!["file/projects/rollout/readme.md".to_owned()],
        "the declined attachment's stable reference is projected onto the approved property"
    );
    assert!(list_of(&record, "artifacts").is_empty());
    assert!(
        record.body().contains("not retained; stays at its source"),
        "the body link targets the source when the bytes were not retained"
    );
    assert!(!record.body().contains("../artifacts/"));
}

#[test]
fn a_type_excluded_attachment_lands_in_skipped_attachments() {
    let case = Case::new("skipped-type");
    case.write_source(README, "# Rollout reference\n\nPlain text evidence.\n");

    // `text/plain` is outside this run's include set, so the Field declines it
    // rather than staging bytes core would have to reject.
    let pdf_only = SyncOptions {
        artifact_media_types: Some(vec!["application/pdf".to_owned()]),
        ..SyncOptions::default()
    };
    let report = case.sync_one(&pdf_only);
    require_complete(&report);
    assert_eq!(report.counts.attachments_skipped, 1);
    assert_eq!(report.counts.artifacts_stored, 0);
    let (_, record) = case.note_for("file/projects/rollout/readme.md");
    assert_eq!(list_of(&record, "skipped_attachments").len(), 1);
}

#[test]
fn a_cursor_stored_at_another_format_version_is_not_replayed_and_reports_a_gap() {
    let case = Case::new("cursor-format");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    require_complete(&case.sync_one(&incremental()));

    // Hand-write a cursor at a format version the Field does not declare. Core
    // must start unbounded and say so, rather than handing the Field a token it
    // may misread.
    write_cursor(
        case.notebook(),
        FIELD_ID,
        &StoredCursor {
            cursor: "local-walk/v99;opaque".to_owned(),
            cursor_format_version: 99,
            covers_record_seq_through: 7,
            committed_at: "2026-08-22T09:45:00+00:00".to_owned(),
        },
    )
    .unwrap_or_else(|error| panic!("the cursor must be writable: {error}"));

    let report = case.sync_one(&incremental());
    require_complete(&report);
    assert!(
        report.cursor_recovery_gap,
        "the recovery gap must be reported"
    );
    assert_eq!(
        report.counts.records_accepted, 1,
        "an unbounded run re-reports the object"
    );
    assert_eq!(
        report.counts.unchanged, 1,
        "and reconciliation makes it a no-op"
    );
    assert_eq!(case.notes().len(), 1);
}

#[test]
fn every_note_a_sync_writes_validates_through_the_public_validator() {
    let case = Case::new("validates");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    case.write_source(
        "projects/rollout/spec.pdf",
        "%PDF-1.7\n1 0 obj\ntrailer\n%%EOF\n",
    );
    case.write_source("inbox/scratch.txt", "Plain text.\n");
    require_complete(&case.sync_one(&incremental()));

    // `validated_notes` parses and validates every file with the format crate's
    // own public validator, so the bytes core wrote are provably canonical.
    let notes = case.validated_notes();
    assert_eq!(notes.len(), 3);
    for (name, record) in &notes {
        assert!(
            text_of(record, "content_hash").is_some(),
            "{name} must carry a content hash"
        );
        assert!(
            text_of(record, "source_identity").is_some(),
            "{name} must carry its portable source identity"
        );
    }
}

#[test]
fn a_disabled_field_is_skipped_rather_than_run() {
    let case = Case::new("disabled");
    case.write_source(README, "# Rollout reference\n\nThree steps.\n");
    let mut config = fieldnotes_store::read_field_config(case.notebook(), FIELD_ID)
        .unwrap_or_else(|error| panic!("the configuration must be readable: {error}"))
        .unwrap_or_else(|| panic!("the Field must be configured"));
    config.enabled = false;
    fieldnotes_store::write_field_config(case.notebook(), &config)
        .unwrap_or_else(|error| panic!("the configuration must be writable: {error}"));

    let report = case.sync_one(&incremental());
    assert_eq!(report.outcome, FieldRunOutcome::Skipped);
    assert!(case.notes().is_empty());
}
