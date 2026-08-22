//! Extracting the configured root directory from `CollectRequest.config`.

use std::fmt;
use std::path::{Path, PathBuf};

use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

/// Why the configured root could not be resolved.
#[derive(Debug)]
pub(crate) enum ConfigError {
    /// `root_path` is missing from `config`.
    Missing,
    /// `root_path` is present but is not a text scalar.
    WrongShape,
    /// `root_path` does not resolve to a readable directory.
    Unusable {
        /// The configured path that could not be resolved.
        path: PathBuf,
        /// Why, in reviewable terms.
        reason: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing => write!(
                f,
                "config.{} is required and names the directory to collect from",
                crate::constants::CONFIG_ROOT_PATH
            ),
            ConfigError::WrongShape => write!(
                f,
                "config.{} must be a text scalar naming an absolute directory path",
                crate::constants::CONFIG_ROOT_PATH
            ),
            ConfigError::Unusable { path, reason } => {
                write!(
                    f,
                    "configured root {} is unusable: {reason}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Resolves the configured root to its canonical absolute path.
///
/// The canonical path becomes the only boundary this Field ever reads
/// inside for the rest of the run: [`crate::walk::walk`] never follows a
/// symlink, so this directory is the sole reachable region.
pub(crate) fn resolve_root(config: &ConfigMap) -> Result<PathBuf, ConfigError> {
    let value = config
        .get(crate::constants::CONFIG_ROOT_PATH)
        .ok_or(ConfigError::Missing)?;
    let PropertyValue::Text(text) = value else {
        return Err(ConfigError::WrongShape);
    };
    let configured = Path::new(text);
    let canonical = std::fs::canonicalize(configured).map_err(|error| ConfigError::Unusable {
        path: configured.to_path_buf(),
        reason: error.to_string(),
    })?;
    if !canonical.is_dir() {
        return Err(ConfigError::Unusable {
            path: canonical,
            reason: "the configured root is not a directory".to_owned(),
        });
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, resolve_root};
    use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

    fn config_key(name: &str) -> fieldnotes_field_protocol::grammar::PropertyNameToken {
        fieldnotes_field_protocol::grammar::PropertyNameToken::parse(name)
            .unwrap_or_else(|error| panic!("{name} must be a valid config key: {error}"))
    }

    #[test]
    fn a_missing_root_path_is_reported() {
        let config = ConfigMap::new();
        assert!(matches!(resolve_root(&config), Err(ConfigError::Missing)));
    }

    #[test]
    fn a_non_text_root_path_is_reported() {
        let mut config = ConfigMap::new();
        config.insert(config_key("root_path"), PropertyValue::Boolean(true));
        assert!(matches!(
            resolve_root(&config),
            Err(ConfigError::WrongShape)
        ));
    }

    #[test]
    fn a_readable_directory_resolves_to_its_canonical_path() -> std::io::Result<()> {
        let temp = fieldnotes_test_support::TempDir::new("config-root")?;
        let mut config = ConfigMap::new();
        config.insert(
            config_key("root_path"),
            PropertyValue::Text(temp.path().display().to_string()),
        );
        let resolved =
            resolve_root(&config).unwrap_or_else(|error| panic!("must resolve: {error}"));
        assert!(resolved.is_dir());
        Ok(())
    }
}
