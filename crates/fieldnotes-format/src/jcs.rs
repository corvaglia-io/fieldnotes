//! RFC 8785 (JSON Canonicalization Scheme) number and string serialization.
//!
//! Numbers follow section 3.2.2.3 (ECMAScript `Number::toString`): negative
//! zero becomes `0`, and only finite binary64 values are serializable.
//! Strings follow section 3.2.2.2: two-character escapes for the mandated
//! controls, `\u00xx` for the remaining C0 controls, and every other Unicode
//! code point literal (no optional solidus escaping).

/// Serializes a finite binary64 number per RFC 8785 section 3.2.2.3.
///
/// Returns `None` for NaN and infinities, which are invalid in Fieldnotes
/// frontmatter.
#[must_use]
pub fn format_number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        // Positive and negative zero both serialize as `0`.
        return Some("0".to_owned());
    }
    let negative = value < 0.0;
    let magnitude = value.abs();

    // Rust's LowerExp formatting produces the shortest round-trip decimal
    // mantissa, matching the digits ECMAScript's ToString selects.
    let exp_form = format!("{magnitude:e}");
    let (mantissa, exponent) = exp_form.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let k = i32::try_from(digits.len()).ok()?;
    // value = 0.digits * 10^n
    let n = exponent + 1;

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        let split = usize::try_from(n).ok()?;
        out.push_str(&digits[..split]);
        out.push('.');
        out.push_str(&digits[split..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let e = n - 1;
        if e >= 0 {
            out.push('+');
        }
        out.push_str(&e.to_string());
    }
    Some(out)
}

/// Serializes text per RFC 8785 section 3.2.2.2, including surrounding quotes.
#[must_use]
pub fn serialize_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{0009}' => out.push_str("\\t"),
            '\u{000a}' => out.push_str("\\n"),
            '\u{000c}' => out.push_str("\\f"),
            '\u{000d}' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Decodes a double-quoted RFC 8785/JSON string starting at the beginning of
/// `text`. Returns the decoded value and the number of bytes consumed.
pub(crate) fn decode_string(text: &str) -> Option<(String, usize)> {
    let mut chars = text.char_indices();
    match chars.next() {
        Some((_, '"')) => {}
        _ => return None,
    }
    let mut out = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((out, index + 1)),
            '\\' => {
                let (_, escape) = chars.next()?;
                match escape {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{0008}'),
                    'f' => out.push('\u{000c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let high = decode_hex4(&mut chars)?;
                        if (0xd800..=0xdbff).contains(&high) {
                            // Surrogate pair.
                            match (chars.next(), chars.next()) {
                                (Some((_, '\\')), Some((_, 'u'))) => {}
                                _ => return None,
                            }
                            let low = decode_hex4(&mut chars)?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return None;
                            }
                            let code = 0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00);
                            out.push(char::from_u32(code)?);
                        } else {
                            out.push(char::from_u32(high)?);
                        }
                    }
                    _ => return None,
                }
            }
            c if (c as u32) < 0x20 => return None,
            c => out.push(c),
        }
    }
    None
}

fn decode_hex4(chars: &mut core::str::CharIndices<'_>) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..4 {
        let (_, c) = chars.next()?;
        value = value * 16 + c.to_digit(16)?;
    }
    Some(value)
}

/// Parses a plain scalar as a JSON-grammar number literal.
///
/// Returns `Ok(None)` when the token is not shaped like a JSON number at all,
/// so callers can treat it as a different scalar kind.
pub(crate) fn parse_number(token: &str) -> Result<Option<f64>, NumberError> {
    if !is_json_number(token) {
        return Ok(None);
    }
    if !token.contains('.') && !token.contains('e') && !token.contains('E') {
        // Integer literal: reject values outside the exact binary64 range
        // instead of rounding.
        let integer: i128 = token.parse().map_err(|_| NumberError::OutOfRange)?;
        if integer.abs() > fieldnotes_domain::value::MAX_EXACT_INTEGER {
            return Err(NumberError::OutOfRange);
        }
        #[allow(clippy::cast_precision_loss)]
        return Ok(Some(integer as f64));
    }
    let value: f64 = token.parse().map_err(|_| NumberError::Malformed)?;
    if !value.is_finite() {
        return Err(NumberError::NonFinite);
    }
    // Normalize negative zero at parse time.
    if value == 0.0 {
        Ok(Some(0.0))
    } else {
        Ok(Some(value))
    }
}

