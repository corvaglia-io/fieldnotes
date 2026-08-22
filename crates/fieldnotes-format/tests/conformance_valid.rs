//! IG1 conformance: every normative valid fixture parses, validates, and
//! re-emits byte-for-byte identical canonical output.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use fieldnotes_format::{
    canonical_record_string, content_hash_value, instance_yaml_string, parse_instance_yaml,
    parse_record, validate_note_filename, validate_record,
};

type TestResult = Result<(), Box<dyn Error>>;

fn corpus_root() -> PathBuf {
    fieldnotes_test_support::fixtures_root()
        .join("notebooks")
        .join("proposed-v1")
}

fn collect_markdown_files(root: &Path, out: &mut Vec<PathBuf>) -> TestResult {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out)?;
        } else if path.extension().is_some_and(|extension| extension == "md")
            && path.file_name().is_some_and(|name| name != "README.md")
        {
            out.push(path);
        }
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every record file in the valid corpus round-trips byte-for-byte, including
/// the same-ID left/right pair, whose divergence is conflict-detection input
/// rather than a parse failure.
#[test]
fn valid_corpus_round_trips_byte_for_byte() -> TestResult {
    let mut files = Vec::new();
    collect_markdown_files(&corpus_root(), &mut files)?;
    files.sort();
    assert_eq!(
        files.len(),
        26,
        "expected the 26 frozen record fixtures, found {files:?}"
    );
    for path in &files {
        let bytes = fs::read(path)?;
        let record = parse_record(&bytes).map_err(|e| format!("{}: parse: {e}", path.display()))?;
        validate_record(&record).map_err(|e| format!("{}: validate: {e}", path.display()))?;
        let canonical = canonical_record_string(&record)
            .map_err(|e| format!("{}: emit: {e}", path.display()))?;
        let original = String::from_utf8(bytes)?;
        assert_eq!(
            canonical,
            original,
            "canonical round-trip diverged for {}",
            path.display()
        );
    }
    Ok(())
}

/// Every Note stored under a `notes/` directory carries its canonical
/// UTC filename; conflict-bundle candidates use fixed bundle names instead.
#[test]
fn valid_note_filenames_match_computed_names() -> TestResult {
    let mut files = Vec::new();
    collect_markdown_files(&corpus_root(), &mut files)?;
    files.sort();
    let mut checked = 0usize;
    for path in &files {
        let in_notes_dir = path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|dir| dir == "notes");
        if !in_notes_dir {
            continue;
        }
        let record = parse_record(&fs::read(path)?)?;
        validate_record(&record)?;
        validate_note_filename(&record, &file_name(path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        checked += 1;
    }
    // Thirteen notes plus the same-id left/right pair.
    assert_eq!(checked, 15);
    Ok(())
}

/// Every embedded `content_hash` recomputes exactly from its normalized body.
#[test]
fn embedded_content_hashes_recompute_from_bodies() -> TestResult {
    let mut files = Vec::new();
    collect_markdown_files(&corpus_root(), &mut files)?;
    files.sort();
    let mut checked = 0usize;
    for path in &files {
        let record = parse_record(&fs::read(path)?)?;
        let Some(fieldnotes_domain::Value::Scalar(fieldnotes_domain::Scalar::Text(embedded))) =
            record.get("content_hash")
        else {
            continue;
        };
        assert_eq!(
            &content_hash_value(record.body()),
            embedded,
            "content_hash mismatch for {}",
            path.display()
        );
        checked += 1;
    }
    // Thirteen notes, two conflict candidates, and the same-id pair.
    assert_eq!(checked, 17);
    Ok(())
}

/// `.fieldnotes/instance.yaml` parses under the exact three-key schema and
/// re-serializes byte-for-byte.
#[test]
fn instance_metadata_round_trips() -> TestResult {
    let path = corpus_root().join(".fieldnotes").join("instance.yaml");
    let bytes = fs::read(&path)?;
    let metadata = parse_instance_yaml(&bytes)?;
    assert_eq!(
        metadata.instance_id.to_string(),
        "fn_01a02837-2de0-7a2b-8c41-f2481851192a"
    );
    assert_eq!(metadata.created_at.to_string(), "2026-08-22T08:45:00+02:00");
    assert_eq!(metadata.name.as_deref(), Some("fixture-workstation"));
    assert_eq!(instance_yaml_string(&metadata), String::from_utf8(bytes)?);
    Ok(())
}

/// The same-ID left/right Notes are each individually valid while their
/// bodies (and content hashes) diverge.
#[test]
fn same_id_pair_is_individually_valid_but_divergent() -> TestResult {
    let base = corpus_root().join("conflicts").join("same-id");
    let name =
        "20260822T100000Z_outlook_mail_work_mail_note_01a028ea-9f60-7000-8000-00000000000b.md";
    let left = parse_record(&fs::read(base.join("left").join("notes").join(name))?)?;
    let right = parse_record(&fs::read(base.join("right").join("notes").join(name))?)?;
    validate_record(&left)?;
    validate_record(&right)?;
    assert_eq!(left.id(), right.id());
    assert_ne!(left.body(), right.body());
    Ok(())
}
