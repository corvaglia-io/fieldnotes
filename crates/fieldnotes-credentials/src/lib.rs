//! Credential-provider boundary for Fieldnotes.
//!
//! This crate is the security-critical trust path for Microsoft
//! authentication and every future authenticated source: it stores refresh
//! tokens, mints short-lived access tokens, and hands the latter to core over
//! the protected channel described in
//! [A2 section 12](../../../docs/approvals/A2-field-protocol.md). It never
//! stores an access token and it never lets a refresh token reach a Field
//! process; see [`oauth`] for the boundary between the two.
//!
//! # Design summary
//!
//! - [`reference::CredentialRef`] is the non-secret currency: configuration,
//!   logs, and diagnostics may hold and print it freely. It never carries
//!   material.
//! - [`secret::Secret`] is the guarded currency: its `Debug` implementation
//!   always redacts, it has no `Display` implementation at all (so a stray
//!   `{}` format fails to compile rather than leaking), and its backing
//!   buffer is zeroized when dropped.
//! - [`provider::CredentialProvider`] retrieves and stores a [`secret::Secret`]
//!   by a [`reference::CredentialRef`], and reports absence, unavailability,
//!   revocation, and expiry through [`error::CredentialError`] rather than an
//!   opaque failure.
//! - [`keychain`] is the default provider, backed by the platform keychain
//!   (macOS Keychain, Windows Credential Manager, Linux Secret Service)
//!   through the `keyring` crate.
//! - [`mod@env`] is the explicit, opt-in provider for CI and headless use that
//!   the roadmap requires. It is never a silent fallback: a caller must name
//!   it deliberately, and it is read-only (it cannot honor `store`).
//! - [`pkce`] and [`oauth`] implement the OAuth 2.0 authorization-code flow
//!   with PKCE for a public client (RFC 7636, RFC 8252-style loopback
//!   redirect). Device-code flow is deliberately not implemented anywhere in
//!   this crate: it is a well-documented phishing vector and Microsoft Entra
//!   is actively restricting it.
//!
//! # What this crate deliberately does not do
//!
//! - It does not call the Microsoft Graph API or any other resource server;
//!   it only talks to the OAuth authorize and token endpoints. Graph
//!   transport is a separate crate's responsibility.
//! - It does not launch a system browser. Building the authorize URL and
//!   running the loopback listener are this crate's job; invoking an
//!   OS-appropriate "open this URL" mechanism belongs to the composition
//!   root (the CLI, when `fields auth` is wired).
//! - It does not read the wall clock or an OS random source directly.
//!   Callers inject [`fieldnotes_domain::Clock`] and
//!   [`fieldnotes_domain::RandomSource`], which keeps every code path in this
//!   crate deterministic and testable without touching real time or entropy.
//! - It never persists an access token, a code verifier, or a `state` value
//!   to a file, a cache, or notebook material. Everything other than a
//!   stored refresh token lives in memory for the life of one authorization
//!   or one collection run.

pub mod env;
pub mod error;
pub mod keychain;
pub mod oauth;
mod percent;
pub mod pkce;
pub mod provider;
pub mod reference;
pub mod secret;

pub use error::CredentialError;
pub use provider::CredentialProvider;
pub use reference::{CredentialRef, CredentialRefError};
pub use secret::Secret;
