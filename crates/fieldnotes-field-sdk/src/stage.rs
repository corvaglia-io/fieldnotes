//! Staging local bytes for core to hash, and the handle-naming convention
//! that pairs a staged file with the record that references it.
//!
//! Every Field with local bytes to stage needs exactly the same three steps,
//! in exactly this order: read the bytes once, compute their SHA-256 over
//! that same in-memory copy, and write them into the run's staging
//! directory under a handle the record's artifact reference then names.
//! [`stage_and_hash`] does the last two together so a Field never reads or
//! hashes the bytes twice for a file it actually stages. [`sha256_hex`] is
//! the same digest alone, for the same bytes when they end up not staged at
//! all -- excluded by retention policy, say -- whose reference still names
//! their digest as a detection aid. Core always recomputes its own digest
//! over the staged file regardless (see
//! [`fieldnotes_field_protocol::artifact::resolve_artifact`]), so neither
//! digest is ever load-bearing on its own.

use std::fmt;
use std::io;
use std::path::Path;

use fieldnotes_field_protocol::artifact::{ArtifactHandle, HandleError};
use sha2::{Digest, Sha256};

/// Why staging bytes failed.
#[derive(Debug)]
pub enum StageError {
    /// `handle` does not satisfy the closed handle grammar.
    InvalidHandle(HandleError),
    /// The staging directory could not be created, or the bytes could not be
    /// written.
    Io(io::Error),
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageError::InvalidHandle(error) => write!(f, "invalid staging handle: {error}"),
            StageError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StageError::InvalidHandle(error) => Some(error),
            StageError::Io(error) => Some(error),
        }
    }
}

/// The handle a Field emits for the artifact attached to the record with
/// per-run sequence number `seq`: `a`, followed by `seq` zero-padded to seven
/// digits.
///
/// This is a convention, not a protocol requirement -- a handle only has to
/// satisfy [`ArtifactHandle`]'s closed grammar -- but it is a convenient
/// default for any Field with at most one staged artifact per record,
/// keeping a staged file's name legible in a directory listing while never
/// colliding within one run (a v1 sequence number is unique per run).
#[must_use]
pub fn handle_for_seq(seq: u64) -> String {
    format!("a{seq:07}")
}

/// Computes the lowercase hex SHA-256 digest of `bytes`, without staging
/// them.
///
/// Useful when a Field needs the same "detection aid" digest a staged
/// artifact reference's `sha256` member carries -- core always recomputes its
/// own digest over the staged bytes regardless (see
/// [`fieldnotes_field_protocol::artifact::resolve_artifact`]), so a declared
/// digest is never load-bearing -- for an artifact whose bytes end up not
/// staged at all: for example, one excluded by the run's media-type
/// retention policy, whose reference still names the digest of the bytes it
/// declined to keep.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    crate::hex::to_lower_hex(&hasher.finalize())
}

/// Hashes `bytes` once and writes them into `staging_dir` under `handle`,
/// returning the lowercase hex SHA-256 digest.
///
/// `handle` is validated against the closed artifact-handle grammar before
/// anything touches the filesystem, so a hostile or malformed handle is
/// [`StageError::InvalidHandle`] rather than a write attempted at an
/// unintended path. The staging directory is created if it does not already
/// exist: core creates it before spawning a Field, but creating it here too
/// costs nothing and lets this helper also work against a directory a test
/// created without going through a real collection request.
pub fn stage_and_hash(
    staging_dir: &Path,
    handle: &str,
    bytes: &[u8],
) -> Result<String, StageError> {
    let handle = ArtifactHandle::parse(handle).map_err(StageError::InvalidHandle)?;
    std::fs::create_dir_all(staging_dir).map_err(StageError::Io)?;
    let digest = sha256_hex(bytes);
    std::fs::write(handle.resolve_in(staging_dir), bytes).map_err(StageError::Io)?;
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::{handle_for_seq, sha256_hex, stage_and_hash};
    use fieldnotes_test_support::TempDir;

    #[test]
    fn the_seq_handle_convention_zero_pads_to_seven_digits() {
        assert_eq!(handle_for_seq(1), "a0000001");
        assert_eq!(handle_for_seq(42), "a0000042");
        assert_eq!(handle_for_seq(1_000_000), "a1000000");
    }

    #[test]
    fn sha256_hex_matches_the_digest_staging_returns_without_writing_anything()
    -> std::io::Result<()> {
        let staging = TempDir::new("sha256-hex")?;
        let hashed_only = sha256_hex(b"hello world");
        let staged = stage_and_hash(staging.path(), "a0000002", b"hello world")
            .unwrap_or_else(|error| panic!("must stage: {error}"));
        assert_eq!(hashed_only, staged);
        Ok(())
    }

    #[test]
    fn staging_writes_the_exact_bytes_and_returns_their_digest() -> std::io::Result<()> {
        let staging = TempDir::new("stage-bytes")?;
        let digest = stage_and_hash(staging.path(), "a0000001", b"hello world")
            .unwrap_or_else(|error| panic!("must stage: {error}"));
        // The well-known SHA-256 of "hello world".
        assert_eq!(
            digest,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        let staged = std::fs::read(staging.path().join("a0000001"))?;
        assert_eq!(staged, b"hello world");
        Ok(())
    }

    #[test]
    fn an_invalid_handle_is_refused_before_touching_the_filesystem() -> std::io::Result<()> {
        let staging = TempDir::new("stage-invalid-handle")?;
        match stage_and_hash(staging.path(), "../../etc/passwd", b"hostile") {
            Err(super::StageError::InvalidHandle(_)) => {}
            other => panic!("a traversal-shaped handle must be refused, got {other:?}"),
        }
        assert!(std::fs::read_dir(staging.path())?.next().is_none());
        Ok(())
    }

    #[test]
    fn staging_creates_the_directory_if_it_does_not_already_exist() -> std::io::Result<()> {
        let parent = TempDir::new("stage-missing-dir")?;
        let nested = parent.path().join("run-staging");
        assert!(!nested.exists());
        let _ = stage_and_hash(&nested, "a0000001", b"bytes")
            .unwrap_or_else(|error| panic!("must stage: {error}"));
        assert!(nested.join("a0000001").exists());
        Ok(())
    }
}
