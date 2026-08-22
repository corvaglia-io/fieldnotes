//! End-to-end use-case tests over a real notebook on a real filesystem.
//!
//! Everything written here is read back through the format crate's own parser
//! and validator, so the assertions are about the public contract rather than
//! about this implementation's internal state.

use std::path::{Path, PathBuf};

use fieldnotes_app::{
    AppError, Kernel, NoteRequest, NoteSource, create_note, init, inspect, status,
};
use fieldnotes_domain::{Datetime, RecordKind};
use fieldnotes_format::{
    canonical_record_string, content_hash_value, parse_record, validate_note_filename,
    validate_record,
};
use fieldnotes_store::{InitState, Notebook};
use fieldnotes_test_support::{CountingRandom, FixedClock, TempDir};

/// 2026-08-22T08:45:00+02:00 in Unix milliseconds.
const FIXED_MILLIS: u64 = 1_787_381_100_000;

/// A minimal ISO base-media audio file: an `ftyp` box declaring the `M4A `
/// brand, which content detection maps to `audio/mp4`.
const VOICE_BYTES: &[u8] = b"\0\0\0\x18ftypM4A \0\0\0\0mdat-audio-payload";

/// A minimal PNG signature plus payload.
const IMAGE_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n-pretend-pixels-";

fn kernel() -> Result<Kernel<FixedClock, CountingRandom>, AppError> {
    Kernel::new(FixedClock(FIXED_MILLIS), CountingRandom::new(1), 120)
}

fn io(error: std::io::Error, path: &Path) -> AppError {
    AppError::Store(fieldnotes_store::StoreError::io("access", path, error))
}

fn temp(label: &str) -> Result<TempDir, AppError> {
    TempDir::new(label).map_err(|error| io(error, Path::new(".")))
}

/// Initializes a notebook and returns it with its kernel.
fn notebook(root: &Path) -> Result<(Notebook, Kernel<FixedClock, CountingRandom>), AppError> {
    let mut kernel = kernel()?;
    let outcome = init(&mut kernel, root, Some("test-notebook"))?;
    assert_eq!(outcome.state, InitState::Created);
    Ok((Notebook::open(root)?, kernel))
}

/// Asserts that the file at `path` is a canonical, valid Note whose filename
/// agrees with its frontmatter, and returns its text.
fn assert_canonical_note(path: &Path) -> Result<String, AppError> {
    let bytes = std::fs::read(path).map_err(|error| io(error, path))?;
    let record = parse_record(&bytes)?;
    validate_record(&record)?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    validate_note_filename(&record, filename)?;
    let text = String::from_utf8(bytes).unwrap_or_default();
    // Canonical means the emitter reproduces the file byte for byte.
    assert_eq!(canonical_record_string(&record)?, text);
    // The declared body hash recomputes.
    if let Some(fieldnotes_domain::Value::Scalar(fieldnotes_domain::Scalar::Text(declared))) =
        record.get("content_hash")
    {
        assert_eq!(*declared, content_hash_value(record.body()));
    } else {
        panic!("a Note written by the kernel always declares content_hash");
    }
    Ok(text)
}

#[test]
fn init_creates_instance_metadata_and_is_idempotent() -> Result<(), AppError> {
    let temp = temp("init")?;
    let root = temp.path().join("notebook");
    let mut kernel = kernel()?;
    let first = init(&mut kernel, &root, Some("workstation"))?;
    assert_eq!(first.state, InitState::Created);
    // The instance UUIDv7 timestamp and `created_at` agree, as A1 requires.
    assert_eq!(
        first.instance.instance_id.uuid().unix_millis(),
        u64::try_from(first.instance.created_at.unix_millis()).unwrap_or_default()
    );
    assert_eq!(
        first.instance.created_at.to_string(),
        "2026-08-22T08:45:00+02:00"
    );

    // Re-running adopts the existing notebook without changing its identity.
    let mut second_kernel = Kernel::new(FixedClock(FIXED_MILLIS + 5), CountingRandom::new(9), 0)?;
    let second = init(&mut second_kernel, &root, Some("renamed"))?;
    assert_eq!(second.state, InitState::AlreadyInitialized);
    assert_eq!(second.instance, first.instance);
    Ok(())
}

