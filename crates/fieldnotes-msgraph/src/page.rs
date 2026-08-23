//! Incremental pagination and delta collection.
//!
//! [`PageStream`] follows Graph's `@odata.nextLink` one page at a time,
//! yielding items as an [`Iterator`] rather than accumulating a whole
//! collection in memory first — a mailbox is large, and a Field consuming
//! this crate should never need more than one page resident at once. When
//! the final page carries `@odata.deltaLink` instead of `@odata.nextLink`,
//! [`PageStream::delta_token`] exposes it as an opaque [`DeltaToken`] the
//! caller persists in its own cursor state and resumes from on the next
//! sync, via [`DeltaStart::Resume`].

use std::collections::VecDeque;
use std::fmt;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::client::GraphClient;
use crate::clock::RetryClock;
use crate::error::GraphError;
use crate::request::GraphRequest;
use crate::token::AccessToken;
use crate::transport::HttpTransport;
use fieldnotes_domain::RandomSource;

/// An opaque Graph delta cursor.
///
/// This is exactly Graph's `@odata.deltaLink`. It is not a secret — a
/// delta link is a paging cursor a Field is expected to persist in its own
/// cursor state, the same way a portable source key or a checkpoint offset
/// is persisted — but it is still meaningless outside this crate's
/// [`DeltaStart::Resume`], so it is kept as an opaque newtype rather than a
/// bare [`String`] a caller might be tempted to parse or edit.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct DeltaToken(String);

impl DeltaToken {
    /// The raw delta-link value, for a caller's own cursor persistence.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for DeltaToken {
    fn from(value: String) -> Self {
        DeltaToken(value)
    }
}

impl fmt::Display for DeltaToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a delta collection begins.
pub enum DeltaStart {
    /// A first delta collection against a resource, such as
    /// `GraphRequest::new("/me/messages")`. This crate appends the `/delta`
    /// segment; the caller supplies the resource and any `$select`/`$top`
    /// it wants on the initial page.
    Initial(GraphRequest),
    /// Resumes a delta collection from a [`DeltaToken`] a previous run
    /// persisted. Graph resumes exactly where that link left off; no other
    /// query parameter is meaningful alongside it.
    Resume(DeltaToken),
}

/// A collection page, generic over the item type a connector deserializes
/// into.
///
/// This crate does not decide what a mail message, event, or contact looks
/// like — that mapping is a separate Field's job. It only knows how to
/// follow Graph's paging envelope around whatever type the caller supplies.
#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct CollectionEnvelope<T> {
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink", default)]
    delta_link: Option<String>,
    #[serde(default = "Vec::new")]
    value: Vec<T>,
}

enum PageCursor {
    Pending(String),
    Done,
}

/// A lazily-fetched sequence of items across one or more Graph pages.
///
/// Yields `Result<Item, GraphError>` one item at a time, fetching the next
/// page only once the buffered page is exhausted. After the iterator ends
/// (`next()` returns `None`), call [`PageStream::delta_token`] to retrieve
/// the delta cursor, if the collection was a delta collection and the final
/// page carried one.
pub struct PageStream<'a, T, C, R, Item> {
    client: &'a GraphClient<T, C, R>,
    token: &'a AccessToken,
    operation: &'static str,
    cursor: PageCursor,
    buffer: VecDeque<Item>,
    delta_token: Option<DeltaToken>,
}

impl<'a, T, C, R, Item> PageStream<'a, T, C, R, Item>
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
    Item: DeserializeOwned,
{
    pub(crate) fn new(
        client: &'a GraphClient<T, C, R>,
        token: &'a AccessToken,
        start_url: String,
        operation: &'static str,
    ) -> Self {
        PageStream {
            client,
            token,
            operation,
            cursor: PageCursor::Pending(start_url),
            buffer: VecDeque::new(),
            delta_token: None,
        }
    }

    /// The delta cursor to persist, once the collection has produced its
    /// final page.
    ///
    /// `None` before the stream is exhausted, and `None` after exhaustion
    /// if this was a plain (non-delta) collection.
    #[must_use]
    pub fn delta_token(&self) -> Option<&DeltaToken> {
        self.delta_token.as_ref()
    }

    /// Fetches the next page, if any, extending the buffer.
    ///
    /// Returns `Ok(true)` if a page was fetched (even an empty one — Graph
    /// may return an empty intermediate page before a further link),
    /// `Ok(false)` if the collection was already exhausted, and `Err` if the
    /// fetch failed or the next link pointed outside the configured
    /// authority.
    fn fetch_next_page(&mut self) -> Result<bool, GraphError> {
        let url = match std::mem::replace(&mut self.cursor, PageCursor::Done) {
            PageCursor::Pending(url) => url,
            PageCursor::Done => return Ok(false),
        };
        if !self.client.trusts_authority(&url) {
            return Err(GraphError::UntrustedContinuation {
                operation: self.operation,
            });
        }
        let response = self.client.execute_get(self.token, &url, self.operation)?;
        let envelope: CollectionEnvelope<Item> =
            self.client.parse_json(&response, self.operation)?;
        self.buffer.extend(envelope.value);
        self.cursor = match (envelope.next_link, envelope.delta_link) {
            (Some(next), _) => PageCursor::Pending(next),
            (None, Some(delta)) => {
                self.delta_token = Some(DeltaToken(delta));
                PageCursor::Done
            }
            (None, None) => PageCursor::Done,
        };
        Ok(true)
    }
}

impl<'a, T, C, R, Item> Iterator for PageStream<'a, T, C, R, Item>
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
    Item: DeserializeOwned,
{
    type Item = Result<Item, GraphError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.buffer.pop_front() {
                return Some(Ok(item));
            }
            match self.fetch_next_page() {
                Ok(true) => continue,
                Ok(false) => return None,
                Err(error) => {
                    // Do not retry the same page indefinitely on the next
                    // `next()` call: the cursor was already consumed by
                    // `fetch_next_page`, so the stream simply ends here.
                    return Some(Err(error));
                }
            }
        }
    }
}
