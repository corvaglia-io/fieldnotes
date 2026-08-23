//! Turning a classified Graph failure into an actionable diagnostic.
//!
//! `fieldnotes-msgraph` has already done the hard part: it retried what was
//! worth retrying, refused to retry what was not, redacted every string it
//! kept from a Graph error body, and returned a variant that answers "what
//! should the caller do next". This module is only the translation from that
//! answer into A2's closed diagnostic-code vocabulary and exit-code set, so an
//! expired token, a consent problem, and throttling are three visibly
//! different outcomes rather than one generic failure.
//!
//! Nothing here can leak a token. A [`GraphError`] carries no URL and no
//! credential material by construction, and the access token was registered
//! with the transport's own redactor before the request that failed was ever
//! made.

use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
use fieldnotes_field_protocol::message::Severity;
use fieldnotes_msgraph::GraphError;

/// How this Field reports one Graph failure.
#[derive(Debug, Clone)]
pub(crate) struct Classified {
    /// The diagnostic severity. Always `error` for a failure that stopped
    /// collection, because an error-severity diagnostic is one of the signals
    /// A2 section 10 uses to disqualify deletion.
    pub(crate) severity: Severity,
    /// The closed-vocabulary diagnostic code.
    pub(crate) code: DiagnosticCode,
    /// The advisory retry delay, when Graph asked for one.
    pub(crate) retry_after_seconds: Option<u32>,
    /// The process exit code this failure ends the run with.
    pub(crate) exit: ProtocolExit,
    /// An already-redacted, actionable message for a human.
    pub(crate) message: String,
}

/// Whether Graph's own error code points at administrator consent rather than
/// at this user's own grant.
fn needs_admin_consent(code: Option<&str>) -> bool {
    code.is_some_and(|code| {
        let lowered = code.to_ascii_lowercase();
        lowered.contains("consent") || lowered.contains("authorizationrequestdenied")
    })
}

