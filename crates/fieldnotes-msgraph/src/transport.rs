//! The seam between Graph request execution and the underlying HTTP stack.
//!
//! [`HttpTransport`] is the only way [`GraphClient`](crate::client::GraphClient)
//! reaches the network. Tests substitute [`crate::testing::ScriptedTransport`]
//! (see [`crate::testing`]) so the whole retry/pagination/error-taxonomy
//! surface is exercised without a tenant, a network, or credentials; the
//! shipping implementation is [`UreqTransport`].
//!
//! [`GraphHttpRequest`] can only ever represent a `GET`: it has no method
//! field and no way to attach a body. That is deliberate, not incidental —
//! see the module docs on [`crate::request`].

use std::fmt;
use std::time::Duration;

/// One outbound Graph call. Always a `GET`.
///
/// [`fmt::Debug`] on this type never prints the full URL or header values:
/// it prints the path with any query string stripped and the header
/// *names* only. A [`HttpTransport`] implementation still needs the real
/// `url()`/`headers()` accessors to do its job — nothing stops an
/// implementation from logging what it was given — but the type itself
/// never leaks the bearer token or a signed query string through an
/// incidental `{:?}` the way a naive `#[derive(Debug)]` would.
pub struct GraphHttpRequest {
    url: String,
    headers: Vec<(String, String)>,
}

impl GraphHttpRequest {
    pub(crate) fn new(url: String) -> Self {
        GraphHttpRequest {
            url,
            headers: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The full absolute URL, including its query string.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The request headers, including `Authorization`.
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }
}

impl fmt::Debug for GraphHttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path_only = self.url.split('?').next().unwrap_or(&self.url);
        let header_names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("GraphHttpRequest")
            .field("method", &"GET")
            .field("path", &path_only)
            .field("header_names", &header_names)
            .finish()
    }
}

/// One inbound HTTP response, exactly as received.
///
/// Like [`GraphHttpRequest`], [`fmt::Debug`] prints only the status, header
/// *names*, and body length — never a header value or the body content,
/// since a response is untrusted input this crate has not yet classified or
/// sanitized.
pub struct GraphHttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl GraphHttpResponse {
    /// Builds a response from its parts. Public so a custom
    /// [`HttpTransport`] (including a fixture-driven test double) can
    /// construct one.
    #[must_use]
    pub fn new(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        GraphHttpResponse {
            status,
            headers,
            body,
        }
    }

    /// The HTTP status code.
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The first header value matching `name`, compared case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The response body, exactly as received.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl fmt::Debug for GraphHttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header_names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("GraphHttpResponse")
            .field("status", &self.status)
            .field("header_names", &header_names)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// A transport-level failure: the request never produced a classifiable
/// HTTP response (connection refused, TLS failure, timeout, malformed HTTP
/// framing, or a fixture wired to fail).
///
/// The message is opaque on purpose. [`crate::client::GraphClient`] passes
/// it through its own [`Redactor`](fieldnotes_field_protocol::redact::Redactor)
/// before it can reach a [`GraphError`](crate::error::GraphError), but a
/// custom [`HttpTransport`] should still avoid putting request headers or
/// full URLs into this message, since this crate cannot inspect what a
/// third-party implementation chooses to do with the value before handing
/// it here.
#[derive(Debug, Clone)]
pub struct TransportError {
    message: String,
}

impl TransportError {
    /// Wraps a short, human-readable description of the failure.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        TransportError {
            message: message.into(),
        }
    }

    /// The description this transport failure carries.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TransportError {}

/// The seam a real or fake HTTP stack implements.
///
/// Exactly one method, taking a request that can only ever be a `GET`: this
/// is the entire surface [`GraphClient`](crate::client::GraphClient) uses to
/// reach the network, and the entire surface a test needs to fake.
pub trait HttpTransport {
    /// Executes one `GET` and returns whatever response arrived, or a
    /// transport-level failure if no response was received at all.
    ///
    /// A non-2xx status is not an error at this layer: [`GraphHttpResponse`]
    /// is returned for any status the server actually sent, and
    /// classifying it is [`crate::client::GraphClient`]'s job.
    fn execute(&self, request: &GraphHttpRequest) -> Result<GraphHttpResponse, TransportError>;
}

