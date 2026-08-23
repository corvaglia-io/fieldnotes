//! Wire shapes for the Microsoft Graph `contact` resource and its delta feed.
//!
//! These are vendor structures, deliberately kept separate from
//! [`crate::record`], which maps them onto Fieldnotes vocabulary. Every field
//! is optional except `id`: Graph's `$select` only returns what was asked
//! for, and a delta feed's `@removed` entries carry nothing but `id` and the
//! removal marker.

use serde::Deserialize;

/// One Graph `emailAddress` value.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphEmailAddress {
    /// The address itself, such as `alice@example.com`.
    #[serde(default)]
    pub(crate) address: Option<String>,
}

/// A delta feed's removal marker, present only on a `@removed` entry.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphRemoved {
    /// Why the entry was removed. Graph's contact delta only ever removes by
    /// deletion, but the reason string itself is not load-bearing here: any
    /// `@removed` entry at all means the contact is gone from this scope.
    #[serde(default)]
    #[allow(dead_code, reason = "kept for fixture readability; not branched on")]
    pub(crate) reason: Option<String>,
}

/// One Graph `contact` resource, or one delta-feed removal marker.
///
/// A single `struct` for both shapes -- rather than an enum with a custom
/// deserializer -- because Graph's delta feed interleaves ordinary contacts
/// and `@removed` entries in one JSON array with no other discriminator, and
/// [`GraphContact::is_removed`] is the one place that distinction is made.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphContact {
    /// The contact's stable Graph ID within its mailbox.
    pub(crate) id: String,
    /// Present only on a delta-feed removal.
    #[serde(rename = "@removed", default)]
    pub(crate) removed: Option<GraphRemoved>,
    /// The display name Outlook renders, and this Field's `title` fallback.
    #[serde(rename = "displayName", default)]
    pub(crate) display_name: Option<String>,
    /// The stated given name, used only by the person/organization heuristic
    /// in [`crate::record`].
    #[serde(rename = "givenName", default)]
    pub(crate) given_name: Option<String>,
    /// The stated surname, used only by the same heuristic.
    #[serde(default)]
    pub(crate) surname: Option<String>,
    /// The stated employer name.
    #[serde(rename = "companyName", default)]
    pub(crate) company_name: Option<String>,
    /// The stated job title.
    #[serde(rename = "jobTitle", default)]
    pub(crate) job_title: Option<String>,
    /// Every stated email address, in source order.
    #[serde(rename = "emailAddresses", default)]
    pub(crate) email_addresses: Vec<GraphEmailAddress>,
    /// Every stated business phone number, in source order.
    #[serde(rename = "businessPhones", default)]
    pub(crate) business_phones: Vec<String>,
    /// Every stated home phone number, in source order.
    #[serde(rename = "homePhones", default)]
    pub(crate) home_phones: Vec<String>,
    /// The stated mobile phone number.
    #[serde(rename = "mobilePhone", default)]
    pub(crate) mobile_phone: Option<String>,
    /// When the contact was last changed upstream, an explicit-offset or
    /// `Z`-suffixed RFC 3339 instant.
    #[serde(rename = "lastModifiedDateTime", default)]
    pub(crate) last_modified_date_time: Option<String>,
    /// When the contact was created upstream, the fallback event instant when
    /// `lastModifiedDateTime` is absent.
    #[serde(rename = "createdDateTime", default)]
    pub(crate) created_date_time: Option<String>,
    /// Graph's own opaque change-tracking token for this contact. Retained as
    /// display evidence (`source.version`); this Field declares
    /// `source_version_ordering: unsupported` (see [`crate::manifest`]), so
    /// core never compares two of these to decide which is newer.
    #[serde(rename = "changeKey", default)]
    pub(crate) change_key: Option<String>,
}

impl GraphContact {
    /// Whether this entry is a delta-feed removal rather than a live contact.
    #[must_use]
    pub(crate) fn is_removed(&self) -> bool {
        self.removed.is_some()
    }

    /// Every distinct, non-empty phone number stated for this contact, in
    /// source order: business numbers, then home numbers, then the mobile
    /// number.
    pub(crate) fn phone_numbers(&self) -> impl Iterator<Item = &str> {
        self.business_phones
            .iter()
            .chain(self.home_phones.iter())
            .map(String::as_str)
            .chain(self.mobile_phone.as_deref())
            .filter(|number| !number.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::GraphContact;

    #[test]
    fn a_removed_entry_carries_only_its_id_and_the_removal_marker() {
        let contact: GraphContact =
            serde_json::from_str(r#"{"id":"AAMkAGI2CONTACT01","@removed":{"reason":"deleted"}}"#)
                .unwrap_or_else(|error| panic!("must parse: {error}"));
        assert!(contact.is_removed());
        assert_eq!(contact.id, "AAMkAGI2CONTACT01");
    }

    #[test]
    fn an_ordinary_contact_is_not_removed() {
        let contact: GraphContact = serde_json::from_str(r#"{"id":"x","displayName":"Alice"}"#)
            .unwrap_or_else(|error| panic!("must parse: {error}"));
        assert!(!contact.is_removed());
    }

    #[test]
    fn phone_numbers_are_yielded_business_then_home_then_mobile() {
        let contact: GraphContact = serde_json::from_str(
            r#"{"id":"x","businessPhones":["+41 44 123 45 67"],"homePhones":["+41 44 999 00 00"],"mobilePhone":"+41 79 111 22 33"}"#,
        )
        .unwrap_or_else(|error| panic!("must parse: {error}"));
        let numbers: Vec<&str> = contact.phone_numbers().collect();
        assert_eq!(
            numbers,
            vec!["+41 44 123 45 67", "+41 44 999 00 00", "+41 79 111 22 33"]
        );
    }

    #[test]
    fn empty_phone_entries_are_skipped() {
        let contact: GraphContact =
            serde_json::from_str(r#"{"id":"x","businessPhones":[""],"mobilePhone":"   "}"#)
                .unwrap_or_else(|error| panic!("must parse: {error}"));
        assert_eq!(contact.phone_numbers().count(), 0);
    }
}
