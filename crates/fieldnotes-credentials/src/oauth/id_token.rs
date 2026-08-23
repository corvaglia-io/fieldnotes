//! Reading **which account signed in** out of an OpenID Connect ID token.
//!
//! # Why decoding this token is correct, and decoding an access token is not
//!
//! An **ID token** is issued *to the client that requested it*, for the sole
//! purpose of telling that client who signed in. OpenID Connect Core section 2
//! defines its claims as the client's own input, and Microsoft documents the
//! ID token as a token the application is expected to read. Decoding it here is
//! therefore the documented, intended use.
//!
//! An **access token** is the opposite: it is issued *to the resource server*,
//! and Microsoft documents it as opaque to the client, explicitly warning that
//! its format may change without notice and that a client must not parse it.
//! Nothing in this crate ever parses one, and nothing should: that is the whole
//! reason one of these two is acceptable to decode and the other is not.
//!
//! # This value is for display and confirmation only, never authorization
//!
//! [`AccountId`] exists so a person can be *shown* which principal a stored
//! credential authenticates as, and can confirm it is the one they meant. It is
//! never an authorization input:
//!
//! - This module does **not** verify the ID token's signature, its `iss`, its
//!   `aud`, its `nonce`, or its expiry. It does not need to: the token arrived
//!   over the TLS-authenticated token-endpoint response for a request this
//!   process itself made, so it is not an attacker-supplied bearer assertion
//!   being presented to us for a decision. That also means the value carries
//!   none of the guarantees an authorization decision would require.
//! - Nothing anywhere in Fieldnotes may grant or deny access, authorize a
//!   deletion, choose a scope, or select a credential based on this value. A
//!   future reader will be tempted, because it looks like an identity. It is a
//!   *label*, and treating a label as a decision is exactly how a confused
//!   deputy is built.
//!
//! # The ID token itself is credential-adjacent
//!
//! It is a signed bearer assertion about a person. It is therefore handled the
//! same way this crate handles every other piece of token material:
//!
//! - it arrives inside the token-endpoint response body, which
//!   [`crate::oauth::token`] zeroizes as soon as it has extracted what it
//!   needs;
//! - it is wrapped in a [`Secret`] the instant it is parsed out, so it has no
//!   [`core::fmt::Display`] at all and a redacting [`core::fmt::Debug`];
//! - it never leaves `parse_token_response`'s own scope in
//!   [`crate::oauth::token`]: [`crate::oauth::TokenSet`] carries the extracted
//!   [`AccountId`] and *not* the token, so there is no field for a stray
//!   `{:?}`, a serializer, or a log line to reach;
//! - the decoded claims buffer is a [`Zeroizing`] buffer for the same reason;
//! - it is never persisted. Only [`AccountId`] is retained.

use core::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::secret::Secret;

/// The longest account identifier this module will accept.
///
/// RFC 5321 bounds an email address's forward path at 256 octets, and an
/// `oid`/`sub` claim is a GUID or a short opaque string, so anything longer than
/// this is not a plausible account identifier and is refused rather than
/// truncated: a truncated identifier displayed next to a different truncated
/// identifier is worse than reporting the account as unknown.
pub const MAX_ACCOUNT_BYTES: usize = 256;

/// The account a stored credential authenticates as, for display and
/// confirmation only.
///
/// Non-secret: it is safe in output, in `fields status`, and in a Field's
/// non-secret configuration file. It is still **personal data** — it names a
/// person — so see this module's documentation for what may and may not be done
/// with it. It is emphatically not an authorization input.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(String);

impl AccountId {
    /// Validates `text` as a displayable account identifier.
    ///
    /// The value came from a remote authorization server, so it is treated as
    /// untrusted display text even though it arrived over TLS: it is trimmed,
    /// bounded by [`MAX_ACCOUNT_BYTES`], and refused outright if it contains a
    /// control character or a Unicode bidirectional/line-separator control.
    /// Those are the characters that let remote text rewrite a terminal line or
    /// reorder a rendered identifier, which is precisely the confusion this
    /// whole feature exists to remove.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_ACCOUNT_BYTES {
            return None;
        }
        if trimmed.chars().any(is_display_hostile) {
            return None;
        }
        Some(AccountId(trimmed.to_owned()))
    }

    /// The validated textual form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for AccountId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Whether `character` could rewrite or reorder a rendered line.
