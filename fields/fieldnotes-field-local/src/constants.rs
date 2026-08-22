//! Constants shared between the `describe` and `collect` operations.
//!
//! Keeping them in one place means the manifest this Field declares and the
//! records and cursors it actually emits can never drift apart by accident.

/// The registered A1 Field stem this Field's manifest declares.
pub(crate) const FIELD_STEM: &str = "local";

/// The registered connector property prefix, including its trailing
/// underscore.
pub(crate) const PROPERTY_PREFIX: &str = "local_";

/// The connector driver name.
pub(crate) const DRIVER_NAME: &str = "local-reference";

/// The Field's own cursor encoding version.
///
/// Changing this is a migration: core refuses to replay a stored cursor
/// written at a different version rather than handing this Field a token it
/// might misread (A2 section 4 and section 9).
pub(crate) const CURSOR_FORMAT_VERSION: u16 = 1;

/// The `file` capability slice's connector-local object kind and the A1
/// primary Note type it maps to.
pub(crate) const OBJECT_KIND_FILE: &str = "file";

/// The `document` capability slice's connector-local object kind and the A1
/// primary Note type it maps to.
pub(crate) const OBJECT_KIND_DOCUMENT: &str = "document";

/// The non-secret configuration key naming the configured root directory.
pub(crate) const CONFIG_ROOT_PATH: &str = "root_path";
