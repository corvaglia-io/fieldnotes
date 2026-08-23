//! The bearer access token a caller supplies for one Graph request.
//!
//! This crate never acquires, refreshes, or stores a token: another crate
//! owns the credential layer, and A0 forbids this crate from importing a
//! host credential-store adapter. [`AccessToken`] exists only to carry a
//! caller-supplied token through this crate's calls while making it
//! structurally hard to leak.

use std::fmt;

/// A bearer access token supplied by the caller for exactly the requests it
/// authorizes.
///
/// `AccessToken` deliberately has no [`fmt::Display`] implementation and a
/// [`fmt::Debug`] implementation that never prints the token, so a stray
/// `println!("{token}")`, a `format!("{:?}", ...)` on a struct that embeds
/// one, or a panic message that interpolates one cannot compile into a leak
/// by accident. [`GraphClient`](crate::client::GraphClient) additionally
/// registers every token it is given with its `Redactor` before making a
/// request, so even a token value that ends up embedded in server-supplied
/// text (for example echoed back in a malformed error body) is scrubbed
/// before it reaches a [`GraphError`](crate::error::GraphError).
#[derive(Clone)]
pub struct AccessToken(String);

impl AccessToken {
    /// Wraps a caller-supplied bearer token.
    ///
    /// The token is never validated, parsed, or interpreted; it is passed
    /// through to the `Authorization` header exactly as given.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        AccessToken(token.into())
    }

    /// The raw token value.
    ///
    /// Restricted to this crate: the only legitimate uses are building the
    /// `Authorization` header and registering the value with a [`Redactor`]
    /// before it can leak into an error.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The `Authorization` header value carrying this token.
    pub(crate) fn header_value(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use super::AccessToken;

    #[test]
    fn debug_output_never_contains_the_token() {
        let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary-7a1c9e02");
        let printed = format!("{token:?}");
        assert!(!printed.contains("canary"), "leaked: {printed}");
        assert_eq!(printed, "AccessToken(REDACTED)");
    }

    #[test]
    fn header_value_wraps_the_token_as_a_bearer_header() {
        let token = AccessToken::new("abc123");
        assert_eq!(token.header_value(), "Bearer abc123");
    }
}
