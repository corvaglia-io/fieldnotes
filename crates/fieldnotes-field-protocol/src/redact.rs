//! Core's second redaction pass.
//!
//! The redaction obligation is two-layered:
//!
//! 1. **the Field classifies and sanitizes before emission**, replacing each
//!    removed value with the exact marker [`REDACTION_MARKER`] and naming it in
//!    the diagnostic's `redacted` list, so a reviewer can see that redaction
//!    happened rather than guessing;
//! 2. **core applies its own second pass** over every diagnostic member and over
//!    captured standard error before display or persistence.
//!
//! Core never persists raw standard error: it captures it into a bounded ring
//! buffer, redacts it, and notes truncation.
//!
//! Redaction is defense in depth, not permission to log a secret first. And it
//! is an obligation on **Fieldnotes' own output only**. Per ruling 3, Fieldnotes
//! performs no secret scanning of notebook content and never rejects collected
//! evidence for containing secret-looking text. A credential a colleague pasted
//! into an email is evidence; a credential Fieldnotes holds is a secret. This
//! module governs the second and says nothing about the first.

use std::collections::BTreeSet;

use crate::message::{DiagnosticEvent, Validate};
use crate::value::DetailValue;

/// The only permitted replacement text for material that was removed.
pub const REDACTION_MARKER: &str = "[redacted]";

/// Key names whose value is redacted wherever core sees one.
///
/// Covers authorization and cookie headers; token, password, secret, code, and
/// signature fields; and protected-channel material.
const SENSITIVE_KEY_FRAGMENTS: [&str; 14] = [
    "authorization",
    "cookie",
    "token",
    "password",
    "passwd",
    "secret",
    "signature",
    "credential",
    "assertion",
    "client_secret",
    "refresh",
    "session",
    "continuation_url",
    "www_authenticate",
];

/// Query or form parameter names whose value is redacted inside a URL or an
/// error string.
const SENSITIVE_PARAMETERS: [&str; 10] = [
    "access_token",
    "id_token",
    "refresh_token",
    "code",
    "client_secret",
    "password",
    "sig",
    "signature",
    "token",
    "secret",
];

/// Core's redactor.
///
/// Holds the exact secrets core itself granted on the protected channel, so the
/// value it delivered can never be echoed back out even in a shape no pattern
/// would match. Registering a secret is defense in depth on top of the
/// pattern-based pass, not a replacement for it.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    known_secrets: BTreeSet<String>,
}

impl Redactor {
    /// A redactor with the pattern rules and no registered secrets.
    #[must_use]
    pub fn new() -> Self {
        Redactor::default()
    }

    /// Registers an exact secret core granted on the protected channel.
    ///
    /// Short values are ignored: redacting every occurrence of a two-character
    /// string would destroy the message without protecting anything.
    pub fn register_secret(&mut self, secret: &str) {
        if secret.len() >= 8 {
            self.known_secrets.insert(secret.to_owned());
        }
    }

