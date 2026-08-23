//! The Field-owned incremental resume cursor: an opaque wrapper around a
//! Graph delta token.
//!
//! # Format
//!
//! ```text
//! outlook-calendar-delta/v1;dt=<percent-encoded-odata-delta-link>
//! ```
//!
//! `dt` is Graph's own `@odata.deltaLink`, exactly as
//! [`fieldnotes_msgraph::DeltaToken`] carries it, percent-encoded because a
//! delta link is itself a full URL containing characters this Field's cursor
//! grammar does not admit unescaped (notably `:` and `?`).
//!
//! # Why it can lag but never lead
//!
//! This Field commits a cursor **only** at the end of a run in which every
//! page it asked for was actually fetched and every item on those pages was
//! either turned into an accepted record or an accepted tombstone --
//! [`crate::collect`] never advances past a page it has not yet durably
//! reported. Graph's own delta semantics do the rest: a delta token is a
//! literal continuation point in an append-only change feed, so resuming
//! from a stored token re-offers exactly what has not yet been reported and
//! nothing that has. Re-emitting an event already collected is always safe
//! and idempotent, because [`crate::record`] derives the portable
//! `(source_scope, source_identity)` key from Graph's own immutable event
//! `id`, unaffected by how many times the same delta range is replayed.
//! Skipping ahead is not: a delta token this Field never actually reached
//! the end of, offered anyway, would drop every item between where the Field
//! stopped and where the token points, permanently -- Graph's change feed
//! does not replay what a token has already passed. That is why this cursor
//! carries only what Graph itself confirmed as the *end* of a fully-read
//! page sequence, never an intermediate `@odata.nextLink`.

use fieldnotes_field_protocol::grammar::{Cursor as CursorToken, GrammarError};
use fieldnotes_msgraph::DeltaToken;

/// The tag every cursor this Field writes begins with.
const TAG: &str = "outlook-calendar-delta/v1";

/// The decoded state of a cursor this Field previously offered: Graph's own
/// opaque delta continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CursorState {
    pub(crate) delta_token: DeltaToken,
}

impl CursorState {
    /// Encodes this state as an opaque cursor token.
    pub(crate) fn encode(&self) -> Result<CursorToken, GrammarError> {
        let text = format!(
            "{TAG};dt={}",
            fieldnotes_field_sdk::percent::encode(self.delta_token.as_str())
        );
        CursorToken::parse(&text)
    }

    /// Decodes a previously offered cursor.
    ///
    /// `None` means the text is not one of this Field's own cursors, or is
    /// corrupt despite matching this Field's tag -- either way, the caller
    /// must treat this run as having no usable resume point rather than
    /// trust a token it cannot read.
    #[must_use]
    pub(crate) fn decode(token: &CursorToken) -> Option<Self> {
        let text = token.as_str();
        let mut segments = text.split(';');
        if segments.next() != Some(TAG) {
            return None;
        }
        let mut delta_token = None;
        for segment in segments {
            let value = segment.strip_prefix("dt=")?;
            delta_token = Some(fieldnotes_field_sdk::percent::decode(value)?);
        }
        delta_token.map(|raw| CursorState {
            delta_token: DeltaToken::from(raw),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::CursorState;
    use fieldnotes_msgraph::DeltaToken;

    #[test]
    fn encode_then_decode_round_trips() {
        let state = CursorState {
            delta_token: DeltaToken::from(
                "https://graph.microsoft.com/v1.0/me/calendarView/delta?$deltatoken=abc:123"
                    .to_owned(),
            ),
        };
        let token = state
            .encode()
            .unwrap_or_else(|error| panic!("must encode: {error}"));
        let decoded = CursorState::decode(&token).unwrap_or_else(|| panic!("must decode: {token}"));
        assert_eq!(decoded, state);
    }

    #[test]
    fn a_foreign_cursor_text_decodes_to_none() {
        let foreign = fieldnotes_field_protocol::grammar::Cursor::parse("local-walk/v1;hw=100")
            .unwrap_or_else(|error| panic!("must parse as a cursor at all: {error}"));
        assert_eq!(CursorState::decode(&foreign), None);
    }

    #[test]
    fn a_tag_only_cursor_with_no_token_decodes_to_none() {
        let malformed =
            fieldnotes_field_protocol::grammar::Cursor::parse("outlook-calendar-delta/v1")
                .unwrap_or_else(|error| panic!("must parse as a cursor at all: {error}"));
        assert_eq!(CursorState::decode(&malformed), None);
    }
}
