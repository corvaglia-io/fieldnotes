//! Extracting the non-secret connector configuration from
//! `CollectRequest.config`.
//!
//! A2 section 5 states that `config` is non-secret by construction and that a
//! Field must never treat any value in it as a secret. Nothing this module
//! reads is a credential: the tenant identifier is a public directory
//! identifier, and the mail folder is a well-known folder name or an opaque
//! Graph folder identifier.
//!
//! # Why the tenant identifier is configured rather than discovered
//!
//! The portable exact-source scope this Field derives is
//! `microsoft-graph:tenant/<tenant-id>`, matching the frozen
//! `outlook_mail_work` fixtures. A `Mail.Read`-only token cannot read
//! `/organization` (that needs `Organization.Read.All`) and cannot read `/me`
//! (that needs `User.Read`), so the tenant identifier is not reachable from
//! the scopes this Field declares. Widening the declared scope set purely to
//! learn a public identifier would be the wrong trade, so core supplies it as
//! non-secret configuration instead. See the crate's final report for the
//! coordinator question this leaves open.

use std::fmt;

use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

/// The resolved, validated non-secret configuration for one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MailConfig {
    /// The Microsoft Entra tenant identifier the mailbox belongs to,
    /// lowercased so two instances spelling it differently still derive the
    /// same portable scope.
    pub(crate) tenant_id: String,
    /// The mail folder to collect from: a well-known folder name such as
    /// `inbox`, or an opaque Graph folder identifier.
    pub(crate) mail_folder: String,
}

/// Why the configuration could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigError {
    /// A required key is absent.
    Missing(&'static str),
    /// A key is present but is not a text scalar.
    WrongShape(&'static str),
    /// A key is present and textual but does not satisfy its own shape.
    Unusable {
        /// The configuration key.
        key: &'static str,
        /// Why, in reviewable terms. Never echoes the offending value, which
        /// is untrusted input.
        reason: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(key) => write!(
                f,
                "config.{key} is required: it names the Microsoft Entra tenant this mailbox \
                 belongs to, which is a public directory identifier and the basis of this \
                 Field's portable source scope"
            ),
            ConfigError::WrongShape(key) => {
                write!(f, "config.{key} must be a text scalar")
            }
            ConfigError::Unusable { key, reason } => {
                write!(f, "config.{key} is unusable: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

fn text_of<'a>(config: &'a ConfigMap, key: &'static str) -> Result<Option<&'a str>, ConfigError> {
    match config.get(key) {
        None => Ok(None),
        Some(PropertyValue::Text(text)) => Ok(Some(text.as_str())),
        Some(_) => Err(ConfigError::WrongShape(key)),
    }
}

/// Whether `text` is a GUID in the canonical `8-4-4-4-12` lowercase
/// hexadecimal spelling.
fn is_guid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        })
}

/// Whether `text` is safe to place inside an OData single-quoted string in a
/// resource path.
///
/// The conservative set is ASCII alphanumerics, `-`, `_`, `=`, and `.`, which
/// covers every well-known Graph mail folder name and the base64url-shaped
/// folder identifiers Graph returns. A single quote, a parenthesis, a slash,
/// or a percent is refused outright rather than escaped, because a folder
/// identifier has no legitimate reason to contain one and refusing is the only
/// way to be sure a configured value can never widen the resource this Field
/// reads.
fn is_folder_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 512
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'=' | b'.'))
}

