//! The token-endpoint exchange: turning an authorization code (or a stored
//! refresh token) into an access token, plus the transport abstraction that
//! keeps a real network call out of this crate's own test suite.
//!
//! Two things this module is careful about because they are the actual point
//! of this crate:
//!
//! - **The response body is the one place a real network response's raw text
//!   contains a secret in the clear.** `request_token` zeroizes it as soon
//!   as it has extracted what it needs, in addition to every value it
//!   extracts becoming a [`crate::Secret`] immediately.
//! - **The parsed JSON DTO never derives `Debug`.** A derived `Debug` would
//!   print `access_token`/`refresh_token`/`id_token` in the clear; not deriving
//!   it means there is nothing for a stray `{:?}` to print.
//!
//! # The ID token stops here
//!
//! When the authorization request included the `openid` scope, the response
//! also carries an `id_token`. That token is credential-adjacent — a signed
//! bearer assertion about a person — so it is confined to
//! `parse_token_response`'s own stack frame: it is wrapped in a [`Secret`] the
//! instant `serde` hands it over, read once by
//! [`crate::oauth::id_token::account_from_id_token`], and dropped (zeroized)
//! before this function returns. [`TokenSet`] carries only the extracted
//! [`AccountId`], so there is no field on any returned type for a logger, a
//! serializer, a `Debug`, or an error message to reach.
//!
//! # Why `openid`/`profile` are requested at all
//!
//! Neither grants access to anything. `openid` asks the authorization server to
//! identify the signed-in principal to this client, and `profile` is what makes
//! Microsoft Entra include the human-recognizable `preferred_username` claim.
//! They exist precisely to answer "who just signed in", they need no
//! administrative consent, and without them a stored credential is anonymous:
//! three Fields authenticated in three browser flows can silently be three
//! different people, because a browser reuses whatever session is already open.
//! The `email` scope is deliberately *not* requested — `profile` already yields
//! a recognizable name, so `email` would add a second personal-data claim for
//! nothing. See [`crate::oauth::id_token`] for what the resulting value may and
//! may not be used for.

use core::time::Duration;

use serde::Deserialize;

use fieldnotes_domain::Clock;

use crate::error::CredentialError;
use crate::oauth::id_token::{self, AccountId};
use crate::secret::Secret;

/// One token-endpoint HTTP response, as far as this crate cares: a status
/// code and a body. Header handling (beyond `Content-Type`, which every
/// implementation sends as `application/json` for this endpoint) is not
/// needed for this exchange.
pub struct TokenHttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response body, expected to be JSON.
    pub body: String,
}

/// A failure making the HTTP request itself (DNS, TCP, TLS, or a transport
/// implementation's own I/O error). A non-2xx HTTP response with a body is
/// not a `TransportError`; it is a successful transport call whose body
/// `request_token` classifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError(pub String);

impl core::fmt::Display for TransportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "token transport error: {}", self.0)
    }
}

impl std::error::Error for TransportError {}

/// The token endpoint's HTTP transport, abstracted so tests substitute a
/// fake and never make a real network call.
///
/// Implementations must send `form` as
/// `application/x-www-form-urlencoded`, matching every OAuth 2.0 token
/// endpoint's expected request shape (RFC 6749 section 4.1.3).
pub trait TokenTransport {
    /// Posts `form` to `endpoint` and returns the raw HTTP response.
    fn post_form(
        &self,
        endpoint: &str,
        form: &[(&str, &str)],
    ) -> Result<TokenHttpResponse, TransportError>;
}

/// The real [`TokenTransport`], backed by a blocking `ureq` agent over TLS.
///
/// Deliberately thin: it URL-encodes the form, issues the POST, and reads
/// the body into a `String`. All response interpretation happens in
/// `request_token`, which is transport-independent and covered by this
/// crate's fake-transport tests.
pub struct UreqTokenTransport {
    agent: ureq::Agent,
}

impl UreqTokenTransport {
    /// Builds a transport with a bounded per-call timeout.
    ///
    /// The timeout applies to the whole request (connect, send, and receive);
    /// a token-endpoint exchange that has not finished by then fails rather
    /// than blocking the interactive flow indefinitely.
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // A 4xx/5xx token-endpoint response still carries the OAuth
            // `error`/`error_description` body `request_token` classifies
            // (RFC 6749 section 5.2); with the default `true`, `ureq` would
            // discard that body and surface only a bare status code.
            .http_status_as_error(false)
            .build();
        UreqTokenTransport {
            agent: config.into(),
        }
    }
}

