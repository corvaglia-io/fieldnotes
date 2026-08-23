//! The non-secret credential reference.
//!
//! A2 section 12 draws a hard line: configuration and the collection request
//! carry a **reference**, never a value. The wire type for that reference in
//! the Field protocol is `fieldnotes_field_protocol::grammar::ProfileRef`, but
//! this crate deliberately does not depend on the protocol crate (this is a
//! lower, standalone layer that the protocol crate's host implementation can
//! use, not the other way around). [`CredentialRef`] is this crate's own copy
//! of the same non-secret grammar so the two stay interchangeable byte for
//! byte without a cross-crate dependency: `[a-z][a-z0-9_]*`, one to sixty-three
//! bytes.

use core::fmt;

/// Errors produced while validating a [`CredentialRef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CredentialRefError {
    /// The value is empty.
    Empty,
    /// The value exceeds 63 ASCII bytes.
    TooLong,
    /// The value does not match `[a-z][a-z0-9_]*`.
    InvalidCharacters,
}

impl fmt::Display for CredentialRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialRefError::Empty => write!(f, "credential reference is empty"),
            CredentialRefError::TooLong => {
                write!(f, "credential reference exceeds 63 ASCII bytes")
            }
            CredentialRefError::InvalidCharacters => {
                write!(f, "credential reference does not match [a-z][a-z0-9_]*")
            }
        }
    }
}

impl std::error::Error for CredentialRefError {}

/// A validated, non-secret name for a configured credential profile, such as
/// `microsoft_acme`.
///
/// This is the public currency of the [`crate::provider::CredentialProvider`]
/// abstraction: it is safe to place in ordinary Field configuration, in a
/// diagnostic, or in a log line. It never carries credential material and
/// carries no information about where or whether material exists.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CredentialRef(String);

impl CredentialRef {
    /// Validates `text` against `[a-z][a-z0-9_]*`, one to sixty-three bytes.
    pub fn parse(text: &str) -> Result<Self, CredentialRefError> {
        if text.is_empty() {
            return Err(CredentialRefError::Empty);
        }
        if text.len() > 63 {
            return Err(CredentialRefError::TooLong);
        }
        let mut bytes = text.bytes();
        // `is_empty` above guarantees a first byte.
        let Some(first) = bytes.next() else {
            return Err(CredentialRefError::Empty);
        };
        if !first.is_ascii_lowercase() {
            return Err(CredentialRefError::InvalidCharacters);
        }
        if !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return Err(CredentialRefError::InvalidCharacters);
        }
        Ok(CredentialRef(text.to_owned()))
    }

    /// The validated textual form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CredentialRef {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_lower_snake_case() -> Result<(), CredentialRefError> {
        assert_eq!(
            CredentialRef::parse("microsoft_acme")?.as_str(),
            "microsoft_acme"
        );
        assert!(CredentialRef::parse("a").is_ok());
        assert!(CredentialRef::parse("a0_9").is_ok());
        Ok(())
    }

    #[test]
    fn rejects_empty_uppercase_leading_digit_and_overlong() {
        assert_eq!(CredentialRef::parse(""), Err(CredentialRefError::Empty));
        assert_eq!(
            CredentialRef::parse("Microsoft"),
            Err(CredentialRefError::InvalidCharacters)
        );
        assert_eq!(
            CredentialRef::parse("9start"),
            Err(CredentialRefError::InvalidCharacters)
        );
        assert_eq!(
            CredentialRef::parse("has space"),
            Err(CredentialRefError::InvalidCharacters)
        );
        let overlong = "a".repeat(64);
        assert_eq!(
            CredentialRef::parse(&overlong),
            Err(CredentialRefError::TooLong)
        );
        let exactly_63 = "a".repeat(63);
        assert!(CredentialRef::parse(&exactly_63).is_ok());
    }

    #[test]
    fn display_round_trips() -> Result<(), CredentialRefError> {
        let reference = CredentialRef::parse("outlook_mail_work")?;
        assert_eq!(reference.to_string(), "outlook_mail_work");
        Ok(())
    }
}