/// Classifies one Graph failure.
#[must_use]
pub(crate) fn classify(error: &GraphError) -> Classified {
    match error {
        GraphError::ReauthenticationRequired(detail) => Classified {
            severity: Severity::Error,
            code: DiagnosticCode::AuthExpired,
            retry_after_seconds: None,
            exit: ProtocolExit::Authentication,
            message: format!(
                "the access token for this mailbox is no longer accepted ({detail}). \
                 Re-authenticate this credential profile; retrying with the same token cannot \
                 succeed."
            ),
        },
        GraphError::PermissionDenied(detail) => {
            let consent = needs_admin_consent(detail.code());
            Classified {
                severity: Severity::Error,
                code: if consent {
                    DiagnosticCode::PermissionAdminConsentRequired
                } else {
                    DiagnosticCode::PermissionDenied
                },
                retry_after_seconds: None,
                exit: ProtocolExit::Authorization,
                message: if consent {
                    format!(
                        "reading this mailbox needs administrator consent for the Mail.Read \
                         scope ({detail}). A tenant administrator must grant it; retrying \
                         cannot."
                    )
                } else {
                    format!(
                        "this account is not permitted to read the requested mail ({detail}). \
                         Check that the credential profile is authorized for the Mail.Read \
                         scope and for this mailbox."
                    )
                },
            }
        }
        GraphError::Throttled(detail) => Classified {
            severity: Severity::Error,
            code: DiagnosticCode::RateLimitThrottled,
            retry_after_seconds: detail
                .retry_after()
                .and_then(|delay| u32::try_from(delay.as_secs()).ok()),
            exit: ProtocolExit::SourceUnavailable,
            message: format!(
                "Microsoft Graph is still throttling this mailbox after the transport exhausted \
                 its retry budget ({detail}). The cursor did not advance, so the next run \
                 resumes from the same point."
            ),
        },
        GraphError::ServiceUnavailable(detail) => Classified {
            severity: Severity::Error,
            code: DiagnosticCode::SourceUnavailable,
            retry_after_seconds: detail
                .retry_after()
                .and_then(|delay| u32::try_from(delay.as_secs()).ok()),
            exit: ProtocolExit::SourceUnavailable,
            message: format!(
                "Microsoft Graph reported a transient server fault after the transport exhausted \
                 its retry budget ({detail}). The cursor did not advance."
            ),
        },
        GraphError::InvalidRequest(detail) => Classified {
            severity: Severity::Error,
            code: DiagnosticCode::InternalError,
            retry_after_seconds: None,
            exit: ProtocolExit::Internal,
            message: format!(
                "Microsoft Graph rejected a request this Field built ({detail}). Retrying the \
                 identical request cannot help; this is a defect in this connector or an \
                 unusable configured mail folder."
            ),
        },
        GraphError::UntrustedContinuation { operation } => Classified {
            severity: Severity::Error,
            code: DiagnosticCode::SourceUnavailable,
            retry_after_seconds: None,
            exit: ProtocolExit::SourceUnavailable,
            message: format!(
                "a continuation link received while running '{operation}' pointed outside the \
                 configured Graph authority and was refused before the token was attached to \
                 it. Nothing was collected past that page."
            ),
        },
        GraphError::MalformedResponse { operation, reason } => Classified {
            severity: Severity::Error,
            code: DiagnosticCode::SourceUnavailable,
            retry_after_seconds: None,
            exit: ProtocolExit::SourceUnavailable,
            message: format!(
                "the response to '{operation}' could not be trusted: {reason}. Nothing from that \
                 page was collected."
            ),
        },
        GraphError::Transport { operation, reason } => Classified {
            severity: Severity::Error,
            code: DiagnosticCode::SourceUnavailable,
            retry_after_seconds: None,
            exit: ProtocolExit::SourceUnavailable,
            message: format!(
                "'{operation}' failed before any response arrived, after the transport exhausted \
                 its retry budget: {reason}"
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::classify;
    use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
    use fieldnotes_msgraph::testing::{ScriptedTransport, json_response};
    use fieldnotes_msgraph::{AccessToken, GraphClient, GraphRequest};

    /// The unique canary this module's tests use in place of a token. It is
    /// asserted absent from every classified message.
    const TOKEN_CANARY: &str = "FIXTURE-NOT-A-REAL-TOKEN-canary-outlook-mail";

    fn client(
        responses: Vec<
            Result<fieldnotes_msgraph::GraphHttpResponse, fieldnotes_msgraph::TransportError>,
        >,
    ) -> GraphClient<
        ScriptedTransport,
        fieldnotes_msgraph::testing::FakeRetryClock,
        fieldnotes_test_support::CountingRandom,
    > {
        GraphClient::new(
            ScriptedTransport::new(responses),
            fieldnotes_msgraph::testing::FakeRetryClock::new(0),
            fieldnotes_test_support::CountingRandom::new(3),
        )
    }

    #[derive(serde::Deserialize)]
    struct Ignored {}

    fn failure(status: u16, body: &str) -> fieldnotes_msgraph::GraphError {
        let client = client(vec![json_response(status, body)]);
        let token = AccessToken::new(TOKEN_CANARY);
        match client.get::<Ignored>(&token, GraphRequest::new("/me/messages"), "test read") {
            Err(error) => error,
            Ok(_) => panic!("a {status} response must not classify as success"),
        }
    }

    #[test]
    fn an_expired_token_is_actionable_and_distinct_from_a_consent_problem() {
        let classified = classify(&failure(
            401,
            r#"{"error":{"code":"InvalidAuthenticationToken","message":"Access token has expired."}}"#,
        ));
        assert_eq!(classified.code, DiagnosticCode::AuthExpired);
        assert_eq!(classified.exit, ProtocolExit::Authentication);
        assert!(classified.message.contains("Re-authenticate"));
        assert!(!classified.message.contains("canary"), "leaked a token");
    }

    #[test]
    fn a_consent_problem_names_the_administrator_action() {
        let classified = classify(&failure(
            403,
            r#"{"error":{"code":"AuthorizationRequestDenied","message":"Insufficient privileges."}}"#,
        ));
        assert_eq!(
            classified.code,
            DiagnosticCode::PermissionAdminConsentRequired
        );
        assert_eq!(classified.exit, ProtocolExit::Authorization);
        assert!(classified.message.contains("administrator"));
    }

    #[test]
    fn a_plain_denial_is_a_permission_problem_not_a_consent_one() {
        let classified = classify(&failure(
            403,
            r#"{"error":{"code":"ErrorAccessDenied","message":"Access is denied."}}"#,
        ));
        assert_eq!(classified.code, DiagnosticCode::PermissionDenied);
    }

    #[test]
    fn exhausted_throttling_carries_the_advisory_delay() {
        // Five scripted 429s, which is exactly the transport's default attempt
        // budget: the classification under test is the one a caller sees only
        // after every retry has already been honoured and exhausted.
        let throttled = || {
            fieldnotes_msgraph::testing::json_response_with_retry_after(
                429,
                7,
                r#"{"error":{"code":"ApplicationThrottled"}}"#,
            )
        };
        let client = client(vec![
            throttled(),
            throttled(),
            throttled(),
            throttled(),
            throttled(),
        ]);
        let token = AccessToken::new(TOKEN_CANARY);
        let error =
            match client.get::<Ignored>(&token, GraphRequest::new("/me/messages"), "test read") {
                Err(error) => error,
                Ok(_) => panic!("a 429 must not classify as success"),
            };
        let classified = classify(&error);
        assert_eq!(classified.code, DiagnosticCode::RateLimitThrottled);
        assert_eq!(classified.retry_after_seconds, Some(7));
        assert_eq!(classified.exit, ProtocolExit::SourceUnavailable);
    }

    #[test]
    fn a_transient_server_fault_is_distinguishable_from_throttling() {
        let classified = classify(&failure(503, r#"{"error":{"code":"ServiceUnavailable"}}"#));
        assert_eq!(classified.code, DiagnosticCode::SourceUnavailable);
        assert_eq!(classified.exit, ProtocolExit::SourceUnavailable);
    }

    #[test]
    fn a_request_this_field_built_wrong_is_reported_as_this_fields_own_defect() {
        let classified = classify(&failure(
            400,
            r#"{"error":{"code":"ErrorInvalidIdMalformed"}}"#,
        ));
        assert_eq!(classified.code, DiagnosticCode::InternalError);
        assert_eq!(classified.exit, ProtocolExit::Internal);
    }

    #[test]
    fn no_classified_message_can_carry_a_token() {
        for (status, body) in [
            (
                401,
                r#"{"error":{"code":"InvalidAuthenticationToken","message":"token FIXTURE-NOT-A-REAL-TOKEN-canary-outlook-mail rejected"}}"#,
            ),
            (
                403,
                r#"{"error":{"code":"ErrorAccessDenied","message":"FIXTURE-NOT-A-REAL-TOKEN-canary-outlook-mail"}}"#,
            ),
        ] {
            let classified = classify(&failure(status, body));
            assert!(
                !classified.message.contains("canary"),
                "a Graph body echoing the token must still be redacted: {}",
                classified.message
            );
        }
    }
}
