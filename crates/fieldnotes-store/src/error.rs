//! Storage failures, reported with the path that produced them.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use fieldnotes_format::ValidationError;

/// Why a storage operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// A filesystem operation failed.
    Io {
        /// What the store was doing, for an actionable message.
        action: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// A file on disk violates the public notebook contract.
    Invalid {
        /// The offending file.
        path: PathBuf,
        /// Why it was rejected.
        source: ValidationError,
    },
    /// No notebook was found at or above the starting directory.
    NotANotebook {
        /// Where discovery started.
        start: PathBuf,
    },
    /// The requested notebook root exists but is not a directory.
    NotADirectory {
        /// The offending path.
        path: PathBuf,
    },
    /// The directory holds content that is not a Fieldnotes notebook, so
    /// initialization refuses to adopt it.
    UnexpectedTree {
        /// The candidate notebook root.
        path: PathBuf,
        /// One unexpected entry name, as evidence for the refusal.
        entry: String,
    },
    /// A stored artifact's bytes no longer hash to its content-addressed ID.
    ArtifactCorrupt {
        /// The stored file.
        path: PathBuf,
    },
    /// The user profile file exists but does not parse.
    ///
    /// A profile is meant to be hand-edited, so a malformed file fails loudly
    /// with the offending line rather than silently falling back to unset
    /// settings, which would hide the user's own typo.
    InvalidProfile {
        /// The offending file.
        path: PathBuf,
        /// Why it was rejected.
        message: String,
    },
    /// A Field configuration or operational-state file exists but does not
    /// parse.
    ///
    /// Field configuration is meant to be durable, reviewable state; a
    /// malformed file fails loudly with the offending file and reason rather
    /// than silently defaulting to an empty or disabled Field, which would
    /// hide a hand-edit mistake or a partial write that reached disk outside
    /// the atomic-write path.
    InvalidFieldConfig {
        /// The offending file.
        path: PathBuf,
        /// Why it was rejected.
        message: String,
    },
    /// A Field configuration's `config` map used a key name that is
    /// unambiguously credential-shaped, such as `password` or `api_key`.
    ///
    /// This is a fixed denylist on **key names**, not a scan of stored
    /// values: Fieldnotes performs no secret scanning of content (ADR 0006
    /// ruling 3). Field configuration has no credential field at all — a
    /// named credential-profile *reference* is the `0.1.3` shape — so a key
    /// that is only ever used to hold a secret is refused by name before it
    /// is ever written to disk.
    CredentialShapedConfigKey {
        /// The offending key.
        key: String,
    },
}

impl StoreError {
    /// Builds an I/O failure with its context.
    ///
    /// Public so that callers doing notebook-adjacent I/O — reading a file the
    /// user asked to import, for instance — report failures in the same shape.
    pub fn io(action: &'static str, path: impl AsRef<Path>, source: io::Error) -> Self {
        StoreError::Io {
            action,
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// A stable lowercase kind label for machine-readable output.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            StoreError::Io { .. } => "io",
            StoreError::Invalid { .. } => "invalid_file",
            StoreError::NotANotebook { .. } => "not_a_notebook",
            StoreError::NotADirectory { .. } => "not_a_directory",
            StoreError::UnexpectedTree { .. } => "unexpected_tree",
            StoreError::ArtifactCorrupt { .. } => "artifact_corrupt",
            StoreError::InvalidProfile { .. } => "invalid_profile",
            StoreError::InvalidFieldConfig { .. } => "invalid_field_config",
            StoreError::CredentialShapedConfigKey { .. } => "credential_shaped_config_key",
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io {
                action,
                path,
                source,
            } => write!(f, "failed to {action} `{}`: {source}", path.display()),
            StoreError::Invalid { path, source } => {
                write!(f, "`{}` is not a valid record: {source}", path.display())
            }
            StoreError::NotANotebook { start } => write!(
                f,
                "no Fieldnotes notebook found at or above `{}`; run `fieldnotes init` first",
                start.display()
            ),
            StoreError::NotADirectory { path } => {
                write!(f, "`{}` is not a directory", path.display())
            }
            StoreError::UnexpectedTree { path, entry } => write!(
                f,
                "`{}` is not a Fieldnotes notebook and is not empty (found `{entry}`); \
                 choose an empty directory or an existing notebook",
                path.display()
            ),
            StoreError::ArtifactCorrupt { path } => write!(
                f,
                "stored artifact `{}` no longer matches its content address",
                path.display()
            ),
            StoreError::InvalidProfile { path, message } => {
                write!(f, "profile `{}` is malformed: {message}", path.display())
            }
            StoreError::InvalidFieldConfig { path, message } => write!(
                f,
                "Field configuration `{}` is malformed: {message}",
                path.display()
            ),
            StoreError::CredentialShapedConfigKey { key } => write!(
                f,
                "configuration key `{key}` looks like it is meant to hold a credential; \
                 Field configuration is non-secret by contract, so credential material must \
                 never be stored here"
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Io { source, .. } => Some(source),
            StoreError::Invalid { source, .. } => Some(source),
            _ => None,
        }
    }
}
