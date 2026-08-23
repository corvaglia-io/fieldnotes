//! The Microsoft Graph response shapes this Field deserializes.
//!
//! `fieldnotes-msgraph` is generic over the response type a connector defines,
//! so these types live here rather than in the transport: the transport knows
//! how to follow a paging envelope, retry a throttled request, and classify a
//! failure, and deliberately does not know what a mail message looks like.
//!
//! # Everything is optional on purpose
//!
//! A Graph response is untrusted input (A2 section 14). Every member below is
//! optional and defaulted, and no struct denies unknown fields, so a response
//! that omits something this Field asked for, or carries something newer than
//! this build knows, degrades to a missing value rather than failing the whole
//! page. What is genuinely required -- an identifier, an instant -- is checked
//! by the mapping step ([`crate::record`]), which can then report *which*
//! message was unusable instead of losing the page it arrived on.

use serde::Deserialize;

/// Graph's `@removed` annotation on a delta item: the source stating that an
/// object is gone from the collected scope.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Removed {
    /// Graph's own reason, typically `deleted` or `changed`.
    #[serde(default)]
    pub(crate) reason: Option<String>,
}

impl Removed {
    /// Whether this annotation is evidence that the object was **deleted**,
    /// rather than merely that it left the collected scope.
    ///
    /// Graph reports `reason: "deleted"` for a genuine deletion and
    /// `reason: "changed"` when a folder-scoped delta loses sight of an item
    /// that still exists -- most commonly because the user moved it to another
    /// folder. Only the first is an authoritative deletion; treating the second
    /// as one would remove the Note for a message that is still in the
    /// mailbox, which is precisely the "absence is not deletion" mistake A2
    /// section 10 exists to prevent.
    pub(crate) fn is_authoritative_deletion(&self) -> bool {
        self.reason
            .as_deref()
            .is_some_and(|reason| reason.eq_ignore_ascii_case("deleted"))
    }
}

/// One mail address as Graph spells it.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct EmailAddress {
    /// The SMTP address.
    #[serde(default)]
    pub(crate) address: Option<String>,
}

/// Graph's `recipient` wrapper around one mail address.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Recipient {
    /// The wrapped address.
    #[serde(rename = "emailAddress", default)]
    pub(crate) email_address: Option<EmailAddress>,
}

impl Recipient {
    /// The SMTP address this recipient names, trimmed and ASCII-lowercased by
    /// the `mail_address_lowercase` normalization rule the manifest declares.
    pub(crate) fn normalized_address(&self) -> Option<String> {
        let address = self
            .email_address
            .as_ref()?
            .address
            .as_ref()?
            .trim()
            .to_ascii_lowercase();
        if address.is_empty() {
            None
        } else {
            Some(address)
        }
    }
}

/// Graph's `itemBody`.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ItemBody {
    /// `text` or `html`.
    #[serde(rename = "contentType", default)]
    pub(crate) content_type: Option<String>,
    /// The body itself.
    #[serde(default)]
    pub(crate) content: Option<String>,
}

/// One mail message, or one delta annotation reporting its removal.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct GraphMessage {
    /// The opaque per-mailbox message identifier.
    #[serde(default)]
    pub(crate) id: Option<String>,
    /// Present exactly when this delta item reports a removal.
    #[serde(rename = "@removed", default)]
    pub(crate) removed: Option<Removed>,
    /// Graph's opaque change token, carried as `source.version`.
    #[serde(rename = "changeKey", default)]
    pub(crate) change_key: Option<String>,
    /// The subject line.
    #[serde(default)]
    pub(crate) subject: Option<String>,
    /// The message body.
    #[serde(default)]
    pub(crate) body: Option<ItemBody>,
    /// Graph's own short plain-text preview, used when no body arrived.
    #[serde(rename = "bodyPreview", default)]
    pub(crate) body_preview: Option<String>,
    /// When the mailbox received the message.
    #[serde(rename = "receivedDateTime", default)]
    pub(crate) received_date_time: Option<String>,
    /// When the sender sent it, used when no received instant arrived.
    #[serde(rename = "sentDateTime", default)]
    pub(crate) sent_date_time: Option<String>,
    /// The `From` header.
    #[serde(default)]
    pub(crate) from: Option<Recipient>,
    /// The envelope sender, used when no `From` arrived.
    #[serde(default)]
    pub(crate) sender: Option<Recipient>,
    /// The `To` header, in the order the message carries it.
    #[serde(rename = "toRecipients", default)]
    pub(crate) to_recipients: Option<Vec<Recipient>>,
    /// The `Cc` header, in the order the message carries it.
    #[serde(rename = "ccRecipients", default)]
    pub(crate) cc_recipients: Option<Vec<Recipient>>,
    /// The `Bcc` header, in the order the mailbox reports it.
    #[serde(rename = "bccRecipients", default)]
    pub(crate) bcc_recipients: Option<Vec<Recipient>>,
    /// The `Reply-To` header.
    #[serde(rename = "replyTo", default)]
    pub(crate) reply_to: Option<Vec<Recipient>>,
    /// Graph's conversation identifier.
    #[serde(rename = "conversationId", default)]
    pub(crate) conversation_id: Option<String>,
    /// The RFC 5322 `Message-ID`.
    #[serde(rename = "internetMessageId", default)]
    pub(crate) internet_message_id: Option<String>,
    /// Sender-declared importance.
    #[serde(default)]
    pub(crate) importance: Option<String>,
    /// Whether the message claims to have attachments.
    #[serde(rename = "hasAttachments", default)]
    pub(crate) has_attachments: Option<bool>,
    /// Whether the mailbox reports the message as read.
    #[serde(rename = "isRead", default)]
    pub(crate) is_read: Option<bool>,
    /// Whether the mailbox reports the message as an unsent draft.
    #[serde(rename = "isDraft", default)]
    pub(crate) is_draft: Option<bool>,
    /// Outlook categories.
    #[serde(default)]
    pub(crate) categories: Option<Vec<String>>,
    /// The folder the message was in when collected.
    #[serde(rename = "parentFolderId", default)]
    pub(crate) parent_folder_id: Option<String>,
}

