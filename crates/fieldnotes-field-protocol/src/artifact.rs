//! Staged-artifact handle resolution and digest verification.
//!
//! Original bytes reach core by staged file with a **core-derived path**. Core
//! creates the per-run staging directory, names its absolute path in the
//! collection request, and removes it when the run ends. The Field writes each
//! original into that directory under a handle, and the record references the
//! handle.
//!
//! # Path safety is structural, not defensive
//!
//! A handle is a single path segment from a closed character set. It admits no
//! dot, no separator, and no traversal sequence, and it excludes the reserved
//! Windows device names, so a handle **cannot be a path however it is spelled**
//! and traversal is a grammar error rather than something to sanitize. Core
//! joins the handle to the staging directory with
//! [`std::path::Path::join`] — never by string concatenation — opens it without
//! following symlinks, requires a regular file whose identity is unchanged
//! between check and use, and bounds the read by the declared length.
//!
//! Core **always** computes its own SHA-256 over the staged bytes. A
//! Field-declared digest is a detection aid: a disagreement rejects the record.
//! Trusting a declared digest would let a connector bug or a corrupted download
//! produce a Note pointing at an artifact identity that does not describe its
//! bytes.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};

use crate::codes::RejectionCode;
use crate::limits::Limits;
use crate::message::{ArtifactKind, ArtifactRef};

/// Reserved Windows device names a handle may never spell.
///
/// Excluded on **every** platform, so a notebook does not become non-portable
/// by being written on one.
pub const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Why an artifact handle was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleError {
    /// Empty, or longer than the 64-byte grammar allows.
    Length {
        /// The refused length in bytes.
        bytes: usize,
    },
    /// Contains a byte the closed character set excludes, which includes every
    /// separator, every dot, and therefore every traversal sequence.
    Character {
        /// The refused byte.
        byte: u8,
    },
    /// Does not begin with a lowercase letter or a digit.
    FirstCharacter,
    /// Spells a reserved Windows device name.
    ReservedDeviceName,
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandleError::Length { bytes } => write!(
                f,
                "an artifact handle is 1 to {} bytes, not {bytes}",
                ArtifactHandle::MAX_BYTES
            ),
            HandleError::Character { byte } => write!(
                f,
                "an artifact handle is a single segment of [a-z0-9_-]; byte {byte:#04x} is not in \
                 that set, so a separator, a dot, or a traversal sequence is a grammar error \
                 rather than something to sanitize"
            ),
            HandleError::FirstCharacter => write!(
                f,
                "an artifact handle begins with a lowercase letter or a digit"
            ),
            HandleError::ReservedDeviceName => write!(
                f,
                "an artifact handle never spells a reserved Windows device name, on any platform"
            ),
        }
    }
}

impl std::error::Error for HandleError {}

/// A validated single-segment staging name.
///
/// The only way to obtain one is [`ArtifactHandle::parse`], so a value of this
/// type is always safe to join to the staging directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactHandle(String);

impl ArtifactHandle {
    /// The longest handle the grammar admits.
    pub const MAX_BYTES: usize = 64;

    /// Validates a handle against the closed grammar.
    pub fn parse(text: &str) -> Result<Self, HandleError> {
        if text.is_empty() || text.len() > Self::MAX_BYTES {
            return Err(HandleError::Length { bytes: text.len() });
        }
        let bytes = text.as_bytes();
        match bytes[0] {
            b'a'..=b'z' | b'0'..=b'9' => {}
            _ => return Err(HandleError::FirstCharacter),
        }
        for byte in bytes {
            let allowed = byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || *byte == b'_'
                || *byte == b'-';
            if !allowed {
                return Err(HandleError::Character { byte: *byte });
            }
        }
        if RESERVED_DEVICE_NAMES.contains(&text) {
            return Err(HandleError::ReservedDeviceName);
        }
        Ok(ArtifactHandle(text.to_owned()))
    }

    /// The validated handle.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path this handle resolves to inside `staging_dir`.
    ///
    /// Uses [`Path::join`] with a single validated segment, so the result can
    /// never leave the staging directory.
    #[must_use]
    pub fn resolve_in(&self, staging_dir: &Path) -> std::path::PathBuf {
        staging_dir.join(&self.0)
    }
}

impl fmt::Display for ArtifactHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ArtifactHandle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ArtifactHandle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ArtifactHandle::parse(&text).map_err(de::Error::custom)
    }
}

/// What core already stores, for the `digest_only` reference kind.
///
/// Core accepts a digest-only reference only when it already stores that
/// digest, and otherwise rejects the record so the Field retries with bytes.
/// This is what stops a mail Field from re-downloading every forwarded
/// attachment on every sync, and it is safe because A1 already establishes that
/// the same digest is the same bytes.
pub trait ArtifactDigestIndex {
    /// Whether the notebook already stores an artifact with this digest.
    fn contains_digest(&self, digest: &str) -> bool;
}

