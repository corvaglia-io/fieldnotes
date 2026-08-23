//! OAuth 2.0 authorization-code flow with PKCE, for a public client.
//!
//! This module is deliberately the whole of this crate's authentication
//! surface: it implements RFC 6749's authorization-code grant plus RFC 7636's
//! PKCE extension and an RFC 8252-style loopback redirect, and nothing else.
//! **Device-code flow (RFC 8628) is not implemented anywhere in this crate,
//! on purpose**: it is a well-documented phishing vector (a user can be
//! tricked into approving a code an attacker generated, on a real login
//! page, with no way to tell the two attempts apart), and Microsoft Entra is
//! actively restricting it for exactly that reason.
//!
//! # How the pieces fit together
//!
//! 1. [`crate::pkce::CodeVerifier::generate`] and
//!    [`crate::pkce::State::generate`] produce this attempt's secret verifier
//!    and CSRF nonce from injected randomness.
//! 2. [`authorize::AuthorizeRequest::url`] builds the authorize URL,
//!    including the verifier's `S256` challenge and the `state` value — the
//!    composition root opens this URL in a system browser (this crate does
//!    not launch one; see the crate-level documentation).
//! 3. [`loopback::LoopbackListener`] binds an ephemeral loopback port before
//!    step 2 (so its port is known for the redirect URI) and, after the
//!    browser navigates back, validates `state` and extracts the
//!    authorization code.
//! 4. [`broker::AccessTokenProvider::complete_authorization`] redeems the
//!    code and verifier at the token endpoint ([`token::exchange_code`]),
//!    stores the returned refresh token, and returns a minted access token.
//! 5. On every later collection run,
//!    [`broker::AccessTokenProvider::mint_access_token`] loads the stored
//!    refresh token, redeems it ([`token::refresh`]), and rotates the stored
//!    value in place when the response carries a new one.
//!
//! Only the access token minted in steps 4 and 5 ever crosses into
//! [`crate::provider`]'s caller-facing surface as something meant to leave
//! this crate (over the protected channel to a Field, per A2 section 12).
//! The refresh token stays inside steps 2-5, touching only a
//! [`crate::provider::CredentialProvider`].

pub mod authorize;
pub mod broker;
pub mod loopback;
pub mod token;

pub use authorize::AuthorizeRequest;
pub use broker::AccessTokenProvider;
pub use loopback::{CallbackResult, LoopbackError, LoopbackListener};
pub use token::{
    AccessToken, TokenHttpResponse, TokenSet, TokenTransport, TransportError, UreqTokenTransport,
};
