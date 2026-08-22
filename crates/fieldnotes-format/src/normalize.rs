//! Markdown body normalization, version 1.
//!
//! Normalization requires valid UTF-8, removes one leading UTF-8 BOM at
//! ingestion, converts CRLF and lone CR to LF, preserves the Unicode
//! code-point sequence without NFC/NFKC (or any other) normalization,
//! preserves all other whitespace and content, and ends the body with exactly
//! one final LF.

use std::borrow::Cow;

use crate::error::ValidationError;

/// Strips one leading UTF-8 BOM if present.
pub(crate) fn strip_bom(input: &str) -> &str {
    input.strip_prefix('\u{feff}').unwrap_or(input)
}

/// Converts CRLF and lone CR to LF, borrowing when the input has no CR.
pub(crate) fn to_lf(input: &str) -> Cow<'_, str> {
    if !input.contains('\r') {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Normalizes raw body bytes per the v1 rules.
pub fn normalize_body_bytes(input: &[u8]) -> Result<String, ValidationError> {
    let text = core::str::from_utf8(input).map_err(|_| ValidationError::InvalidUtf8)?;
    Ok(normalize_body_str(text))
}

/// Normalizes an already-decoded body string per the v1 rules.
#[must_use]
pub fn normalize_body_str(input: &str) -> String {
    let mut out = to_lf(strip_bom(input)).into_owned();
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_line_endings_and_final_lf() {
        assert_eq!(normalize_body_str("a\r\nb\rc"), "a\nb\nc\n");
        assert_eq!(normalize_body_str("a\n\n\n"), "a\n");
        assert_eq!(normalize_body_str("a"), "a\n");
        assert_eq!(normalize_body_str(""), "\n");
    }

    #[test]
    fn strips_leading_bom_only() {
        assert_eq!(normalize_body_str("\u{feff}# T\n"), "# T\n");
        assert_eq!(normalize_body_str("x\u{feff}y\n"), "x\u{feff}y\n");
    }

    #[test]
    fn preserves_unicode_and_interior_whitespace() {
        let body = "  lead\ttab  \n\nGrüße Café\u{0308}\n";
        assert_eq!(normalize_body_str(body), body);
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert_eq!(
            normalize_body_bytes(&[0xff, 0xfe]),
            Err(ValidationError::InvalidUtf8)
        );
    }
}
