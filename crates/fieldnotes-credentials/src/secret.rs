//! The guarded secret-carrying type.
//!
//! [`Secret`] is this crate's answer to "a stray formatting call cannot leak
//! material into a log, a diagnostic, or a panic message": it has no
//! [`core::fmt::Display`] implementation at all, so `println!("{}", secret)`
//! fails to compile rather than printing the value; its [`core::fmt::Debug`]
//! implementation always prints the fixed marker `"[redacted]"`; and its
//! backing buffer is a [`zeroize::Zeroizing`] wrapper, which overwrites the
//! buffer with zero bytes before it is freed. This mirrors
//! `fieldnotes_field_protocol::message::CredentialMaterial`'s redacting
//! `Debug`, documented in that crate's `redact` module, which this crate does
//! not depend on but stays deliberately consistent with.
//!
//! Every refresh token and access token this crate hands anywhere is a
//! [`Secret`]. The only way the value inside ever becomes a plain [`str`] is
//! the explicit, named [`Secret::expose_secret`] call, which a reviewer can
//! grep for.

use core::fmt;

use zeroize::Zeroizing;

/// A secret value whose `Debug` implementation always redacts and which has
/// no `Display` implementation at all.
///
/// The backing buffer is zeroized when the value is dropped. This is
/// best-effort: it protects against the buffer's bytes surviving in freed
/// heap memory for a stray later read (a heap-scanning crash dump, for
/// example), not against a privileged debugger inspecting the process while
/// it is still alive, a compromised process, or a malicious trusted Field
/// that has already been handed the value.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Wraps `value` as a secret.
    ///
    /// The caller gives up its own copy's contents to this wrapper's
    /// zeroize-on-drop discipline; if the caller already had `value` in a
    /// plain `String` it captured from elsewhere (such as a token endpoint's
    /// parsed JSON response), it should not keep or log that original.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Secret(Zeroizing::new(value.into()))
    }

    /// Exposes the secret value as a string slice.
    ///
    /// Named deliberately so a reviewer or a `grep -r expose_secret` can find
    /// every place a secret leaves this wrapper.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// Whether the secret is empty.
    ///
    /// Exists so a caller can validate presence without exposing the value
    /// for a simple emptiness check.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Secret").field(&"[redacted]").finish()
    }
}

impl PartialEq for Secret {
    /// Compares the exposed values.
    ///
    /// This is not constant-time. It exists for tests asserting a secret
    /// round-tripped through storage, not for comparing user-supplied input
    /// against a stored value on an authentication path (this crate never
    /// does that: every credential comparison here is `state` or PKCE
    /// verifier equality, which [`crate::oauth::loopback`] performs
    /// separately without going through this type).
    fn eq(&self, other: &Self) -> bool {
        self.expose_secret() == other.expose_secret()
    }
}

impl Eq for Secret {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_value() {
        let secret = Secret::new("FIXTURE-NOT-A-REAL-TOKEN-canary-3a91f0");
        let rendered = format!("{secret:?}");
        assert!(!rendered.contains("canary"), "leaked: {rendered}");
        assert_eq!(rendered, "Secret(\"[redacted]\")");
    }

    #[test]
    fn expose_secret_returns_the_exact_value() {
        let secret = Secret::new("exact-value");
        assert_eq!(secret.expose_secret(), "exact-value");
    }

    #[test]
    fn equality_compares_exposed_values() {
        assert_eq!(Secret::new("same"), Secret::new("same"));
        assert_ne!(Secret::new("a"), Secret::new("b"));
    }
}
