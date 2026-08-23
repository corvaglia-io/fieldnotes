//! Mapping a contact's stated anchors onto A1's `identities` list.
//!
//! # What this module is not
//!
//! Fieldnotes does not perform identity mapping. An anchor is evidence one
//! source object asserts about itself -- "this contact record states this
//! email address" -- never a claim that two contacts are the same person,
//! and never a substitute for the portable exact-source key
//! ([`crate::scope`]) that reconciles a Note. This module normalizes and
//! namespaces exactly the anchors one Graph contact states and nothing more:
//! it never merges, deduplicates across contacts, or infers a relationship.
//! Core alone projects the anchors this module returns onto the shared,
//! set-like `identities` property; a Field never supplies that list directly.

use fieldnotes_field_protocol::grammar::{IdentityNamespace, RuleName};
use fieldnotes_field_protocol::message::{IdentityAnchor, IdentityScopeClass};

/// The normalization rule name and version this Field declares for an email
/// anchor, echoed on every anchor of that namespace per A2 section 7.
pub(crate) const EMAIL_NORMALIZATION_RULE: &str = "email_lowercase_trim";
pub(crate) const EMAIL_NORMALIZATION_VERSION: u16 = 1;

/// The normalization rule name and version this Field declares for a phone
/// anchor.
pub(crate) const PHONE_NORMALIZATION_RULE: &str = "phone_digits_e164_like";
pub(crate) const PHONE_NORMALIZATION_VERSION: u16 = 1;

fn rule(text: &str) -> RuleName {
    RuleName::parse(text).unwrap_or_else(|error| panic!("{text:?} must be a rule name: {error}"))
}

fn namespace(text: &str) -> IdentityNamespace {
    IdentityNamespace::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be an identity namespace: {error}"))
}

/// Lowercases and trims a stated email address.
///
/// This is the whole normalization: Fieldnotes does not validate that the
/// result is a deliverable mailbox, only that the same stated address always
/// normalizes to the same anchor value.
#[must_use]
fn normalize_email(stated: &str) -> Option<String> {
    let trimmed = stated.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

/// Keeps a leading `+` and every ASCII digit from a stated phone number,
/// dropping formatting characters (spaces, parentheses, hyphens, dots).
///
/// This is a normalization for stable comparison, not E.164 validation:
/// Fieldnotes does not verify that the result is a dialable number, only
/// that the same stated number always normalizes to the same anchor value.
#[must_use]
fn normalize_phone(stated: &str) -> Option<String> {
    let trimmed = stated.trim();
    let mut normalized = String::with_capacity(trimmed.len());
    for (index, ch) in trimmed.chars().enumerate() {
        if (ch == '+' && index == 0) || ch.is_ascii_digit() {
            normalized.push(ch);
        }
    }
    let digit_count = normalized.chars().filter(char::is_ascii_digit).count();
    if digit_count == 0 {
        None
    } else {
        Some(normalized)
    }
}

/// Builds the `email:` anchor for one stated address, or `None` when the
/// address is blank.
#[must_use]
pub(crate) fn email_anchor(stated: &str) -> Option<IdentityAnchor> {
    let value = normalize_email(stated)?;
    Some(IdentityAnchor {
        namespace: namespace(crate::constants::ANCHOR_NAMESPACE_EMAIL),
        value,
        scope_class: IdentityScopeClass::NormalizedChannel,
        scope: None,
        normalization_rule: Some(rule(EMAIL_NORMALIZATION_RULE)),
        normalization_version: Some(EMAIL_NORMALIZATION_VERSION),
        role: None,
    })
}

/// Builds the `phone:` anchor for one stated number, or `None` when nothing
/// dialable remains after normalization.
#[must_use]
pub(crate) fn phone_anchor(stated: &str) -> Option<IdentityAnchor> {
    let value = normalize_phone(stated)?;
    Some(IdentityAnchor {
        namespace: namespace(crate::constants::ANCHOR_NAMESPACE_PHONE),
        value,
        scope_class: IdentityScopeClass::NormalizedChannel,
        scope: None,
        normalization_rule: Some(rule(PHONE_NORMALIZATION_RULE)),
        normalization_version: Some(PHONE_NORMALIZATION_VERSION),
        role: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{email_anchor, normalize_email, normalize_phone, phone_anchor};

    #[test]
    fn email_is_lowercased_and_trimmed() {
        assert_eq!(
            normalize_email("  Alice@Example.COM "),
            Some("alice@example.com".to_owned())
        );
        assert_eq!(normalize_email("   "), None);
    }

    #[test]
    fn phone_keeps_a_leading_plus_and_digits_only() {
        assert_eq!(
            normalize_phone("+41 44 123 45 67"),
            Some("+41441234567".to_owned())
        );
        assert_eq!(
            normalize_phone("(044) 123-45-67"),
            Some("0441234567".to_owned())
        );
        assert_eq!(normalize_phone("not a number"), None);
    }

    #[test]
    fn a_plus_only_in_the_interior_is_dropped_not_treated_as_a_prefix() {
        // A plus is only ever meaningful as the very first character of a
        // stated number; one appearing elsewhere is stray punctuation.
        assert_eq!(normalize_phone("044+123"), Some("044123".to_owned()));
    }

    #[test]
    fn anchors_carry_the_declared_normalization_rule_and_version() {
        let anchor = email_anchor("Bob@Example.net")
            .unwrap_or_else(|| panic!("must build an anchor for a non-blank address"));
        assert_eq!(anchor.namespace.as_str(), "email");
        assert_eq!(anchor.value, "bob@example.net");
        assert_eq!(
            anchor
                .normalization_rule
                .map(|rule| rule.as_str().to_owned()),
            Some("email_lowercase_trim".to_owned())
        );
        assert_eq!(anchor.normalization_version, Some(1));

        let phone = phone_anchor("+1 (555) 010-0100")
            .unwrap_or_else(|| panic!("must build an anchor for a non-blank number"));
        assert_eq!(phone.namespace.as_str(), "phone");
        assert_eq!(phone.value, "+15550100100");
    }
}
