//! The Field-owned incremental resume cursor: a Graph delta token, opaque to
//! core.
//!
//! # Format
//!
//! ```text
//! outlook-mail/v1[;dt=<percent-encoded @odata.deltaLink>]
//! ```
//!
//! `dt` carries Graph's own `@odata.deltaLink` verbatim, percent-encoded so
//! neither the cursor's own `;` and `=` delimiters nor any byte of the link can
//! be confused for cursor structure. The `dt` segment is absent when no delta
//! collection has completed yet, which a later run reads as "start delta from
//! the beginning".
//!
//! A delta link is a paging cursor, not a credential: Graph returns it in the
//! response body, it authorizes nothing on its own, and every request made
//! with it still carries the bearer token separately. It is never a secret, so
//! carrying it here does not put a secret in a cursor. The access token is
//! never any part of this cursor, and this module never sees one.
//!
//! # Why it can lag but never lead
//!
//! [`CursorState::adopt`] is the only way a delta token ever enters a cursor,
//! and it is reached only when **all** of the following hold:
//!
//! - the run was not bounded by a window (a windowed run is bounded evidence
//!   and its coverage says nothing about the rest of the mailbox, so it
//!   re-offers the previous cursor completely unchanged);
//! - the delta [`PageStream`](fieldnotes_msgraph::PageStream) reached its final
//!   page, which is the only circumstance in which Graph supplies a delta link
//!   at all -- a stream cut short by a Graph error, by the run's record bound,
//!   or by a deserialization failure yields `None` and cannot advance
//!   anything;
//! - every message on every fetched page was mapped and emitted without a
//!   single error.
//!
//! Any failure anywhere therefore freezes the cursor at its previous value,
//! and the next run re-collects from there. Re-emitting a message already
//! collected is idempotent through the portable exact-source key
//! `(source_scope, source_identity)`: core recognizes the replay as the same
//! current state. Skipping one would lose it permanently, because a delta
//! collection never revisits what its token has already passed.
//!
//! When the encoded cursor would exceed the run's `max_cursor_bytes`, the
//! delta token is dropped rather than truncated: a truncated delta link would
//! be a token that *looks* resumable and silently resumes from the wrong
//! place, which is the one failure mode this format must not have.

use fieldnotes_field_protocol::grammar::{Cursor as CursorToken, GrammarError};
use fieldnotes_msgraph::DeltaToken;

/// The tag every cursor this Field writes begins with.
const TAG: &str = "outlook-mail/v1";

/// The segment key carrying the percent-encoded delta link.
const DELTA_KEY: &str = "dt=";

/// The decoded state of a cursor this Field previously offered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CursorState {
    /// Graph's `@odata.deltaLink` from the last delta collection that
    /// completed without a single error, when there has been one.
    delta_link: Option<String>,
}

impl CursorState {
    /// The state a Field with no resumable delta collection offers.
    #[must_use]
    pub(crate) fn empty() -> Self {
        CursorState { delta_link: None }
    }

    /// The delta token to resume from, if any.
    #[must_use]
    pub(crate) fn resume_token(&self) -> Option<DeltaToken> {
        self.delta_link
            .as_ref()
            .map(|link| DeltaToken::from(link.clone()))
    }

    /// Whether this state can resume a delta collection at all.
    #[must_use]
    pub(crate) fn is_resumable(&self) -> bool {
        self.delta_link.is_some()
    }

    /// The state to offer after a complete, error-free, unwindowed delta
    /// collection that ended with `token`.
    #[must_use]
    pub(crate) fn adopt(token: &DeltaToken) -> Self {
        CursorState {
            delta_link: Some(token.as_str().to_owned()),
        }
    }

    /// Encodes this state as an opaque cursor token.
    pub(crate) fn encode(&self) -> Result<CursorToken, GrammarError> {
        let mut text = TAG.to_owned();
        if let Some(link) = &self.delta_link {
            text.push(';');
            text.push_str(DELTA_KEY);
            text.push_str(&fieldnotes_field_sdk::percent::encode(link));
        }
        CursorToken::parse(&text)
    }

    /// Decodes a previously offered cursor.
    ///
    /// `None` means the text is not one of this Field's own cursors. Core
    /// already refuses to replay a cursor written at a different declared
    /// `cursor_format_version` (A2 section 9), so this only fires for a cursor
    /// that is corrupt despite matching this Field's own format version.
    /// Either way the caller starts an unbounded delta collection rather than
    /// trust a token it cannot read.
    #[must_use]
    pub(crate) fn decode(token: &CursorToken) -> Option<Self> {
        let text = token.as_str();
        let mut segments = text.split(';');
        if segments.next() != Some(TAG) {
            return None;
        }
        let mut state = CursorState::empty();
        for segment in segments {
            let encoded = segment.strip_prefix(DELTA_KEY)?;
            if encoded.is_empty() {
                continue;
            }
            state.delta_link = Some(fieldnotes_field_sdk::percent::decode(encoded)?);
        }
        Some(state)
    }
}