impl Default for UreqTokenTransport {
    /// A ten-second timeout, comfortably longer than a token-endpoint round
    /// trip on an ordinary connection and comfortably shorter than the
    /// ten-minute default collection-run ceiling this exchange happens
    /// outside of.
    fn default() -> Self {
        UreqTokenTransport::new(Duration::from_secs(10))
    }
}

impl TokenTransport for UreqTokenTransport {
    fn post_form(
        &self,
        endpoint: &str,
        form: &[(&str, &str)],
    ) -> Result<TokenHttpResponse, TransportError> {
        let encoded = encode_form(form);
        let result = self
            .agent
            .post(endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(&encoded);
        match result {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let body = response
                    .body_mut()
                    .read_to_string()
                    .map_err(|error| TransportError(error.to_string()))?;
                Ok(TokenHttpResponse { status, body })
            }
            Err(ureq::Error::StatusCode(status)) => {
                // `ureq` treats a non-2xx status as `Err`, but a token
                // endpoint's error body (`{"error": "invalid_grant", ...}`)
                // is exactly what `request_token` needs to classify, so this
                // is not a transport failure. `ureq`'s `Error::StatusCode`
                // does not carry the body, so callers relying on it alone
                // would lose the classification detail; this crate instead
                // configures the agent to treat non-2xx as `Ok` via
                // `http_status_as_error(false)` at construction, and this
                // arm exists only as a defensive fallback if that ever stops
                // being honored by a future `ureq` release.
                Ok(TokenHttpResponse {
                    status,
                    body: String::new(),
                })
            }
            Err(error) => Err(TransportError(error.to_string())),
        }
    }
}

