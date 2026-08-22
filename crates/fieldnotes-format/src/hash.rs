//! The v0.1 SHA-256 hash domains: artifact bytes, normalized content, and
//! canonical semantic records.
//!
//! Different domains must never be compared as though interchangeable. The
//! content and record domains are separated in-band by an ASCII label and one
//! NUL byte.

use fieldnotes_domain::ArtifactId;
use sha2::{Digest, Sha256};

use crate::extension::canonical_extension;

const CONTENT_DOMAIN: &[u8] = b"fieldnotes-content-v1\0";
const RECORD_DOMAIN: &[u8] = b"fieldnotes-record-v1\0";

/// Lowercase hexadecimal encoding of arbitrary bytes.
fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// SHA-256 over a domain label, one NUL byte, then the payload, rendered as
/// `<label><lowercase-hex>`.
fn domain_separated_value(domain: &[u8], label: &str, payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(payload);
    let mut out = String::from(label);
    out.push_str(&hex_lower(&hasher.finalize()));
    out
}

/// Lowercase hexadecimal SHA-256 of exact bytes, with no domain prefix.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

/// The content-addressed artifact ID for exact original bytes.
#[must_use]
pub fn artifact_id_for_bytes(bytes: &[u8]) -> ArtifactId {
    let digest = Sha256::digest(bytes);
    ArtifactId::from_digest(digest.into())
}

/// The notebook-relative original path `artifacts/<artifact-id>.<extension>`.
///
/// `media_type` is the detected media type, if reliably available; unknown or
/// conflicting types fall back to `.bin` through the canonical registry.
#[must_use]
pub fn artifact_relative_path(id: &ArtifactId, media_type: Option<&str>) -> String {
    format!("artifacts/{id}.{}", canonical_extension(media_type))
}

/// The public `fn-content-v1-sha256:<hex>` value over a normalized body.
///
/// The SHA-256 input is `fieldnotes-content-v1`, one NUL byte, then the
/// normalized Markdown body bytes.
#[must_use]
pub fn content_hash_value(normalized_body: &str) -> String {
    domain_separated_value(
        CONTENT_DOMAIN,
        "fn-content-v1-sha256:",
        normalized_body.as_bytes(),
    )
}

/// The `fn-record-v1-sha256:<hex>` fingerprint over a canonical semantic
/// record encoding.
///
/// The SHA-256 input is `fieldnotes-record-v1`, one NUL byte, then the exact
/// canonical semantic record bytes.
#[must_use]
pub fn record_fingerprint(canonical_semantic_record: &str) -> String {
    domain_separated_value(
        RECORD_DOMAIN,
        "fn-record-v1-sha256:",
        canonical_semantic_record.as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_sha256_matches_the_known_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// The known artifact ID for `tests/fixtures/hashes/proposed-v1/artifact-input.bin`,
    /// reused (per that corpus's README) to demonstrate the ADR 0008
    /// registry additions without introducing a new byte payload: the
    /// artifact ID depends only on bytes, never on media type, so varying
    /// the assumed media type here only changes the resolved extension.
    const KNOWN_ARTIFACT_ID: &str =
        "artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17";

    #[test]
    fn adr_0008_artifact_path_vectors_match_the_documented_corpus_table()
    -> Result<(), fieldnotes_domain::IdError> {
        let id = ArtifactId::parse(KNOWN_ARTIFACT_ID)?;
        for (media_type, expected_path) in [
            (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.docx",
            ),
            (
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.xlsx",
            ),
            (
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.pptx",
            ),
            (
                "application/vnd.oasis.opendocument.text",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.odt",
            ),
            (
                "application/vnd.oasis.opendocument.spreadsheet",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.ods",
            ),
            (
                "application/vnd.oasis.opendocument.presentation",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.odp",
            ),
            (
                "text/csv",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.csv",
            ),
            (
                "application/rtf",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.rtf",
            ),
            (
                "image/heic",
                "artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.heic",
            ),
        ] {
            assert_eq!(
                artifact_relative_path(&id, Some(media_type)),
                expected_path,
                "{media_type} must resolve to {expected_path}"
            );
        }
        Ok(())
    }

    #[test]
    fn domain_prefixes_separate_the_hash_spaces() {
        let body = "x\n";
        let content = content_hash_value(body);
        let record = record_fingerprint(body);
        assert!(content.starts_with("fn-content-v1-sha256:"));
        assert!(record.starts_with("fn-record-v1-sha256:"));
        assert_ne!(
            content["fn-content-v1-sha256:".len()..],
            record["fn-record-v1-sha256:".len()..]
        );
    }
}