fn is_display_hostile(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            // Unicode line and paragraph separators.
            '\u{2028}' | '\u{2029}'
            // Bidirectional marks, embeddings, overrides, and isolates.
            | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            // A zero-width byte-order mark in the middle of text.
            | '\u{feff}'
        )
}

/// The ID token claims this module reads.
///
/// Deliberately does **not** derive `Debug`: these are a named person's
/// identifiers, and not deriving it means there is nothing for a stray `{:?}`
/// to print. Every member is optional because an authorization server is free
/// to omit any of them, and an absent claim must leave the account unknown
/// rather than fail the sign-in.
#[derive(Deserialize)]
struct Claims {
    /// Microsoft Entra's human-recognizable sign-in name, present when the
    /// `profile` scope was granted. This is the claim a person can actually
    /// confirm.
    #[serde(default)]
    preferred_username: Option<String>,
    /// The user principal name, on issuers that send it instead.
    #[serde(default)]
    upn: Option<String>,
    /// The mail address, present only when the `email` scope was granted (which
    /// core does not request; see [`crate::oauth::token`]).
    #[serde(default)]
    email: Option<String>,
    /// The immutable directory object identifier: opaque, but stable.
    #[serde(default)]
    oid: Option<String>,
    /// The subject identifier: opaque, and pairwise per application.
    #[serde(default)]
    sub: Option<String>,
}

