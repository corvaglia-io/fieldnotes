//! The Field-owned resume cursor: a Graph delta link, wrapped opaquely.
//!
//! # Why it can lag but never lead
//!
//! A Graph delta link is itself already the exact resume point Graph commits
//! to honouring: resuming from one replays every change (including a
//! removal) that happened at or after the moment that link was issued, and
//! never after. This Field never advances the cursor itself -- it only ever
//! re-emits whatever [`fieldnotes_msgraph::PageStream::delta_token`] returns
//! once a full page of the delta feed has actually been read and turned into
//! records -- so the offered cursor always reflects a point this Field has
//! fully accounted for. Re-emitting a contact core already holds is a no-op
//! through the portable exact-source key; skipping one because the cursor
//! was advanced past it would lose that contact permanently. The asymmetry
//! is deliberate and matches A2 section 9 exactly.
//!
//! # Format
//!
//! ```text
//! outlook-contacts-delta/v1;<raw-delta-link>
//! ```
//!
//! The raw delta link is Graph's own `@odata.deltaLink` value, an absolute
//! URL, carried verbatim after the tag. Core never parses this string; only
//! this Field's own driver does, by handing it back to
//! [`fieldnotes_msgraph::DeltaStart::Resume`] unchanged.

use fieldnotes_msgraph::DeltaToken;

use fieldnotes_field_protocol::grammar::{Cursor as CursorToken, GrammarError};

const TAG: &str = "outlook-contacts-delta/v1";

/// Encodes a Graph delta token as this Field's cursor.
pub(crate) fn encode(delta_token: &DeltaToken) -> Result<CursorToken, GrammarError> {
    CursorToken::parse(&format!("{TAG};{}", delta_token.as_str()))
}

/// Decodes a previously offered cursor back into a Graph delta token.
///
/// `None` means the text is not one of this Field's own cursors -- core
/// already refuses to replay a cursor written at a different declared
/// `cursor_format_version` (A2 section 9), so this only fires for a cursor
/// that is corrupt despite matching this Field's own format version. Either
/// way, the caller starts an unbounded initial delta collection rather than
/// trust a token it cannot read.
#[must_use]
pub(crate) fn decode(token: &CursorToken) -> Option<DeltaToken> {
    let text = token.as_str();
    let raw = text.strip_prefix(TAG)?.strip_prefix(';')?;
    if raw.is_empty() {
        return None;
    }
    Some(DeltaToken::from(raw.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    use fieldnotes_field_protocol::grammar::Cursor as CursorToken;
    use fieldnotes_msgraph::DeltaToken;

    #[test]
    fn encode_then_decode_round_trips() {
        let delta = DeltaToken::from(
            "https://graph.microsoft.com/v1.0/me/contacts/delta?$deltatoken=abc123".to_owned(),
        );
        let token = encode(&delta).unwrap_or_else(|error| panic!("must encode: {error}"));
        assert!(token.as_str().starts_with("outlook-contacts-delta/v1;"));
        let decoded = decode(&token).unwrap_or_else(|| panic!("must decode"));
        assert_eq!(decoded, delta);
    }

    #[test]
    fn a_foreign_cursor_text_decodes_to_none() {
        let foreign = CursorToken::parse("local-walk/v1;hw=0")
            .unwrap_or_else(|error| panic!("must parse as a cursor at all: {error}"));
        assert_eq!(decode(&foreign), None);
    }

    #[test]
    fn an_empty_delta_link_decodes_to_none() {
        let malformed = CursorToken::parse("outlook-contacts-delta/v1;")
            .unwrap_or_else(|error| panic!("must parse as a cursor at all: {error}"));
        assert_eq!(decode(&malformed), None);
    }
}
