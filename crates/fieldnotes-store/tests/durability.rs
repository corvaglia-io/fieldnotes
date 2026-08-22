//! Crash-safety, replay, and rename behaviour of the durable writer.
//!
//! These tests exercise the staging path directly, because that is the only way
//! to observe the states a crash could leave behind without actually killing a
//! process mid-write.

use std::path::Path;

use fieldnotes_domain::{Datetime, FieldId, FieldStemRegistry, NoteType, RecordId};
use fieldnotes_format::{CanonicalRecord, RecordBuilder, parse_record, validate_record};
use fieldnotes_store::atomic::StagedFile;
use fieldnotes_store::{Notebook, ScanOptions, StoreError, replace_note, scan, write_note};
use fieldnotes_test_support::TempDir;

const NOTE_ID: &str = "note_01a02844-f150-7000-8000-000000000001";
const INSTANCE_ID: &str = "fn_01a02837-2de0-7a2b-8c41-f2481851192a";

fn fail(reason: &'static str) -> StoreError {
    StoreError::Invalid {
        path: Path::new(".").to_path_buf(),
        source: fieldnotes_format::ValidationError::InvalidInstanceMetadata { reason },
    }
}

fn temp(label: &str) -> Result<TempDir, StoreError> {
    TempDir::new(label).map_err(|error| StoreError::io("create temporary directory", ".", error))
}

/// A valid `self` text Note with the given event time.
fn note(occurred_at: &str, body: &str) -> Result<CanonicalRecord, StoreError> {
    let note_id = RecordId::parse(NOTE_ID).map_err(|_| fail("note id"))?;
    let instance_id = RecordId::parse(INSTANCE_ID).map_err(|_| fail("instance id"))?;
    let field = FieldId::parse("self", FieldStemRegistry::v1()).map_err(|_| fail("field id"))?;
    let occurred_at = Datetime::parse(occurred_at).map_err(|_| fail("occurred_at"))?;
    let mut builder =
        RecordBuilder::note(&note_id, &instance_id, &field, NoteType::Text, occurred_at);
    builder.set_body(body);
    builder.build().map_err(|source| StoreError::Invalid {
        path: Path::new(".").to_path_buf(),
        source,
    })
}

fn notebook(label: &str) -> Result<(TempDir, Notebook), StoreError> {
    let temp = temp(label)?;
    let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
    // Discovery needs instance metadata, but these tests only exercise the
    // writer, so the layout alone is enough.
    Ok((temp, notebook))
}

#[test]
fn an_interrupted_write_leaves_no_valid_looking_partial_note() -> Result<(), StoreError> {
    let (_temp, notebook) = notebook("crash")?;
    let record = note("2026-08-22T09:00:00+02:00", "Complete body.\n")?;
    let filename = record.note_filename().map_err(|_| fail("note filename"))?;
    let destination = notebook.notes_dir().join(&filename);

    // Stage the first half of the bytes: the state a crash mid-write leaves.
    let partial = &record.bytes()[..record.bytes().len() / 2];
    let staged = StagedFile::create(&notebook.notes_dir(), partial)?;

    // The staged file is not at the destination path, and the destination does
    // not exist at all.
    assert_ne!(staged.temp_path(), destination.as_path());
    assert!(!destination.exists(), "the destination must stay absent");

    // The partial bytes are not a parseable Note, and the scan ignores the
    // staging file entirely rather than treating it as notebook content.
    let staged_bytes = std::fs::read(staged.temp_path())
        .map_err(|error| StoreError::io("read", staged.temp_path(), error))?;
    assert!(parse_record(&staged_bytes).is_err());
    let scanned = scan(&notebook, ScanOptions::default())?;
    assert!(scanned.notes.is_empty());
    // It is reported as an interrupted write instead of being hidden.
    assert_eq!(
        scanned.stray_staged_files,
        vec![staged.temp_path().to_path_buf()]
    );

    // Abandoning the write removes the staging file and still leaves no Note.
    let temp_path = staged.temp_path().to_path_buf();
    drop(staged);
    assert!(!temp_path.exists());
    assert!(!destination.exists());
    assert!(scan(&notebook, ScanOptions::default())?.notes.is_empty());
    Ok(())
}