    /// Redacts one string.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        let mut result = text.to_owned();
        for secret in &self.known_secrets {
            if result.contains(secret.as_str()) {
                result = result.replace(secret.as_str(), REDACTION_MARKER);
            }
        }
        result = redact_url_userinfo(&result);
        result = redact_parameters(&result);
        redact_key_values(&result)
    }

    /// Whether a string still contains a registered secret after redaction.
    ///
    /// A secret-canary test asserts this is false for argv, the inherited
    /// environment, standard output, standard error, logs, diagnostics, cursors,
    /// Notes, and artifacts.
    #[must_use]
    pub fn leaks(&self, text: &str) -> bool {
        self.known_secrets
            .iter()
            .any(|secret| text.contains(secret.as_str()))
    }

    /// Redacts every member of a diagnostic before display or persistence.
    ///
    /// The result is still a valid diagnostic: redaction replaces values, never
    /// structure.
    #[must_use]
    pub fn redact_diagnostic(&self, diagnostic: &DiagnosticEvent) -> DiagnosticEvent {
        let mut redacted = diagnostic.clone();
        if let Ok(message) =
            crate::grammar::MessageText::parse(&self.redact(diagnostic.message.as_str()))
        {
            redacted.message = message;
        }
        if let Some(detail) = &diagnostic.detail {
            let mut rebuilt = crate::value::DiagnosticDetail::new();
            for (name, value) in detail.iter() {
                let replacement = match value {
                    DetailValue::Text(text) => {
                        if is_sensitive_key(name) {
                            DetailValue::Text(REDACTION_MARKER.to_owned())
                        } else {
                            DetailValue::Text(self.redact(text))
                        }
                    }
                    other => other.clone(),
                };
                if let Ok(key) = crate::grammar::PropertyNameToken::parse(name) {
                    rebuilt.insert(key, replacement);
                }
            }
            redacted.detail = Some(rebuilt);
        }
        if let Some(source) = &mut redacted.source
            && let Some(url) = &source.url
        {
            source.url = Some(self.redact(url));
        }
        // The result must still satisfy the contract; if redaction somehow
        // produced something invalid, the original is safer than a broken frame
        // only because the original is never displayed without this pass, so
        // fall back to a fully redacted message instead.
        if redacted.validate().is_err() {
            redacted = diagnostic.clone();
            if let Ok(message) = crate::grammar::MessageText::parse(REDACTION_MARKER) {
                redacted.message = message;
            }
            redacted.detail = None;
        }
        redacted
    }

    /// Redacts captured standard error, line by line.
    #[must_use]
    pub fn redact_log(&self, captured: &str) -> String {
        captured
            .lines()
            .map(|line| self.redact(line))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn is_sensitive_key(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    SENSITIVE_KEY_FRAGMENTS
        .iter()
        .any(|fragment| lowered.contains(fragment))
}

/// Replaces `scheme://user:password@host` with `scheme://[redacted]@host`.
fn redact_url_userinfo(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme_end) = rest.find("://") {
        let after = &rest[scheme_end + 3..];
        // Userinfo ends at the first '@' before any '/', '?', or whitespace.
        let boundary = after
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
            .unwrap_or(after.len());
        match after[..boundary].find('@') {
            Some(at) if after[..at].contains(':') => {
                result.push_str(&rest[..scheme_end + 3]);
                result.push_str(REDACTION_MARKER);
                result.push('@');
                rest = &after[at + 1..];
            }
            _ => {
                result.push_str(&rest[..scheme_end + 3]);
                rest = after;
            }
        }
    }
    result.push_str(rest);
    result
}

/// Replaces `name=value` for a sensitive parameter name, in a URL or an error
/// string.
fn redact_parameters(text: &str) -> String {
    let mut result = text.to_owned();
    for parameter in SENSITIVE_PARAMETERS {
        let mut rebuilt = String::with_capacity(result.len());
        let mut rest = result.as_str();
        loop {
            let Some(position) = find_parameter(rest, parameter) else {
                rebuilt.push_str(rest);
                break;
            };
            let after = position + parameter.len() + 1;
            rebuilt.push_str(&rest[..after]);
            rebuilt.push_str(REDACTION_MARKER);
            let tail = &rest[after..];
            let end = tail.find(['&', ';', ' ', '"', '\'']).unwrap_or(tail.len());
            rest = &tail[end..];
        }
        result = rebuilt;
    }
    result
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn find_parameter(text: &str, parameter: &str) -> Option<usize> {
    // `to_ascii_lowercase` never changes a string's byte length, so an offset
    // in the lowered copy is an offset in the original.
    let lowered = text.to_ascii_lowercase();
    let needle = format!("{parameter}=");
    let bytes = lowered.as_bytes();
    let mut from = 0;
    while let Some(offset) = lowered[from..].find(&needle) {
        let position = from + offset;
        let starts_a_name = position == 0 || !is_word_byte(bytes[position - 1]);
        if starts_a_name {
            return Some(position);
        }
        from = position + needle.len();
    }
    None
}

fn utf8_width(byte: u8) -> usize {
    match byte {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        // A continuation or invalid lead byte: advance one byte so the scan
        // always terminates. `text` is valid UTF-8, so this is unreachable at a
        // character boundary.
        _ => 1,
    }
}

/// Replaces `Key: value`, `key=value`, and `"key": "value"` for a sensitive key
/// name.
///
/// Scans once, left to right, so the output is deterministic and the slices
/// always land on character boundaries.
fn redact_key_values(text: &str) -> String {
    let lowered = text.to_ascii_lowercase();
    let bytes = lowered.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        let starts_a_name = index == 0 || !is_word_byte(bytes[index - 1]);
        let matched = if starts_a_name {
            SENSITIVE_KEY_FRAGMENTS
                .iter()
                .find(|fragment| lowered[index..].starts_with(**fragment))
                .copied()
        } else {
            None
        };
        if let Some(fragment) = matched
            && let Some((value_start, value_end)) =
                sensitive_value_span(bytes, index + fragment.len())
        {
            result.push_str(&text[index..value_start]);
            result.push_str(REDACTION_MARKER);
            index = value_end;
            continue;
        }
        let width = utf8_width(bytes[index]);
        let end = (index + width).min(text.len());
        result.push_str(&text[index..end]);
        index = end;
    }
    result
}

