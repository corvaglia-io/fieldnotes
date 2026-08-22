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

/// Lowercase hexadecimal SHA-256 of exact bytes, with no domain prefix.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
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
    let mut hasher = Sha256::new();
    hasher.update(CONTENT_DOMAIN);
    hasher.update(normalized_body.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::from("fn-content-v1-sha256:");
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The `fn-record-v1-sha256:<hex>` fingerprint over a canonical semantic
/// record encoding.
///
/// The SHA-256 input is `fieldnotes-record-v1`, one NUL byte, then the exact
/// canonical semantic record bytes.
#[must_use]
pub fn record_fingerprint(canonical_semantic_record: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RECORD_DOMAIN);
    hasher.update(canonical_semantic_record.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::from("fn-record-v1-sha256:");
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
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
