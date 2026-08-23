//! Microsoft Graph `event` resource shapes and the `calendarView/delta`
//! request this Field builds from them.
//!
//! Nothing here decides Fieldnotes vocabulary -- that is [`crate::record`]'s
//! job. This module only knows how to ask Graph the right question and how
//! to decode what comes back.
//!
//! # Why `calendarView`, not `events`
//!
//! Graph's plain `/events` collection returns a recurring series as one
//! `seriesMaster` object; expanding it into the instances a calendar Note
//! actually needs is left to the caller. `/calendarView` (and its `/delta`
//! sibling) does that expansion server-side: every response item is a
//! `singleInstance`, `occurrence`, or `exception` already scoped to the
//! requested window, each with its own immutable `id` distinct from its
//! series master's. That is what makes gate R4's "a recurring series must
//! not duplicate instances" hold structurally: this Field never collects a
//! series master as its own object at all, so there is no second identity
//! for the same occurrence to collide with.
//!
//! # Why the initial request is built by hand
//!
//! [`fieldnotes_msgraph::GraphRequest`] has no builder step for a bare (non
//! `$`-prefixed) OData query parameter, and `calendarView`'s `startDateTime`
//! / `endDateTime` window bounds are exactly that. See this crate's final
//! report for why that gap belongs in `fieldnotes-msgraph` rather than here.

use fieldnotes_field_protocol::grammar::OffsetDatetime;
use fieldnotes_msgraph::GraphRequest;
use serde::Deserialize;

use crate::constants::SELECT_FIELDS;

/// One Graph `emailAddress` value.
///
/// Graph's own `name` sub-field is deliberately not decoded here: this
/// Field maps the address only, as both the registered `organizer`/
/// `participants` properties and the `email_v1` identity anchor expect, and
/// decoding a field this Field never reads would just be dead weight on
/// every response.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphEmailAddress {
    #[serde(default)]
    pub(crate) address: Option<String>,
}

/// One Graph `recipient` value: an organizer or an attendee's envelope.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphRecipient {
    #[serde(rename = "emailAddress", default)]
    pub(crate) email_address: Option<GraphEmailAddress>,
}

/// One Graph `attendee` value.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphAttendee {
    #[serde(rename = "emailAddress", default)]
    pub(crate) email_address: Option<GraphEmailAddress>,
}

/// One Graph `dateTimeTimeZone` value.
///
/// `date_time` carries no offset of its own; `time_zone` names it separately.
/// This Field never sends a `Prefer: outlook.timezone` header (the transport
/// exposes no way to set one, and this Field does not need one), so Graph's
/// documented default applies and every instant arrives already in `"UTC"`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphDateTimeTimeZone {
    #[serde(rename = "dateTime")]
    pub(crate) date_time: String,
    #[serde(rename = "timeZone")]
    pub(crate) time_zone: String,
}

/// One Graph `responseStatus` value: the signed-in mailbox's own RSVP.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphResponseStatus {
    #[serde(default)]
    pub(crate) response: Option<String>,
}

/// Why a delta item carried no content.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphRemoved {
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "decoded for completeness and future diagnostics; every removal this Field \
                   observes is treated identically regardless of the stated reason"
    )]
    pub(crate) reason: Option<String>,
}

/// One item from a `calendarView/delta` page: either a present event or a
/// `@removed` tombstone marker.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphEvent {
    pub(crate) id: String,
    #[serde(rename = "@removed", default)]
    pub(crate) removed: Option<GraphRemoved>,
    #[serde(default)]
    pub(crate) subject: Option<String>,
    #[serde(default)]
    pub(crate) start: Option<GraphDateTimeTimeZone>,
    #[serde(default)]
    pub(crate) end: Option<GraphDateTimeTimeZone>,
    #[serde(rename = "isAllDay", default)]
    pub(crate) is_all_day: Option<bool>,
    #[serde(rename = "isCancelled", default)]
    pub(crate) is_cancelled: Option<bool>,
    #[serde(default)]
    pub(crate) organizer: Option<GraphRecipient>,
    #[serde(default)]
    pub(crate) attendees: Option<Vec<GraphAttendee>>,
    #[serde(rename = "type", default)]
    pub(crate) event_type: Option<String>,
    #[serde(rename = "seriesMasterId", default)]
    pub(crate) series_master_id: Option<String>,
    #[serde(rename = "webLink", default)]
    pub(crate) web_link: Option<String>,
    #[serde(rename = "changeKey", default)]
    pub(crate) change_key: Option<String>,
    #[serde(rename = "responseStatus", default)]
    pub(crate) response_status: Option<GraphResponseStatus>,
}

impl GraphEvent {
    /// Whether this item is an authoritative Graph delta removal rather than
    /// a present event.
    #[must_use]
    pub(crate) fn is_removed(&self) -> bool {
        self.removed.is_some()
    }
}

