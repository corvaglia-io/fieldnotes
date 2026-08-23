//! The `CredentialProvider` abstraction.
//!
//! A2's model is reference in configuration, material on a protected channel.
//! [`CredentialProvider`] is the storage half of that model: it retrieves and
//! stores a [`Secret`] by a non-secret [`CredentialRef`], and nothing else.
//! It does not know about OAuth, PKCE, access tokens, or the protected
//! channel to a Field; those live in [`crate::oauth`] and are built on top of
//! this trait, not inside it.
//!
//! Two implementations ship in this crate: [`crate::keychain`] (the default,
//! backed by the platform keychain) and [`crate::env`] (explicit, opt-in, for
//! CI and headless use). Both are thin adapters over this trait so tests can
//! substitute a fake and never touch a real keychain or a real environment
//! variable.

use crate::error::CredentialError;
use crate::reference::CredentialRef;
use crate::secret::Secret;

/// Retrieves and stores a credential by a non-secret reference.
///
/// Implementations must never log, print, or otherwise emit the secret value
/// they hold; [`Secret`]'s redacting `Debug` and missing `Display` make doing
/// so accidentally difficult, but an implementation that formats a raw
/// `keyring`-crate or environment-provider error must still take care not to
/// echo material embedded in that lower-level error (see
/// [`crate::keychain`]'s error-mapping rule for the concrete case this
/// matters).
pub trait CredentialProvider {
    /// Retrieves the credential stored under `reference`.
    ///
    /// Returns [`CredentialError::Absent`] when nothing is stored, or a
    /// provider-specific error (never one that requires having attempted a
    /// network call, since raw storage has no notion of an OAuth grant's
    /// validity; [`CredentialError::Revoked`] and [`CredentialError::Expired`]
    /// are produced by [`crate::oauth`], not by a provider's `retrieve`).
    fn retrieve(&self, reference: &CredentialRef) -> Result<Secret, CredentialError>;

    /// Stores `material` under `reference`, replacing any existing value.
    ///
    /// A provider backed by a real keychain overwrites the entry in place
    /// using the platform's own atomic update, rather than deleting and
    /// re-creating it: refresh-token rotation (A2 section 12, and this
    /// crate's `oauth::broker` module) depends on there never being a window
    /// in which the reference names no credential at all.
    ///
    /// Returns [`CredentialError::Unavailable`] for a provider that cannot
    /// honor writes at all, such as [`crate::env::EnvCredentialProvider`].
    fn store(&self, reference: &CredentialRef, material: &Secret) -> Result<(), CredentialError>;

    /// Removes any credential stored under `reference`.
    ///
    /// Removing an already-absent credential is not an error: the
    /// post-condition ("nothing is stored under this reference") already
    /// holds.
    fn clear(&self, reference: &CredentialRef) -> Result<(), CredentialError>;
}

#[cfg(test)]
pub(crate) mod fakes {
    //! An in-memory `CredentialProvider` fake shared by this crate's tests
    //! (and, deliberately, `pub(crate)` rather than merely `#[cfg(test)]` in
    //! a leaf module, so both unit tests here and integration-style tests
    //! elsewhere in this crate can use one implementation instead of each
    //! rolling its own).

    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::{CredentialError, CredentialProvider, CredentialRef, Secret};

    /// An in-memory credential store for tests. Never touches a real
    /// keychain or the process environment.
    #[derive(Default)]
    pub(crate) struct FakeCredentialProvider {
        entries: Mutex<HashMap<String, Secret>>,
    }

    /// Locks `mutex`, recovering the inner value on poison rather than
    /// panicking: a fake used only in tests must not itself be a source of
    /// `unwrap`/`expect` panics.
    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    impl FakeCredentialProvider {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn seed(&self, reference: &CredentialRef, material: Secret) {
            lock(&self.entries).insert(reference.as_str().to_owned(), material);
        }
    }

    impl CredentialProvider for FakeCredentialProvider {
        fn retrieve(&self, reference: &CredentialRef) -> Result<Secret, CredentialError> {
            lock(&self.entries)
                .get(reference.as_str())
                .cloned()
                .ok_or(CredentialError::Absent)
        }

        fn store(
            &self,
            reference: &CredentialRef,
            material: &Secret,
        ) -> Result<(), CredentialError> {
            lock(&self.entries).insert(reference.as_str().to_owned(), material.clone());
            Ok(())
        }

        fn clear(&self, reference: &CredentialRef) -> Result<(), CredentialError> {
            lock(&self.entries).remove(reference.as_str());
            Ok(())
        }
    }

    #[test]
    fn round_trips_store_retrieve_and_clear() -> Result<(), Box<dyn std::error::Error>> {
        let provider = FakeCredentialProvider::new();
        let reference = CredentialRef::parse("microsoft_acme")?;
        assert_eq!(provider.retrieve(&reference), Err(CredentialError::Absent));
        provider.store(&reference, &Secret::new("refresh-token-value"))?;
        assert_eq!(
            provider.retrieve(&reference)?,
            Secret::new("refresh-token-value")
        );
        provider.clear(&reference)?;
        assert_eq!(provider.retrieve(&reference), Err(CredentialError::Absent));
        Ok(())
    }
}