impl GraphMessage {
    /// Whether this delta item reports a removal rather than current state.
    pub(crate) fn is_removed(&self) -> bool {
        self.removed.is_some()
    }

    /// Whether this delta item is an authoritative deletion, as opposed to an
    /// item that merely left the collected folder scope. See
    /// [`Removed::is_authoritative_deletion`].
    pub(crate) fn is_authoritative_deletion(&self) -> bool {
        self.removed
            .as_ref()
            .is_some_and(Removed::is_authoritative_deletion)
    }
}

/// The Graph `@odata.type` discriminator of a file attachment: the only
/// attachment kind with original bytes at the mail endpoint.
pub(crate) const FILE_ATTACHMENT_TYPE: &str = "#microsoft.graph.fileAttachment";

/// One attachment's metadata, and its bytes when they were requested.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct GraphAttachment {
    /// Graph's type discriminator.
    #[serde(rename = "@odata.type", default)]
    pub(crate) odata_type: Option<String>,
    /// The opaque attachment identifier, stable within its message.
    #[serde(default)]
    pub(crate) id: Option<String>,
    /// The attachment's own filename, retained as display evidence only.
    #[serde(default)]
    pub(crate) name: Option<String>,
    /// The media type the source declares.
    #[serde(rename = "contentType", default)]
    pub(crate) content_type: Option<String>,
    /// The size the source declares, in bytes.
    #[serde(default)]
    pub(crate) size: Option<i64>,
    /// Whether the attachment is rendered inline in the body.
    #[serde(rename = "isInline", default)]
    pub(crate) is_inline: Option<bool>,
    /// The attachment bytes, base64-encoded, present only when this Field
    /// asked for them.
    #[serde(rename = "contentBytes", default)]
    pub(crate) content_bytes: Option<String>,
}

impl GraphAttachment {
    /// Whether this attachment is a file attachment, and therefore the only
    /// kind whose original bytes this Field can retain.
    pub(crate) fn is_file_attachment(&self) -> bool {
        self.odata_type.as_deref() == Some(FILE_ATTACHMENT_TYPE)
    }

    /// The declared size, clamped into an unsigned byte count. A negative or
    /// absent size is treated as unknown, which the caller resolves
    /// conservatively.
    pub(crate) fn declared_bytes(&self) -> Option<u64> {
        self.size.and_then(|size| u64::try_from(size).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphAttachment, GraphMessage};

    #[test]
    fn a_removal_annotation_is_recognized_without_any_content() {
        let json = r#"{"id":"AAMkAGI2GONE01","@removed":{"reason":"deleted"}}"#;
        let message: GraphMessage =
            serde_json::from_str(json).unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert!(message.is_removed());
        assert_eq!(message.subject, None);
        assert!(message.is_authoritative_deletion());
    }

    #[test]
    fn a_moved_item_is_a_removal_from_scope_but_not_an_authoritative_deletion() {
        let json = r#"{"id":"AAMkAGI2MOVED01","@removed":{"reason":"changed"}}"#;
        let message: GraphMessage =
            serde_json::from_str(json).unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert!(message.is_removed());
        assert!(
            !message.is_authoritative_deletion(),
            "an item that merely left the collected folder is still in the mailbox"
        );
    }

    #[test]
    fn an_unknown_member_does_not_fail_the_page() {
        let json = r#"{"id":"AAMkAGI2TQABAAAA","somethingNewerThanThisBuild":42}"#;
        let message: GraphMessage =
            serde_json::from_str(json).unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert_eq!(message.id.as_deref(), Some("AAMkAGI2TQABAAAA"));
    }

    #[test]
    fn an_address_is_trimmed_and_lowercased() {
        let json = r#"{"emailAddress":{"name":"Alice","address":"  Alice@Example.COM "}}"#;
        let recipient: super::Recipient =
            serde_json::from_str(json).unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert_eq!(
            recipient.normalized_address(),
            Some("alice@example.com".to_owned())
        );
    }

    #[test]
    fn an_empty_address_is_no_address_at_all() {
        let json = r#"{"emailAddress":{"name":"Nobody","address":"   "}}"#;
        let recipient: super::Recipient =
            serde_json::from_str(json).unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert_eq!(recipient.normalized_address(), None);
    }

    #[test]
    fn only_a_file_attachment_claims_to_carry_bytes() {
        let file: GraphAttachment = serde_json::from_str(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1","size":10}"##,
        )
        .unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert!(file.is_file_attachment());
        assert_eq!(file.declared_bytes(), Some(10));

        let item: GraphAttachment = serde_json::from_str(
            r##"{"@odata.type":"#microsoft.graph.itemAttachment","id":"a2","size":-1}"##,
        )
        .unwrap_or_else(|error| panic!("must deserialize: {error}"));
        assert!(!item.is_file_attachment());
        assert_eq!(item.declared_bytes(), None);
    }
}