impl ArtifactDigestIndex for std::collections::BTreeSet<String> {
    fn contains_digest(&self, digest: &str) -> bool {
        self.contains(digest)
    }
}

/// An index that stores nothing, for a run where no digest-only reference can
/// be honoured.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStoredArtifacts;

impl ArtifactDigestIndex for NoStoredArtifacts {
    fn contains_digest(&self, _digest: &str) -> bool {
        false
    }
}

/// An accepted artifact reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedArtifact {
    /// **Core's own** digest, which is what the A1 artifact identity and the
    /// notebook path are derived from.
    pub digest: String,
    /// The byte length core measured, or the length of the bytes it already
    /// stores.
    pub byte_length: u64,
    /// Whether core reused bytes it already stored rather than reading staged
    /// bytes.
    pub reused: bool,
}

/// Why an artifact reference was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRejection {
    /// The rejection code core reports.
    pub code: RejectionCode,
    /// Why, in reviewable terms.
    pub detail: String,
}

impl ArtifactRejection {
    fn new(code: RejectionCode, detail: impl Into<String>) -> Self {
        ArtifactRejection {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ArtifactRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for ArtifactRejection {}

/// The size of the streaming read buffer.
const READ_CHUNK: usize = 64 * 1024;

/// `FILE_FLAG_OPEN_REPARSE_POINT`, so a Windows open reaches the reparse point
/// itself instead of following a symlink or junction out of the staging
/// directory.
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// A file's identity, used to require that the entry checked before the open is
/// the entry the open actually reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    /// Present on every platform: a swap between check and use almost always
    /// changes the length too.
    length: u64,
}

impl FileIdentity {
    fn of(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
                length: metadata.len(),
            }
        }
        #[cfg(not(unix))]
        {
            FileIdentity {
                length: metadata.len(),
            }
        }
    }
}

fn open_without_following(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

/// Resolves one artifact reference, computing core's own digest over staged
/// bytes.
///
/// `staging_dir` must be the directory core named in the collection request.
/// The reference's handle is joined to it and nowhere else.
pub fn resolve_artifact(
    staging_dir: &Path,
    reference: &ArtifactRef,
    limits: &Limits,
    index: &dyn ArtifactDigestIndex,
) -> Result<ResolvedArtifact, ArtifactRejection> {
    match reference.kind {
        ArtifactKind::DigestOnly => {
            let Some(declared) = &reference.sha256 else {
                return Err(ArtifactRejection::new(
                    RejectionCode::ProtocolSchemaInvalid,
                    "a digest-only reference is nothing but its digest",
                ));
            };
            if index.contains_digest(declared.as_str()) {
                Ok(ResolvedArtifact {
                    digest: declared.as_str().to_owned(),
                    byte_length: 0,
                    reused: true,
                })
            } else {
                Err(ArtifactRejection::new(
                    RejectionCode::ArtifactUnknownDigest,
                    format!(
                        "no artifact with digest {declared} is stored, and core will not create a \
                         Note referencing bytes it does not hold; the Field must retry with bytes"
                    ),
                ))
            }
        }
        ArtifactKind::Staged => resolve_staged(staging_dir, reference, limits),
    }
}

fn resolve_staged(
    staging_dir: &Path,
    reference: &ArtifactRef,
    limits: &Limits,
) -> Result<ResolvedArtifact, ArtifactRejection> {
    // The grammar check happens first, so a hostile spelling is refused before
    // anything touches a filesystem.
    let handle = reference
        .parsed_handle()
        .map_err(|error| ArtifactRejection::new(error.code, error.message))?;
    let Some(declared_length) = reference.byte_length else {
        return Err(ArtifactRejection::new(
            RejectionCode::ProtocolSchemaInvalid,
            "a staged reference declares its byte length so core can bound the read",
        ));
    };
    if declared_length > limits.max_artifact_bytes {
        return Err(ArtifactRejection::new(
            RejectionCode::ArtifactOversized,
            format!(
                "the declared length {declared_length} exceeds the run's artifact bound {}",
                limits.max_artifact_bytes
            ),
        ));
    }

    let path = handle.resolve_in(staging_dir);
    let before = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ArtifactRejection::new(
                RejectionCode::ArtifactMissingStagedFile,
                format!("handle {handle} names nothing inside the run's staging directory"),
            ));
        }
        Err(error) => {
            return Err(ArtifactRejection::new(
                RejectionCode::ArtifactMissingStagedFile,
                format!("handle {handle} could not be examined: {error}"),
            ));
        }
    };
    if !before.file_type().is_file() {
        // A symlink, a directory, a socket, or a device. v1's closed vocabulary
        // pins this to `artifact.invalid_handle`, per transcript 11.
        return Err(ArtifactRejection::new(
            RejectionCode::ArtifactInvalidHandle,
            format!(
                "the staged entry for handle {handle} is not a regular file, so core reads \
                 nothing: it refuses symlinks and non-regular files rather than following them \
                 out of the staging directory"
            ),
        ));
    }
    let identity_before = FileIdentity::of(&before);

    let mut file = open_without_following(&path).map_err(|error| {
        ArtifactRejection::new(
            RejectionCode::ArtifactMissingStagedFile,
            format!("handle {handle} could not be opened: {error}"),
        )
    })?;
    let after = file.metadata().map_err(|error| {
        ArtifactRejection::new(
            RejectionCode::ArtifactInvalidHandle,
            format!("the opened staged file for handle {handle} could not be examined: {error}"),
        )
    })?;
    if !after.file_type().is_file() || FileIdentity::of(&after) != identity_before {
        return Err(ArtifactRejection::new(
            RejectionCode::ArtifactInvalidHandle,
            format!(
                "the staged entry for handle {handle} changed identity between check and use, so \
                 core reads nothing"
            ),
        ));
    }

    // Stream, bounded by one byte past the declared length so a longer file is
    // detected rather than silently truncated.
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; READ_CHUNK];
    let mut measured: u64 = 0;
    let ceiling = declared_length.saturating_add(1);
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            ArtifactRejection::new(
                RejectionCode::ArtifactMissingStagedFile,
                format!("staged bytes for handle {handle} could not be read: {error}"),
            )
        })?;
        if read == 0 {
            break;
        }
        measured = measured.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if measured > ceiling {
            return Err(ArtifactRejection::new(
                RejectionCode::ArtifactLengthMismatch,
                format!(
                    "handle {handle} declared {declared_length} bytes but the staged file is \
                     longer; core rejects it rather than truncating the read"
                ),
            ));
        }
        hasher.update(&buffer[..read]);
    }
    if measured != declared_length {
        return Err(ArtifactRejection::new(
            RejectionCode::ArtifactLengthMismatch,
            format!("handle {handle} declared {declared_length} bytes but staged {measured}"),
        ));
    }

    let digest = hex(&hasher.finalize());
    if let Some(declared) = &reference.sha256
        && declared.as_str() != digest
    {
        return Err(ArtifactRejection::new(
            RejectionCode::ArtifactDigestMismatch,
            format!(
                "core's own digest {digest} disagrees with the declared {declared}; the declared \
                 digest is a detection aid and never the basis for the artifact ID"
            ),
        ));
    }
    Ok(ResolvedArtifact {
        digest,
        byte_length: measured,
        reused: false,
    })
}

