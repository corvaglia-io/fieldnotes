//! IG1 conformance: every invalid-corpus fixture is rejected with the exact
//! conceptual error required by the frozen rejection table.

use std::error::Error;
use std::fs;
use std::path::Path;

use fieldnotes_format::{ValidationError, parse_record, validate_note_filename, validate_record};

type TestResult = Result<(), Box<dyn Error>>;

/// Runs the complete acceptance pipeline the way a notebook reader would:
/// parse, validate, then compare the whole filename against the computed one.
fn check_fixture(path: &Path) -> Result<(), ValidationError> {
    let bytes = fs::read(path).map_err(|_| ValidationError::InvalidUtf8)?;
    let record = parse_record(&bytes)?;
    validate_record(&record)?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    validate_note_filename(&record, &file_name)
}

/// The frozen rejection table from the invalid-corpus README.
const EXPECTED: [(&str, &str); 13] = [
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab01.md",
        "frontmatter.nested_mapping",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab02.md",
        "frontmatter.array_object",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab03.md",
        "frontmatter.mixed_list",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab04.md",
        "frontmatter.null",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab05.md",
        "frontmatter.duplicate_key",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab06.md",
        "frontmatter.custom_tag",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab07.md",
        "datetime.offset_required",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab08.md",
        "property.list_required",
    ),
    (
        "20260822T093614Z_teams_work_message_note_01a028d5-90c0-7248-a74b-c8bc1085ab09.md",
        "property.unknown_unprefixed",
    ),
    (
        "20260822T093614Z_local_work_file_note_01a028d5-90c0-7248-a74b-c8bc1085ab0b.md",
        "source.scope_required",
    ),
    (
        "20260822T093615Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0c.md",
        "filename.mismatch",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0d.md",
        "frontmatter.document_marker",
    ),
    (
        "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0e.md",
        "property.foreign_prefix",
    ),
];

#[test]
fn invalid_corpus_is_rejected_for_the_right_reasons() -> TestResult {
    let root = fieldnotes_test_support::fixtures_root()
        .join("notebooks")
        .join("proposed-v1-invalid");
    for (fixture, expected_label) in EXPECTED {
        let path = root.join(fixture);
        assert!(path.is_file(), "missing frozen fixture {fixture}");
        match check_fixture(&path) {
            Ok(()) => return Err(format!("{fixture} was accepted").into()),
            Err(error) => assert_eq!(
                error.conceptual_label(),
                expected_label,
                "{fixture} rejected as {error:?}"
            ),
        }
    }

    // The table covers the complete frozen invalid corpus.
    let mut fixture_count = 0usize;
    for entry in fs::read_dir(&root)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "md")
            && path.file_name().is_some_and(|name| name != "README.md")
        {
            fixture_count += 1;
        }
    }
    assert_eq!(fixture_count, EXPECTED.len());
    Ok(())
}

/// Structured error details accompany the conceptual labels.
#[test]
fn rejections_carry_specific_error_details() -> TestResult {
    let root = fieldnotes_test_support::fixtures_root()
        .join("notebooks")
        .join("proposed-v1-invalid");
    match check_fixture(
        &root.join("20260822T093615Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0c.md"),
    ) {
        Err(ValidationError::FilenameMismatch { expected, actual }) => {
            assert_eq!(
                expected,
                "20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0c.md"
            );
            assert_eq!(
                actual,
                "20260822T093615Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0c.md"
            );
        }
        other => return Err(format!("unexpected result: {other:?}").into()),
    }
    match check_fixture(
        &root.join("20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab07.md"),
    ) {
        Err(ValidationError::OffsetRequired { key }) => assert_eq!(key, "occurred_at"),
        other => return Err(format!("unexpected result: {other:?}").into()),
    }
    match check_fixture(
        &root.join(
            "20260822T093614Z_teams_work_message_note_01a028d5-90c0-7248-a74b-c8bc1085ab09.md",
        ),
    ) {
        Err(ValidationError::UnknownUnprefixed { key }) => assert_eq!(key, "chat_id"),
        other => return Err(format!("unexpected result: {other:?}").into()),
    }
    match check_fixture(
        &root.join("20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0e.md"),
    ) {
        Err(ValidationError::ForeignPrefix { key }) => assert_eq!(key, "teams_chat_id"),
        other => return Err(format!("unexpected result: {other:?}").into()),
    }
    Ok(())
}
