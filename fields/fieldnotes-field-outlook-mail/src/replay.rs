//! Fixture-replay mode: answering Graph reads from sanitized recorded
//! responses on disk, with no network and no token.
//!
//! `docs/roadmap.md` and `AGENTS.md` both require recorded, sanitized fixtures
//! over live-account dependencies in ordinary tests, and A2's release gates
//! require this Field to be exercised as a **real child process** through the
//! conformance kit. Those two requirements meet here: the conformance kit
//! spawns this binary and can set one environment entry, so the binary itself
//! must be able to read its Graph responses from a recorded script instead of
//! from Microsoft.
//!
//! # This is not a secret channel
//!
//! [`crate::constants::FIXTURE_SCRIPT_VARIABLE`] names a **file of recorded
//! JSON responses**. No token, no tenant, and no address is passed through the
//! environment, and in fixture mode this Field asks for no credential grant at
//! all, because a replayed response needs no authorization. The placeholder
//! token the client is constructed with is a compile-time constant in this
//! file and is never read from anywhere.
//!
//! # The script format
//!
//! The script file is an ordered list of exchanges, and each `body_file` is a
//! plain file name resolved beside it:
//!
//! ```json
//! [
//!   { "expect_url_contains": "/messages/delta", "status": 200, "body_file": "delta-page-1.json" },
//!   { "status": 429, "retry_after_seconds": 1, "body_file": "throttled.json" }
//! ]
//! ```
//!
//! Responses are served in order. When an entry names `expect_url_contains`
//! and the request's URL does not contain it, the transport answers with a
//! transport failure naming the mismatch rather than serving the wrong body:
//! a fixture that silently answers the wrong request would make a passing test
//! meaningless.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use fieldnotes_msgraph::{
    AccessToken, GraphHttpRequest, GraphHttpResponse, HttpTransport, RetryClock, TransportError,
};
use serde::Deserialize;

/// The non-secret placeholder token fixture mode presents.
///
/// Recorded responses need no authorization, so this value authorizes nothing
/// and is deliberately self-describing if it ever appears anywhere.
pub(crate) const PLACEHOLDER_TOKEN: &str = "FIXTURE-NOT-A-REAL-TOKEN-outlook-mail-replay";

/// One scripted exchange.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    /// A substring the requested URL must contain, when the fixture asserts
    /// one.
    #[serde(default)]
    expect_url_contains: Option<String>,
    /// The HTTP status to answer with.
    status: u16,
    /// The file beside the script holding the response body.
    #[serde(default)]
    body_file: Option<String>,
    /// The `Retry-After` header to answer with, in seconds.
    #[serde(default)]
    retry_after_seconds: Option<u64>,
    /// Answer with a transport-level failure instead of a response.
    #[serde(default)]
    transport_error: Option<String>,
}

/// Why a fixture script could not be loaded.
#[derive(Debug)]
pub(crate) struct ReplayError(String);

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the Graph fixture script is unusable: {}", self.0)
    }
}

impl std::error::Error for ReplayError {}

/// An [`HttpTransport`] that answers a recorded script instead of the network.
pub(crate) struct ReplayTransport {
    exchanges: RefCell<VecDeque<(Exchange, Vec<u8>)>>,
}

impl ReplayTransport {
    /// Loads `script_path` and every body file it names from beside it.
    pub(crate) fn load(script_path: &Path) -> Result<Self, ReplayError> {
        let script = std::fs::read_to_string(script_path)
            .map_err(|error| ReplayError(format!("{}: {error}", script_path.display())))?;
        let exchanges: Vec<Exchange> = serde_json::from_str(&script)
            .map_err(|error| ReplayError(format!("{}: {error}", script_path.display())))?;
        let directory = script_path.parent().unwrap_or(Path::new("."));
        let mut loaded = VecDeque::with_capacity(exchanges.len());
        for exchange in exchanges {
            let body = match &exchange.body_file {
                Some(name) => {
                    // A fixture body is named, never composed from a path: a
                    // fixture script is developer-authored, but treating the
                    // name as a bare file name costs nothing.
                    if name.contains('/') || name.contains('\\') || name.contains("..") {
                        return Err(ReplayError(format!(
                            "{name:?} must be a plain file name beside the fixture script"
                        )));
                    }
                    std::fs::read(directory.join(name))
                        .map_err(|error| ReplayError(format!("{name}: {error}")))?
                }
                None => Vec::new(),
            };
            loaded.push_back((exchange, body));
        }
        Ok(ReplayTransport {
            exchanges: RefCell::new(loaded),
        })
    }
}