/// Extracts the signed-in account from an ID token, or `None`.
///
/// `None` covers every way this can fail to produce something worth showing —
/// a token that is not three dot-separated segments, a payload that is not
/// base64url, a payload that is not JSON, a claim set that names no usable
/// identifier, and a claim whose value would be hostile to display. **None of
/// those is an error**: an authorization that produced a working refresh token
/// succeeded, and the account simply stays unknown, to be reported as unknown
/// and fixed by re-authenticating.
///
/// The claim preference order is deliberate: `preferred_username`, then `upn`,
/// then `email`, then `oid`, then `sub`. The first three are what a person can
/// recognize, and recognition is the entire point — an operator confirming
/// "this is my mailbox, not the administrator's" cannot do that from a GUID.
/// The two opaque claims are the last resort, so an issuer that sends no
/// human-readable claim still yields something two Fields can be compared on.
///
/// This is the one place in Fieldnotes that reads inside a token, and it reads
/// an ID token, which is documented as the client's to read. See this module's
/// documentation for why an access token is never treated this way.
#[must_use]
pub fn account_from_id_token(id_token: &Secret) -> Option<AccountId> {
    let mut segments = id_token.expose_secret().split('.');
    let (_header, payload, _signature) = match (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) {
        // Exactly three segments: JWS Compact Serialization (RFC 7515 section
        // 7.1). Anything else is not an ID token this module will read.
        (Some(header), Some(payload), Some(signature), None) => (header, payload, signature),
        _ => return None,
    };
    // Compact serialization is unpadded, but an issuer that pads anyway should
    // not cost a user their account display.
    let unpadded = payload.trim_end_matches('=');
    let decoded = Zeroizing::new(URL_SAFE_NO_PAD.decode(unpadded).ok()?);
    let claims: Claims = serde_json::from_slice(&decoded).ok()?;
    [
        claims.preferred_username,
        claims.upn,
        claims.email,
        claims.oid,
        claims.sub,
    ]
    .into_iter()
    .flatten()
    .find_map(|candidate| AccountId::parse(&candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a JWS-shaped fixture token with `claims_json` as its payload.
    ///
    /// The header and signature segments are deliberately nonsense: this module
    /// verifies no signature (see the module documentation), so a fixture that
    /// carried a real one would imply a guarantee that does not exist.
    fn fixture_token(claims_json: &str) -> Secret {
        Secret::new(format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#),
            URL_SAFE_NO_PAD.encode(claims_json.as_bytes()),
            "FIXTURE-NOT-A-REAL-SIGNATURE"
        ))
    }

    #[test]
    fn the_human_recognizable_claim_wins() {
        let token = fixture_token(
            r#"{"preferred_username":"mailbox.owner@example.test",
                "upn":"other@example.test",
                "oid":"00000000-0000-0000-0000-000000000001",
                "sub":"opaque-subject"}"#,
        );
        let account = account_from_id_token(&token)
            .unwrap_or_else(|| panic!("a well-formed fixture must yield an account"));
        assert_eq!(account.as_str(), "mailbox.owner@example.test");
    }

    #[test]
    fn each_fallback_claim_is_used_in_order() {
        for (claims, expected) in [
            (r#"{"upn":"upn@example.test"}"#, "upn@example.test"),
            (r#"{"email":"mail@example.test"}"#, "mail@example.test"),
            (
                r#"{"oid":"11111111-2222-3333-4444-555555555555"}"#,
                "11111111-2222-3333-4444-555555555555",
            ),
            (
                r#"{"sub":"pairwise-subject-value"}"#,
                "pairwise-subject-value",
            ),
        ] {
            let account = account_from_id_token(&fixture_token(claims))
                .unwrap_or_else(|| panic!("`{claims}` must yield an account"));
            assert_eq!(account.as_str(), expected);
        }
    }

    #[test]
    fn a_malformed_token_leaves_the_account_unknown_without_failing() {
        for malformed in [
            // Not three segments.
            "",
            "only-one-segment",
            "two.segments",
            "four.segments.are.too.many",
            // A payload that is not base64url.
            "aGVhZGVy.not base64url!.sig",
            // Valid base64url that is not JSON.
            "aGVhZGVy.bm90IGpzb24.sig",
        ] {
            assert_eq!(
                account_from_id_token(&Secret::new(malformed)),
                None,
                "`{malformed}` must be unknown, not an error"
            );
        }
        // Well-formed JSON that names no identifier at all.
        assert_eq!(
            account_from_id_token(&fixture_token(r#"{"tid":"a-tenant","iat":1}"#)),
            None
        );
        // Present but empty or whitespace claims are not identifiers either.
        assert_eq!(
            account_from_id_token(&fixture_token(r#"{"preferred_username":"   "}"#)),
            None
        );
    }

    #[test]
    fn a_display_hostile_claim_is_refused_rather_than_rendered() {
        // A claim that could rewrite a terminal line, reorder a rendered
        // identifier, or split output across lines never becomes an account.
        for hostile in [
            "owner@example.test\nADMIN: all clear",
            "owner@example.test\u{1b}[2K",
            "owner@example.test\u{202e}",
            "owner\u{2066}@example.test",
            "owner@example.test\u{2028}second line",
        ] {
            assert_eq!(
                AccountId::parse(hostile),
                None,
                "`{hostile:?}` must be refused"
            );
        }
        let overlong = format!("{}@example.test", "a".repeat(MAX_ACCOUNT_BYTES));
        assert_eq!(AccountId::parse(&overlong), None);
    }

    #[test]
    fn a_padded_payload_is_still_read() {
        // Compact serialization is unpadded, but padding must not cost a user
        // their account display.
        let padded = format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(b"{}"),
            base64::engine::general_purpose::URL_SAFE
                .encode(br#"{"preferred_username":"owner@example.test"}"#),
            "sig"
        );
        let account = account_from_id_token(&Secret::new(padded))
            .unwrap_or_else(|| panic!("a padded payload must still decode"));
        assert_eq!(account.as_str(), "owner@example.test");
    }

    #[test]
    fn the_account_type_renders_but_the_token_never_does() {
        let account = AccountId::parse("owner@example.test")
            .unwrap_or_else(|| panic!("a plain address must parse"));
        assert_eq!(account.to_string(), "owner@example.test");
        assert_eq!(format!("{account:?}"), "AccountId(\"owner@example.test\")");
        // And the token it came from has no Display at all and a redacting
        // Debug, which is what keeps it out of every output path.
        let token = fixture_token(r#"{"preferred_username":"owner@example.test"}"#);
        assert_eq!(format!("{token:?}"), "Secret(\"[redacted]\")");
    }
}
