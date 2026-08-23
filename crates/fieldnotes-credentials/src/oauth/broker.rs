//! Minting access tokens from a stored refresh token, and completing a fresh
//! authorization, with atomic refresh-token rotation.
//!
//! [`AccessTokenProvider`] is the seam between [`crate::provider::CredentialProvider`]
//! (storage) and [`crate::oauth::token`] (the network exchange). It is the
//! only place in this crate that stores a refresh token as a side effect of
//! a network call, which is why the rotation rule lives here: per A2 section
//! 12 and this crate's brief, "a refresh response may carry a new refresh
//! token, which must replace the stored one atomically." Concretely, that
//! means calling [`crate::provider::CredentialProvider::store`] once with the
//! new value; this module never calls `clear` before `store`, so there is no
//! self-inflicted window in which `reference` names no credential at all
//! (see [`crate::keychain`] for why `store` itself is already an atomic
//! overwrite at the OS level).

use fieldnotes_domain::Clock;

use crate::error::CredentialError;
use crate::oauth::token::{self, AccessToken, TokenTransport};
use crate::provider::CredentialProvider;
use crate::reference::CredentialRef;

/// Mints access tokens for a configured OAuth application, backed by a
/// [`CredentialProvider`] for refresh-token storage and a [`TokenTransport`]
/// for the network exchange.
///
/// Holds only non-secret configuration (`token_endpoint`, `client_id`) plus
/// borrowed collaborators; it never itself holds a refresh or access token
/// between calls.
pub struct AccessTokenProvider<'a> {
    provider: &'a dyn CredentialProvider,
    transport: &'a dyn TokenTransport,
    clock: &'a dyn Clock,
    token_endpoint: String,
    client_id: String,
}

impl<'a> AccessTokenProvider<'a> {
    /// Builds a broker for one OAuth application.
    ///
    /// `token_endpoint` and `client_id` are non-secret Field configuration
    /// (per this crate's brief, "your crate consumes them; it does not store
    /// them"), so this type does not persist them anywhere beyond its own
    /// lifetime.
    #[must_use]
    pub fn new(
        provider: &'a dyn CredentialProvider,
        transport: &'a dyn TokenTransport,
        clock: &'a dyn Clock,
        token_endpoint: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        AccessTokenProvider {
            provider,
            transport,
            clock,
            token_endpoint: token_endpoint.into(),
            client_id: client_id.into(),
        }
    }

