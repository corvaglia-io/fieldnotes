//! The read-only Graph reads this Field makes, and nothing else.
//!
//! Every call here goes through [`fieldnotes_msgraph::GraphClient`], which is
//! `GET`-only by construction, so no write against a mailbox is expressible
//! from this Field at all. Pagination, `Retry-After` handling, exponential
//! backoff, response-size bounds, continuation-authority checks, and error
//! classification all belong to the transport; this module only chooses which
//! resources to read and which fields to `$select`.

use fieldnotes_domain::RandomSource;
use fieldnotes_field_protocol::message::Window;
use fieldnotes_msgraph::{
    AccessToken, DeltaStart, DeltaToken, GraphClient, GraphError, GraphRequest, HttpTransport,
    PageStream, RequestBuildError, RetryClock,
};

use crate::api::{GraphAttachment, GraphMessage};

/// The message members this Field maps, and no others.
///
/// Selecting exactly what is mapped keeps the response the transport has to
/// bound and parse, and the surface [`crate::record`] has to consider, as
/// small as the mapping allows.
const MESSAGE_FIELDS: [&str; 18] = [
    "id",
    "changeKey",
    "subject",
    "body",
    "bodyPreview",
    "receivedDateTime",
    "sentDateTime",
    "from",
    "sender",
    "toRecipients",
    "ccRecipients",
    "bccRecipients",
    "replyTo",
    "conversationId",
    "internetMessageId",
    "importance",
    "hasAttachments",
    "isRead",
];

/// The remaining message members, split out only because a `$select` list is
/// written as one comma-joined value and this keeps each array's length
/// obvious at a glance.
const MESSAGE_FIELDS_EXTRA: [&str; 3] = ["isDraft", "categories", "parentFolderId"];

/// The attachment metadata this Field needs to apply the run's retention
/// policy. Deliberately **excludes** `contentBytes`: the policy decision is
/// made from size and media type before any byte is fetched.
const ATTACHMENT_METADATA_FIELDS: [&str; 5] = ["id", "name", "contentType", "size", "isInline"];

/// Percent-encodes one path segment for a Graph resource path.
///
/// Graph mail and attachment identifiers are opaque base64url-shaped tokens.
/// This encodes only what genuinely cannot be left raw in a path -- the
/// segment and query delimiters, the percent sign itself, the plus sign (which
/// too many intermediaries still read as a space), and anything non-printable
/// or non-ASCII -- and leaves the base64url alphabet plus `=` padding
/// untouched, so an ordinary identifier appears in the URL exactly as Graph
/// gave it. An identifier can therefore never introduce a new path segment or
/// a query parameter, however it is spelled.
#[must_use]
pub(crate) fn encode_path_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        let safe = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'~' | b'=' | b'!' | b'*' | b'(' | b')'
            );
        if safe {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        _ => char::from(b'A' + (nibble - 10)),
    }
}

/// Renders one A2 window bound as an OData datetime literal.
///
/// The window's own bounds already carry an explicit numeric offset, which
/// Graph accepts, so the value crosses verbatim rather than being re-zoned by
/// this Field.
fn odata_instant(value: &fieldnotes_field_protocol::grammar::OffsetDatetime) -> String {
    value.to_string()
}

/// The read-only mail reads this Field performs against one mail folder.
pub(crate) struct MailReader<'a, T, C, R> {
    client: &'a GraphClient<T, C, R>,
    token: &'a AccessToken,
    /// The already-validated well-known folder name or Graph folder
    /// identifier ([`crate::config`] refuses anything that could widen the
    /// resource path).
    folder: &'a str,
}

