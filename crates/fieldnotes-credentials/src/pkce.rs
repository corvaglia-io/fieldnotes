//! PKCE (RFC 7636) code verifier and challenge generation.
//!
//! Fieldnotes always uses the `S256` challenge method. RFC 7636 exists
//! because an authorization code alone is not enough to protect a public
//! client that cannot hold a client secret: a code intercepted in transit (a
//! malicious app registering the same custom scheme, a proxy, a shared
//! system clipboard) could otherwise be redeemed by anyone. Binding the
//! authorization request to a verifier only this process holds, and proving
//! that binding at the token endpoint with the verifier itself, closes that
//! gap without needing a client secret a public client cannot keep anyway.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use fieldnotes_domain::RandomSource;

/// A PKCE code verifier: RFC 7636's "high-entropy cryptographic random
/// STRING using the unreserved characters `[A-Z] / [a-z] / [0-9] / '-' / '.'
/// / '_' / '~'`, with a minimum length of 43 characters and a maximum length
/// of 128 characters."
///
/// This is a secret in the sense that it must not be observable by anything
/// other than this process and the token endpoint over TLS, but it is
/// short-lived (one authorization attempt) and is never stored, so it is
/// represented as a plain validated string rather than [`crate::Secret`]:
/// wrapping it would not change its handling, since it never crosses a
/// storage boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeVerifier(String);

impl CodeVerifier {
    /// Generates a verifier from 32 bytes of injected randomness,
    /// base64url-encoded (no padding), matching RFC 7636's own recommended
    /// construction (section 4.1) and yielding exactly 43 characters, all
    /// drawn from the unreserved-character alphabet the RFC requires.
    #[must_use]
    pub fn generate(random: &mut dyn RandomSource) -> Self {
        let mut bytes = [0u8; 32];
        random.fill_bytes(&mut bytes);
        CodeVerifier(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Validates an existing string against RFC 7636's verifier grammar.
    ///
    /// Exposed for completeness and for tests; the crate's own flow always
    /// uses [`CodeVerifier::generate`].
    pub fn parse(text: &str) -> Result<Self, PkceError> {
        if text.len() < 43 || text.len() > 128 {
            return Err(PkceError::InvalidLength);
        }
        if !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
        {
            return Err(PkceError::InvalidCharacters);
        }
        Ok(CodeVerifier(text.to_owned()))
    }

    /// The verifier's textual form, sent to the token endpoint alongside the
    /// authorization code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derives the `S256` code challenge for this verifier.
    #[must_use]
    pub fn challenge(&self) -> CodeChallenge {
        let digest = Sha256::digest(self.0.as_bytes());
        CodeChallenge(URL_SAFE_NO_PAD.encode(digest))
    }
}

/// Errors produced while validating a caller-supplied [`CodeVerifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PkceError {
    /// The value is not 43 to 128 characters.
    InvalidLength,
    /// The value contains a character outside RFC 7636's unreserved set.
    InvalidCharacters,
}

impl core::fmt::Display for PkceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PkceError::InvalidLength => write!(f, "code verifier is not 43 to 128 characters"),
            PkceError::InvalidCharacters => {
                write!(
                    f,
                    "code verifier contains a character outside RFC 7636's unreserved set"
                )
            }
        }
    }
}

impl std::error::Error for PkceError {}

/// A PKCE `S256` code challenge: `BASE64URL-ENCODE(SHA256(code_verifier))`,
/// with no padding.
///
/// Non-secret: it is sent in the authorize-URL query string in the clear.
/// Knowing the challenge does not let an attacker derive the verifier
/// (SHA-256 is one-way), which is the whole point of using `S256` rather than
/// RFC 7636's `plain` method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeChallenge(String);

impl CodeChallenge {
    /// The challenge's textual form, sent in the authorize request.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A CSRF-protection `state` value for the authorize request.
///
/// Generated the same way as a [`CodeVerifier`] (32 random bytes,
/// base64url-encoded) but kept as a distinct type so the two values, which
/// serve different purposes and travel on different channels (`state` comes
/// back on the untrusted redirect; the verifier is sent directly to the
/// token endpoint over TLS and never appears in a URL), cannot be confused
/// at a call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State(String);

impl State {
    /// Generates a new random state value.
    #[must_use]
    pub fn generate(random: &mut dyn RandomSource) -> Self {
        let mut bytes = [0u8; 32];
        random.fill_bytes(&mut bytes);
        State(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// The state's textual form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedRandom(Vec<u8>);
    impl RandomSource for FixedRandom {
        fn fill_bytes(&mut self, buffer: &mut [u8]) {
            for (slot, value) in buffer.iter_mut().zip(self.0.iter().cycle()) {
                *slot = *value;
            }
        }
    }

    /// RFC 7636 Appendix B's worked example: a fixed 32-byte octet sequence,
    /// its base64url verifier encoding, and the `S256` challenge derived
    /// from it. This is the exact known-answer vector the RFC publishes, not
    /// a value this implementation invented.
    #[test]
    fn matches_the_rfc_7636_appendix_b_known_answer_vector() {
        #[rustfmt::skip]
        let octets: [u8; 32] = [
            116, 24, 223, 180, 151, 153, 224, 37,
            79, 250, 96, 125, 216, 173, 187, 186,
            22, 212, 37, 77, 105, 214, 191, 240,
            91, 88, 5, 88, 83, 132, 141, 121,
        ];
        let verifier = CodeVerifier(URL_SAFE_NO_PAD.encode(octets));
        assert_eq!(
            verifier.as_str(),
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        );
        assert_eq!(
            verifier.challenge().as_str(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_verifiers_are_43_characters_from_the_unreserved_alphabet() {
        let mut random = FixedRandom(vec![0xa5; 32]);
        let verifier = CodeVerifier::generate(&mut random);
        assert_eq!(verifier.as_str().len(), 43);
        assert!(CodeVerifier::parse(verifier.as_str()).is_ok());
    }

    #[test]
    fn generated_verifiers_differ_with_different_randomness() {
        let mut a = FixedRandom(vec![0x01; 32]);
        let mut b = FixedRandom(vec![0x02; 32]);
        assert_ne!(
            CodeVerifier::generate(&mut a).as_str(),
            CodeVerifier::generate(&mut b).as_str()
        );
    }

    #[test]
    fn parse_rejects_wrong_length_and_bad_characters() {
        assert_eq!(
            CodeVerifier::parse("too-short"),
            Err(PkceError::InvalidLength)
        );
        let too_long = "a".repeat(129);
        assert_eq!(
            CodeVerifier::parse(&too_long),
            Err(PkceError::InvalidLength)
        );
        let has_space = format!(" {}", "a".repeat(42));
        assert_eq!(
            CodeVerifier::parse(&has_space),
            Err(PkceError::InvalidCharacters)
        );
    }

    #[test]
    fn state_values_are_url_safe_and_differ_per_generation() {
        let mut a = FixedRandom(vec![0x10; 32]);
        let mut b = FixedRandom(vec![0x20; 32]);
        let state_a = State::generate(&mut a);
        let state_b = State::generate(&mut b);
        assert_ne!(state_a.as_str(), state_b.as_str());
        assert!(
            state_a
                .as_str()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        );
    }
}