/// The default per-call timeout [`UreqTransport::new`] applies.
const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A safety cap on response bytes actually read off the wire, independent
/// of any `Content-Length` the server claims.
///
/// This is enforced here, at the lowest layer that touches the socket, and
/// again by [`crate::client::GraphClient`] as defense in depth for a custom
/// transport that does not enforce it itself.
const DEFAULT_TRANSPORT_MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

/// The shipping [`HttpTransport`]: a blocking [`ureq`] agent over rustls.
///
/// See the crate-level docs for why `ureq` was chosen over `reqwest` and
/// other alternatives.
pub struct UreqTransport {
    agent: ureq::Agent,
    max_response_bytes: u64,
}

impl UreqTransport {
    /// A transport with the default per-call timeout and response-size cap.
    #[must_use]
    pub fn new() -> Self {
        UreqTransport::with_timeout(DEFAULT_CALL_TIMEOUT)
    }

    /// A transport with an explicit per-call timeout.
    ///
    /// The timeout bounds one HTTP call; it is independent of and smaller
    /// in scope than [`crate::client::RetryPolicy`]'s total-elapsed bound,
    /// which governs the whole retry sequence.
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            // This crate classifies non-2xx responses itself: it needs the
            // headers (`Retry-After`) and body (Graph's error envelope) that
            // ureq's own status-as-error handling would discard.
            .http_status_as_error(false)
            // Refuse to fetch anything but https, and never follow a
            // redirect that would carry the Authorization header across an
            // authority change (ureq's default `redirect_auth_headers` is
            // already `Never`; this only pins the scheme).
            .https_only(true)
            .timeout_per_call(Some(timeout))
            .build();
        UreqTransport {
            agent: config.into(),
            max_response_bytes: DEFAULT_TRANSPORT_MAX_RESPONSE_BYTES,
        }
    }

    /// Overrides the response-size cap.
    #[must_use]
    pub fn with_max_response_bytes(mut self, max_response_bytes: u64) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        UreqTransport::new()
    }
}

impl HttpTransport for UreqTransport {
    fn execute(&self, request: &GraphHttpRequest) -> Result<GraphHttpResponse, TransportError> {
        let mut builder = self.agent.get(request.url());
        for (name, value) in request.headers() {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let mut response = builder
            .call()
            .map_err(|error| TransportError::new(error.to_string()))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    value.to_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let body = response
            .body_mut()
            .with_config()
            .limit(self.max_response_bytes)
            .read_to_vec()
            .map_err(|error| TransportError::new(error.to_string()))?;
        Ok(GraphHttpResponse::new(status, headers, body))
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphHttpRequest, GraphHttpResponse};

    #[test]
    fn request_debug_never_prints_the_query_string_or_header_values() {
        let request = GraphHttpRequest::new(
            "https://graph.microsoft.com/v1.0/me/messages?$skiptoken=abc".to_owned(),
        )
        .with_header(
            "Authorization",
            "Bearer FIXTURE-NOT-A-REAL-TOKEN-canary-4b2f",
        )
        .with_header("Accept", "application/json");
        let printed = format!("{request:?}");
        assert!(!printed.contains("canary"), "leaked: {printed}");
        assert!(!printed.contains("skiptoken"), "leaked: {printed}");
        assert!(printed.contains("Authorization"));
        assert!(printed.contains("/me/messages"));
    }

    #[test]
    fn response_debug_never_prints_header_values_or_body() {
        let response = GraphHttpResponse::new(
            401,
            vec![(
                "WWW-Authenticate".to_owned(),
                "Bearer error=\"invalid_token\"".to_owned(),
            )],
            b"{\"error\":{\"code\":\"InvalidAuthenticationToken\"}}".to_vec(),
        );
        let printed = format!("{response:?}");
        assert!(!printed.contains("invalid_token"), "leaked: {printed}");
        assert!(
            !printed.contains("InvalidAuthenticationToken"),
            "leaked: {printed}"
        );
        assert!(printed.contains("401"));
        assert!(printed.contains("WWW-Authenticate"));
    }

    #[test]
    fn response_header_lookup_is_case_insensitive() {
        let response = GraphHttpResponse::new(
            429,
            vec![("Retry-After".to_owned(), "5".to_owned())],
            vec![],
        );
        assert_eq!(response.header("retry-after"), Some("5"));
    }
}
