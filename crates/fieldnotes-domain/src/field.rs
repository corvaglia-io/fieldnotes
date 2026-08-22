//! Field IDs, the registered Field-stem set, and connector property prefixes.
//!
//! `self` is the only one-part Field ID. External Field IDs are
//! `<registered-field-stem>_<user-label>`, where both parts match
//! `[a-z][a-z0-9]*(?:_[a-z0-9]+)*`, each part is at most 31 ASCII bytes, and
//! the complete ID is at most 63 bytes. The stem/label split is validated
//! against the configured registered stem set, never guessed from underscores.

use core::fmt;
use std::sync::LazyLock;

/// Errors produced while validating a Field ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldIdError {
    /// The complete Field ID exceeds 63 ASCII bytes.
    TooLong,
    /// The stem or label exceeds 31 ASCII bytes.
    PartTooLong,
    /// The stem or label does not match `[a-z][a-z0-9]*(?:_[a-z0-9]+)*`.
    InvalidPart,
    /// No registered stem produces a valid stem/label split.
    UnknownStem,
}

impl fmt::Display for FieldIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldIdError::TooLong => write!(f, "field ID exceeds 63 ASCII bytes"),
            FieldIdError::PartTooLong => write!(f, "field stem or label exceeds 31 ASCII bytes"),
            FieldIdError::InvalidPart => write!(f, "field stem or label violates the part grammar"),
            FieldIdError::UnknownStem => {
                write!(f, "field ID does not start with a registered stem")
            }
        }
    }
}

impl std::error::Error for FieldIdError {}

/// Whether `part` matches `[a-z][a-z0-9]*(?:_[a-z0-9]+)*`.
#[must_use]
pub fn is_valid_field_part(part: &str) -> bool {
    if !part.is_ascii() {
        return false;
    }
    let mut segments = part.split('_');
    let Some(first) = segments.next() else {
        return false;
    };
    let mut first_bytes = first.bytes();
    match first_bytes.next() {
        Some(b'a'..=b'z') => {}
        _ => return false,
    }
    if !first_bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) {
        return false;
    }
    segments.all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

/// The configured set of registered external Field stems.
///
/// `self` is not a stem: it is the reserved one-part built-in Field ID.
#[derive(Debug, Clone)]
pub struct FieldStemRegistry {
    stems: Vec<String>,
}

impl FieldStemRegistry {
    /// Builds a registry from external stems, rejecting invalid or over-long parts.
    pub fn new<I>(stems: I) -> Result<Self, FieldIdError>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut collected = Vec::new();
        for stem in stems {
            let stem = stem.into();
            if stem.len() > 31 {
                return Err(FieldIdError::PartTooLong);
            }
            if !is_valid_field_part(&stem) {
                return Err(FieldIdError::InvalidPart);
            }
            collected.push(stem);
        }
        Ok(FieldStemRegistry { stems: collected })
    }

    /// The approved v0.1 external stems: `local`, `outlook_mail`,
    /// `outlook_calendar`, `outlook_contacts`, `teams`, and `jira`.
    #[must_use]
    pub fn v1() -> &'static Self {
        static REGISTRY: LazyLock<FieldStemRegistry> = LazyLock::new(|| FieldStemRegistry {
            stems: [
                "local",
                "outlook_mail",
                "outlook_calendar",
                "outlook_contacts",
                "teams",
                "jira",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        });
        &REGISTRY
    }

    /// Iterates the registered external stems.
    pub fn stems(&self) -> impl Iterator<Item = &str> {
        self.stems.iter().map(String::as_str)
    }

    /// Whether `name` carries a registered source-property prefix
    /// (`<stem>_` followed by at least one more byte).
    #[must_use]
    pub fn has_registered_prefix(&self, name: &str) -> bool {
        self.stems.iter().any(|stem| {
            name.strip_prefix(stem.as_str())
                .and_then(|rest| rest.strip_prefix('_'))
                .is_some_and(|rest| !rest.is_empty())
        })
    }
}

/// A validated Field ID: `self` or `<registered-stem>_<user-label>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldId(String);

impl FieldId {
    /// Validates `text` against the configured registered stem set.
    pub fn parse(text: &str, registry: &FieldStemRegistry) -> Result<Self, FieldIdError> {
        if text == "self" {
            return Ok(FieldId(text.to_owned()));
        }
        if text.len() > 63 {
            return Err(FieldIdError::TooLong);
        }
        for stem in registry.stems() {
            let Some(rest) = text.strip_prefix(stem) else {
                continue;
            };
            let Some(label) = rest.strip_prefix('_') else {
                continue;
            };
            if label.len() > 31 {
                return Err(FieldIdError::PartTooLong);
            }
            if !is_valid_field_part(label) {
                return Err(FieldIdError::InvalidPart);
            }
            return Ok(FieldId(text.to_owned()));
        }
        Err(FieldIdError::UnknownStem)
    }

    /// The validated textual form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FieldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_self_and_registered_external_ids() -> Result<(), FieldIdError> {
        let registry = FieldStemRegistry::v1();
        assert_eq!(FieldId::parse("self", registry)?.as_str(), "self");
        assert_eq!(
            FieldId::parse("outlook_mail_work", registry)?.as_str(),
            "outlook_mail_work"
        );
        assert_eq!(FieldId::parse("teams_wxs", registry)?.as_str(), "teams_wxs");
        assert_eq!(
            FieldId::parse("jira_acme_eu", registry)?.as_str(),
            "jira_acme_eu"
        );
        Ok(())
    }

    #[test]
    fn rejects_unknown_stems_and_bad_labels() {
        let registry = FieldStemRegistry::v1();
        assert_eq!(
            FieldId::parse("outlook", registry),
            Err(FieldIdError::UnknownStem)
        );
        assert_eq!(
            FieldId::parse("self_extra", registry),
            Err(FieldIdError::UnknownStem)
        );
        assert_eq!(
            FieldId::parse("teams_", registry),
            Err(FieldIdError::InvalidPart)
        );
        assert_eq!(
            FieldId::parse("teams_9abc", registry),
            Err(FieldIdError::InvalidPart)
        );
        assert_eq!(
            FieldId::parse("teams_Work", registry),
            Err(FieldIdError::InvalidPart)
        );
    }

    #[test]
    fn enforces_byte_limits() {
        let registry = FieldStemRegistry::v1();
        let label_31 = "a".repeat(31);
        let label_32 = "a".repeat(32);
        assert!(FieldId::parse(&format!("teams_{label_31}"), registry).is_ok());
        assert_eq!(
            FieldId::parse(&format!("teams_{label_32}"), registry),
            Err(FieldIdError::PartTooLong)
        );
        let oversized = format!("outlook_calendar_{}", "a".repeat(60));
        assert_eq!(
            FieldId::parse(&oversized, registry),
            Err(FieldIdError::TooLong)
        );
    }

    #[test]
    fn prefix_detection_requires_a_suffix() {
        let registry = FieldStemRegistry::v1();
        assert!(registry.has_registered_prefix("outlook_mail_importance"));
        assert!(registry.has_registered_prefix("local_media_type"));
        assert!(!registry.has_registered_prefix("outlook_mail_"));
        assert!(!registry.has_registered_prefix("chat_id"));
    }
}
