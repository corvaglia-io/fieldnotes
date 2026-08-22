//! Strict flat-YAML-subset scanner for frontmatter and instance metadata.
//!
//! This is a hand-written parser for the byte grammar A1 defines: top-level
//! `key: value` scalar lines and block-style lists of scalars, nothing else.
//! `null`, nested mappings, inline objects, flow sequences, duplicate keys,
//! anchors, aliases, tags, block scalars, comments, document markers, and
//! multiple documents are rejected with typed errors.

use std::collections::BTreeSet;

use fieldnotes_domain::property::is_valid_property_name;

use crate::error::ValidationError;
use crate::jcs;

/// How a scalar was written in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarStyle {
    /// A YAML plain (unquoted) scalar.
    Plain,
    /// A double-quoted scalar decoded with the JSON string rules.
    DoubleQuoted,
}

/// A scanned scalar token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawScalar {
    /// The decoded text (for double-quoted) or the literal token (for plain).
    pub text: String,
    /// The input style.
    pub style: ScalarStyle,
    /// One-based line number in the scanned block.
    pub line: usize,
}

/// A scanned raw value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawValue {
    /// A single scalar.
    Scalar(RawScalar),
    /// A block-style list of scalars.
    List(Vec<RawScalar>),
}

/// One scanned `key: value` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawEntry {
    /// The property key.
    pub key: String,
    /// The raw value.
    pub value: RawValue,
    /// One-based line number of the key line.
    pub line: usize,
}

enum ScalarContext {
    TopLevel,
    ListItem,
}

fn is_core_null(token: &str) -> bool {
    matches!(token, "~" | "null" | "Null" | "NULL")
}

/// Scans a flat block of `key: value` lines. `lines` must not contain the
/// `---` delimiters; line numbers are one-based within the block.
pub(crate) fn parse_flat_block(lines: &[&str]) -> Result<Vec<RawEntry>, ValidationError> {
    let mut entries = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];
        let line_no = index + 1;
        if line.is_empty() {
            return Err(ValidationError::BlankLine { line: line_no });
        }
        if line == "..." || line == "---" || line.starts_with("--- ") || line.starts_with("... ") {
            return Err(ValidationError::DocumentMarker { line: line_no });
        }
        if line != line.trim_end() {
            return Err(ValidationError::MalformedLine { line: line_no });
        }
        if line.starts_with('#') {
            return Err(ValidationError::Comment { line: line_no });
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(ValidationError::BadIndentation { line: line_no });
        }
        let Some(colon) = line.find(':') else {
            return Err(ValidationError::MalformedLine { line: line_no });
        };
        let key = &line[..colon];
        if !is_valid_property_name(key) {
            return Err(ValidationError::InvalidPropertyName {
                key: key.to_owned(),
            });
        }
        if !seen.insert(key.to_owned()) {
            return Err(ValidationError::DuplicateKey {
                key: key.to_owned(),
            });
        }
        let rest = &line[colon + 1..];

        if rest.is_empty() {
            // Block value: a list of `  - ` items, or an invalid nested/null shape.
            index += 1;
            let mut items = Vec::new();
            while index < lines.len()
                && (lines[index].starts_with(' ') || lines[index].starts_with('\t'))
            {
                let item_line = lines[index];
                let item_no = index + 1;
                if item_line != item_line.trim_end() {
                    return Err(ValidationError::MalformedLine { line: item_no });
                }
                if let Some(content) = item_line.strip_prefix("  - ") {
                    if content.is_empty() || content.starts_with(' ') {
                        return Err(ValidationError::BadIndentation { line: item_no });
                    }
                    if content.starts_with("- ") || content == "-" {
                        return Err(ValidationError::NestedMapping { line: item_no });
                    }
                    let scalar =
                        parse_scalar_token(content, item_no, &ScalarContext::ListItem, key)?;
                    items.push(scalar);
                    index += 1;
                } else if item_line == "  -" {
                    return Err(ValidationError::NullValue {
                        key: key.to_owned(),
                    });
                } else {
                    // An indented line that is not a list item: a nested
                    // mapping if it is key-shaped, bad indentation otherwise.
                    let content = item_line.trim_start();
                    if content.ends_with(':') || content.contains(": ") {
                        return Err(ValidationError::NestedMapping { line: item_no });
                    }
                    return Err(ValidationError::BadIndentation { line: item_no });
                }
            }
            if items.is_empty() {
                return Err(ValidationError::NullValue {
                    key: key.to_owned(),
                });
            }
            entries.push(RawEntry {
                key: key.to_owned(),
                value: RawValue::List(items),
                line: line_no,
            });
        } else if let Some(value_text) = rest.strip_prefix(' ') {
            if value_text.is_empty() || value_text.starts_with(' ') {
                return Err(ValidationError::MalformedLine { line: line_no });
            }
            let scalar = parse_scalar_token(value_text, line_no, &ScalarContext::TopLevel, key)?;
            entries.push(RawEntry {
                key: key.to_owned(),
                value: RawValue::Scalar(scalar),
                line: line_no,
            });
            index += 1;
        } else {
            return Err(ValidationError::MalformedLine { line: line_no });
        }
    }
    Ok(entries)
}

