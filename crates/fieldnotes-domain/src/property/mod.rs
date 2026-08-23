//! Property-name grammar and the shared property registry.
//!
//! Property names match `[a-z][a-z0-9_]*` and are at most 63 ASCII bytes.
//! Source-specific properties additionally begin with their Field's registered
//! prefix; that prefix rule lives with the stem registry in [`crate::field`].
//! The approved names, scalar types, and list semantics themselves live in
//! [`registry`].

pub mod registry;

/// Whether `name` matches `[a-z][a-z0-9_]*` and is at most 63 ASCII bytes.
#[must_use]
pub fn is_valid_property_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 63 || !name.is_ascii() {
        return false;
    }
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(b'a'..=b'z') => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::is_valid_property_name;

    #[test]
    fn enforces_grammar_and_length() {
        assert!(is_valid_property_name("occurred_at"));
        assert!(is_valid_property_name("outlook_mail_internet_message_id"));
        assert!(is_valid_property_name(&"a".repeat(63)));
        assert!(!is_valid_property_name(&"a".repeat(64)));
        assert!(!is_valid_property_name(""));
        assert!(!is_valid_property_name("_leading"));
        assert!(!is_valid_property_name("9leading"));
        assert!(!is_valid_property_name("Upper"));
        assert!(!is_valid_property_name("with-hyphen"));
        assert!(!is_valid_property_name("grüsse"));
    }
}
