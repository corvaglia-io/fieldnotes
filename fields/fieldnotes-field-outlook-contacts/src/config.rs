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
    /// [`crate::constants::CONFIG_MAILBOX`] was configured, but Microsoft
    /// Graph exposes no documented contacts-delta resource this Field could
    /// build from it.
    ///
    /// Graph's own `contact: delta` reference documents only
    /// `/me/contactFolders/{id}/contacts/delta` and
    /// `/users/{id}/contactFolders/{id}/contacts/delta` -- both requiring a
    /// specific contact folder ID -- alongside the undocumented but
    /// functioning `/me/contacts/delta` shortcut this Field's own default
    /// (no `mailbox` configured) already relies on. There is no equivalent
    /// bare shortcut documented, confirmed, or even reported working for
    /// another mailbox, and this Field does not yet accept a contact-folder
    /// ID as configuration to build the one path Graph does document for
    /// another mailbox. Rather than send a request that can only 404,
    /// configuring `mailbox` at all is refused here, before any request is
    /// built.
    MailboxUnsupported,
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
            ConfigError::MailboxUnsupported => write!(
                f,
                "config.{} is not usable in this release: Microsoft Graph does not expose a \
                 documented contacts-delta feed for another mailbox without also naming a \
                 specific contact folder, which this Field does not yet accept as \
                 configuration; leave config.{} unset to collect the signed-in user's own \
                 contacts",
                crate::constants::CONFIG_MAILBOX,
                crate::constants::CONFIG_MAILBOX,
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// This Field's resolved, non-secret configuration for one run.
pub(crate) struct ResolvedConfig {
    /// The tenant a contact's portable scope is derived from.
    pub(crate) tenant_id: String,
    /// The signed-in user's Graph resource segment, `/me`.
    ///
    /// Always `/me` in this release: [`resolve`] refuses
    /// [`crate::constants::CONFIG_MAILBOX`] outright rather than build a
    /// `/users/<mailbox>` segment Graph has no documented contacts-delta
    /// resource for (see [`ConfigError::MailboxUnsupported`]). Kept as its
    /// own field, rather than inlined at each call site, so the one place
    /// that would grow a real mailbox segment -- if this Field ever accepts
    /// a contact-folder ID and can build
    /// `/users/{mailbox}/contactFolders/{id}/contacts/delta` -- stays this
    /// one field.
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
///
/// Refuses at this stage, before any Graph request is built, rather than
/// leave [`crate::constants::CONFIG_MAILBOX`] to surface only as a remote
/// 404: see [`ConfigError::MailboxUnsupported`].
pub(crate) fn resolve(config: &ConfigMap) -> Result<ResolvedConfig, ConfigError> {
    let tenant_id =
        text_of(config, crate::constants::CONFIG_TENANT_ID)?.ok_or(ConfigError::MissingTenantId)?;
    if text_of(config, crate::constants::CONFIG_MAILBOX)?.is_some() {
        return Err(ConfigError::MailboxUnsupported);
    }
    let graph_base_url = text_of(config, crate::constants::CONFIG_GRAPH_BASE_URL)?
        .unwrap_or_else(|| fieldnotes_msgraph::DEFAULT_BASE_URL.to_owned());
    Ok(ResolvedConfig {
        tenant_id,
        mailbox_resource: "/me".to_owned(),
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
    fn a_configured_graph_base_url_is_honoured_with_no_mailbox_set() {
        let mut config = ConfigMap::new();
        config.insert(key("tenant_id"), PropertyValue::Text("tenant-a".to_owned()));
        config.insert(
            key("graph_base_url"),
            PropertyValue::Text("http://127.0.0.1:9999".to_owned()),
        );
        let resolved = resolve(&config).unwrap_or_else(|error| panic!("must resolve: {error}"));
        assert_eq!(resolved.mailbox_resource, "/me");
        assert_eq!(resolved.graph_base_url, "http://127.0.0.1:9999");
    }

    /// The regression case for this Field's own release-day 404: Microsoft
    /// Graph has no documented contacts-delta resource for another mailbox
    /// without also naming a specific contact folder, so a configured
    /// `mailbox` -- however plausible-looking -- can only ever produce a
    /// request that 404s. It must be refused here, before any request is
    /// built, never sent to Graph to fail remotely.
    #[test]
    fn a_configured_mailbox_is_refused_at_validation_before_any_request() {
        let mut config = ConfigMap::new();
        config.insert(key("tenant_id"), PropertyValue::Text("tenant-a".to_owned()));
        config.insert(
            key("mailbox"),
            PropertyValue::Text("bob@example.net".to_owned()),
        );
        assert!(matches!(
            resolve(&config),
            Err(ConfigError::MailboxUnsupported)
        ));
    }

    /// An empty `mailbox` value is just as unusable as any other and must be
    /// refused the same way, not treated as "absent".
    #[test]
    fn an_empty_configured_mailbox_is_also_refused() {
        let mut config = ConfigMap::new();
        config.insert(key("tenant_id"), PropertyValue::Text("tenant-a".to_owned()));
        config.insert(key("mailbox"), PropertyValue::Text(String::new()));
        assert!(matches!(
            resolve(&config),
            Err(ConfigError::MailboxUnsupported)
        ));
    }
}