/// Builds the initial `calendarView/delta` request for `window`, with this
/// Field's fixed field selection.
///
/// Built as one hand-assembled resource string rather than through
/// [`GraphRequest`]'s query builder, because `startDateTime` and
/// `endDateTime` are bare OData parameters `GraphRequest` has no method for,
/// and because [`GraphRequest::into_delta`] is private to `fieldnotes-msgraph`
/// and only reachable through [`fieldnotes_msgraph::GraphClient::delta`],
/// which appends `/delta` *after* whatever resource path it is given --
/// appending it after an already-embedded query string would put the
/// segment in the wrong place. Passing the request built here through
/// [`fieldnotes_msgraph::GraphClient::list`] instead works correctly: list
/// and delta share the exact same paging/delta-follow logic, so the
/// resulting [`fieldnotes_msgraph::PageStream::delta_token`] behaves
/// identically either way.
#[must_use]
pub(crate) fn initial_delta_request(
    window_from: OffsetDatetime,
    window_to: OffsetDatetime,
) -> GraphRequest {
    let select = SELECT_FIELDS.join(",");
    let query = format!(
        "startDateTime={}&endDateTime={}&$select={}",
        fieldnotes_field_sdk::percent::encode(&window_from.to_string()),
        fieldnotes_field_sdk::percent::encode(&window_to.to_string()),
        fieldnotes_field_sdk::percent::encode(&select),
    );
    GraphRequest::new(format!("/me/calendarView/delta?{query}"))
}

#[cfg(test)]
mod tests {
    use super::{GraphEvent, initial_delta_request};
    use fieldnotes_field_protocol::grammar::OffsetDatetime;
    use fieldnotes_msgraph::testing::{FakeRetryClock, ScriptedTransport, json_response};
    use fieldnotes_msgraph::transport::{
        GraphHttpRequest, GraphHttpResponse, HttpTransport, TransportError,
    };
    use fieldnotes_msgraph::{AccessToken, GraphClient};
    use fieldnotes_test_support::CountingRandom;
    use std::rc::Rc;

    fn datetime(text: &str) -> OffsetDatetime {
        OffsetDatetime::parse(text).unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    /// Shares one [`ScriptedTransport`] with the test after [`GraphClient`]
    /// has taken ownership of a transport, so the exact URL it requested can
    /// still be inspected -- the only way to observe what
    /// [`initial_delta_request`] built, since
    /// [`fieldnotes_msgraph::GraphRequest::into_url`] is private to that
    /// crate and reachable only through a real request.
    struct SharedTransport(Rc<ScriptedTransport>);

    impl HttpTransport for SharedTransport {
        fn execute(&self, request: &GraphHttpRequest) -> Result<GraphHttpResponse, TransportError> {
            self.0.execute(request)
        }
    }

    #[test]
    fn the_initial_request_embeds_delta_before_the_query_and_encodes_the_window() {
        let inner = Rc::new(ScriptedTransport::new(vec![json_response(
            200,
            r#"{"value":[]}"#,
        )]));
        let client = GraphClient::new(
            SharedTransport(Rc::clone(&inner)),
            FakeRetryClock::new(0),
            CountingRandom::new(0),
        );
        let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN");
        let request = initial_delta_request(
            datetime("2026-08-22T00:00:00+00:00"),
            datetime("2026-08-29T00:00:00+00:00"),
        );
        let collected: Vec<_> = client
            .list::<GraphEvent>(&token, request, "list calendar events (initial)")
            .collect();
        assert_eq!(collected.len(), 0);

        let urls = inner.requested_urls();
        assert_eq!(urls.len(), 1);
        let url = &urls[0];
        assert!(url.starts_with("https://graph.microsoft.com/v1.0/me/calendarView/delta?"));
        assert!(url.contains("startDateTime=2026-08-22T00%3a00%3a00%2b00%3a00"));
        assert!(url.contains("endDateTime=2026-08-29T00%3a00%3a00%2b00%3a00"));
        assert!(url.contains("$select=id%2csubject"));
    }

    #[test]
    fn a_removed_item_decodes_with_no_other_content_required() {
        let json = serde_json::json!({
            "id": "AAMkAGI2EVENT01",
            "@removed": {"reason": "deleted"}
        });
        let event: GraphEvent =
            serde_json::from_value(json).unwrap_or_else(|error| panic!("must decode: {error}"));
        assert!(event.is_removed());
    }

    #[test]
    fn a_present_event_decodes_its_full_shape() {
        let json = serde_json::json!({
            "id": "AAMkAGI2EVENT01",
            "subject": "Migration planning",
            "start": {"dateTime": "2026-08-22T09:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-08-22T09:45:00.0000000", "timeZone": "UTC"},
            "isAllDay": false,
            "isCancelled": false,
            "organizer": {"emailAddress": {"name": "Sam", "address": "sam@example.net"}},
            "attendees": [
                {"emailAddress": {"name": "Alice", "address": "alice@example.com"}}
            ],
            "type": "singleInstance",
            "webLink": "https://outlook.office.com/calendar/item/AAMkAGI2EVENT01",
            "changeKey": "calendar-version-1",
            "responseStatus": {"response": "accepted"}
        });
        let event: GraphEvent =
            serde_json::from_value(json).unwrap_or_else(|error| panic!("must decode: {error}"));
        assert!(!event.is_removed());
        assert_eq!(event.subject.as_deref(), Some("Migration planning"));
        assert_eq!(event.event_type.as_deref(), Some("singleInstance"));
    }
}
