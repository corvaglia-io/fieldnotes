//! Executable conformance cases for the `local` Field, driven against the
//! real compiled binary as a child process through the reusable protocol
//! conformance kit -- the same harness that validates the fixture Field.

mod support;

use std::fs;

use fieldnotes_field_protocol::codes::{RejectionCode, RunOutcome};
use fieldnotes_field_protocol::conformance::manifest_snapshot;

use support::{Case, record_events, touch_forward};

#[test]
fn describe_reports_a_complete_self_declaration() {
    let case = Case::new("describe");
    let manifest = case.manifest();

    assert_eq!(manifest.field_stem.as_str(), "local");
    assert_eq!(
        manifest
            .property_prefix
            .as_ref()
            .map(|prefix| prefix.as_str()),
        Some("local_")
    );
    assert_eq!(manifest.declared_properties.len(), 5);
    assert_eq!(manifest.capabilities.len(), 2);
    assert_eq!(
        manifest.collection.deletion.snapshot,
        fieldnotes_field_protocol::message::SnapshotAuthority::Authoritative
    );
    assert_eq!(
        manifest.collection.deletion.tombstones,
        fieldnotes_field_protocol::message::TombstoneAuthority::Unsupported
    );
    assert!(
        manifest
            .collection
            .supported_modes
            .contains(&fieldnotes_field_protocol::message::CollectionMode::Snapshot)
    );
}

#[test]
fn a_successful_incremental_run_emits_a_record_and_a_checkpoint() {
    let case = Case::new("incremental-basic");
    fs::write(case.root_dir().join("readme.md"), b"# Hello\n\nWorld.\n")
        .unwrap_or_else(|error| panic!("write: {error}"));

    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.incremental_plan(support::COLLECT_RUN));

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "rejection: {:?}",
        run.rejection
    );
    assert_eq!(run.report.records_accepted, 1);
    assert!(
        run.last_cursor().is_some(),
        "a successful run must commit a checkpoint"
    );
    let records = record_events(&run);
    assert_eq!(records[0].source.identity.as_str(), "file/readme.md");
    assert_eq!(
        records[0].note_type.as_ref().map(|t| t.as_str()),
        Some("file")
    );
}

#[test]
fn resumption_from_a_committed_cursor_does_not_re_emit_a_settled_file() {
    let case = Case::new("resume");
    fs::write(case.root_dir().join("a.txt"), b"unchanged")
        .unwrap_or_else(|error| panic!("write: {error}"));

    let manifest = case.manifest();
    let first = case.collect(&manifest, &case.incremental_plan(support::COLLECT_RUN));
    assert_eq!(first.report.records_accepted, 1);
    let cursor = first
        .last_cursor()
        .unwrap_or_else(|| panic!("a cursor must be committed"))
        .to_owned();

    let plan = support::with_cursor(case.incremental_plan(support::RESUME_RUN), &cursor, 1);
    let second = case.collect(&manifest, &plan);

    assert_eq!(
        second.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        second.rejection
    );
    assert_eq!(
        second.report.records_accepted, 0,
        "an unchanged file must not be re-emitted on resumption"
    );
}

#[test]
fn a_changed_file_produces_an_updated_record_for_the_same_source_key() {
    let case = Case::new("changed-file");
    let path = case.root_dir().join("notes.txt");
    fs::write(&path, b"first version").unwrap_or_else(|error| panic!("write: {error}"));

    let manifest = case.manifest();
    let first = case.collect(&manifest, &case.incremental_plan(support::COLLECT_RUN));
    assert_eq!(first.report.records_accepted, 1);
    let first_key = (
        record_events(&first)[0].source.scope.as_str().to_owned(),
        record_events(&first)[0].source.identity.as_str().to_owned(),
    );
    let cursor = first
        .last_cursor()
        .unwrap_or_else(|| panic!("a cursor must be committed"))
        .to_owned();

    fs::write(&path, b"second version, with different content entirely")
        .unwrap_or_else(|error| panic!("write: {error}"));
    touch_forward(&path, 5);

    let plan = support::with_cursor(case.incremental_plan(support::RESUME_RUN), &cursor, 1);
    let second = case.collect(&manifest, &plan);

    assert_eq!(
        second.report.records_accepted, 1,
        "the changed file must be re-emitted"
    );
    let second_key = (
        record_events(&second)[0].source.scope.as_str().to_owned(),
        record_events(&second)[0]
            .source
            .identity
            .as_str()
            .to_owned(),
    );
    assert_eq!(
        first_key, second_key,
        "the same source object must keep the same portable exact-source key"
    );
}

