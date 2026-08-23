//! Fixture-driven test doubles for this crate's own tests, and for any
//! downstream Field crate that wants to exercise `fieldnotes-msgraph`
//! against recorded, sanitized Graph responses instead of a live tenant.
//!
//! `docs/roadmap.md` and `AGENTS.md` both require recorded, sanitized
//! fixtures over live-account dependencies in ordinary tests. This module is
//! the seam that makes that possible: [`ScriptedTransport`] answers a fixed,
//! ordered script of responses instead of making a network call, and
//! [`FakeRetryClock`] advances a virtual clock instead of blocking a test
//! thread on a real sleep.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::Duration;

use crate::clock::RetryClock;
use crate::transport::{GraphHttpRequest, GraphHttpResponse, HttpTransport, TransportError};

/// An [`HttpTransport`] that answers a fixed, ordered script of responses.
///
/// Every call to [`HttpTransport::execute`] pops the next scripted outcome;
/// calling it more times than the script provides is a test bug, reported as
/// a [`TransportError`] rather than a panic. Every requested URL is recorded
/// in order, so a test can assert exactly which pages were fetched (and, for
/// the authority-check tests, that a rejected continuation was never
/// requested at all).
pub struct ScriptedTransport {
    responses: RefCell<VecDeque<Result<GraphHttpResponse, TransportError>>>,
    requested_urls: RefCell<Vec<String>>,
}

impl ScriptedTransport {
    /// Builds a transport that answers `responses` in order, one per call.
    #[must_use]
    pub fn new(responses: Vec<Result<GraphHttpResponse, TransportError>>) -> Self {
        ScriptedTransport {
            responses: RefCell::new(responses.into_iter().collect()),
            requested_urls: RefCell::new(Vec::new()),
        }
    }

    /// Every URL requested so far, in request order.
    #[must_use]
    pub fn requested_urls(&self) -> Vec<String> {
        self.requested_urls.borrow().clone()
    }
}

impl HttpTransport for ScriptedTransport {
    fn execute(&self, request: &GraphHttpRequest) -> Result<GraphHttpResponse, TransportError> {
        self.requested_urls
            .borrow_mut()
            .push(request.url().to_owned());
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(TransportError::new("scripted transport script exhausted")))
    }
}

/// Builds a successful JSON response.
pub fn json_response(status: u16, body: &str) -> Result<GraphHttpResponse, TransportError> {
    Ok(GraphHttpResponse::new(
        status,
        vec![("Content-Type".to_owned(), "application/json".to_owned())],
        body.as_bytes().to_vec(),
    ))
}

/// Builds a response carrying a `Retry-After` header, in seconds.
pub fn json_response_with_retry_after(
    status: u16,
    retry_after_seconds: u64,
    body: &str,
) -> Result<GraphHttpResponse, TransportError> {
    Ok(GraphHttpResponse::new(
        status,
        vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Retry-After".to_owned(), retry_after_seconds.to_string()),
        ],
        body.as_bytes().to_vec(),
    ))
}

/// A [`RetryClock`] that advances a virtual clock instead of sleeping, and
/// records every requested sleep so a test can assert the backoff sequence
/// without waiting for it in real time.
pub struct FakeRetryClock {
    now_millis: Cell<u64>,
    sleeps: RefCell<Vec<Duration>>,
}

impl FakeRetryClock {
    /// A clock starting at tick `start_millis`.
    #[must_use]
    pub fn new(start_millis: u64) -> Self {
        FakeRetryClock {
            now_millis: Cell::new(start_millis),
            sleeps: RefCell::new(Vec::new()),
        }
    }

    /// Every duration [`RetryClock::sleep`] was asked to wait, in call
    /// order.
    #[must_use]
    pub fn sleeps(&self) -> Vec<Duration> {
        self.sleeps.borrow().clone()
    }
}

impl RetryClock for FakeRetryClock {
    fn now_millis(&self) -> u64 {
        self.now_millis.get()
    }

    fn sleep(&self, duration: Duration) {
        self.sleeps.borrow_mut().push(duration);
        let advanced = self
            .now_millis
            .get()
            .saturating_add(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
        self.now_millis.set(advanced);
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeRetryClock, ScriptedTransport, json_response};
    use crate::clock::RetryClock;
    use crate::transport::{GraphHttpRequest, HttpTransport};

    #[test]
    fn scripted_transport_answers_in_order_and_records_urls() {
        let transport = ScriptedTransport::new(vec![
            json_response(200, "{\"value\":[1]}"),
            json_response(200, "{\"value\":[2]}"),
        ]);
        let first =
            match transport.execute(&GraphHttpRequest::new("https://graph.example/a".to_owned())) {
                Ok(response) => response,
                Err(error) => panic!("expected a scripted response, got {error}"),
            };
        let second =
            match transport.execute(&GraphHttpRequest::new("https://graph.example/b".to_owned())) {
                Ok(response) => response,
                Err(error) => panic!("expected a scripted response, got {error}"),
            };
        assert_eq!(first.status(), 200);
        assert_eq!(second.status(), 200);
        assert_eq!(
            transport.requested_urls(),
            vec!["https://graph.example/a", "https://graph.example/b"]
        );
    }

    #[test]
    fn fake_retry_clock_advances_by_the_requested_sleep() {
        let clock = FakeRetryClock::new(1_000);
        clock.sleep(std::time::Duration::from_millis(250));
        assert_eq!(clock.now_millis(), 1_250);
        assert_eq!(clock.sleeps(), vec![std::time::Duration::from_millis(250)]);
    }
}
