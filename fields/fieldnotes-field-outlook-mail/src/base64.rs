//! Standard base64 decoding for Graph's `contentBytes`.
//!
//! Graph delivers a file attachment's original bytes as a base64 string inside
//! the JSON attachment resource. The alternative endpoint that returns raw
//! bytes (`.../attachments/{id}/$value`) is not reachable through
//! `fieldnotes-msgraph`, whose response handling is JSON-only, so a decoder is
//! needed here.
//!
//! This is deliberately strict rather than forgiving. It accepts exactly the
//! RFC 4648 standard alphabet with correct padding, ignoring only ASCII
//! whitespace (which some services insert to wrap long lines), and rejects
//! everything else. Attachment bytes become an artifact whose identity *is*
//! their digest, so a decoder that silently skipped an unexpected character
//! would produce different bytes from the ones the source holds, under an
//! identity claiming to describe them. Refusing is the only safe answer.
//!
//! This belongs in a shared Microsoft-Field layer: Calendar attachments and
//! Teams hosted content arrive the same way. See the crate's final report.

/// Why a base64 payload could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Base64Error {
    /// A byte outside the standard alphabet, padding, and ASCII whitespace.
    InvalidCharacter,
    /// The significant length is not a whole number of 4-character groups, or
    /// padding appeared somewhere other than the end.
    InvalidLength,
    /// A padded group carried bits that a correct encoder would have left
    /// zero, which means the payload was not produced by a conforming
    /// encoder.
    NonCanonicalPadding,
}

impl std::fmt::Display for Base64Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Base64Error::InvalidCharacter => "it contains a character outside base64",
            Base64Error::InvalidLength => "its length is not a whole number of base64 groups",
            Base64Error::NonCanonicalPadding => "its final group carries non-zero padding bits",
        };
        write!(f, "the attachment content could not be decoded: {reason}")
    }
}

impl std::error::Error for Base64Error {}

fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes standard base64 with padding, ignoring ASCII whitespace.
pub(crate) fn decode(text: &str) -> Result<Vec<u8>, Base64Error> {
    let mut significant: Vec<u8> = Vec::with_capacity(text.len());
    let mut padding = 0usize;
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            // A payload character after padding has already started is not a
            // conforming encoding.
            return Err(Base64Error::InvalidLength);
        }
        significant.push(sextet(byte).ok_or(Base64Error::InvalidCharacter)?);
    }
    if padding > 2 || !(significant.len() + padding).is_multiple_of(4) {
        return Err(Base64Error::InvalidLength);
    }
    let expected_padding = match significant.len() % 4 {
        0 => 0,
        2 => 2,
        3 => 1,
        _ => return Err(Base64Error::InvalidLength),
    };
    if padding != expected_padding {
        return Err(Base64Error::InvalidLength);
    }

    let mut bytes = Vec::with_capacity(significant.len() * 3 / 4);
    let (groups, remainder) = significant.as_chunks::<4>();
    for chunk in groups {
        let packed = (u32::from(chunk[0]) << 18)
            | (u32::from(chunk[1]) << 12)
            | (u32::from(chunk[2]) << 6)
            | u32::from(chunk[3]);
        bytes.push((packed >> 16) as u8);
        bytes.push((packed >> 8) as u8);
        bytes.push(packed as u8);
    }
    match remainder {
        [] => {}
        [first, second] => {
            if second & 0b0000_1111 != 0 {
                return Err(Base64Error::NonCanonicalPadding);
            }
            bytes.push((u32::from(*first) << 2 | u32::from(*second) >> 4) as u8);
        }
        [first, second, third] => {
            if third & 0b0000_0011 != 0 {
                return Err(Base64Error::NonCanonicalPadding);
            }
            bytes.push((u32::from(*first) << 2 | u32::from(*second) >> 4) as u8);
            bytes.push((u32::from(*second) << 4 | u32::from(*third) >> 2) as u8);
        }
        // A four-element chunking cannot leave four or more bytes over.
        _ => return Err(Base64Error::InvalidLength),
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{Base64Error, decode};

    #[test]
    fn the_rfc_4648_test_vectors_round_trip() {
        let cases: [(&str, &[u8]); 7] = [
            ("", b""),
            ("Zg==", b"f"),
            ("Zm8=", b"fo"),
            ("Zm9v", b"foo"),
            ("Zm9vYg==", b"foob"),
            ("Zm9vYmE=", b"fooba"),
            ("Zm9vYmFy", b"foobar"),
        ];
        for (encoded, expected) in cases {
            assert_eq!(decode(encoded).as_deref(), Ok(expected), "{encoded}");
        }
    }

    #[test]
    fn wrapped_lines_decode_because_only_whitespace_is_ignored() {
        assert_eq!(decode("Zm9v\r\nYmFy").as_deref(), Ok(&b"foobar"[..]));
    }

    #[test]
    fn a_character_outside_the_alphabet_is_refused_rather_than_skipped() {
        assert_eq!(decode("Zm9v!mFy"), Err(Base64Error::InvalidCharacter));
        assert_eq!(decode("Zm9v-mFy"), Err(Base64Error::InvalidCharacter));
    }

    #[test]
    fn a_truncated_payload_is_refused() {
        assert_eq!(decode("Zm9vY"), Err(Base64Error::InvalidLength));
        assert_eq!(decode("Zg="), Err(Base64Error::InvalidLength));
        assert_eq!(decode("Zg==="), Err(Base64Error::InvalidLength));
    }

    #[test]
    fn payload_after_padding_is_refused() {
        assert_eq!(decode("Zg==Zg=="), Err(Base64Error::InvalidLength));
    }

    #[test]
    fn non_zero_padding_bits_are_refused() {
        // "Zh==" would decode the same first byte as "Zg==" while carrying
        // bits a conforming encoder would have zeroed.
        assert_eq!(decode("Zh=="), Err(Base64Error::NonCanonicalPadding));
    }

    #[test]
    fn all_byte_values_survive_a_decode_of_their_own_encoding() {
        // Encoded with an independent tool; decoding it must return exactly
        // 0x00..=0x0f, which is the property that matters for artifact bytes.
        assert_eq!(
            decode("AAECAwQFBgcICQoLDA0ODw==").as_deref(),
            Ok(&[
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f
            ][..])
        );
    }
}
