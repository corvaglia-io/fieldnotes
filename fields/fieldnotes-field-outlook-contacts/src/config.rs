//! Extracting this Field's non-secret configuration from
//! `CollectRequest.config`.

use std::fmt;

use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

/// Why the configuration could not be resolved.
#[derive(Debug)]
pub(crate) enum ConfigError {
    /// [`crate::constants::CONFIG_TENANT_ID`] is missing.
    MissingTenantId,
    /// A configured key is present but is not a text scalar.
    WrongShape {
        /// The offending key.
        key: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingTenantId => write!(
                f,
                "config.{} is required in this release and names the tenant a contact's \
                 portable scope is derived from",
                crate::constants::CONFIG_TENANT_ID
            ),
            ConfigError::WrongShape { key } => {
                write!(f, "config.{key} must be a text scalar")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// This Field's resolved, non-secret configuration for one run.
pub(crate) struct ResolvedConfig {
    /// The tenant a contact's portable scope is derived from.
    pub(crate) tenant_id: String,
    /// The target mailbox's Graph resource segment: `/me` for the signed-in
    /// user, or `/users/<mailbox>` when [`crate::constants::CONFIG_MAILBOX`]
    /// names one.
    pub(crate) mailbox_resource: String,
    /// The Graph service root to call against.
    pub(crate) graph_base_url: String,
}

fn text_of(config: &ConfigMap, key: &'static str) -> Result<Option<String>, ConfigError> {
    match config.get(key) {
        None => Ok(None),
        Some(PropertyValue::Text(text)) => Ok(Some(text.clone())),
        Some(_) => Err(ConfigError::WrongShape { key }),
    }
}

/// Resolves this Field's configuration for one collect run.
pub(crate) fn resolve(config: &ConfigMap) -> Result<ResolvedConfig, ConfigError> {
    let tenant_id =
        text_of(config, crate::constants::CONFIG_TENANT_ID)?.ok_or(ConfigError::MissingTenantId)?;
    let mailbox_resource = match text_of(config, crate::constants::CONFIG_MAILBOX)? {
        Some(mailbox) => format!("/users/{mailbox}"),
        None => "/me".to_owned(),
    };
    let graph_base_url = text_of(config, crate::constants::CONFIG_GRAPH_BASE_URL)?
        .unwrap_or_else(|| fieldnotes_msgraph::DEFAULT_BASE_URL.to_owned());
    Ok(ResolvedConfig {
        tenant_id,
        mailbox_resource,
        graph_base_url,
    })
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, resolve};
    use fieldnotes_field_protocol::grammar::PropertyNameToken;
    use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

    fn key(name: &str) -> PropertyNameToken {
        PropertyNameToken::parse(name).unwrap_or_else(|error| panic!("{name}: {error}"))
    }

    #[test]
    fn a_missing_tenant_id_is_reported() {
        let config = ConfigMap::new();
        assert!(matches!(
            resolve(&config),
            Err(ConfigError::MissingTenantId)
        ));
    }

    #[test]
    fn a_bare_tenant_defaults_to_the_signed_in_users_own_mailbox() {
        let mut config = ConfigMap::new();
        config.insert(key("tenant_id"), PropertyValue::Text("tenant-a".to_owned()));
        let resolved = resolve(&config).unwrap_or_else(|error| panic!("must resolve: {error}"));
        assert_eq!(resolved.tenant_id, "tenant-a");
        assert_eq!(resolved.mailbox_resource, "/me");
        assert_eq!(
            resolved.graph_base_url,
            fieldnotes_msgraph::DEFAULT_BASE_URL
        );
    }

    #[test]
    fn a_configured_mailbox_and_base_url_are_honoured() {
        let mut config = ConfigMap::new();
        config.insert(key("tenant_id"), PropertyValue::Text("tenant-a".to_owned()));
        config.insert(
            key("mailbox"),
            PropertyValue::Text("bob@example.net".to_owned()),
        );
        config.insert(
            key("graph_base_url"),
            PropertyValue::Text("http://127.0.0.1:9999".to_owned()),
        );
        let resolved = resolve(&config).unwrap_or_else(|error| panic!("must resolve: {error}"));
        assert_eq!(resolved.mailbox_resource, "/users/bob@example.net");
        assert_eq!(resolved.graph_base_url, "http://127.0.0.1:9999");
    }
}
