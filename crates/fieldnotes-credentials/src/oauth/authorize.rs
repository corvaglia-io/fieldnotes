//! Building the OAuth 2.0 authorization-code-with-PKCE authorize URL.
//!
//! Nothing in this module is secret: `client_id` and `tenant_id`-derived
//! endpoints are ordinary Field configuration (per this crate's brief, "your
//! crate consumes them; it does not store them"), the PKCE code challenge is
//! the one-way hash of the verifier, and `state` is a CSRF nonce, not a
//! credential. This URL is safe to log, and this crate's own diagnostics may
//! include it.

use percent_encoding::utf8_percent_encode;

use crate::percent::QUERY_VALUE;
use crate::pkce::{CodeChallenge, State};

/// The parameters needed to build an authorization-code-with-PKCE authorize
/// request for a public client.
///
/// This does not include a client secret: a public client has none, and
/// device-code flow (which would replace this entirely) is deliberately not
/// implemented anywhere in this crate.
#[derive(Debug, Clone)]
pub struct AuthorizeRequest {
    /// The provider's authorize endpoint, such as
    /// `https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize`.
    pub authorize_endpoint: String,
    /// The application's (public, non-secret) client ID.
    pub client_id: String,
    /// The loopback redirect URI the authorization server sends the user
    /// back to, typically built from [`crate::oauth::loopback::LoopbackListener::redirect_uri`].
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
    /// The CSRF-protection state value, generated once per attempt and
    /// checked again when the redirect comes back
    /// ([`crate::oauth::loopback::LoopbackListener::await_callback`]).
    pub state: State,
    /// The PKCE `S256` code challenge derived from this attempt's verifier.
    pub code_challenge: CodeChallenge,
}

impl AuthorizeRequest {
    /// Builds the complete authorize URL a browser should be pointed at.
    ///
    /// Opening that URL in a system browser is the composition root's job
    /// (see this crate's top-level documentation); this method only builds
    /// the string.
    #[must_use]
    pub fn url(&self) -> String {
        let scope = self.scopes.join(" ");
        let mut url = self.authorize_endpoint.clone();
        url.push(if url.contains('?') { '&' } else { '?' });
        for (name, value) in [
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("scope", scope.as_str()),
            ("state", self.state.as_str()),
            ("code_challenge", self.code_challenge.as_str()),
            ("code_challenge_method", "S256"),
        ] {
            url.push_str(name);
            url.push('=');
            url.push_str(&utf8_percent_encode(value, QUERY_VALUE).to_string());
            url.push('&');
        }
        // Remove the trailing '&' left by the loop above.
        url.pop();
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkce::CodeVerifier;

    struct FixedRandom;
    impl fieldnotes_domain::RandomSource for FixedRandom {
        fn fill_bytes(&mut self, buffer: &mut [u8]) {
            buffer.fill(0x42);
        }
    }

    fn sample() -> AuthorizeRequest {
        let verifier = CodeVerifier::parse("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
            .unwrap_or_else(|error| {
                panic!("fixture verifier must be valid RFC 7636 grammar: {error}")
            });
        AuthorizeRequest {
            authorize_endpoint: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
                .to_owned(),
            client_id: "11111111-1111-1111-1111-111111111111".to_owned(),
            redirect_uri: "http://127.0.0.1:51820/callback".to_owned(),
            scopes: vec!["offline_access".to_owned(), "Mail.Read".to_owned()],
            state: State::generate(&mut FixedRandom),
            code_challenge: verifier.challenge(),
        }
    }

    #[test]
    fn url_carries_every_required_parameter() {
        let url = sample().url();
        assert!(url.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=11111111"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(!url.ends_with('&'));
    }

    #[test]
    fn scopes_are_space_joined_and_percent_encoded() {
        let url = sample().url();
        assert!(url.contains("scope=offline_access%20Mail.Read"));
    }

    #[test]
    fn redirect_uri_reserved_characters_are_percent_encoded() {
        let url = sample().url();
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A51820%2Fcallback"));
    }
}
