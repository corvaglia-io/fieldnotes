//! A shared RFC 3986 component-encoding rule.
//!
//! [`percent_encoding::NON_ALPHANUMERIC`] escapes every non-alphanumeric
//! byte, including RFC 3986's own unreserved marks (`-`, `.`, `_`, `~`).
//! That is not wrong — a receiving server percent-decodes back to the exact
//! original bytes either way — but it needlessly mangles values this crate
//! builds that are *already* drawn from that unreserved alphabet (a PKCE
//! code challenge, this crate's `state` nonce, both base64url), turning a
//! literal `-` into `%2D` for no reason. [`QUERY_VALUE`] escapes what RFC
//! 3986 actually requires and leaves the four unreserved marks alone, which
//! both this crate's own authorize-URL query string
//! ([`crate::oauth::authorize`]) and its token-endpoint form body
//! ([`crate::oauth::token`]) use for every value they encode.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};

/// Percent-encodes every byte outside `[A-Za-z0-9] / "-" / "." / "_" / "~"`.
pub(crate) const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[cfg(test)]
mod tests {
    use super::*;
    use percent_encoding::utf8_percent_encode;

    #[test]
    fn leaves_unreserved_marks_literal() {
        let encoded =
            utf8_percent_encode("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM", QUERY_VALUE)
                .to_string();
        assert_eq!(encoded, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn still_escapes_reserved_and_unsafe_characters() {
        let encoded = utf8_percent_encode("a b/c?d", QUERY_VALUE).to_string();
        assert_eq!(encoded, "a%20b%2Fc%3Fd");
    }
}
