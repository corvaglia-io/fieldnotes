//! The platform-keychain [`CredentialProvider`].
//!
//! [`KeyringCredentialProvider`] is the default provider named in the
//! roadmap and `docs/security.md`: macOS Keychain, Windows Credential
//! Manager, and Linux Secret Service, behind the single `keyring` crate API
//! (see this crate's `Cargo.toml` for why `keyring` was chosen over hand
//! rolling three platform FFI/D-Bus adapters). Which concrete backend runs is
//! decided entirely by `keyring`'s own `cfg`-gated platform selection at
//! compile time; this module contains no platform-specific code of its own,
//! so every platform gets the strongest storage `keyring` offers for it
//! rather than a lowest-common-denominator shim.
//!
//! # Error-mapping rule
//!
//! `keyring::Error` derives `Debug`, and two of its variants
//! (`BadEncoding(Vec<u8>)` and `BadDataFormat(Vec<u8>, _)`) carry the raw
//! bytes of a credential that failed to decode as UTF-8 or failed a store's
//! internal format check. A stray `{:?}` on a `keyring::Error` value could
//! therefore print secret bytes. This module never does that: every mapping
//! below reads the error's `Display` text (which `keyring` itself writes to
//! omit those bytes; see its `error.rs`) rather than its `Debug` text, and no
//! function in this module accepts a `keyring::Error` as a parameter it might
//! forward verbatim. This module's `bad_encoding_never_leaks_the_raw_bytes`
//! test proves this for the one variant that actually contains
//! attacker-controllable bytes.

use keyring::Entry;

use crate::error::CredentialError;
use crate::provider::CredentialProvider;
use crate::reference::CredentialRef;
use crate::secret::Secret;

/// A [`CredentialProvider`] backed by the operating system's credential
/// store, through the `keyring` crate.
///
/// One `KeyringCredentialProvider` serves every [`CredentialRef`]: the
/// reference becomes the keychain entry's account name, under a fixed
/// service name scoped to this Fieldnotes installation.
pub struct KeyringCredentialProvider {
    service: String,
}

impl KeyringCredentialProvider {
    /// Builds a provider under the given keychain service name.
    ///
    /// `service` should identify this application to the platform keychain
    /// (for example `"fieldnotes"`), distinct from any other application
    /// that might store entries in the same keychain.
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        KeyringCredentialProvider {
            service: service.into(),
        }
    }

    fn entry(&self, reference: &CredentialRef) -> Result<Entry, CredentialError> {
        Entry::new(&self.service, reference.as_str()).map_err(map_keyring_error)
    }
}

impl CredentialProvider for KeyringCredentialProvider {
    fn retrieve(&self, reference: &CredentialRef) -> Result<Secret, CredentialError> {
        let entry = self.entry(reference)?;
        let value = entry.get_password().map_err(map_keyring_error)?;
        Ok(Secret::new(value))
    }

    fn store(&self, reference: &CredentialRef, material: &Secret) -> Result<(), CredentialError> {
        let entry = self.entry(reference)?;
        // `set_password` overwrites an existing entry in place using the
        // platform's own atomic update; this crate never deletes an entry
        // before writing its replacement, which is what refresh-token
        // rotation (`crate::oauth::broker`) depends on.
        entry
            .set_password(material.expose_secret())
            .map_err(map_keyring_error)
    }

    fn clear(&self, reference: &CredentialRef) -> Result<(), CredentialError> {
        let entry = self.entry(reference)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            // Deleting an already-absent credential satisfies the
            // post-condition the trait documents.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(other) => Err(map_keyring_error(other)),
        }
    }
}

/// Maps a `keyring::Error` to this crate's actionable, secret-free
/// [`CredentialError`].
///
/// Reads only `keyring::Error`'s `Display` text, never its `Debug` text; see
/// this module's doc comment.
fn map_keyring_error(error: keyring::Error) -> CredentialError {
    match error {
        keyring::Error::NoEntry => CredentialError::Absent,
        keyring::Error::PlatformFailure(_)
        | keyring::Error::NoStorageAccess(_)
        | keyring::Error::NoDefaultStore
        | keyring::Error::NotSupportedByStore(_) => CredentialError::Unavailable(error.to_string()),
        keyring::Error::Ambiguous(_) => {
            CredentialError::Unavailable("multiple matching keychain entries".to_owned())
        }
        // `Display` for these already omits the raw bytes (see this crate's
        // `Cargo.toml` comment and this module's doc comment); still, build
        // the message from a fixed string rather than `error.to_string()` so
        // a future `keyring` release changing what `Display` includes cannot
        // silently reopen the leak.
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(_, _) => {
            CredentialError::Backend("stored credential is not validly encoded".to_owned())
        }
        keyring::Error::BadStoreFormat(_)
        | keyring::Error::TooLong(_, _)
        | keyring::Error::Invalid(_, _) => CredentialError::Backend(error.to_string()),
        other => CredentialError::Backend(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_encoding_never_leaks_the_raw_bytes() {
        let canary = b"FIXTURE-NOT-A-REAL-TOKEN-canary-raw-bytes".to_vec();
        let mapped = map_keyring_error(keyring::Error::BadEncoding(canary));
        let rendered = format!("{mapped:?}");
        assert!(!rendered.contains("canary"), "leaked: {rendered}");
        let displayed = mapped.to_string();
        assert!(!displayed.contains("canary"), "leaked: {displayed}");
    }

    #[test]
    fn bad_data_format_never_leaks_the_raw_bytes() {
        let canary = b"FIXTURE-NOT-A-REAL-TOKEN-canary-data-format".to_vec();
        let platform_error: Box<dyn std::error::Error + Send + Sync> =
            "wrapped platform detail".into();
        let mapped = map_keyring_error(keyring::Error::BadDataFormat(canary, platform_error));
        let rendered = format!("{mapped:?}");
        assert!(!rendered.contains("canary"), "leaked: {rendered}");
    }

    #[test]
    fn no_entry_maps_to_absent() {
        assert_eq!(
            map_keyring_error(keyring::Error::NoEntry),
            CredentialError::Absent
        );
    }

    /// The one test in this crate that can touch a real OS keychain. It is
    /// skipped unless a developer explicitly opts in, per this crate's
    /// testability requirement that the ordinary suite never touches the
    /// developer's actual keychain.
    #[test]
    fn real_keychain_round_trip_when_explicitly_opted_in() -> Result<(), Box<dyn std::error::Error>>
    {
        if std::env::var("FIELDNOTES_CREDENTIALS_TEST_REAL_KEYCHAIN").is_err() {
            eprintln!(
                "skipping real-keychain test; set \
                 FIELDNOTES_CREDENTIALS_TEST_REAL_KEYCHAIN=1 to opt in"
            );
            return Ok(());
        }
        let provider = KeyringCredentialProvider::new("fieldnotes-credentials-test-suite");
        let reference = CredentialRef::parse("test_real_keychain_roundtrip")?;
        provider.store(&reference, &Secret::new("integration-test-value"))?;
        assert_eq!(
            provider.retrieve(&reference)?,
            Secret::new("integration-test-value")
        );
        provider.clear(&reference)?;
        assert_eq!(provider.retrieve(&reference), Err(CredentialError::Absent));
        Ok(())
    }
}