fn parse_scalar_token(
    token: &str,
    line: usize,
    context: &ScalarContext,
    key: &str,
) -> Result<RawScalar, ValidationError> {
    let first = token.as_bytes()[0];
    match first {
        b'&' => Err(ValidationError::Anchor { line }),
        b'*' => Err(ValidationError::Alias { line }),
        b'!' => Err(ValidationError::CustomTag { line }),
        b'{' => Err(ValidationError::NestedMapping { line }),
        b'[' => Err(ValidationError::FlowSequence { line }),
        b'|' | b'>' => Err(ValidationError::BlockScalar { line }),
        b'\'' => Err(ValidationError::SingleQuoted { line }),
        b'"' => {
            let (decoded, consumed) =
                jcs::decode_string(token).ok_or(ValidationError::InvalidString { line })?;
            if consumed != token.len() {
                return Err(ValidationError::MalformedLine { line });
            }
            Ok(RawScalar {
                text: decoded,
                style: ScalarStyle::DoubleQuoted,
                line,
            })
        }
        _ => {
            if token.contains(" #") {
                return Err(ValidationError::Comment { line });
            }
            if token.ends_with(':') || token.contains(": ") {
                return Err(match context {
                    ScalarContext::TopLevel => ValidationError::NestedMapping { line },
                    ScalarContext::ListItem => ValidationError::ArrayObject { line },
                });
            }
            if is_core_null(token) {
                return Err(ValidationError::NullValue {
                    key: key.to_owned(),
                });
            }
            Ok(RawScalar {
                text: token.to_owned(),
                style: ScalarStyle::Plain,
                line,
            })
        }
    }
}

/// Whether `text` resolves as a string (not null/bool/int/float) under the
/// YAML 1.2 Core Schema.
#[must_use]
pub fn core_schema_resolves_string(text: &str) -> bool {
    !(is_core_null(text) || is_core_bool(text) || is_core_int(text) || is_core_float(text))
}

fn is_core_bool(token: &str) -> bool {
    matches!(
        token,
        "true" | "True" | "TRUE" | "false" | "False" | "FALSE"
    )
}

fn is_core_int(token: &str) -> bool {
    let unsigned = token.strip_prefix(['-', '+']).unwrap_or(token);
    if let Some(octal) = unsigned.strip_prefix("0o") {
        return !octal.is_empty() && octal.bytes().all(|b| (b'0'..=b'7').contains(&b));
    }
    if let Some(hex) = unsigned.strip_prefix("0x") {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit())
}