    /// Completes a fresh authorization: exchanges an authorization code and
    /// its PKCE verifier for a token set, stores the returned refresh token
    /// under `reference`, and returns the minted access token.
    ///
    /// Fails with [`CredentialError::Backend`] if the token endpoint does not
    /// return a refresh token at all (typically a missing `offline_access`
    /// scope in the request), since this crate has nothing to store in that
    /// case and a caller silently getting only a one-hour access token with
    /// no way to renew it would be a worse failure mode than an explicit
    /// error here.
    pub fn complete_authorization(
        &self,
        reference: &CredentialRef,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<AccessToken, CredentialError> {
        let token_set = token::exchange_code(
            self.transport,
            self.clock,
            &self.token_endpoint,
            &self.client_id,
            code,
            redirect_uri,
            code_verifier,
        )?;
        let Some(refresh_token) = &token_set.refresh_token else {
            return Err(CredentialError::Backend(
                "token endpoint did not return a refresh token; request the offline_access scope"
                    .to_owned(),
            ));
        };
        self.provider.store(reference, refresh_token)?;
        Ok(token_set.access_token)
    }

    /// Mints a fresh access token for an already-authorized profile.
    ///
    /// Loads the stored refresh token, redeems it at the token endpoint, and
    /// — this is the rotation step — stores a new refresh token in its place
    /// whenever the response includes one. When the response includes none,
    /// the previously stored refresh token is left untouched, which is
    /// correct for an authorization server that does not rotate on every
    /// use.
    ///
    /// This crate's brief notes that an access token's typical lifetime
    /// comfortably exceeds the default collection-run ceiling, so this is
    /// meant to be called once before a run starts, not polled mid-run.
    pub fn mint_access_token(
        &self,
        reference: &CredentialRef,
    ) -> Result<AccessToken, CredentialError> {
        let refresh_token = self.provider.retrieve(reference)?;
        let token_set = token::refresh(
            self.transport,
            self.clock,
            &self.token_endpoint,
            &self.client_id,
            &refresh_token,
        )?;
        if let Some(rotated) = &token_set.refresh_token {
            self.provider.store(reference, rotated)?;
        }
        Ok(token_set.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::token::{TokenHttpResponse, TransportError};
    use crate::provider::fakes::FakeCredentialProvider;
    use crate::secret::Secret;

    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn unix_millis(&self) -> u64 {
            self.0
        }
    }

    struct ScriptedTransport {
        body: String,
    }
    impl TokenTransport for ScriptedTransport {
        fn post_form(
            &self,
            _endpoint: &str,
            _form: &[(&str, &str)],
        ) -> Result<TokenHttpResponse, TransportError> {
            Ok(TokenHttpResponse {
                status: 200,
                body: self.body.clone(),
            })
        }
    }

    fn reference() -> CredentialRef {
        match CredentialRef::parse("microsoft_wxs") {
            Ok(reference) => reference,
            Err(error) => panic!("test fixture reference must parse: {error}"),
        }
    }

    #[test]
    fn complete_authorization_stores_the_refresh_token_and_returns_an_access_token()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = FakeCredentialProvider::new();
        let transport = ScriptedTransport {
            body: r#"{"access_token":"AT-1","refresh_token":"RT-1","expires_in":3600}"#.to_owned(),
        };
        let clock = FixedClock(0);
        let broker = AccessTokenProvider::new(
            &provider,
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
        );
        let reference = reference();
        let access_token = broker.complete_authorization(
            &reference,
            "auth-code",
            "http://127.0.0.1:1/callback",
            "verifier",
        )?;
        assert_eq!(access_token.expose_secret(), "AT-1");
        assert_eq!(provider.retrieve(&reference)?, Secret::new("RT-1"));
        Ok(())
    }

    #[test]
    fn complete_authorization_without_a_refresh_token_is_an_error_and_stores_nothing()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = FakeCredentialProvider::new();
        let transport = ScriptedTransport {
            body: r#"{"access_token":"AT-1","expires_in":3600}"#.to_owned(),
        };
        let clock = FixedClock(0);
        let broker = AccessTokenProvider::new(
            &provider,
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
        );
        let reference = reference();
        let result = broker.complete_authorization(
            &reference,
            "auth-code",
            "http://127.0.0.1:1/callback",
            "verifier",
        );
        assert!(matches!(result, Err(CredentialError::Backend(_))));
        assert_eq!(provider.retrieve(&reference), Err(CredentialError::Absent));
        Ok(())
    }

    #[test]
    fn mint_access_token_rotates_the_stored_refresh_token_when_one_is_returned()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = FakeCredentialProvider::new();
        let reference = reference();
        provider.seed(&reference, Secret::new("RT-old"));
        let transport = ScriptedTransport {
            body: r#"{"access_token":"AT-2","refresh_token":"RT-new","expires_in":3600}"#
                .to_owned(),
        };
        let clock = FixedClock(0);
        let broker = AccessTokenProvider::new(
            &provider,
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
        );
        let access_token = broker.mint_access_token(&reference)?;
        assert_eq!(access_token.expose_secret(), "AT-2");
        assert_eq!(provider.retrieve(&reference)?, Secret::new("RT-new"));
        Ok(())
    }

    #[test]
    fn mint_access_token_leaves_the_stored_refresh_token_untouched_when_none_is_returned()
    -> Result<(), Box<dyn std::error::Error>> {
        let provider = FakeCredentialProvider::new();
        let reference = reference();
        provider.seed(&reference, Secret::new("RT-stable"));
        let transport = ScriptedTransport {
            body: r#"{"access_token":"AT-3","expires_in":3600}"#.to_owned(),
        };
        let clock = FixedClock(0);
        let broker = AccessTokenProvider::new(
            &provider,
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
        );
        let access_token = broker.mint_access_token(&reference)?;
        assert_eq!(access_token.expose_secret(), "AT-3");
        assert_eq!(provider.retrieve(&reference)?, Secret::new("RT-stable"));
        Ok(())
    }

    #[test]
    fn mint_access_token_reports_absent_when_nothing_was_ever_stored() {
        let provider = FakeCredentialProvider::new();
        let transport = ScriptedTransport {
            body: String::new(),
        };
        let clock = FixedClock(0);
        let broker = AccessTokenProvider::new(
            &provider,
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
        );
        let result = broker.mint_access_token(&reference());
        assert_eq!(result, Err(CredentialError::Absent));
    }
}
