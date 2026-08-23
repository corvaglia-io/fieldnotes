//! Minimal, dependency-free URL helpers: query-value percent-encoding and
//! authority comparison.
//!
//! A general-purpose URL crate was deliberately not added. The only two
//! operations this crate needs are encoding a query parameter value and
//! comparing two absolute URLs' `scheme://host[:port]` authority, and both
//! are small enough to implement directly and test exhaustively rather than
//! take on another dependency for.

/// Percent-encodes `value` for use as one query-string value.
///
/// Encodes everything outside the RFC 3986 "unreserved" set
/// (`ALPHA / DIGIT / "-" / "." / "_" / "~"`) as a UTF-8 byte sequence of
/// `%XX` escapes. This is deliberately conservative: encoding more than
/// strictly required (for example a comma inside a `$select` list) is always
/// safe, whereas under-encoding is not.
#[must_use]
pub(crate) fn percent_encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0f));
        }
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}

/// The `scheme://host[:port]` authority portion of an absolute URL, or
/// `None` if `url` has no recognizable `scheme://` prefix.
fn scheme_and_authority(url: &str) -> Option<(&str, &str)> {
    let separator = url.find("://")?;
    let scheme = &url[..separator];
    let rest = &url[separator + 3..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some((scheme, &rest[..end]))
}

/// Whether `candidate` shares its scheme and authority with `trusted`.
///
/// Used before this crate ever attaches a bearer token to a URL the *server*
/// supplied (an `@odata.nextLink`, an `@odata.deltaLink`, or a persisted,
/// caller-resumed [`DeltaToken`](crate::page::DeltaToken)), so a malicious or
/// corrupted response cannot redirect a subsequent authenticated request to
/// an attacker-controlled host. A malformed or relative `candidate` is never
/// trusted.
#[must_use]
pub(crate) fn same_authority(trusted: &str, candidate: &str) -> bool {
    match (
        scheme_and_authority(trusted),
        scheme_and_authority(candidate),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{percent_encode_query_value, same_authority};

    #[test]
    fn unreserved_characters_pass_through_unchanged() {
        assert_eq!(percent_encode_query_value("abc-DEF_123.~"), "abc-DEF_123.~");
    }

    #[test]
    fn reserved_and_non_ascii_bytes_are_percent_encoded() {
        assert_eq!(percent_encode_query_value("a b"), "a%20b");
        assert_eq!(percent_encode_query_value("\"quoted\""), "%22quoted%22");
        assert_eq!(percent_encode_query_value("caf\u{e9}"), "caf%C3%A9");
    }

    #[test]
    fn same_scheme_and_host_matches_regardless_of_path() {
        assert!(same_authority(
            "https://graph.microsoft.com/v1.0/me",
            "https://graph.microsoft.com/v1.0/me/messages?$skiptoken=abc"
        ));
    }

    #[test]
    fn a_different_host_never_matches() {
        assert!(!same_authority(
            "https://graph.microsoft.com/v1.0/me",
            "https://evil.example.net/v1.0/me/messages"
        ));
    }

    #[test]
    fn a_different_scheme_never_matches() {
        assert!(!same_authority(
            "https://graph.microsoft.com/v1.0/me",
            "http://graph.microsoft.com/v1.0/me"
        ));
    }

    #[test]
    fn a_relative_or_malformed_candidate_never_matches() {
        assert!(!same_authority(
            "https://graph.microsoft.com/v1.0/me",
            "/v1.0/me/messages"
        ));
    }
}
