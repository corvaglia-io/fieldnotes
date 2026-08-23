//! The Graph client: request execution, retry/backoff, and error
//! classification.
//!
//! [`GraphClient`] is the one type a Field author constructs. It borrows an
//! [`HttpTransport`] to reach the network, a [`RetryClock`] to measure and
//! wait out backoff, and a [`RandomSource`] to jitter it, and turns those
//! into `GET`-only, retried, classified, size-bounded Graph requests.

use std::cell::RefCell;
use std::time::Duration;

use fieldnotes_domain::RandomSource;
use fieldnotes_field_protocol::redact::Redactor;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::clock::RetryClock;
use crate::error::{GraphError, GraphErrorDetail, MAX_MESSAGE_LEN};
use crate::page::{DeltaStart, PageStream};
use crate::request::GraphRequest;
use crate::token::AccessToken;
use crate::transport::{GraphHttpRequest, GraphHttpResponse, HttpTransport};

/// The default Microsoft Graph v1.0 service root.
pub const DEFAULT_BASE_URL: &str = "https://graph.microsoft.com/v1.0";

/// A conservative default cap on a single response body, applied again here
/// as defense in depth even though [`crate::transport::UreqTransport`]
/// already enforces one independent of `Content-Length`.
const DEFAULT_MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Retry and backoff bounds for one logical Graph request (a single `GET`,
/// or one page fetch within a [`PageStream`]).
///
/// Every bound is explicit and finite:
///
/// - `max_attempts` caps the number of HTTP calls made for one logical
///   request, counting the first;
/// - `base_delay` and `max_delay` bound the exponential-with-full-jitter
///   backoff used when the server did not specify a wait via `Retry-After`;
/// - `max_elapsed` bounds the *total* wall-clock time spent retrying,
///   including any `Retry-After` this crate chose to honor. If honoring a
///   server-requested wait would exceed the remaining budget, this crate
///   gives up rather than oversleeping past it.
///
/// Both bounds apply independently; whichever is reached first ends the
/// retry sequence.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    max_attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
    max_elapsed: Duration,
}

impl RetryPolicy {
    /// A policy with explicit bounds.
    #[must_use]
    pub fn new(
        max_attempts: u32,
        base_delay: Duration,
        max_delay: Duration,
        max_elapsed: Duration,
    ) -> Self {
        RetryPolicy {
            max_attempts: max_attempts.max(1),
            base_delay,
            max_delay,
            max_elapsed,
        }
    }
}

impl Default for RetryPolicy {
    /// Five attempts, starting at 250ms and capped at 30s per attempt, with
    /// a two-minute total budget.
    fn default() -> Self {
        RetryPolicy::new(
            5,
            Duration::from_millis(250),
            Duration::from_secs(30),
            Duration::from_secs(120),
        )
    }
}

/// Whether Graph's own error envelope classifies a status as retryable, and
/// which [`GraphError`] variant a terminal or exhausted failure becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    ReauthenticationRequired,
    PermissionDenied,
    Throttled,
    ServiceUnavailable,
    InvalidRequest,
}

impl ErrorKind {
    fn classify(status: u16) -> Self {
        match status {
            401 => ErrorKind::ReauthenticationRequired,
            403 => ErrorKind::PermissionDenied,
            429 => ErrorKind::Throttled,
            500 | 502 | 503 | 504 => ErrorKind::ServiceUnavailable,
            _ => ErrorKind::InvalidRequest,
        }
    }

    fn is_retryable(self) -> bool {
        matches!(self, ErrorKind::Throttled | ErrorKind::ServiceUnavailable)
    }

    fn into_error(self, detail: GraphErrorDetail) -> GraphError {
        match self {
            ErrorKind::ReauthenticationRequired => GraphError::ReauthenticationRequired(detail),
            ErrorKind::PermissionDenied => GraphError::PermissionDenied(detail),
            ErrorKind::Throttled => GraphError::Throttled(detail),
            ErrorKind::ServiceUnavailable => GraphError::ServiceUnavailable(detail),
            ErrorKind::InvalidRequest => GraphError::InvalidRequest(detail),
        }
    }
}

/// Graph's JSON error envelope, parsed leniently: any field may be absent,
/// and a body that is not this shape at all simply yields `None` rather
/// than a parse error, since a Graph error's own body is not itself load
/// bearing for classification (the HTTP status already is).
#[derive(Deserialize, Default)]
struct GraphErrorEnvelope {
    #[serde(default)]
    error: GraphErrorObject,
}

#[derive(Deserialize, Default)]
struct GraphErrorObject {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(rename = "innerError", default)]
    inner_error: Option<GraphInnerError>,
}

#[derive(Deserialize, Default)]
struct GraphInnerError {
    #[serde(rename = "request-id", default)]
    request_id: Option<String>,
}

fn bound_len(text: String, max: usize) -> String {
    if text.len() <= max {
        text
    } else {
        // `String` is UTF-8; truncate at a character boundary at or before
        // `max` so the result stays valid.
        let mut end = max;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text[..end].to_owned()
    }
}

