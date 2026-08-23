//! The explicit environment-variable [`CredentialProvider`] for CI and
//! headless use.
//!
//! `docs/security.md` requires this provider to be "opt-in and obvious,
//! never a silent fallback that masks a missing keychain entry." This module
//! enforces that in three ways:
//!
//! 1. There is no `Default` implementation and no way to construct
//!    [`EnvCredentialProvider`] without naming the exact environment variable
//!    it reads; a caller cannot reach for a plausible-looking default and get
//!    this provider by accident.
//! 2. [`EnvCredentialProvider::store`] and [`EnvCredentialProvider::clear`]
//!    always return [`CredentialError::Unavailable`]: this provider reads a
//!    value a human or a CI system placed in the environment ahead of time,
//!    it does not manage one, and it must never appear to have "saved"
//!    anything.
//! 3. It reads the variable with [`std::env::var`] into this process's own
//!    memory only. Per `docs/security.md`, that is not the same as copying a
//!    secret into a child process's environment, and this crate never spawns
//!    a child process at all.
//!
//! The lookup-to-[`CredentialError`] mapping is factored into a free function,
//! `map_lookup`, taking a plain `Result<String, std::env::VarError>` rather
//! than reading the environment itself. This is deliberate for testability:
//! recent Rust made `std::env::set_var`/`remove_var` `unsafe` (concurrent
//! mutation of the process environment is undefined behavior on some
//! platforms), and this crate forbids `unsafe_code` outright, so its tests
//! cannot set up a real environment variable to exercise. Constructing a
//! `Result<String, std::env::VarError>` value directly needs no such call,
//! so `map_lookup` is fully testable without ever touching the real
//! process environment.

use crate::error::CredentialError;
use crate::provider::CredentialProvider;
use crate::reference::CredentialRef;
use crate::secret::Secret;

/// Reads a credential from a single, explicitly named environment variable.
///
/// This provider is read-only: [`CredentialProvider::store`] and
/// [`CredentialProvider::clear`] always fail. A caller who wants an
/// environment-backed profile obtains the refresh token once (by running the
/// interactive PKCE flow on a machine that has a browser and a keychain) and
/// places it in the named variable ahead of time; this provider never writes
/// it anywhere.
pub struct EnvCredentialProvider {
    variable_name: String,
}

impl EnvCredentialProvider {
    /// Builds a provider that reads `variable_name`.
    ///
    /// Naming the exact variable is mandatory and deliberate: nothing in
    /// this crate derives a variable name from a [`CredentialRef`]
    /// automatically, so selecting this provider is always a visible,
    /// specific choice a caller made, not an implicit fallback.
    #[must_use]
    pub fn new(variable_name: impl Into<String>) -> Self {
        EnvCredentialProvider {
            variable_name: variable_name.into(),
        }
    }
}

impl CredentialProvider for EnvCredentialProvider {
    /// Retrieves the credential from the configured environment variable.
    ///
    /// `reference` is accepted for interface parity with other providers,
    /// but this provider does not use it to select among multiple variables:
    /// one `EnvCredentialProvider` reads exactly one variable.
    fn retrieve(&self, _reference: &CredentialRef) -> Result<Secret, CredentialError> {
        map_lookup(std::env::var(&self.variable_name), &self.variable_name)
    }

    fn store(&self, _reference: &CredentialRef, _material: &Secret) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable(
            "the environment-variable provider is read-only; set the variable out of band"
                .to_owned(),
        ))
    }

    fn clear(&self, _reference: &CredentialRef) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable(
            "the environment-variable provider is read-only; unset the variable out of band"
                .to_owned(),
        ))
    }
}

/// The pure mapping from a `std::env::var` result to an actionable
/// [`CredentialError`] or a [`Secret`]. See the module doc comment for why
/// this is a free function taking the lookup result rather than a method
/// that performs the lookup itself.
fn map_lookup(
    result: Result<String, std::env::VarError>,
    variable_name: &str,
) -> Result<Secret, CredentialError> {
    match result {
        Ok(value) if value.is_empty() => Err(CredentialError::Unavailable(format!(
            "environment variable {variable_name} is set but empty"
        ))),
        Ok(value) => Ok(Secret::new(value)),
        Err(std::env::VarError::NotPresent) => Err(CredentialError::Absent),
        Err(std::env::VarError::NotUnicode(_)) => Err(CredentialError::Unavailable(format!(
            "environment variable {variable_name} is not valid Unicode"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_present_value_becomes_a_secret() -> Result<(), Box<dyn std::error::Error>> {
        let secret = map_lookup(Ok("canary-value-not-a-real-secret".to_owned()), "SOME_VAR")?;
        assert_eq!(secret.expose_secret(), "canary-value-not-a-real-secret");
        Ok(())
    }

    #[test]
    fn a_missing_variable_is_absent_not_unavailable() {
        assert_eq!(
            map_lookup(Err(std::env::VarError::NotPresent), "SOME_VAR"),
            Err(CredentialError::Absent)
        );
    }

    #[test]
    fn an_empty_value_is_unavailable_not_a_secret() {
        assert!(matches!(
            map_lookup(Ok(String::new()), "SOME_VAR"),
            Err(CredentialError::Unavailable(_))
        ));
    }

    #[test]
    fn non_unicode_is_unavailable() {
        assert!(matches!(
            map_lookup(
                Err(std::env::VarError::NotUnicode(std::ffi::OsString::from(
                    "irrelevant"
                ))),
                "SOME_VAR"
            ),
            Err(CredentialError::Unavailable(_))
        ));
    }

    #[test]
    fn store_and_clear_are_never_honored() -> Result<(), Box<dyn std::error::Error>> {
        let provider = EnvCredentialProvider::new("FIELDNOTES_CREDENTIALS_TEST_ENV_PROVIDER_RO");
        let reference = CredentialRef::parse("microsoft_acme")?;
        assert!(matches!(
            provider.store(&reference, &Secret::new("x")),
            Err(CredentialError::Unavailable(_))
        ));
        assert!(matches!(
            provider.clear(&reference),
            Err(CredentialError::Unavailable(_))
        ));
        Ok(())
    }

    #[test]
    fn retrieve_reports_absent_for_a_variable_that_is_certainly_unset()
    -> Result<(), Box<dyn std::error::Error>> {
        // No `set_var`/`remove_var` call anywhere in this module (see the
        // module doc comment for why); this only relies on nobody's real
        // environment defining a variable with this exact, deliberately
        // unlikely name.
        let provider =
            EnvCredentialProvider::new("FIELDNOTES_CREDENTIALS_TEST_DELIBERATELY_UNSET_3f9c9d2b1a");
        let reference = CredentialRef::parse("microsoft_acme")?;
        assert_eq!(provider.retrieve(&reference), Err(CredentialError::Absent));
        Ok(())
    }
}