/// Encodes `state`, self-policing against the run's cursor-byte bound.
///
/// A delta link that will not fit is **dropped**, never truncated: the next
/// run then starts an unbounded delta collection, which costs work and loses
/// nothing. A truncated link would resume from an unknown place, which is the
/// one thing a cursor may never do.
pub(crate) fn encode_within_limit(
    state: &CursorState,
    max_cursor_bytes: u64,
) -> Option<CursorToken> {
    if let Ok(token) = state.encode()
        && u64::try_from(token.as_str().len()).unwrap_or(u64::MAX) <= max_cursor_bytes
    {
        return Some(token);
    }
    CursorState::empty().encode().ok()
}

#[cfg(test)]
mod tests {
    use super::{CursorState, encode_within_limit};
    use fieldnotes_msgraph::DeltaToken;

    const LINK: &str = "https://graph.microsoft.com/v1.0/me/mailFolders('inbox')/messages/delta?$deltatoken=FIXTURE_DELTA_1";

    #[test]
    fn an_empty_state_encodes_to_the_bare_tag_and_round_trips() {
        let state = CursorState::empty();
        let token = state
            .encode()
            .unwrap_or_else(|error| panic!("must encode: {error}"));
        assert_eq!(token.as_str(), "outlook-mail/v1");
        assert_eq!(CursorState::decode(&token), Some(state));
    }

    #[test]
    fn an_adopted_delta_link_round_trips_exactly() {
        let state = CursorState::adopt(&DeltaToken::from(LINK.to_owned()));
        let token = state
            .encode()
            .unwrap_or_else(|error| panic!("must encode: {error}"));
        let decoded = CursorState::decode(&token).unwrap_or_else(|| panic!("must decode: {token}"));
        assert_eq!(decoded, state);
        assert_eq!(
            decoded
                .resume_token()
                .map(|token| token.as_str().to_owned()),
            Some(LINK.to_owned())
        );
    }

    #[test]
    fn the_encoded_cursor_never_exposes_the_links_own_delimiters() {
        let state = CursorState::adopt(&DeltaToken::from(LINK.to_owned()));
        let token = state
            .encode()
            .unwrap_or_else(|error| panic!("must encode: {error}"));
        // Exactly one ';' -- the cursor's own separator -- and exactly one
        // '=' -- the cursor's own `dt=` key.
        assert_eq!(token.as_str().matches(';').count(), 1);
        assert_eq!(token.as_str().matches('=').count(), 1);
        assert!(token.as_str().is_ascii());
    }

    #[test]
    fn a_foreign_cursor_decodes_to_none_rather_than_being_misread() {
        for foreign in ["local-walk/v1;hw=1755000000", "outlook-mail/v2;dt=abc"] {
            let token = fieldnotes_field_protocol::grammar::Cursor::parse(foreign)
                .unwrap_or_else(|error| panic!("must parse as a cursor at all: {error}"));
            assert_eq!(CursorState::decode(&token), None, "{foreign}");
        }
    }

    #[test]
    fn a_corrupt_segment_decodes_to_none() {
        let token = fieldnotes_field_protocol::grammar::Cursor::parse("outlook-mail/v1;dt=100%zz")
            .unwrap_or_else(|error| panic!("must parse as a cursor at all: {error}"));
        assert_eq!(CursorState::decode(&token), None);
    }

    #[test]
    fn a_delta_link_too_long_for_the_bound_is_dropped_not_truncated() {
        let long = format!("{LINK}{}", "A".repeat(5000));
        let state = CursorState::adopt(&DeltaToken::from(long));
        let token = encode_within_limit(&state, 4096)
            .unwrap_or_else(|| panic!("a bare cursor must always encode"));
        assert_eq!(token.as_str(), "outlook-mail/v1");
        let decoded = CursorState::decode(&token).unwrap_or_else(|| panic!("must decode: {token}"));
        assert!(
            !decoded.is_resumable(),
            "a dropped delta token must read as 'start over', never as a partial token"
        );
    }

    #[test]
    fn a_delta_link_within_the_bound_survives_the_self_policing() {
        let state = CursorState::adopt(&DeltaToken::from(LINK.to_owned()));
        let token = encode_within_limit(&state, 4096)
            .unwrap_or_else(|| panic!("must encode within the bound"));
        assert_eq!(
            CursorState::decode(&token).and_then(|state| state.resume_token()),
            Some(DeltaToken::from(LINK.to_owned()))
        );
    }
}