impl<'a, T, C, R> MailReader<'a, T, C, R>
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    /// Binds a reader to one client, one token, and one mail folder.
    pub(crate) fn new(
        client: &'a GraphClient<T, C, R>,
        token: &'a AccessToken,
        folder: &'a str,
    ) -> Self {
        MailReader {
            client,
            token,
            folder,
        }
    }

    /// The messages collection of the configured folder.
    fn messages_resource(&self) -> String {
        format!("/me/mailFolders('{}')/messages", self.folder)
    }

    fn selected(&self, resource: String) -> GraphRequest {
        GraphRequest::new(resource)
            .select(
                MESSAGE_FIELDS
                    .iter()
                    .chain(MESSAGE_FIELDS_EXTRA.iter())
                    .copied(),
            )
            .top(crate::constants::PAGE_SIZE)
    }

    /// Starts or resumes the delta collection of the configured folder.
    ///
    /// A delta collection is the only read that reports removals, and the only
    /// one that yields a resumable token.
    pub(crate) fn delta(
        &self,
        resume: Option<DeltaToken>,
    ) -> PageStream<'a, T, C, R, GraphMessage> {
        let start = match resume {
            Some(token) => DeltaStart::Resume(token),
            None => DeltaStart::Initial(self.selected(self.messages_resource())),
        };
        self.client.delta(self.token, start, "collect mail delta")
    }

    /// Lists the messages received inside `window`, ordered by received
    /// instant.
    ///
    /// Graph's mail delta endpoint admits neither `$filter` nor `$orderby`, so
    /// a bounded window is a filtered plain list rather than a delta
    /// collection. That is why a windowed run never produces a delta token and
    /// never advances this Field's cursor: see [`crate::cursor`].
    pub(crate) fn window(
        &self,
        window: &Window,
    ) -> Result<PageStream<'a, T, C, R, GraphMessage>, RequestBuildError> {
        let predicate = format!(
            "receivedDateTime ge {} and receivedDateTime lt {}",
            odata_instant(&window.from),
            odata_instant(&window.to)
        );
        let request = self
            .selected(self.messages_resource())
            .order_by("receivedDateTime")
            .filter(predicate)?;
        Ok(self
            .client
            .list(self.token, request, "list mail messages in window"))
    }

    /// Reads one message by its opaque identifier, for an explicit
    /// re-collection request.
    pub(crate) fn message(&self, message_id: &str) -> Result<GraphMessage, GraphError> {
        let resource = format!("/me/messages('{}')", encode_path_segment(message_id));
        self.client
            .get(self.token, self.selected(resource), "refetch mail message")
    }

    /// Lists one message's attachment **metadata**, with no bytes.
    pub(crate) fn attachments(&self, message_id: &str) -> PageStream<'a, T, C, R, GraphAttachment> {
        let request = GraphRequest::new(format!(
            "/me/messages('{}')/attachments",
            encode_path_segment(message_id)
        ))
        .select(ATTACHMENT_METADATA_FIELDS);
        self.client
            .list(self.token, request, "list mail attachments")
    }

    /// Reads one attachment including its base64 `contentBytes`.
    ///
    /// Called only after the run's retention policy has already admitted the
    /// attachment, so bytes core would refuse are never fetched at all.
    pub(crate) fn attachment_content(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<GraphAttachment, GraphError> {
        let request = GraphRequest::new(format!(
            "/me/messages('{}')/attachments('{}')",
            encode_path_segment(message_id),
            encode_path_segment(attachment_id)
        ))
        .select(["id", "name", "contentType", "size", "contentBytes"]);
        self.client
            .get(self.token, request, "read mail attachment bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::encode_path_segment;

    #[test]
    fn an_ordinary_graph_identifier_survives_unchanged() {
        let id = "AAMkAGI2TQABAAAA-_0123456789=";
        assert_eq!(encode_path_segment(id), id);
    }

    #[test]
    fn a_path_or_query_delimiter_can_never_survive_an_identifier() {
        for hostile in [
            "../../users('victim')/messages",
            "abc?$select=secret",
            "abc#frag",
            "abc%2f",
            "a b",
            "abc'",
        ] {
            let encoded = encode_path_segment(hostile);
            for forbidden in ['/', '?', '#', ' ', '\''] {
                assert!(
                    !encoded.contains(forbidden),
                    "{hostile:?} encoded to {encoded:?}, which still contains {forbidden:?}"
                );
            }
        }
    }

    #[test]
    fn a_plus_sign_is_encoded_so_no_intermediary_can_read_it_as_a_space() {
        assert_eq!(encode_path_segment("a+b"), "a%2Bb");
    }

    #[test]
    fn every_encoded_byte_is_ascii_regardless_of_input() {
        assert!(encode_path_segment("caf\u{e9}/\u{1f600}").is_ascii());
    }
}
