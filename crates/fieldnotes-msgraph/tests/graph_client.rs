//! Fixture-driven conformance tests for [`fieldnotes_msgraph::GraphClient`].
//!
//! Every fixture under `tests/fixtures/` is hand-written, sanitized JSON:
//! fake message IDs (`AAMkFIXTURE...`), a fake service host
//! (`graph.example.test`), fake GUIDs, and no token anywhere. None of these
//! tests touch a network, a tenant, or a real credential; the retry tests
//! run instantly because [`fieldnotes_msgraph::testing::FakeRetryClock`]
//! advances a virtual clock instead of sleeping.

use std::time::Duration;

use fieldnotes_msgraph::testing::{
    FakeRetryClock, ScriptedTransport, json_response, json_response_with_retry_after,
};
use fieldnotes_msgraph::{
    AccessToken, DeltaStart, GraphClient, GraphError, GraphRequest, RetryPolicy,
};
use fieldnotes_test_support::CountingRandom;
use serde::Deserialize;

const BASE_URL: &str = "https://graph.example.test/v1.0";

#[derive(Debug, Deserialize)]
struct MessageFixture {
    id: String,
    subject: String,
}

fn client(
    transport: ScriptedTransport,
) -> GraphClient<ScriptedTransport, FakeRetryClock, CountingRandom> {
    GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1))
        .with_base_url(BASE_URL)
}

fn client_with_policy(
    transport: ScriptedTransport,
    policy: RetryPolicy,
) -> GraphClient<ScriptedTransport, FakeRetryClock, CountingRandom> {
    client(transport).with_retry_policy(policy)
}

/// Collects a `PageStream` into `(items, error)`, where `error` is set only
/// if the stream ended with one.
fn drain<I>(stream: impl Iterator<Item = Result<I, GraphError>>) -> (Vec<I>, Option<GraphError>) {
    let mut items = Vec::new();
    for outcome in stream {
        match outcome {
            Ok(item) => items.push(item),
            Err(error) => return (items, Some(error)),
        }
    }
    (items, None)
}