#[test]
fn a_complete_staged_file_is_not_a_note_until_it_is_installed() -> Result<(), StoreError> {
    let (_temp, notebook) = notebook("stage-complete")?;
    let record = note("2026-08-22T09:00:00+02:00", "Complete body.\n")?;
    let filename = record.note_filename().map_err(|_| fail("note filename"))?;
    let destination = notebook.notes_dir().join(&filename);

    let staged = StagedFile::create(&notebook.notes_dir(), record.bytes())?;
    // Even though the staged bytes are complete and valid, the Note does not
    // exist: a reader sees nothing until the rename.
    let parsed = parse_record(record.bytes()).map_err(|_| fail("record"))?;
    validate_record(&parsed).map_err(|_| fail("record"))?;
    assert!(!destination.exists());
    assert!(scan(&notebook, ScanOptions::default())?.notes.is_empty());

    let installed = staged.install(&filename)?;
    assert_eq!(installed, destination);
    let scanned = scan(&notebook, ScanOptions::default())?;
    assert_eq!(scanned.notes.len(), 1);
    assert!(scanned.notes[0].is_valid());
    assert!(scanned.stray_staged_files.is_empty());
    Ok(())
}

#[test]
fn writing_the_same_record_twice_converges() -> Result<(), StoreError> {
    let (_temp, notebook) = notebook("replay")?;
    let record = note("2026-08-22T09:00:00+02:00", "Replayed body.\n")?;
    let first = write_note(&notebook, &record)?;
    assert!(!first.replaced);
    let second = write_note(&notebook, &record)?;
    assert!(second.replaced);
    assert_eq!(first.path, second.path);

    let scanned = scan(&notebook, ScanOptions::default())?;
    assert_eq!(scanned.notes.len(), 1, "replay must not duplicate a Note");
    let bytes =
        std::fs::read(&second.path).map_err(|error| StoreError::io("read", &second.path, error))?;
    assert_eq!(bytes, record.bytes());
    assert!(scanned.stray_staged_files.is_empty());
    Ok(())
}

#[test]
fn an_event_time_change_installs_the_new_name_before_removing_the_old() -> Result<(), StoreError> {
    let (_temp, notebook) = notebook("rename")?;
    let original = note("2026-08-22T09:00:00+02:00", "Corrected body.\n")?;
    let first = write_note(&notebook, &original)?;
    assert_eq!(
        first.filename,
        format!("20260822T070000Z_self_text_{NOTE_ID}.md")
    );

    // The same Note ID with a corrected event time.
    let corrected = note("2026-08-23T09:00:00+02:00", "Corrected body.\n")?;
    let second = replace_note(&notebook, &corrected, &first.filename)?;
    assert_eq!(
        second.filename,
        format!("20260823T070000Z_self_text_{NOTE_ID}.md")
    );
    assert_eq!(
        second.removed_previous.as_deref(),
        Some(first.filename.as_str())
    );
    assert!(second.path.is_file(), "the new name is installed");
    assert!(!first.path.exists(), "the old name is removed afterwards");

    // Exactly one Note remains, under the same logical ID.
    let scanned = scan(&notebook, ScanOptions::default())?;
    assert_eq!(scanned.notes.len(), 1);
    assert!(scanned.notes[0].is_valid());
    let Some(record) = scanned.notes[0].record.as_ref() else {
        panic!("the surviving Note parses")
    };
    assert_eq!(record.id().to_string(), NOTE_ID);
    Ok(())
}

#[test]
fn a_filename_that_disagrees_with_frontmatter_is_reported() -> Result<(), StoreError> {
    let (_temp, notebook) = notebook("filename")?;
    let record = note("2026-08-22T09:00:00+02:00", "Body.\n")?;
    // Install the correct bytes under a wrong name, as a hand edit might.
    let wrong = format!("20260101T000000Z_self_text_{NOTE_ID}.md");
    fieldnotes_store::atomic::write_atomic(&notebook.notes_dir(), &wrong, record.bytes())?;
    let scanned = scan(&notebook, ScanOptions::default())?;
    assert_eq!(scanned.notes.len(), 1);
    assert!(!scanned.notes[0].is_valid());
    assert_eq!(scanned.notes[0].problems[0].kind(), "filename_mismatch");
    Ok(())
}
