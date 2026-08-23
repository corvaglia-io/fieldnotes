//! The actionable credential-error type.
//!
//! The crate's brief is to "report actionably when a credential is absent,
//! expired, or revoked." [`CredentialError`] is that report: every variant's
//! [`core::fmt::Display`] text says what happened and, where useful, what to
//! do about it, and no variant ever carries a secret value. A
//! [`crate::provider::CredentialProvider`] implementation can only actually
//! discover [`CredentialError::Absent`], [`CredentialError::Unavailable`],
//! and [`CredentialError::Backend`] on its own, because raw storage has no
//! notion of an OAuth grant's validity; [`CredentialError::Revoked`],
//! [`CredentialError::Expired`], and [`CredentialError::Denied`] are
//! produced by [`crate::oauth`] classifying the token endpoint's own response
//! when it tries to use a stored refresh token or complete an authorization.

use core::fmt;

/// Why a credential operation did not produce usable material.
///
/// This type never contains a secret. Backend messages are built only from
/// safe, non-secret text (see [`crate::keychain`] for the exact rule this
/// crate follows when converting a `keyring::Error`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CredentialError {
    /// No credential is stored under this reference.
    ///
    /// The caller must authenticate this profile before it can be used.
    Absent,
    /// The credential provider itself could not be reached or used, for a
    /// reason unrelated to whether a credential exists.
    ///
    /// Examples: no keychain daemon is running, the platform Secret Service
    /// is locked, or (for the explicit environment-variable provider) the
    /// named variable is unset. The contained text is safe to display; it
    /// never includes credential material.
    Unavailable(String),
    /// The stored refresh token was rejected by the token endpoint as
    /// permanently invalid (an OAuth `invalid_grant` outcome, for example
    /// after the user revoked consent or changed their password).
    ///
    /// The caller must re-authenticate this profile from scratch.
    Revoked,
    /// The credential's validity window has passed and it can no longer be
    /// refreshed.
    ///
    /// The caller must re-authenticate this profile from scratch.
    Expired,
    /// The interactive authorization was explicitly declined, such as the
    /// user cancelling consent in the browser.
    Denied,
    /// A backend-specific failure not covered by the other variants.
    ///
    /// The contained text is safe to display; it never includes credential
    /// material, and in particular never echoes raw bytes a backend reports
    /// as part of a malformed-data error.
    Backend(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::Absent => write!(
                f,
                "no credential is stored for this reference; authenticate this profile before using it"
            ),
            CredentialError::Unavailable(reason) => {
                write!(f, "the credential provider is unavailable: {reason}")
            }
            CredentialError::Revoked => write!(
                f,
                "the stored credential was revoked upstream; re-authenticate this profile"
            ),
            CredentialError::Expired => write!(
                f,
                "the stored credential has expired and cannot be refreshed; re-authenticate this profile"
            ),
            CredentialError::Denied => {
                write!(
                    f,
                    "authorization was declined; re-authenticate this profile to try again"
                )
            }
            CredentialError::Backend(reason) => write!(f, "credential backend error: {reason}"),
        }
    }
}

impl std::error::Error for CredentialError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_actionable_non_empty_text() {
        let variants = [
            CredentialError::Absent,
            CredentialError::Unavailable("keychain daemon not running".to_owned()),
            CredentialError::Revoked,
            CredentialError::Expired,
            CredentialError::Denied,
            CredentialError::Backend("malformed store".to_owned()),
        ];
        for variant in variants {
            let text = variant.to_string();
            assert!(!text.is_empty());
        }
    }
}
