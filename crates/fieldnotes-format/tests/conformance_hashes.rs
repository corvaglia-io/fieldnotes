//! IG1 conformance: every frozen hash vector reproduces exactly.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use fieldnotes_domain::{Scalar, Value};
use fieldnotes_format::{
    artifact_id_for_bytes, artifact_relative_path, normalize_body_bytes, parse_record,
    record_fingerprint, semantic_record_string, sha256_hex, validate_record,
};

type TestResult = Result<(), Box<dyn Error>>;

fn vectors_root() -> PathBuf {
    fieldnotes_test_support::fixtures_root()
        .join("hashes")
        .join("proposed-v1")
}

fn read_expected(name: &str, label: &str) -> TestResult2 {
    let text = fs::read_to_string(vectors_root().join(name))?;
    let value = text.trim_end_matches('\n');
    let hex = value
        .strip_prefix(label)
        .ok_or_else(|| format!("{name} does not carry the `{label}` label"))?;
    Ok((value.to_owned(), hex.to_owned()))
}

type TestResult2 = Result<(String, String), Box<dyn Error>>;

/// Artifact bytes hash to the checked-in digest, derive the frozen artifact
/// ID, and select the `.bin` fallback path when no media type is available.
#[test]
fn artifact_vector_reproduces_digest_id_and_path() -> TestResult {
    let bytes = fs::read(vectors_root().join("artifact-input.bin"))?;
    let (_, expected_hex) = read_expected("artifact-input.sha256", "sha256:")?;
    assert_eq!(sha256_hex(&bytes), expected_hex);

    let artifact_id = artifact_id_for_bytes(&bytes);
    assert_eq!(
        artifact_id.to_string(),
        "artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17"
    );
    assert_eq!(
        artifact_relative_path(&artifact_id, None),
        "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.bin"
    );
    Ok(())
}

/// The normalized-body vector reproduces the `fn-content-v1` digest, and the
/// documented CRLF/CR/BOM/final-LF normalizations converge on the same bytes.
#[test]
fn content_vector_reproduces_the_domain_separated_digest() -> TestResult {
    let bytes = fs::read(vectors_root().join("normalized-body-input.md"))?;
    let normalized = normalize_body_bytes(&bytes)?;
    assert_eq!(
        normalized.as_bytes(),
        bytes.as_slice(),
        "fixture is already normalized"
    );

    let (expected_value, _) =
        read_expected("normalized-body-input.sha256", "fn-content-v1-sha256:")?;
    assert_eq!(
        fieldnotes_format::content_hash_value(&normalized),
        expected_value
    );

    // Paired required-result checks fixed by the A1 algorithm.
    let crlf = normalized.replace('\n', "\r\n");
    assert_eq!(normalize_body_bytes(crlf.as_bytes())?, normalized);
    let lone_cr = normalized.replace('\n', "\r");
    assert_eq!(normalize_body_bytes(lone_cr.as_bytes())?, normalized);
    let mut with_bom = vec![0xef, 0xbb, 0xbf];
    with_bom.extend_from_slice(&bytes);
    assert_eq!(normalize_body_bytes(&with_bom)?, normalized);
    let mut extra_final = bytes.clone();
    extra_final.extend_from_slice(b"\n\n");
    assert_eq!(normalize_body_bytes(&extra_final)?, normalized);
    let missing_final = &bytes[..bytes.len() - 1];
    assert_eq!(normalize_body_bytes(missing_final)?, normalized);
    Ok(())
}

/// The semantic-record source produces the exact checked-in canonical
/// encoding after exclusions and UTC conversion, and its `fn-record-v1`
/// fingerprint matches the frozen digest.
#[test]
fn semantic_record_vector_reproduces_encoding_and_fingerprint() -> TestResult {
    let source_bytes = fs::read(vectors_root().join("semantic-record-source.md"))?;
    let record = parse_record(&source_bytes)?;
    validate_record(&record)?;

    let canonical = semantic_record_string(&record)?;
    let expected_canonical =
        fs::read_to_string(vectors_root().join("semantic-record-canonical.md"))?;
    assert_eq!(canonical, expected_canonical);

    let (expected_value, _) =
        read_expected("semantic-record-canonical.sha256", "fn-record-v1-sha256:")?;
    assert_eq!(record_fingerprint(&canonical), expected_value);

    // Bookkeeping exclusions do not change current payload equality: dropping
    // collection/merge bookkeeping from the source leaves the encoding equal.
    let source_text = String::from_utf8(source_bytes)?;
    let stripped: String = source_text
        .lines()
        .scan(false, |skipping, line| {
            if line.starts_with(' ') {
                // list continuation follows its key's decision
                Some(if *skipping { None } else { Some(line) })
            } else {
                let excluded = [
                    "captured_at:",
                    "collected_by:",
                    "content_hash:",
                    "entities:",
                    "related:",
                    "source_version:",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix));
                *skipping = excluded;
                Some(if excluded { None } else { Some(line) })
            }
        })
        .flatten()
        .map(|line| format!("{line}\n"))
        .collect();
    let stripped_record = parse_record(stripped.as_bytes())?;
    assert_eq!(semantic_record_string(&stripped_record)?, canonical);
    Ok(())
}

/// The frozen conflict bundle's candidate fingerprints recompute from the
/// candidate files, and candidate numbering follows ascending fingerprint.
#[test]
fn conflict_candidate_fingerprints_recompute() -> TestResult {
    let bundle = fieldnotes_test_support::fixtures_root()
        .join("notebooks")
        .join("proposed-v1")
        .join("conflicts")
        .join("conf_01a02905-2c40-7000-8000-000000000001");

    let fingerprint_of = |name: &str| -> Result<String, Box<dyn Error>> {
        let record = parse_record(&fs::read(bundle.join(name))?)?;
        validate_record(&record)?;
        Ok(record_fingerprint(&semantic_record_string(&record)?))
    };
    let first = fingerprint_of("candidate_1.md")?;
    let second = fingerprint_of("candidate_2.md")?;
    assert!(
        first < second,
        "candidate numbering follows ascending fingerprint"
    );

    let conflict = parse_record(&fs::read(bundle.join("conflict.md"))?)?;
    validate_record(&conflict)?;
    let Some(Value::List(items)) = conflict.get("candidate_fingerprints") else {
        return Err("conflict.md lacks candidate_fingerprints".into());
    };
    let listed: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            Scalar::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(listed, vec![first.as_str(), second.as_str()]);
    Ok(())
}
