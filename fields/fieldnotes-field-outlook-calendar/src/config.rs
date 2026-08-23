//! Extracting the configured Entra tenant from `CollectRequest.config` and
//! deriving the portable exact-source scope from it.

use std::fmt;

use fieldnotes_field_protocol::grammar::SourceScope;
use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

/// Why the configured tenant could not be resolved.
#[derive(Debug)]
pub(crate) enum ConfigError {
    /// `tenant_id` is missing from `config`.
    Missing,
    /// `tenant_id` is present but is not a text scalar.
    WrongShape,
    /// `tenant_id` does not compose into a valid portable source scope.
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing => write!(
                f,
                "config.{} is required and names the Entra tenant to collect against",
                crate::constants::CONFIG_TENANT_ID
            ),
            ConfigError::WrongShape => write!(
                f,
                "config.{} must be a text scalar naming the tenant GUID",
                crate::constants::CONFIG_TENANT_ID
            ),
            ConfigError::Invalid(reason) => write!(f, "configured tenant is unusable: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Resolves the configured tenant to the portable exact-source scope every
/// record and cursor in this run shares.
///
/// A tenant GUID is already non-secret and already cross-instance-stable, so
/// unlike the `local` Field's root path this scope embeds it directly rather
/// than hashing it -- exactly the shape every other Microsoft Field's manifest
/// declares (`microsoft-graph:tenant/<tenant-guid>`).
pub(crate) fn resolve_scope(config: &ConfigMap) -> Result<SourceScope, ConfigError> {
    let value = config
        .get(crate::constants::CONFIG_TENANT_ID)
        .ok_or(ConfigError::Missing)?;
    let PropertyValue::Text(tenant_id) = value else {
        return Err(ConfigError::WrongShape);
    };
    let text = format!("{}{tenant_id}", crate::constants::SCOPE_PREFIX);
    SourceScope::parse(&text).map_err(|error| ConfigError::Invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, resolve_scope};
    use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

    fn config_key(name: &str) -> fieldnotes_field_protocol::grammar::PropertyNameToken {
        fieldnotes_field_protocol::grammar::PropertyNameToken::parse(name)
            .unwrap_or_else(|error| panic!("{name} must be a valid config key: {error}"))
    }

    #[test]
    fn a_missing_tenant_id_is_reported() {
        let config = ConfigMap::new();
        assert!(matches!(resolve_scope(&config), Err(ConfigError::Missing)));
    }

    #[test]
    fn a_non_text_tenant_id_is_reported() {
        let mut config = ConfigMap::new();
        config.insert(config_key("tenant_id"), PropertyValue::Boolean(true));
        assert!(matches!(
            resolve_scope(&config),
            Err(ConfigError::WrongShape)
        ));
    }

    #[test]
    fn a_configured_tenant_composes_the_declared_scope_shape() {
        let mut config = ConfigMap::new();
        config.insert(
            config_key("tenant_id"),
            PropertyValue::Text("8d820000-0000-7000-8000-000000000001".to_owned()),
        );
        let scope = resolve_scope(&config).unwrap_or_else(|error| panic!("must resolve: {error}"));
        assert_eq!(
            scope.as_str(),
            "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001"
        );
    }
}