fn hex(bytes: &[u8]) -> String {
    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Two lowercase hex digits per byte, matching A1's artifact identity.
        rendered.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        rendered.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handle_grammar_admits_a_single_segment() {
        assert!(ArtifactHandle::parse("a0001").is_ok());
        assert!(ArtifactHandle::parse("att-0001").is_ok());
        assert!(ArtifactHandle::parse("a_b-9").is_ok());
        assert!(ArtifactHandle::parse(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn the_handle_grammar_refuses_every_way_of_spelling_a_path() {
        for hostile in [
            "../../../../etc/passwd",
            "/Users/joe/.ssh/id_ed25519",
            "sub/dir",
            "sub\\dir",
            "a.b",
            "..",
            ".",
            "C:handle",
            "handle ",
            "Handle",
            "-leading",
            "_leading",
            "",
        ] {
            assert!(
                ArtifactHandle::parse(hostile).is_err(),
                "{hostile:?} must be refused"
            );
        }
        assert!(ArtifactHandle::parse(&"a".repeat(65)).is_err());
    }

    #[test]
    fn reserved_windows_device_names_are_refused_on_every_platform() {
        for reserved in [
            "con", "prn", "aux", "nul", "com1", "com3", "com9", "lpt1", "lpt9",
        ] {
            assert_eq!(
                ArtifactHandle::parse(reserved),
                Err(HandleError::ReservedDeviceName),
                "{reserved} must be refused"
            );
        }
        // A reserved name with a suffix is an ordinary handle.
        assert!(ArtifactHandle::parse("nul1").is_ok());
        assert!(ArtifactHandle::parse("com0").is_ok());
    }

    #[test]
    fn a_handle_resolves_only_inside_the_staging_directory() -> Result<(), HandleError> {
        let handle = ArtifactHandle::parse("a0001")?;
        let staging = Path::new("/tmp/staging");
        let resolved = handle.resolve_in(staging);
        assert_eq!(resolved.parent(), Some(staging));
        assert_eq!(
            resolved.file_name().and_then(std::ffi::OsStr::to_str),
            Some("a0001")
        );
        Ok(())
    }

    #[test]
    fn hex_rendering_matches_the_a1_artifact_spelling() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
    }
}