#[test]
fn text_file_and_voice_notes_are_canonical_and_valid() -> Result<(), AppError> {
    let temp = temp("notes")?;
    let root = temp.path().join("notebook");
    let (notebook, mut kernel) = notebook(&root)?;

    let image_path = temp.path().join("whiteboard.png");
    std::fs::write(&image_path, IMAGE_BYTES).map_err(|error| io(error, &image_path))?;
    let voice_path = temp.path().join("after-the-call.m4a");
    std::fs::write(&voice_path, VOICE_BYTES).map_err(|error| io(error, &voice_path))?;

    let text = create_note(
        &mut kernel,
        &notebook,
        &NoteRequest {
            source: NoteSource::Text,
            text: Some("Ask Alice whether the rollout can begin on Thursday.".to_owned()),
            title: Some("Rollout reminder".to_owned()),
            occurred_at: Some(Datetime::parse("2026-08-22T09:00:00+02:00")?),
        },
    )?;
    let file = create_note(
        &mut kernel,
        &notebook,
        &NoteRequest {
            source: NoteSource::File(image_path.clone()),
            text: Some("Imported photo of the rollout planning whiteboard.".to_owned()),
            title: Some("Meeting whiteboard".to_owned()),
            occurred_at: Some(Datetime::parse("2026-08-22T09:15:00+02:00")?),
        },
    )?;
    let voice = create_note(
        &mut kernel,
        &notebook,
        &NoteRequest {
            source: NoteSource::Voice(voice_path.clone()),
            text: None,
            title: None,
            occurred_at: Some(Datetime::parse("2026-08-22T09:30:00+02:00")?),
        },
    )?;

    let text_body = assert_canonical_note(&text.write.path)?;
    let file_body = assert_canonical_note(&file.write.path)?;
    let voice_body = assert_canonical_note(&voice.write.path)?;

    // Filenames render the event instant in UTC.
    assert_eq!(
        text.write.filename,
        format!("20260822T070000Z_self_text_{}.md", text.note_id)
    );
    assert_eq!(
        file.write.filename,
        format!("20260822T071500Z_self_file_{}.md", file.note_id)
    );
    assert_eq!(
        voice.write.filename,
        format!("20260822T073000Z_self_voice_{}.md", voice.note_id)
    );

    // A text Note carries no artifact reference at all.
    assert!(text.artifact.is_none());
    assert!(!text_body.contains("artifacts"));

    // An import references its original by content-addressed ID, and
    // `attachments` stays absent because a user import is not an attachment.
    let Some(image_artifact) = file.artifact.as_ref() else {
        panic!("a file import always stores bytes")
    };
    assert!(!image_artifact.reused);
    assert!(file_body.contains(&format!("artifacts:\n  - {}\n", image_artifact.id)));
    assert!(!file_body.contains("attachments"));
    assert!(file_body.contains(&format!("`../{}`", image_artifact.relative_path)));
    assert!(image_artifact.relative_path.ends_with(".png"));

    // A voice import is playable: the original bytes are stored unchanged, the
    // canonical extension comes from detection, and the media type is declared.
    let Some(voice_artifact) = voice.artifact.as_ref() else {
        panic!("a voice import always stores bytes")
    };
    assert!(voice_artifact.relative_path.ends_with(".m4a"));
    assert!(voice_body.contains("audio_media_type: audio/mp4\n"));
    assert!(voice_body.contains("type: voice\n"));
    // The title defaulted to the source filename, so the body has a heading.
    assert!(voice_body.contains("# after-the-call.m4a\n"));
    assert_eq!(
        std::fs::read(&voice_artifact.path).map_err(|error| io(error, &voice_artifact.path))?,
        VOICE_BYTES
    );

    // The notebook validates as a whole.
    let report = inspect(&notebook, None)?;
    assert!(report.healthy, "{:?}", report.records);
    assert_eq!(report.records.len(), 3);
    assert_eq!(report.artifacts.len(), 2);
    assert!(report.artifacts.iter().all(|artifact| artifact.referenced));

    // `status` counts what is there.
    let summary = status(&notebook)?;
    assert_eq!(summary.notes_total, 3);
    assert_eq!(summary.notes_valid, 3);
    assert_eq!(summary.artifacts_total, 2);
    assert_eq!(summary.notes_by_type.get("voice"), Some(&1));
    assert_eq!(
        summary.occurred_range,
        Some((
            "2026-08-22T09:00:00+02:00".to_owned(),
            "2026-08-22T09:30:00+02:00".to_owned()
        ))
    );
    Ok(())
}

