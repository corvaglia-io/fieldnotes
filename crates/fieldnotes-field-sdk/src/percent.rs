//! Byte-safe percent-encoding for embedding a path- or identifier-shaped
//! string inside a cursor.
//!
//! A cursor is itself a bounded, delimiter-bearing text token (see
//! [`fieldnotes_field_protocol::grammar::Cursor`]). Any Field whose cursor
//! format lists relative paths, upstream identifiers, or anything else that
//! might contain the cursor's own delimiters -- or arbitrary bytes, such as a
//! multi-byte UTF-8 character -- needs to escape those values before joining
//! them into the cursor text, and unescape them again when decoding a
//! previously offered cursor.
//!
//! Escaping and unescaping both work one byte at a time, and [`decode`] only
//! assembles the result into a `String` once every constituent byte --
//! literal or unescaped -- has been collected in full, so a multi-byte UTF-8
//! sequence is never split across an encode/decode round trip.

/// Percent-encodes every byte of `text` outside a small path-safe set.
///
/// The safe set is ASCII letters, digits, `_`, `.`, `/`, and `-`: what a
/// relative path typically needs left unescaped. Every other byte -- every
/// byte of a multi-byte UTF-8 sequence included, and any delimiter a cursor
/// format reserves for itself, such as a comma or a semicolon -- is rendered
/// as `%XX` lowercase hexadecimal, so [`decode`] always recovers the exact
/// original text.
#[must_use]
pub fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        let safe = byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'-');
        if safe {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}

/// Decodes text [`encode`] produced, or anything following the same `%XX`
/// escaping.
///
/// Returns `None` when `text` contains a `%` not followed by two hexadecimal
/// digits, or when the decoded bytes are not valid UTF-8 -- both signal
/// corruption rather than something to silently repair, exactly like any
/// other part of a cursor a Field cannot otherwise trust.
#[must_use]
pub fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn plain_ascii_is_left_unescaped() {
        assert_eq!(encode("plain.txt"), "plain.txt");
        assert_eq!(decode("plain.txt"), Some("plain.txt".to_owned()));
    }

    #[test]
    fn a_relative_path_round_trips() {
        let text = "projects/rollout/readme.md";
        assert_eq!(decode(&encode(text)), Some(text.to_owned()));
    }

    #[test]
    fn characters_needing_escape_round_trip() {
        let text = "dir/with space, comma.txt";
        let encoded = encode(text);
        assert!(!encoded.contains(' '), "a space must be escaped: {encoded}");
        assert!(!encoded.contains(','), "a comma must be escaped: {encoded}");
        assert_eq!(decode(&encoded), Some(text.to_owned()));
    }

    #[test]
    fn multibyte_utf8_round_trips_without_being_split() {
        let text = "café/日本語/emoji-🎯.md";
        let encoded = encode(text);
        assert!(
            encoded.is_ascii(),
            "an encoded cursor fragment must be ASCII: {encoded}"
        );
        assert_eq!(decode(&encoded), Some(text.to_owned()));
    }

    #[test]
    fn every_encoded_byte_is_ascii_regardless_of_input() {
        let text = "\u{0}\u{1f600}\u{7f}";
        assert!(encode(text).is_ascii());
    }

    #[test]
    fn decode_refuses_a_percent_with_no_digits_following() {
        assert_eq!(decode("100%"), None);
    }

    #[test]
    fn decode_refuses_a_percent_with_only_one_digit_following() {
        assert_eq!(decode("100%2"), None);
    }

    #[test]
    fn decode_refuses_non_hexadecimal_digits() {
        assert_eq!(decode("100%zz"), None);
    }

    #[test]
    fn decode_accepts_upper_and_lower_case_hex_digits() {
        assert_eq!(decode("%2c"), Some(",".to_owned()));
        assert_eq!(decode("%2C"), Some(",".to_owned()));
    }

    #[test]
    fn an_empty_string_round_trips() {
        assert_eq!(encode(""), "");
        assert_eq!(decode(""), Some(String::new()));
    }
}