#[test]
fn a_complete_snapshot_authorizes_deletion_of_a_file_removed_from_the_root() {
    let case = Case::new("snapshot-delete");
    fs::write(case.root_dir().join("keep.txt"), b"keep")
        .unwrap_or_else(|error| panic!("write: {error}"));
    fs::write(case.root_dir().join("remove.txt"), b"remove")
        .unwrap_or_else(|error| panic!("write: {error}"));

    let manifest = case.manifest();
    let baseline = case.collect(&manifest, &case.incremental_plan(support::COLLECT_RUN));
    assert_eq!(baseline.report.records_accepted, 2);
    let scope = support::first_scope(&baseline);

    fs::remove_file(case.root_dir().join("remove.txt"))
        .unwrap_or_else(|error| panic!("remove: {error}"));

    let plan = case
        .snapshot_plan(support::SNAPSHOT_RUN, &scope)
        .resuming_state(baseline.current_state.clone());
    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        run.rejection
    );
    assert_eq!(
        run.report.records_accepted, 1,
        "only the still-present file is reported"
    );
    assert!(
        run.deletion().is_authorized(),
        "a complete snapshot over the requested scope must authorize deletion: {:?}",
        run.deletion()
    );
}

#[cfg(unix)]
#[test]
fn an_errored_walk_produces_a_partial_result_that_is_never_read_as_deletion() {
    use std::os::unix::fs::PermissionsExt;

    let case = Case::new("errored-walk");
    fs::write(case.root_dir().join("visible.txt"), b"visible")
        .unwrap_or_else(|error| panic!("write: {error}"));
    let locked = case.root_dir().join("locked");
    fs::create_dir(&locked).unwrap_or_else(|error| panic!("mkdir: {error}"));
    fs::write(locked.join("hidden.txt"), b"hidden")
        .unwrap_or_else(|error| panic!("write: {error}"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|error| panic!("chmod: {error}"));

    let manifest = case.manifest();
    let plan = case.snapshot_plan(support::SNAPSHOT_RUN, "any-scope-name");
    let run = case.collect(&manifest, &plan);

    // Restore permissions so the temp directory can be removed on drop.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|error| panic!("chmod restore: {error}"));

    assert_eq!(
        run.report.outcome,
        RunOutcome::Partial,
        "durable work happened (the readable file) but the run did not complete: {:?}",
        run.rejection
    );
    assert!(
        run.report.records_accepted >= 1,
        "the readable file must still be reported"
    );
    assert!(
        !run.deletion().is_authorized(),
        "a partial result must never authorize deletion: {:?}",
        run.deletion()
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_escaping_the_configured_root_is_never_collected() {
    let case = Case::new("containment");
    let outside = fieldnotes_test_support::TempDir::new("containment-outside")
        .unwrap_or_else(|error| panic!("outside dir: {error}"));
    fs::write(
        outside.path().join("secret.txt"),
        b"must-never-be-collected",
    )
    .unwrap_or_else(|error| panic!("write: {error}"));
    fs::write(case.root_dir().join("visible.txt"), b"visible")
        .unwrap_or_else(|error| panic!("write: {error}"));
    std::os::unix::fs::symlink(outside.path(), case.root_dir().join("escape"))
        .unwrap_or_else(|error| panic!("symlink: {error}"));

    let manifest = case.manifest();
    let run = case.collect(&manifest, &case.incremental_plan(support::COLLECT_RUN));

    assert_eq!(
        run.report.outcome,
        RunOutcome::Complete,
        "{:?}",
        run.rejection
    );
    assert_eq!(
        run.report.records_accepted, 1,
        "only the file inside the root is collected"
    );
    assert!(
        record_events(&run).iter().all(|record| !record
            .source
            .identity
            .as_str()
            .contains("secret")),
        "the symlink target must never be reachable"
    );
    assert!(
        run.secret_locations("must-never-be-collected").is_empty(),
        "the escaped file's content must never appear anywhere in this Field's output"
    );
}

#[test]
fn a_cursor_format_version_change_is_refused_as_a_migration_not_a_manifest_edit() {
    let case = Case::new("cursor-format");
    let manifest = case.manifest();
    let mut changed = manifest.clone();
    changed.collection.cursor_format_version = manifest.collection.cursor_format_version + 1;

    let stored = manifest_snapshot(&manifest);
    let arriving = manifest_snapshot(&changed);
    match stored.check_against(&arriving) {
        Err(migration) => assert_eq!(
            migration.code,
            RejectionCode::ManifestCursorFormatChanged,
            "core says so instead of replaying a cursor this Field might misread"
        ),
        Ok(()) => panic!("a cursor-format-version change must require a migration"),
    }

    // The same manifest twice is not a migration.
    assert!(stored.check_against(&manifest_snapshot(&manifest)).is_ok());
}