/// Resolves the non-secret configuration for one run.
pub(crate) fn resolve(config: &ConfigMap) -> Result<MailConfig, ConfigError> {
    let tenant = text_of(config, crate::constants::CONFIG_TENANT_ID)?
        .ok_or(ConfigError::Missing(crate::constants::CONFIG_TENANT_ID))?
        .trim()
        .to_ascii_lowercase();
    if !is_guid(&tenant) {
        return Err(ConfigError::Unusable {
            key: crate::constants::CONFIG_TENANT_ID,
            reason: "a tenant identifier is a GUID in the canonical 8-4-4-4-12 form",
        });
    }
    let folder = match text_of(config, crate::constants::CONFIG_MAIL_FOLDER)? {
        Some(text) => text.trim().to_owned(),
        None => crate::constants::DEFAULT_MAIL_FOLDER.to_owned(),
    };
    if !is_folder_token(&folder) {
        return Err(ConfigError::Unusable {
            key: crate::constants::CONFIG_MAIL_FOLDER,
            reason: "a mail folder is a well-known folder name or an opaque Graph folder \
                     identifier, of ASCII letters, digits, '-', '_', '=', and '.' only",
        });
    }
    Ok(MailConfig {
        tenant_id: tenant,
        mail_folder: folder,
    })
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, MailConfig, resolve};
    use fieldnotes_field_protocol::grammar::PropertyNameToken;
    use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};

    fn key(name: &str) -> PropertyNameToken {
        PropertyNameToken::parse(name)
            .unwrap_or_else(|error| panic!("{name} must be a config key: {error}"))
    }

    fn config(entries: &[(&str, PropertyValue)]) -> ConfigMap {
        let mut map = ConfigMap::new();
        for (name, value) in entries {
            map.insert(key(name), value.clone());
        }
        map
    }

    const TENANT: &str = "8d820000-0000-7000-8000-000000000001";

    #[test]
    fn a_missing_tenant_is_reported() {
        assert_eq!(
            resolve(&ConfigMap::new()),
            Err(ConfigError::Missing("tenant_id"))
        );
    }

    #[test]
    fn a_non_text_tenant_is_reported() {
        let map = config(&[("tenant_id", PropertyValue::Boolean(true))]);
        assert_eq!(resolve(&map), Err(ConfigError::WrongShape("tenant_id")));
    }

    #[test]
    fn a_tenant_that_is_not_a_guid_is_refused() {
        let map = config(&[(
            "tenant_id",
            PropertyValue::Text("acme.onmicrosoft.example".to_owned()),
        )]);
        assert!(matches!(
            resolve(&map),
            Err(ConfigError::Unusable {
                key: "tenant_id",
                ..
            })
        ));
    }

    #[test]
    fn the_folder_defaults_to_the_inbox() {
        let map = config(&[("tenant_id", PropertyValue::Text(TENANT.to_owned()))]);
        assert_eq!(
            resolve(&map),
            Ok(MailConfig {
                tenant_id: TENANT.to_owned(),
                mail_folder: "inbox".to_owned(),
            })
        );
    }

    #[test]
    fn a_tenant_is_lowercased_so_two_instances_derive_the_same_scope() {
        let upper = config(&[(
            "tenant_id",
            PropertyValue::Text(TENANT.to_ascii_uppercase()),
        )]);
        let lower = config(&[("tenant_id", PropertyValue::Text(TENANT.to_owned()))]);
        assert_eq!(resolve(&upper), resolve(&lower));
    }

    #[test]
    fn a_folder_that_could_widen_the_resource_path_is_refused() {
        for hostile in [
            "inbox')/../../users('victim",
            "inbox/messages",
            "in box",
            "inbox%2f",
            "inbox'",
        ] {
            let map = config(&[
                ("tenant_id", PropertyValue::Text(TENANT.to_owned())),
                ("mail_folder", PropertyValue::Text(hostile.to_owned())),
            ]);
            assert!(
                matches!(
                    resolve(&map),
                    Err(ConfigError::Unusable {
                        key: "mail_folder",
                        ..
                    })
                ),
                "{hostile:?} must be refused"
            );
        }
    }

    #[test]
    fn an_opaque_graph_folder_identifier_is_accepted() {
        let map = config(&[
            ("tenant_id", PropertyValue::Text(TENANT.to_owned())),
            (
                "mail_folder",
                PropertyValue::Text("AAMkAGI2THEFOLDER01=".to_owned()),
            ),
        ]);
        assert_eq!(
            resolve(&map).map(|resolved| resolved.mail_folder),
            Ok("AAMkAGI2THEFOLDER01=".to_owned())
        );
    }
}
