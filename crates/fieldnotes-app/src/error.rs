//! Application-level failures.

use std::fmt;
use std::path::PathBuf;

use fieldnotes_domain::{DatetimeError, FieldIdError, IdError};
use fieldnotes_format::ValidationError;
use fieldnotes_store::StoreError;

/// Why a use case failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum AppError {
    /// Storage or discovery failed.
    Store(StoreError),
    /// A record could not be built or a file did not validate.
    Record(ValidationError),
    /// An identifier could not be generated.
    Id(IdError),
    /// A datetime could not be represented.
    Datetime(DatetimeError),
    /// The configured UTC offset is outside `-23:59`..=`+23:59`.
    InvalidOffset {
        /// The rejected offset in minutes.
        minutes: i32,
    },
    /// A Note would have no content at all.
    EmptyNote,
    /// A voice import is not a recognized audio file.
    NotAudio {
        /// The rejected file.
        path: PathBuf,
        /// The detected media type, when any type was detected.
        detected: Option<String>,
    },
    /// `inspect` was given a record ID or path that the notebook does not hold.
    UnknownTarget {
        /// The requested target.
        target: String,
    },
    /// A `fields add` candidate ID failed [`FieldId::parse`].
    ///
    /// [`FieldId::parse`]: fieldnotes_domain::FieldId::parse
    InvalidFieldId {
        /// The rejected candidate ID (`<type>_<label>`).
        candidate: String,
        /// Why it was rejected.
        source: FieldIdError,
    },
    /// `self` was named as the target of a Field-configuration command that
    /// only applies to configured external Fields.
    ///
    /// `self` is the built-in Field: it has no process, no executable, and no
    /// configuration file, so it cannot be added, reconfigured, or removed.
    CannotConfigureSelf,
    /// `fields add` named an already-configured Field ID.
    ///
    /// A Field ID is immutable once configured (fields.md): reconfiguring one
    /// in place would risk silently changing what `(instance_id, field_id)`
    /// has meant for Notes it already produced. Remove it first, or choose a
    /// different label.
    FieldAlreadyConfigured {
        /// The already-configured ID.
        id: String,
    },
    /// A Field-configuration command named an ID with no stored configuration
    /// (and that is not the built-in `self` Field).
    FieldNotConfigured {
        /// The unconfigured ID.
        id: String,
    },
    /// A manifest value could not be decoded or did not satisfy the A2
    /// schema's own validation rules.
    InvalidManifest {
        /// Why it was rejected.
        message: String,
    },
    /// A freshly reported manifest disagrees with the stored snapshot in a
    /// way A2 requires a migration for: a declared property's type or
    /// cardinality changed or was removed, or `cursor_format_version`
    /// changed.
    ManifestMigrationRequired {
        /// The disagreement, in reviewable terms.
        detail: String,
    },
    /// A `describe` run did not produce a usable manifest.
    ///
    /// Separate from [`AppError::InvalidManifest`], which is about a manifest
    /// value that exists: this is about not getting one at all, because the
    /// executable could not be started, answered nothing, or answered
    /// something that is not a manifest.
    FieldDescribe {
        /// What happened, already phrased for a user.
        message: String,
    },
    /// A credential could not be resolved, obtained, or delivered.
    ///
    /// Never carries material: see [`crate::credentials::CredentialFailure`].
    Credential(crate::credentials::CredentialFailure),
}

impl AppError {
    /// A stable lowercase kind label for machine-readable output.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Store(error) => error.kind(),
            AppError::Record(_) => "invalid_record",
            AppError::Id(_) => "identifier",
            AppError::Datetime(_) => "datetime",
            AppError::InvalidOffset { .. } => "invalid_offset",
            AppError::EmptyNote => "empty_note",
            AppError::NotAudio { .. } => "not_audio",
            AppError::UnknownTarget { .. } => "unknown_target",
            AppError::InvalidFieldId { .. } => "invalid_field_id",
            AppError::CannotConfigureSelf => "cannot_configure_self",
            AppError::FieldAlreadyConfigured { .. } => "field_already_configured",
            AppError::FieldNotConfigured { .. } => "field_not_configured",
            AppError::InvalidManifest { .. } => "invalid_manifest",
            AppError::ManifestMigrationRequired { .. } => "manifest_migration_required",
            AppError::FieldDescribe { .. } => "field_describe",
            AppError::Credential(failure) => failure.kind(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Store(error) => write!(f, "{error}"),
            AppError::Record(error) => write!(f, "{error}"),
            AppError::Id(error) => write!(f, "could not generate an identifier: {error}"),
            AppError::Datetime(error) => write!(f, "could not represent a datetime: {error}"),
            AppError::InvalidOffset { minutes } => write!(
                f,
                "UTC offset of {minutes} minutes is outside the representable range"
            ),
            AppError::EmptyNote => write!(
                f,
                "a Note needs text, a title, or an imported file; nothing was supplied"
            ),
            AppError::NotAudio { path, detected } => match detected {
                Some(detected) => write!(
                    f,
                    "`{}` is {detected}, not audio; import it with --file instead",
                    path.display()
                ),
                None => write!(
                    f,
                    "`{}` is not a recognized audio file; import it with --file instead",
                    path.display()
                ),
            },
            AppError::UnknownTarget { target } => {
                write!(f, "no record in this notebook matches `{target}`")
            }
            AppError::InvalidFieldId { candidate, source } => {
                write!(f, "`{candidate}` is not a valid Field ID: {source}")
            }
            AppError::CannotConfigureSelf => write!(
                f,
                "`self` is the built-in Field; it has no configuration to add, show, or remove"
            ),
            AppError::FieldAlreadyConfigured { id } => write!(
                f,
                "`{id}` is already configured; remove it first or choose a different label"
            ),
            AppError::FieldNotConfigured { id } => {
                write!(f, "no Field `{id}` is configured in this notebook")
            }
            AppError::InvalidManifest { message } => {
                write!(f, "manifest is invalid: {message}")
            }
            AppError::ManifestMigrationRequired { detail } => {
                write!(f, "manifest change requires a migration: {detail}")
            }
            AppError::FieldDescribe { message } => write!(f, "{message}"),
            AppError::Credential(failure) => write!(f, "{failure}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Store(error) => Some(error),
            AppError::Record(error) => Some(error),
            AppError::Id(error) => Some(error),
            AppError::Datetime(error) => Some(error),
            AppError::InvalidFieldId { source, .. } => Some(source),
            AppError::Credential(failure) => Some(failure),
            _ => None,
        }
    }
}

impl From<crate::credentials::CredentialFailure> for AppError {
    fn from(failure: crate::credentials::CredentialFailure) -> Self {
        AppError::Credential(failure)
    }
}

impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        AppError::Store(error)
    }
}

impl From<ValidationError> for AppError {
    fn from(error: ValidationError) -> Self {
        AppError::Record(error)
    }
}

impl From<IdError> for AppError {
    fn from(error: IdError) -> Self {
        AppError::Id(error)
    }
}

impl From<DatetimeError> for AppError {
    fn from(error: DatetimeError) -> Self {
        AppError::Datetime(error)
    }
}