/// Given the end of a matched key fragment, finds the value span to replace.
fn sensitive_value_span(bytes: &[u8], after_fragment: usize) -> Option<(usize, usize)> {
    let mut cursor = after_fragment;
    // The rest of the key name, such as the `_realm` in `www_authenticate_realm`.
    while cursor < bytes.len() && (is_word_byte(bytes[cursor]) || bytes[cursor] == b'-') {
        cursor += 1;
    }
    // An optional closing quote, then the separator.
    if cursor < bytes.len() && (bytes[cursor] == b'"' || bytes[cursor] == b'\'') {
        cursor += 1;
    }
    while cursor < bytes.len() && bytes[cursor] == b' ' {
        cursor += 1;
    }
    if cursor >= bytes.len() || !(bytes[cursor] == b':' || bytes[cursor] == b'=') {
        return None;
    }
    cursor += 1;
    while cursor < bytes.len() && bytes[cursor] == b' ' {
        cursor += 1;
    }
    let quoted = cursor < bytes.len() && bytes[cursor] == b'"';
    if quoted {
        cursor += 1;
    }
    let value_start = cursor;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let terminates = if quoted {
            byte == b'"'
        } else {
            matches!(byte, b',' | b';' | b'\n' | b'"' | b'&')
        };
        if terminates {
            break;
        }
        cursor += 1;
    }
    if cursor == value_start {
        return None;
    }
    Some((value_start, cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registered_secret_never_survives_redaction() {
        let mut redactor = Redactor::new();
        redactor.register_secret("FIXTURE-NOT-A-REAL-TOKEN-canary-9f14c0a3");
        let line = "token refresh failed with FIXTURE-NOT-A-REAL-TOKEN-canary-9f14c0a3 in flight";
        let redacted = redactor.redact(line);
        assert!(!redacted.contains("canary"), "leaked: {redacted}");
        assert!(redacted.contains(REDACTION_MARKER));
        assert!(redactor.leaks(line));
        assert!(!redactor.leaks(&redacted));
    }

    #[test]
    fn authorization_headers_are_redacted() {
        let redactor = Redactor::new();
        let redacted = redactor.redact("Authorization: Bearer abc.def.ghi");
        assert!(!redacted.contains("abc.def.ghi"), "leaked: {redacted}");
    }

    #[test]
    fn credentials_embedded_in_a_url_are_redacted() {
        let redactor = Redactor::new();
        let redacted = redactor.redact("fetch failed for https://joe:hunter2@example.net/mail");
        assert!(!redacted.contains("hunter2"), "leaked: {redacted}");
        assert!(redacted.contains("example.net/mail"));
    }

    #[test]
    fn pagination_error_parameters_are_redacted() {
        let redactor = Redactor::new();
        let redacted =
            redactor.redact("next page https://graph.example/next?access_token=eyJhbGciOi&top=50");
        assert!(!redacted.contains("eyJhbGciOi"), "leaked: {redacted}");
        assert!(redacted.contains("top=50"));
    }

    #[test]
    fn a_short_value_is_not_registered_as_a_secret() {
        let mut redactor = Redactor::new();
        redactor.register_secret("ab");
        assert_eq!(
            redactor.redact("a table of abbreviations"),
            "a table of abbreviations"
        );
    }

    #[test]
    fn ordinary_text_survives_unchanged() {
        let redactor = Redactor::new();
        let line = "Skipped 1 file above the configured size bound.";
        assert_eq!(redactor.redact(line), line);
    }

    #[test]
    fn captured_logs_are_redacted_line_by_line() {
        let mut redactor = Redactor::new();
        redactor.register_secret("FIXTURE-NOT-A-REAL-TOKEN-canary-1d77b430");
        let captured = "line one\ntoken=FIXTURE-NOT-A-REAL-TOKEN-canary-1d77b430\nline three";
        let redacted = redactor.redact_log(captured);
        assert!(!redacted.contains("canary"), "leaked: {redacted}");
        assert!(redacted.contains("line one"));
        assert!(redacted.contains("line three"));
    }
}