impl HttpTransport for ReplayTransport {
    fn execute(&self, request: &GraphHttpRequest) -> Result<GraphHttpResponse, TransportError> {
        // A mismatch deliberately does **not** consume the exchange, so the
        // transport's own retry of the failed call reports the same mismatch
        // rather than "script exhausted": a fixture must fail with the reason
        // it actually disagreed about.
        {
            let queue = self.exchanges.borrow();
            let Some((next, _)) = queue.front() else {
                return Err(TransportError::new(
                    "the Graph fixture script is exhausted: this run made more requests than the \
                     recorded exchange list covers",
                ));
            };
            if let Some(expected) = &next.expect_url_contains
                && !request.url().contains(expected.as_str())
            {
                return Err(TransportError::new(format!(
                    "the next recorded exchange expects a URL containing {expected:?}, which this \
                     request's URL does not"
                )));
            }
        }
        let Some((exchange, body)) = self.exchanges.borrow_mut().pop_front() else {
            return Err(TransportError::new("the Graph fixture script is exhausted"));
        };
        if let Some(reason) = &exchange.transport_error {
            return Err(TransportError::new(reason.clone()));
        }
        let mut headers = vec![("Content-Type".to_owned(), "application/json".to_owned())];
        if let Some(seconds) = exchange.retry_after_seconds {
            headers.push(("Retry-After".to_owned(), seconds.to_string()));
        }
        Ok(GraphHttpResponse::new(exchange.status, headers, body))
    }
}

/// A [`RetryClock`] that advances a virtual clock instead of sleeping.
///
/// Fixture mode must exercise the transport's real throttling path -- honoring
/// `Retry-After`, then retrying -- without a child process that actually
/// blocks for the requested delay, which would make a conformance case slow
/// and its idle bound flaky.
pub(crate) struct VirtualClock {
    now_millis: Cell<u64>,
}

impl VirtualClock {
    /// A clock starting at tick zero.
    pub(crate) fn new() -> Self {
        VirtualClock {
            now_millis: Cell::new(0),
        }
    }
}

impl RetryClock for VirtualClock {
    fn now_millis(&self) -> u64 {
        self.now_millis.get()
    }

    fn sleep(&self, duration: Duration) {
        let advanced = self
            .now_millis
            .get()
            .saturating_add(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
        self.now_millis.set(advanced);
    }
}

/// A deterministic byte source for retry jitter in fixture mode.
pub(crate) struct FixedJitter;

impl fieldnotes_domain::RandomSource for FixedJitter {
    fn fill_bytes(&mut self, buffer: &mut [u8]) {
        buffer.fill(0x40);
    }
}

/// The placeholder token fixture mode presents to the transport.
#[must_use]
pub(crate) fn placeholder_token() -> AccessToken {
    AccessToken::new(PLACEHOLDER_TOKEN)
}

#[cfg(test)]
mod tests {
    use super::{FixedJitter, ReplayTransport, VirtualClock, placeholder_token};
    use fieldnotes_msgraph::{GraphClient, GraphError, GraphRequest, RetryClock};
    use std::time::Duration;

    fn script(scenario: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("graph")
            .join(format!("script-{scenario}.json"))
    }

    /// A client over the recorded script, so every assertion below runs
    /// through the same request-building path a real run uses. The transport's
    /// own request type is not constructible from outside that crate, which is
    /// exactly why this goes through the client.
    fn client(scenario: &str) -> GraphClient<ReplayTransport, VirtualClock, FixedJitter> {
        let transport = ReplayTransport::load(&script(scenario))
            .unwrap_or_else(|error| panic!("must load: {error}"));
        GraphClient::new(transport, VirtualClock::new(), FixedJitter)
    }

    #[derive(Debug, serde::Deserialize)]
    struct AnyEnvelope {
        #[serde(default)]
        value: Vec<serde_json::Value>,
    }

    #[test]
    fn a_recorded_script_answers_the_request_the_fixture_expected() {
        let client = client("window");
        let request = GraphRequest::new("/me/mailFolders('inbox')/messages")
            .top(50)
            .filter("receivedDateTime ge 2026-08-16T00:00:00+00:00")
            .unwrap_or_else(|error| panic!("must build: {error}"));
        let envelope: AnyEnvelope = client
            .get(&placeholder_token(), request, "test read")
            .unwrap_or_else(|error| panic!("must answer: {error}"));
        assert!(!envelope.value.is_empty());
    }

    #[test]
    fn a_url_the_fixture_did_not_expect_fails_loudly_rather_than_serving_the_wrong_body() {
        // A transport failure is retried by policy and then surfaces as
        // `Transport`, which is what a fixture mismatch should look like: an
        // unmistakable failure, never a wrong body served quietly.
        let client = client("window");
        match client.get::<AnyEnvelope>(
            &placeholder_token(),
            GraphRequest::new("/me/contacts"),
            "test read",
        ) {
            Err(GraphError::Transport { reason, .. }) => {
                assert!(reason.contains("expects a URL containing"), "{reason}");
            }
            other => panic!("a mismatched URL must not be answered: {other:?}"),
        }
    }

    #[test]
    fn a_missing_fixture_script_is_reported_rather_than_panicking() {
        let error = match ReplayTransport::load(std::path::Path::new("/nonexistent-fixture.json")) {
            Err(error) => error,
            Ok(_) => panic!("a missing script cannot load"),
        };
        assert!(error.to_string().contains("unusable"));
    }

    #[test]
    fn a_body_file_that_tries_to_leave_the_fixture_directory_is_refused() {
        let error = match ReplayTransport::load(&script("hostile")) {
            Err(error) => error,
            Ok(_) => panic!("a traversal-shaped body file must be refused"),
        };
        assert!(error.to_string().contains("plain file name"));
    }

    #[test]
    fn the_virtual_clock_advances_without_sleeping() {
        let clock = VirtualClock::new();
        clock.sleep(Duration::from_secs(30));
        assert_eq!(clock.now_millis(), 30_000);
    }
}