#[test]
fn importing_the_same_file_twice_reuses_the_artifact() -> Result<(), AppError> {
    let temp = temp("replay")?;
    let root = temp.path().join("notebook");
    let (notebook, mut kernel) = notebook(&root)?;

    let first_path = temp.path().join("scan.png");
    std::fs::write(&first_path, IMAGE_BYTES).map_err(|error| io(error, &first_path))?;
    // The same bytes under a different name are the same artifact: identity is
    // content, never the filename.
    let second_path = temp.path().join("copy-of-scan.png");
    std::fs::write(&second_path, IMAGE_BYTES).map_err(|error| io(error, &second_path))?;

    let first = create_note(
        &mut kernel,
        &notebook,
        &NoteRequest {
            source: NoteSource::File(first_path),
            text: None,
            title: Some("First import".to_owned()),
            occurred_at: None,
        },
    )?;
    let second = create_note(
        &mut kernel,
        &notebook,
        &NoteRequest {
            source: NoteSource::File(second_path),
            text: None,
            title: Some("Second import".to_owned()),
            occurred_at: None,
        },
    )?;

    let (Some(first_artifact), Some(second_artifact)) = (first.artifact, second.artifact) else {
        panic!("both imports report their artifact")
    };
    assert!(!first_artifact.reused);
    assert!(second_artifact.reused);
    assert_eq!(first_artifact.id, second_artifact.id);
    assert_eq!(first_artifact.path, second_artifact.path);

    let stored = std::fs::read_dir(notebook.artifacts_dir())
        .map_err(|error| io(error, &notebook.artifacts_dir()))?
        .count();
    assert_eq!(stored, 1, "the same bytes are stored once per notebook");

    // Two distinct Notes reference the one artifact, and both are valid.
    let report = inspect(&notebook, None)?;
    assert!(report.healthy);
    assert_eq!(report.records.len(), 2);
    assert_eq!(report.artifacts.len(), 1);
    Ok(())
}

#[test]
fn the_same_operations_produce_identical_bytes() -> Result<(), AppError> {
    let temp = temp("determinism")?;
    let source = temp.path().join("input.png");
    std::fs::write(&source, IMAGE_BYTES).map_err(|error| io(error, &source))?;

    let mut renderings = Vec::new();
    for run in ["first", "second"] {
        let root = temp.path().join(run);
        let (notebook, mut kernel) = notebook(&root)?;
        create_note(
            &mut kernel,
            &notebook,
            &NoteRequest {
                source: NoteSource::Text,
                text: Some("Deterministic body.".to_owned()),
                title: Some("Deterministic".to_owned()),
                occurred_at: Some(Datetime::parse("2026-08-22T09:00:00+02:00")?),
            },
        )?;
        create_note(
            &mut kernel,
            &notebook,
            &NoteRequest {
                source: NoteSource::File(source.clone()),
                text: None,
                title: None,
                occurred_at: Some(Datetime::parse("2026-08-22T10:00:00+02:00")?),
            },
        )?;
        renderings.push(render_tree(&root)?);
    }
    assert_eq!(
        renderings[0], renderings[1],
        "an injected clock and seeded ID source must reproduce every byte"
    );
    // The rendering really did capture content, not just names.
    assert!(renderings[0].contains("instance_id: fn_"));
    Ok(())
}

