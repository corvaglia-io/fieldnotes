//! The error taxonomy a caller acts on.
//!
//! Every variant of [`GraphError`] answers "what should the caller do
//! next", which is the product requirement behind it: gate R3 requires a
//! revoked or expired credential to fail actionably, and gate R5 requires
//! tenant-permission preflight to be actionable. A caller that only wants to
//! know whether to retry can match on [`GraphError::is_retryable_kind`]; a
//! caller that wants to react precisely (re-authenticate, surface an
//! admin-consent prompt, back off) matches on the variant.
//!
//! No variant carries a full URL or an access token. [`GraphErrorDetail`]'s
//! `message` field is bounded in length and passed through a
//! [`Redactor`](fieldnotes_field_protocol::redact::Redactor) by
//! [`crate::client::GraphClient`] before it is ever attached to an error, so
//! even text Graph itself returned cannot carry a leaked secret back out
//! through this crate.

use std::fmt;
use std::time::Duration;

/// The longest `message` text this crate keeps from a Graph error body.
///
/// A malicious or malformed response is untrusted input; there is no reason
/// to hold an unbounded amount of server-supplied text in memory just to
/// report a failure.
pub(crate) const MAX_MESSAGE_LEN: usize = 500;

/// The non-secret detail behind one classified Graph failure.
#[derive(Debug, Clone)]
pub struct GraphErrorDetail {
    pub(crate) operation: &'static str,
    pub(crate) status: u16,
    pub(crate) code: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) retry_after: Option<Duration>,
}

impl GraphErrorDetail {
    /// The caller-supplied label for the request that failed, such as
    /// `"list mail messages"`. Never a URL.
    #[must_use]
    pub fn operation(&self) -> &'static str {
        self.operation
    }

    /// The HTTP status code Graph returned.
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Graph's own short error code (for example `InvalidAuthenticationToken`
    /// or `ErrorAccessDenied`), if the response body parsed as a Graph error
    /// envelope.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// A bounded, redacted excerpt of Graph's own error message, if present.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Graph's request-correlation identifier, if present. Safe to include
    /// in a support request; it identifies a request, not a principal.
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// The delay Graph asked for via `Retry-After`, if this failure carried
    /// one and retries were still exhausted or the budget did not allow
    /// honoring it.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }
}

impl fmt::Display for GraphErrorDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' (status {}", self.operation, self.status)?;
        if let Some(code) = &self.code {
            write!(f, ", code={code}")?;
        }
        if let Some(request_id) = &self.request_id {
            write!(f, ", request_id={request_id}")?;
        }
        write!(f, ")")
    }
}

/// A Graph request outcome a caller can act on.
///
/// See the module docs for the design behind each variant and
/// [`GraphError::is_retryable_kind`] for the retry/no-retry split this crate
/// already applied before returning an error at all.
#[derive(Debug, Clone)]
pub enum GraphError {
    /// The access token was missing, malformed, or has expired (Graph
    /// status 401). This crate never refreshes a token; the caller must
    /// obtain a fresh one from the credential layer and retry the whole
    /// request. Retrying with the same token will not help, so this crate
    /// does not retry it.
    ReauthenticationRequired(GraphErrorDetail),

    /// The token was valid but the tenant or account has not granted, or
    /// has revoked, permission for the request (Graph status 403).
    /// Actionable by a tenant administrator granting consent, not by
    /// retrying.
    PermissionDenied(GraphErrorDetail),

    /// Graph asked the caller to slow down (status 429) and this crate's
    /// retry policy was already exhausted (attempts or total elapsed time)
    /// while honoring it.
    Throttled(GraphErrorDetail),

    /// A transient server-side fault (503 and other 5xx) and this crate's
    /// retry policy was already exhausted while retrying it.
    ServiceUnavailable(GraphErrorDetail),

    /// A permanent client error unrelated to authentication or throttling
    /// (any other 4xx). Retrying the identical request will not help;
    /// changing the request might.
    InvalidRequest(GraphErrorDetail),

    /// A server-supplied continuation (`@odata.nextLink`, `@odata.deltaLink`,
    /// or a caller-resumed [`DeltaToken`](crate::page::DeltaToken)) pointed
    /// outside the configured Graph authority. Refused before the bearer
    /// token was ever attached to it.
    UntrustedContinuation {
        /// The operation that was in progress when the untrusted
        /// continuation was encountered.
        operation: &'static str,
    },

    /// The response could not be trusted as a well-formed Graph JSON
    /// payload: invalid JSON, a shape that did not match what the caller
    /// asked to deserialize, or a body past the configured size bound.
    /// Never partially parsed.
    MalformedResponse {
        /// The operation whose response could not be trusted.
        operation: &'static str,
        /// A fixed, non-sensitive description of why. Never server-supplied
        /// text.
        reason: &'static str,
    },

    /// The underlying HTTP transport failed before any response was
    /// received (connection, TLS, or timeout). This crate's retry policy
    /// was already exhausted while retrying it.
    Transport {
        /// The operation that failed.
        operation: &'static str,
        /// A redacted, bounded description of the transport failure.
        reason: String,
    },
}

impl GraphError {
    /// Whether this crate's retry policy applied to this failure's *kind*
    /// (independent of whether an individual attempt happened to exhaust
    /// its budget). [`GraphError::Throttled`], [`GraphError::ServiceUnavailable`],
    /// and [`GraphError::Transport`] are always returned only after retries
    /// were exhausted; every other variant was never retried because
    /// retrying it could not have helped.
    #[must_use]
    pub fn is_retryable_kind(&self) -> bool {
        matches!(
            self,
            GraphError::Throttled(_)
                | GraphError::ServiceUnavailable(_)
                | GraphError::Transport { .. }
        )
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::ReauthenticationRequired(detail) => {
                write!(f, "graph request {detail} requires a fresh access token")
            }
            GraphError::PermissionDenied(detail) => {
                write!(
                    f,
                    "graph request {detail} was denied; an administrator must grant consent"
                )
            }
            GraphError::Throttled(detail) => {
                write!(
                    f,
                    "graph request {detail} is still throttled after retrying"
                )
            }
            GraphError::ServiceUnavailable(detail) => {
                write!(
                    f,
                    "graph request {detail} failed with a transient server fault after retrying"
                )
            }
            GraphError::InvalidRequest(detail) => {
                write!(
                    f,
                    "graph request {detail} was rejected and will not succeed by retrying"
                )
            }
            GraphError::UntrustedContinuation { operation } => write!(
                f,
                "graph request '{operation}' received a continuation link outside the configured authority; refused"
            ),
            GraphError::MalformedResponse { operation, reason } => {
                write!(
                    f,
                    "graph request '{operation}' returned an untrustworthy response: {reason}"
                )
            }
            GraphError::Transport { operation, reason } => {
                write!(
                    f,
                    "graph request '{operation}' failed before a response arrived: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for GraphError {}