fn encode_form(form: &[(&str, &str)]) -> String {
    form.iter()
        .map(|(name, value)| {
            format!(
                "{}={}",
                percent_encoding::utf8_percent_encode(name, crate::percent::QUERY_VALUE),
                percent_encoding::utf8_percent_encode(value, crate::percent::QUERY_VALUE),
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// A minted access token, ready to hand to a Field over the protected
/// channel described in A2 section 12.
///
/// `Debug` is derived, not hand-written, and is still safe: the inner
/// [`Secret`] field's own `Debug` implementation redacts, so deriving simply
/// composes that guarantee rather than bypassing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessToken {
    value: Secret,
    expires_at_unix_millis: i64,
    scope: Option<String>,
}

impl AccessToken {
    /// The access token value.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        self.value.expose_secret()
    }

    /// The Unix-epoch millisecond instant this token expires.
    #[must_use]
    pub fn expires_at_unix_millis(&self) -> i64 {
        self.expires_at_unix_millis
    }

    /// The granted scope, if the token endpoint reported one.
    #[must_use]
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    /// Whether this token is expired (or within `skew` of expiring) at the
    /// instant `clock` reports.
    ///
    /// This crate's brief notes that an access token's typical one-hour
    /// lifetime comfortably exceeds the ten-minute default collection-run
    /// ceiling, so mid-run refresh is not implemented; this method exists
    /// for a caller minting a token before starting a run to decide whether
    /// the one it already holds is still usable, not for a Field to poll
    /// mid-collection.
    #[must_use]
    pub fn is_expired(&self, clock: &dyn Clock, skew: Duration) -> bool {
        let now = i64::try_from(clock.unix_millis()).unwrap_or(i64::MAX);
        let skew_millis = i64::try_from(skew.as_millis()).unwrap_or(i64::MAX);
        now.saturating_add(skew_millis) >= self.expires_at_unix_millis
    }
}

/// The result of a successful token-endpoint exchange or refresh.
#[derive(Debug, Clone)]
pub struct TokenSet {
    /// The minted access token.
    pub access_token: AccessToken,
    /// A new refresh token, present only when the token endpoint rotated it.
    ///
    /// [`crate::oauth::broker`] is responsible for storing this atomically in
    /// place of the previous one when it is present, and for leaving the
    /// previous one untouched when it is not.
    pub refresh_token: Option<Secret>,
    /// Which account signed in, when the response carried a readable ID token.
    ///
    /// `None` whenever the account could not be learned — no `id_token` in the
    /// response, or one this crate could not read. That is never an error: see
    /// [`crate::oauth::id_token::account_from_id_token`]. **This is a label for
    /// display and confirmation, never an authorization input.**
    ///
    /// Note what this member is *not*: the ID token. That token never leaves
    /// `parse_token_response`'s stack frame.
    pub account: Option<AccountId>,
}

/// The raw token-endpoint success body. Deliberately does not derive
/// `Debug`: see this module's doc comment.
#[derive(Deserialize)]
struct TokenSuccessBody {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: i64,
    #[serde(default)]
    scope: Option<String>,
    /// The OpenID Connect ID token, present when `openid` was among the granted
    /// scopes. Read once, never stored, never returned: see this module's "The
    /// ID token stops here".
    #[serde(default)]
    id_token: Option<String>,
}

/// The raw token-endpoint error body (RFC 6749 section 5.2). Does not derive
/// `Debug` either, since a nonstandard authorization server could echo
/// request content into `error_description`.
#[derive(Deserialize)]
struct TokenErrorBody {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Exchanges an authorization code and its PKCE verifier for a token set.
pub fn exchange_code(
    transport: &dyn TokenTransport,
    clock: &dyn Clock,
    token_endpoint: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenSet, CredentialError> {
    request_token(
        transport,
        clock,
        token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ],
    )
}

/// Redeems a stored refresh token for a new token set.
///
/// The public client sends no client secret: PKCE's binding to this
/// process's own verifier applied only to the authorization-code exchange,
/// and a public client's refresh call is authenticated by possession of the
/// refresh token itself, per RFC 6749 section 6.
pub fn refresh(
    transport: &dyn TokenTransport,
    clock: &dyn Clock,
    token_endpoint: &str,
    client_id: &str,
    refresh_token: &Secret,
) -> Result<TokenSet, CredentialError> {
    request_token(
        transport,
        clock,
        token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token.expose_secret()),
        ],
    )
}

fn request_token(
    transport: &dyn TokenTransport,
    clock: &dyn Clock,
    token_endpoint: &str,
    form: &[(&str, &str)],
) -> Result<TokenSet, CredentialError> {
    let mut response = transport
        .post_form(token_endpoint, form)
        .map_err(|error| CredentialError::Unavailable(error.to_string()))?;
    let result = if (200..300).contains(&response.status) {
        parse_token_response(&response.body, clock)
    } else {
        Err(parse_error(&response.body))
    };
    // Best-effort: the response body held the token in the clear; wipe this
    // process's only remaining copy of the raw text as soon as the values
    // that matter have been extracted into `Secret`s above.
    use zeroize::Zeroize;
    response.body.zeroize();
    result
}

/// Parses a 2xx token-endpoint body into a [`TokenSet`].
///
/// This is the only function in Fieldnotes that reads inside a token, and the
/// token it reads is the ID token, which OpenID Connect issues *to this client*
/// for exactly this purpose. The access token is never inspected: Microsoft
/// documents it as opaque to the client, and it is treated as opaque here.
///
/// The ID token exists only as the local `id_token` binding below. It is a
/// [`Secret`] (no `Display`, redacting `Debug`, zeroized on drop), it is read
/// once, and it is dropped before this function returns. Only the extracted
/// [`AccountId`] survives.
fn parse_token_response(body: &str, clock: &dyn Clock) -> Result<TokenSet, CredentialError> {
    let parsed: TokenSuccessBody = serde_json::from_str(body).map_err(|_| {
        CredentialError::Backend("token endpoint response was not valid".to_owned())
    })?;
    let expires_at_unix_millis = i64::try_from(clock.unix_millis())
        .unwrap_or(i64::MAX)
        .saturating_add(parsed.expires_in.max(0).saturating_mul(1000));
    // Wrapped before it is read, dropped (and zeroized) at the end of this
    // block. An unreadable or absent ID token leaves the account unknown and
    // does not fail the exchange.
    let account = parsed
        .id_token
        .map(Secret::new)
        .as_ref()
        .and_then(id_token::account_from_id_token);
    Ok(TokenSet {
        access_token: AccessToken {
            value: Secret::new(parsed.access_token),
            expires_at_unix_millis,
            scope: parsed.scope,
        },
        refresh_token: parsed.refresh_token.map(Secret::new),
        account,
    })
}

fn parse_error(body: &str) -> CredentialError {
    let Ok(parsed) = serde_json::from_str::<TokenErrorBody>(body) else {
        return CredentialError::Backend(
            "token endpoint returned an unrecognized error".to_owned(),
        );
    };
    classify_oauth_error(&parsed.error, parsed.error_description.as_deref())
}

/// Classifies an OAuth 2.0 `error` code (RFC 6749 section 5.2) into an
/// actionable [`CredentialError`].
///
/// `invalid_grant` covers both a revoked and an expired refresh token in the
/// base OAuth vocabulary; this crate distinguishes them heuristically by
/// checking whether the authorization server's own `error_description`
/// mentions expiry, rather than hardcoding a vendor-specific error-code
/// table (Microsoft's `AADSTS*` codes, for example), which would belong in a
/// Microsoft-specific crate if ever needed.
fn classify_oauth_error(error: &str, description: Option<&str>) -> CredentialError {
    match error {
        "invalid_grant" => {
            let mentions_expiry = description
                .map(|text| text.to_ascii_lowercase().contains("expired"))
                .unwrap_or(false);
            if mentions_expiry {
                CredentialError::Expired
            } else {
                CredentialError::Revoked
            }
        }
        "access_denied" | "consent_required" | "interaction_required" => CredentialError::Denied,
        other => CredentialError::Backend(format!("token endpoint reported error \"{other}\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unwraps the error side of a `Result` a test expects to fail, panicking
    /// with a useful message otherwise. A local stand-in for
    /// `Result::expect_err`, which clippy's `expect_used` lint (deliberately,
    /// including in tests) forbids in this crate.
    fn expect_error<T: core::fmt::Debug>(result: Result<T, CredentialError>) -> CredentialError {
        match result {
            Ok(value) => panic!("expected an error, got Ok({value:?})"),
            Err(error) => error,
        }
    }

    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn unix_millis(&self) -> u64 {
            self.0
        }
    }

    struct FakeTransport {
        status: u16,
        body: String,
    }
    impl TokenTransport for FakeTransport {
        fn post_form(
            &self,
            _endpoint: &str,
            _form: &[(&str, &str)],
        ) -> Result<TokenHttpResponse, TransportError> {
            Ok(TokenHttpResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    #[test]
    fn a_successful_exchange_yields_an_access_token_and_optional_refresh_token() {
        let transport = FakeTransport {
            status: 200,
            body: r#"{"access_token":"AT-canary","refresh_token":"RT-canary","expires_in":3600,"scope":"Mail.Read"}"#.to_owned(),
        };
        let clock = FixedClock(1_000_000);
        let set = exchange_code(
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
            "code",
            "http://127.0.0.1:1/callback",
            "verifier",
        )
        .unwrap_or_else(|error| panic!("expected success: {error}"));
        assert_eq!(set.access_token.expose_secret(), "AT-canary");
        assert_eq!(set.access_token.scope(), Some("Mail.Read"));
        assert_eq!(
            set.access_token.expires_at_unix_millis(),
            1_000_000 + 3_600_000
        );
        assert_eq!(
            set.refresh_token
                .map(|token| token.expose_secret().to_owned()),
            Some("RT-canary".to_owned())
        );
    }

    #[test]
    fn a_response_with_no_rotated_refresh_token_is_none() {
        let transport = FakeTransport {
            status: 200,
            body: r#"{"access_token":"AT-canary","expires_in":3600}"#.to_owned(),
        };
        let clock = FixedClock(0);
        let set = refresh(
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
            &Secret::new("old-refresh-token"),
        )
        .unwrap_or_else(|error| panic!("expected success: {error}"));
        assert!(set.refresh_token.is_none());
    }

    #[test]
    fn invalid_grant_without_expiry_wording_is_revoked() {
        let transport = FakeTransport {
            status: 400,
            body: r#"{"error":"invalid_grant","error_description":"Token has been revoked"}"#
                .to_owned(),
        };
        let clock = FixedClock(0);
        let error = expect_error(refresh(
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
            &Secret::new("old-refresh-token"),
        ));
        assert_eq!(error, CredentialError::Revoked);
    }

    #[test]
    fn invalid_grant_with_expiry_wording_is_expired() {
        let transport = FakeTransport {
            status: 400,
            body:
                r#"{"error":"invalid_grant","error_description":"The refresh token has expired"}"#
                    .to_owned(),
        };
        let clock = FixedClock(0);
        let error = expect_error(refresh(
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
            &Secret::new("old-refresh-token"),
        ));
        assert_eq!(error, CredentialError::Expired);
    }

    #[test]
    fn a_transport_failure_is_unavailable_not_a_panic() {
        struct FailingTransport;
        impl TokenTransport for FailingTransport {
            fn post_form(
                &self,
                _endpoint: &str,
                _form: &[(&str, &str)],
            ) -> Result<TokenHttpResponse, TransportError> {
                Err(TransportError("connection refused".to_owned()))
            }
        }
        let clock = FixedClock(0);
        let error = expect_error(refresh(
            &FailingTransport,
            &clock,
            "https://example.invalid/token",
            "client-id",
            &Secret::new("old-refresh-token"),
        ));
        assert!(matches!(error, CredentialError::Unavailable(_)));
    }

    /// A JWS-shaped ID token whose payload names `preferred_username`.
    ///
    /// The signature segment is deliberately fixture text: nothing in this crate
    /// verifies an ID token's signature, because the token arrived over the
    /// TLS-authenticated response to a request this process made, and because
    /// the extracted value is a display label rather than an authorization
    /// input.
    fn fixture_id_token(username: &str) -> String {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#),
            URL_SAFE_NO_PAD.encode(format!(r#"{{"preferred_username":"{username}"}}"#).as_bytes()),
            "FIXTURE-NOT-A-REAL-ID-TOKEN-SIGNATURE-b41d7e"
        )
    }

    #[test]
    fn an_id_token_in_the_response_yields_the_signed_in_account_and_never_escapes()
    -> Result<(), CredentialError> {
        let id_token = fixture_id_token("mailbox.owner@example.test");
        let transport = FakeTransport {
            status: 200,
            body: format!(
                r#"{{"access_token":"AT-canary","refresh_token":"RT-canary","expires_in":3600,"id_token":"{id_token}"}}"#
            ),
        };
        let clock = FixedClock(0);
        let set = exchange_code(
            &transport,
            &clock,
            "https://example.invalid/token",
            "client-id",
            "code",
            "http://127.0.0.1:1/callback",
            "verifier",
        )?;
        assert_eq!(
            set.account.as_ref().map(AccountId::as_str),
            Some("mailbox.owner@example.test")
        );
        // The returned type has no member holding the ID token, and its own
        // `Debug` — the only formatting a caller can reach for — carries neither
        // the token nor any other material.
        let rendered = format!("{set:?}");
        assert!(!rendered.contains(&id_token), "leaked: {rendered}");
        assert!(!rendered.contains("AT-canary"), "leaked: {rendered}");
        assert!(!rendered.contains("RT-canary"), "leaked: {rendered}");
        Ok(())
    }

    #[test]
    fn a_malformed_or_absent_id_token_leaves_the_account_unknown_without_failing()
    -> Result<(), CredentialError> {
        for body in [
            // No `id_token` at all: an authorization server that was not asked
            // for `openid`, or an older stored grant.
            r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600}"#.to_owned(),
            // Present but not a JWS at all.
            r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600,"id_token":"not-a-token"}"#.to_owned(),
            // Three segments, but the payload is not base64url JSON.
            r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600,"id_token":"aaa.!!!.ccc"}"#.to_owned(),
            // Explicitly null.
            r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600,"id_token":null}"#.to_owned(),
        ] {
            let transport = FakeTransport {
                status: 200,
                body: body.clone(),
            };
            let clock = FixedClock(0);
            let set = exchange_code(
                &transport,
                &clock,
                "https://example.invalid/token",
                "client-id",
                "code",
                "http://127.0.0.1:1/callback",
                "verifier",
            )?;
            // The sign-in succeeded: a usable access token and refresh token.
            assert_eq!(set.access_token.expose_secret(), "AT");
            assert!(set.refresh_token.is_some());
            // And the account is simply unknown.
            assert_eq!(set.account, None, "must be unknown, not an error: {body}");
        }
        Ok(())
    }

    #[test]
    fn access_token_expiry_respects_injected_clock_and_skew() {
        let transport = FakeTransport {
            status: 200,
            body: r#"{"access_token":"AT","expires_in":60}"#.to_owned(),
        };
        let minted_at = FixedClock(0);
        let set = exchange_code(
            &transport,
            &minted_at,
            "https://example.invalid/token",
            "client-id",
            "code",
            "http://127.0.0.1:1/callback",
            "verifier",
        )
        .unwrap_or_else(|error| panic!("expected success: {error}"));
        let just_before = FixedClock(59_000);
        assert!(!set.access_token.is_expired(&just_before, Duration::ZERO));
        let just_after = FixedClock(60_001);
        assert!(set.access_token.is_expired(&just_after, Duration::ZERO));
        let with_skew = FixedClock(55_000);
        assert!(
            set.access_token
                .is_expired(&with_skew, Duration::from_secs(10))
        );
    }
}
