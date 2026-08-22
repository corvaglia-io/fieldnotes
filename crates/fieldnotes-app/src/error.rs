//! Application-level failures.

use std::fmt;
use std::path::PathBuf;

use fieldnotes_domain::{DatetimeError, IdError};
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
            _ => None,
        }
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