/// Renders a notebook's notes, artifacts, and instance metadata as one string
/// of notebook-relative paths and file contents.
fn render_tree(root: &Path) -> Result<String, AppError> {
    let mut rendered = String::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for directory in [
        root.join("notes"),
        root.join("artifacts"),
        root.join(".fieldnotes"),
    ] {
        let entries = std::fs::read_dir(&directory).map_err(|error| io(error, &directory))?;
        for entry in entries {
            let entry = entry.map_err(|error| io(error, &directory))?;
            if entry.path().is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(&file);
        let bytes = std::fs::read(&file).map_err(|error| io(error, &file))?;
        rendered.push_str(&relative.to_string_lossy());
        rendered.push('\n');
        rendered.push_str(&String::from_utf8_lossy(&bytes));
        rendered.push('\n');
    }
    Ok(rendered)
}

#[test]
fn a_note_with_no_content_is_refused() -> Result<(), AppError> {
    let temp = temp("empty")?;
    let root = temp.path().join("notebook");
    let (notebook, mut kernel) = notebook(&root)?;
    let outcome = create_note(
        &mut kernel,
        &notebook,
        &NoteRequest {
            source: NoteSource::Text,
            text: Some("   \n".to_owned()),
            title: None,
            occurred_at: None,
        },
    );
    assert!(matches!(outcome, Err(AppError::EmptyNote)));
    // Nothing was written.
    assert_eq!(status(&notebook)?.notes_total, 0);
    Ok(())
}

#[test]
fn a_voice_import_must_be_audio() -> Result<(), AppError> {
    let temp = temp("voice-guard")?;
    let root = temp.path().join("notebook");
    let (notebook, mut kernel) = notebook(&root)?;
    let path = temp.path().join("not-audio.png");
    std::fs::write(&path, IMAGE_BYTES).map_err(|error| io(error, &path))?;
    let outcome = create_note(
        &mut kernel,
        &notebook,
        &NoteRequest {
            source: NoteSource::Voice(path),
            text: None,
            title: None,
            occurred_at: None,
        },
    );
    match outcome {
        Err(AppError::NotAudio { detected, .. }) => {
            assert_eq!(detected.as_deref(), Some("image/png"));
        }
        other => panic!("expected a NotAudio refusal, got {other:?}"),
    }
    // The refusal happens before anything is stored.
    assert_eq!(status(&notebook)?.artifacts_total, 0);
    Ok(())
}

#[test]
fn inspect_addresses_a_record_by_id_and_reports_a_bad_file() -> Result<(), AppError> {
    let temp = temp("inspect")?;
    let root = temp.path().join("notebook");
    let (notebook, mut kernel) = notebook(&root)?;
    let note = create_note(&mut kernel, &notebook, &NoteRequest::text("Findable."))?;

    let by_id = inspect(&notebook, Some(&note.note_id.to_string()))?;
    assert_eq!(by_id.records.len(), 1);
    assert_eq!(by_id.records[0].body.as_deref(), Some("Findable.\n"));
    assert!(inspect(&notebook, Some("note_missing")).is_err());

    // A hand-damaged file is surfaced, not hidden.
    let damaged = notebook
        .notes_dir()
        .join("20260822T070000Z_self_text_note_01a02844-f150-7000-8000-0000000000ff.md");
    std::fs::write(
        &damaged,
        b"---\nid: note_01a02844-f150-7000-8000-0000000000ff\n",
    )
    .map_err(|error| io(error, &damaged))?;
    let report = inspect(&notebook, None)?;
    assert!(!report.healthy);
    assert_eq!(report.records.len(), 2);
    let Some(bad) = report
        .records
        .iter()
        .find(|record| record.path.ends_with("0000000000ff.md"))
    else {
        panic!("the damaged file must be reported")
    };
    assert!(!bad.valid);
    assert_eq!(bad.problems[0].kind, "invalid");
    assert_eq!(status(&notebook)?.notes_invalid, 1);
    Ok(())
}

#[test]
fn a_generated_note_id_is_a_note_id() -> Result<(), AppError> {
    let temp = temp("kinds")?;
    let root = temp.path().join("notebook");
    let (notebook, mut kernel) = notebook(&root)?;
    let note = create_note(&mut kernel, &notebook, &NoteRequest::text("Kind check."))?;
    assert_eq!(note.note_id.kind(), RecordKind::Note);
    // `captured_at` agrees with the Note ID's own creation instant.
    assert_eq!(
        note.note_id.uuid().unix_millis(),
        u64::try_from(note.captured_at.unix_millis()).unwrap_or_default()
    );
    Ok(())
}