/// Number-literal rejection reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberError {
    /// Shaped like a number but not parseable.
    Malformed,
    /// An integer outside the exactly representable binary64 range.
    OutOfRange,
    /// Overflowed to infinity.
    NonFinite,
}

fn is_json_number(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut index = 0;
    if bytes.first() == Some(&b'-') {
        index += 1;
    }
    // Integer part: `0` or nonzero digit followed by digits.
    let start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let int_len = index - start;
    if int_len == 0 || (int_len > 1 && bytes[start] == b'0') {
        return false;
    }
    // Fraction.
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    // Exponent.
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        index += 1;
        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    index == bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers_like_ecmascript() {
        assert_eq!(format_number(0.0).as_deref(), Some("0"));
        assert_eq!(format_number(-0.0).as_deref(), Some("0"));
        assert_eq!(format_number(84.0).as_deref(), Some("84"));
        assert_eq!(format_number(-84.0).as_deref(), Some("-84"));
        assert_eq!(format_number(0.94).as_deref(), Some("0.94"));
        assert_eq!(format_number(2700.0).as_deref(), Some("2700"));
        assert_eq!(format_number(0.000001).as_deref(), Some("0.000001"));
        assert_eq!(format_number(1e-7).as_deref(), Some("1e-7"));
        assert_eq!(format_number(1e21).as_deref(), Some("1e+21"));
        assert_eq!(
            format_number(1e20).as_deref(),
            Some("100000000000000000000")
        );
        assert_eq!(
            format_number(9_007_199_254_740_992.0).as_deref(),
            Some("9007199254740992")
        );
        assert_eq!(format_number(123.456).as_deref(), Some("123.456"));
        assert_eq!(format_number(1.5e22).as_deref(), Some("1.5e+22"));
        assert_eq!(format_number(f64::NAN), None);
        assert_eq!(format_number(f64::INFINITY), None);
    }

    #[test]
    fn integer_parsing_enforces_exact_binary64_range() {
        assert_eq!(
            parse_number("9007199254740992"),
            Ok(Some(9_007_199_254_740_992.0))
        );
        assert_eq!(
            parse_number("-9007199254740992"),
            Ok(Some(-9_007_199_254_740_992.0))
        );
        assert_eq!(
            parse_number("9007199254740993"),
            Err(NumberError::OutOfRange)
        );
        assert_eq!(parse_number("-0"), Ok(Some(0.0)));
        assert_eq!(parse_number("0.94"), Ok(Some(0.94)));
        assert_eq!(parse_number("not-a-number"), Ok(None));
        assert_eq!(parse_number("+5"), Ok(None));
        assert_eq!(parse_number("05"), Ok(None));
        assert_eq!(parse_number("1e999"), Err(NumberError::NonFinite));
    }

    #[test]
    fn strings_escape_per_rfc_8785() {
        assert_eq!(serialize_string("plain"), "\"plain\"");
        assert_eq!(serialize_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(serialize_string("tab\there"), "\"tab\\there\"");
        assert_eq!(serialize_string("\u{0001}"), "\"\\u0001\"");
        assert_eq!(serialize_string("Grüße/€"), "\"Grüße/€\"");
    }

    #[test]
    fn decodes_json_strings_with_surrogate_pairs() {
        assert_eq!(decode_string("\"abc\""), Some(("abc".to_owned(), 5)));
        assert_eq!(
            decode_string("\"W/\\\"v3\\\"\""),
            Some(("W/\"v3\"".to_owned(), 10))
        );
        assert_eq!(
            decode_string("\"\\ud83d\\ude00\""),
            Some(("\u{1f600}".to_owned(), 14))
        );
        assert_eq!(decode_string("\"unterminated"), None);
    }
}