#[test]
fn pagination_follows_next_link_across_several_pages_one_page_at_a_time() {
    let transport = ScriptedTransport::new(vec![
        json_response(200, include_str!("fixtures/mail_page_1.json")),
        json_response(200, include_str!("fixtures/mail_page_2.json")),
        json_response(200, include_str!("fixtures/mail_page_3_final.json")),
    ]);
    let graph = client(transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-pagination");

    let stream = graph.list::<MessageFixture>(
        &token,
        GraphRequest::new("/me/messages"),
        "list mail messages",
    );
    let (items, error) = drain(stream);

    assert!(error.is_none(), "unexpected error: {error:?}");
    let subjects: Vec<&str> = items.iter().map(|item| item.subject.as_str()).collect();
    assert_eq!(
        subjects,
        vec![
            "Weekly status",
            "Lunch plans",
            "Project update",
            "Meeting notes",
            "Final reminder"
        ]
    );
    assert_eq!(items[0].id, "AAMkFIXTURE0001");
    assert_eq!(items[4].id, "AAMkFIXTURE0005");
}

#[test]
fn a_delta_collection_can_be_started_then_resumed_from_its_persisted_token() {
    let initial_transport = ScriptedTransport::new(vec![json_response(
        200,
        include_str!("fixtures/delta_initial.json"),
    )]);
    let graph = client(initial_transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-delta");

    let mut stream = graph.delta::<MessageFixture>(
        &token,
        DeltaStart::Initial(GraphRequest::new("/me/messages")),
        "initial mail delta",
    );
    let mut first_run_items = Vec::new();
    for outcome in &mut stream {
        match outcome {
            Ok(item) => first_run_items.push(item),
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
    assert_eq!(first_run_items.len(), 2);
    // The delta token from the first run is exactly what a Field persists
    // in its own cursor state and resumes from on the next sync.
    let delta_token = match stream.delta_token() {
        Some(delta_token) => delta_token.clone(),
        None => panic!("expected a delta token after the initial run"),
    };

    let resume_transport = ScriptedTransport::new(vec![json_response(
        200,
        include_str!("fixtures/delta_resume.json"),
    )]);
    let resumed_graph = client(resume_transport);
    let resumed_stream = resumed_graph.delta::<MessageFixture>(
        &token,
        DeltaStart::Resume(delta_token),
        "resume mail delta",
    );
    let (second_run_items, error) = drain(resumed_stream);
    assert!(error.is_none(), "unexpected error: {error:?}");
    assert_eq!(second_run_items.len(), 1);
    assert_eq!(second_run_items[0].subject, "New message since last sync");
}

#[test]
fn throttling_honors_retry_after_then_succeeds() {
    let transport = ScriptedTransport::new(vec![
        json_response_with_retry_after(429, 2, include_str!("fixtures/error_429_throttled.json")),
        json_response(200, include_str!("fixtures/single_message_page.json")),
    ]);
    let graph = client(transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-throttle");

    let stream = graph.list::<MessageFixture>(
        &token,
        GraphRequest::new("/me/messages"),
        "list mail messages",
    );
    let (items, error) = drain(stream);

    assert!(error.is_none(), "unexpected error: {error:?}");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].subject, "Delivered after retry");
}

#[test]
fn a_transient_server_fault_is_retried_and_then_succeeds() {
    let transport = ScriptedTransport::new(vec![
        json_response(
            503,
            include_str!("fixtures/error_503_service_unavailable.json"),
        ),
        json_response(200, include_str!("fixtures/single_message_page.json")),
    ]);
    let graph = client(transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-503");

    let stream = graph.list::<MessageFixture>(
        &token,
        GraphRequest::new("/me/messages"),
        "list mail messages",
    );
    let (items, error) = drain(stream);

    assert!(error.is_none(), "unexpected error: {error:?}");
    assert_eq!(items.len(), 1);
}

#[test]
fn persistent_throttling_is_bounded_by_max_attempts_and_surfaces_throttled() {
    let transport = ScriptedTransport::new(vec![
        json_response_with_retry_after(429, 0, include_str!("fixtures/error_429_throttled.json")),
        json_response_with_retry_after(429, 0, include_str!("fixtures/error_429_throttled.json")),
        json_response_with_retry_after(429, 0, include_str!("fixtures/error_429_throttled.json")),
    ]);
    let policy = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_millis(10),
        Duration::from_secs(60),
    );
    let graph = client_with_policy(transport, policy);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-exhausted");

    let stream = graph.list::<MessageFixture>(
        &token,
        GraphRequest::new("/me/messages"),
        "list mail messages",
    );
    let (items, error) = drain(stream);

    assert!(items.is_empty());
    match error {
        Some(GraphError::Throttled(detail)) => assert_eq!(detail.code(), Some("TooManyRequests")),
        other => panic!("expected Throttled, got {other:?}"),
    }
}

#[test]
fn persistent_service_faults_give_up_once_the_total_elapsed_budget_is_exhausted() {
    let transport = ScriptedTransport::new(vec![
        json_response(
            503,
            include_str!("fixtures/error_503_service_unavailable.json"),
        ),
        json_response(
            503,
            include_str!("fixtures/error_503_service_unavailable.json"),
        ),
        json_response(
            503,
            include_str!("fixtures/error_503_service_unavailable.json"),
        ),
        json_response(
            503,
            include_str!("fixtures/error_503_service_unavailable.json"),
        ),
    ]);
    // A generous attempt count but a tiny total-elapsed budget: the budget,
    // not the attempt count, should be what ends the loop.
    let policy = RetryPolicy::new(
        50,
        Duration::from_secs(10),
        Duration::from_secs(30),
        Duration::from_millis(5),
    );
    let graph = client_with_policy(transport, policy);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-elapsed");

    let stream = graph.list::<MessageFixture>(
        &token,
        GraphRequest::new("/me/messages"),
        "list mail messages",
    );
    let (items, error) = drain(stream);

    assert!(items.is_empty());
    assert!(matches!(error, Some(GraphError::ServiceUnavailable(_))));
}

#[test]
fn an_expired_token_response_is_never_retried_and_is_actionable() {
    let transport = ScriptedTransport::new(vec![json_response(
        401,
        include_str!("fixtures/error_401_expired_token.json"),
    )]);
    let graph = client(transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-expired");

    let result: Result<MessageFixture, GraphError> =
        graph.get(&token, GraphRequest::new("/me"), "get my profile");

    match result {
        Err(GraphError::ReauthenticationRequired(detail)) => {
            assert_eq!(detail.code(), Some("InvalidAuthenticationToken"));
            assert_eq!(detail.status(), 401);
        }
        other => panic!("expected ReauthenticationRequired, got {other:?}"),
    }
}

#[test]
fn a_consent_denied_response_is_never_retried_and_is_actionable() {
    let transport = ScriptedTransport::new(vec![json_response(
        403,
        include_str!("fixtures/error_403_consent_denied.json"),
    )]);
    let graph = client(transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-consent");

    let result: Result<MessageFixture, GraphError> =
        graph.get(&token, GraphRequest::new("/me"), "get my profile");

    match result {
        Err(GraphError::PermissionDenied(detail)) => {
            assert_eq!(detail.code(), Some("ErrorAccessDenied"));
            assert_eq!(detail.status(), 403);
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

#[test]
fn a_next_link_outside_the_configured_authority_is_refused_before_it_is_requested() {
    let transport = ScriptedTransport::new(vec![
        json_response(
            200,
            include_str!("fixtures/mail_page_hostile_next_link.json"),
        ),
        // A second scripted response proves this is never reached: if the
        // client followed the hostile link, this would be consumed.
        json_response(200, include_str!("fixtures/mail_page_3_final.json")),
    ]);
    let graph = client(transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-hostile");

    let stream = graph.list::<MessageFixture>(
        &token,
        GraphRequest::new("/me/messages"),
        "list mail messages",
    );
    let (items, error) = drain(stream);

    assert_eq!(
        items.len(),
        1,
        "the first (trusted) page's item is still yielded"
    );
    assert!(matches!(
        error,
        Some(GraphError::UntrustedContinuation { .. })
    ));
}

#[test]
fn an_oversized_response_is_rejected_rather_than_partially_parsed() {
    let huge_subject = "x".repeat(1024);
    let body =
        format!("{{\"value\":[{{\"id\":\"AAMkFIXTUREBIG\",\"subject\":\"{huge_subject}\"}}]}}");
    let transport = ScriptedTransport::new(vec![json_response(200, &body)]);
    let graph = client(transport).with_max_response_bytes(64);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-oversized");

    let result: Result<MessageFixture, GraphError> = graph.get(
        &token,
        GraphRequest::new("/me/messages/1"),
        "get one message",
    );

    assert!(matches!(result, Err(GraphError::MalformedResponse { .. })));
}

#[test]
fn a_malformed_body_is_rejected_rather_than_partially_parsed() {
    let transport = ScriptedTransport::new(vec![json_response(200, "{ not json")]);
    let graph = client(transport);
    let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-malformed");

    let result: Result<MessageFixture, GraphError> = graph.get(
        &token,
        GraphRequest::new("/me/messages/1"),
        "get one message",
    );

    assert!(matches!(result, Err(GraphError::MalformedResponse { .. })));
}

#[test]
fn a_token_leaked_into_a_transport_failure_message_is_still_redacted() {
    // A well-behaved transport never does this; this simulates a buggy one
    // to prove `GraphClient`'s registered-secret redaction is genuine
    // defense in depth, not merely "our own code never does it".
    let canary = "FIXTURE-NOT-A-REAL-TOKEN-canary-9c2f4a11";
    let transport = ScriptedTransport::new(vec![Err(fieldnotes_msgraph::TransportError::new(
        format!("connect failed while sending Bearer {canary}"),
    ))]);
    let graph = client(transport);
    let token = AccessToken::new(canary);

    let result: Result<MessageFixture, GraphError> =
        graph.get(&token, GraphRequest::new("/me"), "get my profile");

    let debug_form = format!("{result:?}");
    let display_form = match &result {
        Err(error) => error.to_string(),
        Ok(_) => panic!("expected the scripted transport failure to surface as an error"),
    };
    assert!(
        !debug_form.contains(canary),
        "leaked in Debug: {debug_form}"
    );
    assert!(
        !display_form.contains(canary),
        "leaked in Display: {display_form}"
    );
}
