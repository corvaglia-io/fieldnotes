//! Byte-bounded, UTF-8-boundary-safe text truncation.
//!
//! Used to fit a record's body text under
//! [`fieldnotes_field_protocol::limits::Limits::max_body_bytes`] and a
//! diagnostic's message under
//! [`fieldnotes_field_protocol::grammar`]'s `MessageText` bound, without ever
//! splitting a multi-byte UTF-8 character in half -- which would otherwise
//! either panic (slicing mid-character) or silently corrupt the last
//! character kept.

/// Truncates `text` to at most `max_bytes`, landing on the nearest UTF-8
/// character boundary at or before that many bytes.
///
/// Returns the truncated text and how many **characters** -- not bytes --
/// were removed, which is what
/// [`fieldnotes_field_protocol::message::Integrity`]'s `lost_characters`
/// member counts. When `text` already fits, it is returned unchanged and the
/// removed count is `0`.
#[must_use]
pub fn truncate_utf8(text: &str, max_bytes: u64) -> (String, u64) {
    let max = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    if text.len() <= max {
        return (text.to_owned(), 0);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let kept = &text[..end];
    let original_chars = text.chars().count();
    let kept_chars = kept.chars().count();
    (
        kept.to_owned(),
        u64::try_from(original_chars.saturating_sub(kept_chars)).unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::truncate_utf8;

    #[test]
    fn text_within_the_bound_is_returned_unchanged() {
        assert_eq!(truncate_utf8("hello", 10), ("hello".to_owned(), 0));
    }

    #[test]
    fn text_exactly_at_the_bound_is_not_truncated() {
        assert_eq!(truncate_utf8("hello", 5), ("hello".to_owned(), 0));
    }

    #[test]
    fn truncation_never_splits_a_multibyte_character() {
        // Each 'é' is 2 bytes; a 3-byte budget must drop back to 2 bytes (one
        // whole character) rather than split the second character's bytes.
        let (kept, lost) = truncate_utf8("éé", 3);
        assert_eq!(kept, "é");
        assert_eq!(lost, 1);
        assert!(kept.is_char_boundary(kept.len()));
    }

    #[test]
    fn a_limit_landing_exactly_mid_character_drops_the_whole_character() {
        // "a" is 1 byte and '🎯' is 4 bytes, for 5 bytes total. A 3-byte
        // budget falls inside the emoji's own encoding, so only "a" survives.
        let (kept, lost) = truncate_utf8("a🎯", 3);
        assert_eq!(kept, "a");
        assert_eq!(lost, 1);
        assert!(kept.is_char_boundary(kept.len()));
    }

    #[test]
    fn a_limit_one_byte_short_of_a_four_byte_character_drops_it_entirely() {
        let (kept, lost) = truncate_utf8("🎯", 3);
        assert_eq!(kept, "");
        assert_eq!(lost, 1);
    }

    #[test]
    fn a_zero_byte_bound_truncates_everything() {
        let (kept, lost) = truncate_utf8("hello", 0);
        assert_eq!(kept, "");
        assert_eq!(lost, 5);
    }

    #[test]
    fn an_empty_string_is_never_truncated() {
        assert_eq!(truncate_utf8("", 0), (String::new(), 0));
    }
}