fn parse_retry_after(header: Option<&str>) -> Option<Duration> {
    let seconds: u64 = header?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

/// Draws a value in `[0.0, 1.0]` from `random`, for full-jitter backoff.
///
/// This does not need cryptographic quality, only enough spread to avoid a
/// retry thundering herd; it is deliberately the same
/// [`fieldnotes_domain::RandomSource`] trait A1 record-ID generation uses,
/// so one production randomness source, wired by whichever binary composes
/// this crate, serves both.
fn unit_interval(random: &mut dyn RandomSource) -> f64 {
    let mut bytes = [0u8; 8];
    random.fill_bytes(&mut bytes);
    (u64::from_be_bytes(bytes) as f64) / (u64::MAX as f64)
}

fn exponential_jittered_delay(
    policy: &RetryPolicy,
    attempt: u32,
    random: &mut dyn RandomSource,
) -> Duration {
    let exponent = attempt.saturating_sub(1).min(32);
    let factor = 2f64.powi(i32::try_from(exponent).unwrap_or(i32::MAX));
    let uncapped = policy.base_delay.as_secs_f64() * factor;
    let capped = uncapped.min(policy.max_delay.as_secs_f64()).max(0.0);
    Duration::from_secs_f64(capped * unit_interval(random))
}

/// A Microsoft Graph client: retried, classified, size-bounded `GET`
/// execution over an injected [`HttpTransport`].
///
/// Construct with [`GraphClient::new`], narrow with the `with_*` builders,
/// then call [`GraphClient::get`] for a single object or
/// [`GraphClient::list`]/[`GraphClient::delta`] for a paginated or delta
/// collection.
pub struct GraphClient<T, C, R> {
    transport: T,
    clock: C,
    random: RefCell<R>,
    base_url: String,
    retry_policy: RetryPolicy,
    max_response_bytes: usize,
    redactor: RefCell<Redactor>,
}

impl<T, C, R> GraphClient<T, C, R>
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    /// A client against the default Graph v1.0 endpoint, default retry
    /// policy, and default response-size bound.
    #[must_use]
    pub fn new(transport: T, clock: C, random: R) -> Self {
        GraphClient {
            transport,
            clock,
            random: RefCell::new(random),
            base_url: DEFAULT_BASE_URL.to_owned(),
            retry_policy: RetryPolicy::default(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            redactor: RefCell::new(Redactor::new()),
        }
    }

    /// Overrides the Graph service root, for a national cloud endpoint or a
    /// fixture server.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Overrides the retry policy.
    #[must_use]
    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Overrides the response-size bound applied before a body is parsed.
    #[must_use]
    pub fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }

    /// Fetches and deserializes a single Graph object.
    pub fn get<Resp: DeserializeOwned>(
        &self,
        token: &AccessToken,
        request: GraphRequest,
        operation: &'static str,
    ) -> Result<Resp, GraphError> {
        let url = request.into_url(&self.base_url);
        let response = self.execute_get(token, &url, operation)?;
        self.parse_json(&response, operation)
    }

    /// Starts a paginated collection, following `@odata.nextLink` one page
    /// at a time.
    #[must_use]
    pub fn list<'a, Item: DeserializeOwned>(
        &'a self,
        token: &'a AccessToken,
        request: GraphRequest,
        operation: &'static str,
    ) -> PageStream<'a, T, C, R, Item> {
        let url = request.into_url(&self.base_url);
        PageStream::new(self, token, url, operation)
    }

    /// Starts or resumes a delta collection.
    #[must_use]
    pub fn delta<'a, Item: DeserializeOwned>(
        &'a self,
        token: &'a AccessToken,
        start: DeltaStart,
        operation: &'static str,
    ) -> PageStream<'a, T, C, R, Item> {
        let url = match start {
            DeltaStart::Initial(request) => request.into_delta().into_url(&self.base_url),
            DeltaStart::Resume(delta_token) => delta_token.as_str().to_owned(),
        };
        PageStream::new(self, token, url, operation)
    }

    /// Whether `url` shares its scheme and authority with this client's
    /// configured base URL. [`crate::page::PageStream`] calls this before
    /// following any server-supplied continuation link.
    pub(crate) fn trusts_authority(&self, url: &str) -> bool {
        crate::url::same_authority(&self.base_url, url)
    }

    /// Deserializes `response`'s body as `Resp`, after the size bound
    /// [`GraphClient::execute_get`] already enforced.
    pub(crate) fn parse_json<Resp: DeserializeOwned>(
        &self,
        response: &GraphHttpResponse,
        operation: &'static str,
    ) -> Result<Resp, GraphError> {
        serde_json::from_slice(response.body()).map_err(|_| GraphError::MalformedResponse {
            operation,
            reason: "the response body was not valid JSON in the expected shape",
        })
    }

    /// Executes one logical `GET`, retrying and classifying as configured,
    /// and returns the successful response, still unparsed.
    pub(crate) fn execute_get(
        &self,
        token: &AccessToken,
        url: &str,
        operation: &'static str,
    ) -> Result<GraphHttpResponse, GraphError> {
        self.redactor.borrow_mut().register_secret(token.as_str());
        let start = self.clock.now_millis();
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let request = GraphHttpRequest::new(url.to_owned())
                .with_header("Authorization", token.header_value())
                .with_header("Accept", "application/json");
            match self.transport.execute(&request) {
                Ok(response) => {
                    if response.status() < 300 {
                        if response.body().len() > self.max_response_bytes {
                            return Err(GraphError::MalformedResponse {
                                operation,
                                reason: "the response body exceeded the configured size bound",
                            });
                        }
                        return Ok(response);
                    }
                    let kind = ErrorKind::classify(response.status());
                    let retry_after = parse_retry_after(response.header("retry-after"));
                    if kind.is_retryable()
                        && attempt < self.retry_policy.max_attempts
                        && self.wait_for_retry(attempt, start, retry_after)
                    {
                        continue;
                    }
                    return Err(self.build_error(kind, &response, operation, retry_after));
                }
                Err(transport_error) => {
                    if attempt < self.retry_policy.max_attempts
                        && self.wait_for_retry(attempt, start, None)
                    {
                        continue;
                    }
                    let reason = self.redactor.borrow().redact(transport_error.message());
                    return Err(GraphError::Transport { operation, reason });
                }
            }
        }
    }

    /// Waits out one retry, honoring `retry_after` if given, otherwise
    /// exponential-with-full-jitter backoff. Returns `false` (and sleeps
    /// nothing) if honoring the delay would exceed
    /// [`RetryPolicy`]'s total-elapsed bound, so a caller never oversleeps
    /// past it.
    fn wait_for_retry(
        &self,
        attempt: u32,
        start_millis: u64,
        retry_after: Option<Duration>,
    ) -> bool {
        let delay = match retry_after {
            Some(delay) => delay,
            None => {
                let mut random = self.random.borrow_mut();
                exponential_jittered_delay(&self.retry_policy, attempt, &mut *random)
            }
        };
        let elapsed = Duration::from_millis(self.clock.now_millis().saturating_sub(start_millis));
        if elapsed + delay > self.retry_policy.max_elapsed {
            return false;
        }
        self.clock.sleep(delay);
        true
    }

    fn build_error(
        &self,
        kind: ErrorKind,
        response: &GraphHttpResponse,
        operation: &'static str,
        retry_after: Option<Duration>,
    ) -> GraphError {
        let redactor = self.redactor.borrow();
        let parsed = if response.body().len() <= self.max_response_bytes {
            serde_json::from_slice::<GraphErrorEnvelope>(response.body()).ok()
        } else {
            None
        };
        let (code, message, request_id) = match parsed {
            Some(envelope) => (
                envelope.error.code.map(|code| redactor.redact(&code)),
                envelope
                    .error
                    .message
                    .map(|message| bound_len(redactor.redact(&message), MAX_MESSAGE_LEN)),
                envelope
                    .error
                    .inner_error
                    .and_then(|inner| inner.request_id),
            ),
            None => (None, None, None),
        };
        let detail = GraphErrorDetail {
            operation,
            status: response.status(),
            code,
            message,
            request_id,
            retry_after,
        };
        kind.into_error(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::{RetryPolicy, exponential_jittered_delay};
    use fieldnotes_test_support::CountingRandom;
    use std::time::Duration;

    #[test]
    fn backoff_never_exceeds_the_per_attempt_cap() {
        let policy = RetryPolicy::new(
            10,
            Duration::from_millis(100),
            Duration::from_secs(5),
            Duration::from_secs(600),
        );
        let mut random = CountingRandom::new(0);
        for attempt in 1..=10 {
            let delay = exponential_jittered_delay(&policy, attempt, &mut random);
            assert!(
                delay <= policy.max_delay,
                "attempt {attempt} produced {delay:?}"
            );
        }
    }

    #[test]
    fn backoff_is_deterministic_given_the_same_injected_randomness() {
        let policy = RetryPolicy::default();
        let first = exponential_jittered_delay(&policy, 3, &mut CountingRandom::new(7));
        let second = exponential_jittered_delay(&policy, 3, &mut CountingRandom::new(7));
        assert_eq!(first, second);
    }

    #[test]
    fn backoff_grows_with_attempt_number_before_hitting_the_cap() {
        let policy = RetryPolicy::new(
            10,
            Duration::from_millis(100),
            Duration::from_secs(60),
            Duration::from_secs(600),
        );
        // The same fixed seed for both draws isolates the exponential curve
        // from jitter (both draws see the same random fraction) so growth
        // can be asserted directly.
        let first = exponential_jittered_delay(&policy, 1, &mut CountingRandom::new(255));
        let second = exponential_jittered_delay(&policy, 2, &mut CountingRandom::new(255));
        assert!(second >= first);
    }
}