fn is_core_float(token: &str) -> bool {
    let unsigned = token.strip_prefix(['-', '+']).unwrap_or(token);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") {
        return true;
    }
    if matches!(token, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    // [0-9]*(\.[0-9]*)?([eE][-+]?[0-9]+)? with at least one digit somewhere.
    let bytes = unsigned.as_bytes();
    let mut index = 0;
    let mut digits = 0usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
        digits += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
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

    fn scan(text: &str) -> Result<Vec<RawEntry>, ValidationError> {
        let lines: Vec<&str> = text.lines().collect();
        parse_flat_block(&lines)
    }

    #[test]
    fn scans_scalars_and_lists() -> Result<(), ValidationError> {
        let entries = scan("title: Rollout reminder\nto:\n  - joe@example.net\n  - \"x: y\"")?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "title");
        match &entries[1].value {
            RawValue::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].style, ScalarStyle::Plain);
                assert_eq!(items[1].style, ScalarStyle::DoubleQuoted);
                assert_eq!(items[1].text, "x: y");
            }
            RawValue::Scalar(_) => panic!("expected list"),
        }
        Ok(())
    }

    #[test]
    fn rejects_forbidden_yaml_features() {
        assert!(matches!(
            scan("title:\n  value: nested"),
            Err(ValidationError::NestedMapping { .. })
        ));
        assert!(matches!(
            scan("participants:\n  - name: Alice"),
            Err(ValidationError::ArrayObject { .. })
        ));
        assert!(matches!(
            scan("title: null"),
            Err(ValidationError::NullValue { .. })
        ));
        assert!(matches!(
            scan("title:"),
            Err(ValidationError::NullValue { .. })
        ));
        assert!(matches!(
            scan("title: one\ntitle: two"),
            Err(ValidationError::DuplicateKey { .. })
        ));
        assert!(matches!(
            scan("title: !tag x"),
            Err(ValidationError::CustomTag { .. })
        ));
        assert!(matches!(
            scan("title: &anchor x"),
            Err(ValidationError::Anchor { .. })
        ));
        assert!(matches!(
            scan("title: *alias"),
            Err(ValidationError::Alias { .. })
        ));
        assert!(matches!(
            scan("..."),
            Err(ValidationError::DocumentMarker { .. })
        ));
        assert!(matches!(
            scan("title: [a, b]"),
            Err(ValidationError::FlowSequence { .. })
        ));
        assert!(matches!(
            scan("title: {a: b}"),
            Err(ValidationError::NestedMapping { .. })
        ));
        assert!(matches!(
            scan("title: |"),
            Err(ValidationError::BlockScalar { .. })
        ));
        assert!(matches!(
            scan("title: 'single'"),
            Err(ValidationError::SingleQuoted { .. })
        ));
        assert!(matches!(
            scan("title: x # comment"),
            Err(ValidationError::Comment { .. })
        ));
        assert!(matches!(
            scan("Title: x"),
            Err(ValidationError::InvalidPropertyName { .. })
        ));
    }

    #[test]
    fn core_schema_resolution_detects_non_strings() {
        assert!(core_schema_resolves_string("alice@example.com"));
        assert!(core_schema_resolves_string("In Progress"));
        assert!(core_schema_resolves_string("2026-08-22"));
        assert!(!core_schema_resolves_string("1745317800000"));
        assert!(!core_schema_resolves_string("true"));
        assert!(!core_schema_resolves_string("TRUE"));
        assert!(!core_schema_resolves_string("null"));
        assert!(!core_schema_resolves_string("0x1f"));
        assert!(!core_schema_resolves_string("0o17"));
        assert!(!core_schema_resolves_string("+5"));
        assert!(!core_schema_resolves_string(".5"));
        assert!(!core_schema_resolves_string("1e3"));
        assert!(!core_schema_resolves_string(".inf"));
        assert!(!core_schema_resolves_string("-.INF"));
        assert!(!core_schema_resolves_string(".nan"));
    }
}
