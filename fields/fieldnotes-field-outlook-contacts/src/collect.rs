//! The `collect` operation: a Graph contacts delta walk, emitted as records,
//! checkpoints, and diagnostics.
//!
//! # Why this Field only ever commits one checkpoint per run
//!
//! A checkpoint's cursor is Graph's own `@odata.deltaLink`, which
//! [`fieldnotes_msgraph::PageStream`] only produces once the delta feed's
//! final page has actually been drained (see
//! [`fieldnotes_msgraph::PageStream::delta_token`]). There is no meaningful
//! resume point *mid* page: Graph gives out a `deltaLink` only at the very
//! end. So this Field's rule is the simplest one that is still honest: **a
//! checkpoint is offered only when the whole delta feed was consumed without
//! error.** Any error anywhere -- a mapping failure, a classified Graph
//! failure, or the run's own record ceiling being reached -- freezes the
//! cursor at its previous value by offering no checkpoint at all this run,
//! exactly the "a run in which core rejected a record commits no further
//! checkpoint" rule A2 section 9 already states, applied here to every
//! reason a run might stop short.

use std::io::BufRead;
use std::process::ExitCode;

use fieldnotes_domain::Clock;
use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
use fieldnotes_field_protocol::grammar::{
    CheckpointTag, DiagnosticTag, MessageText, OffsetDatetime, ProtocolV1, RunId,
};
use fieldnotes_field_protocol::message::{
    CheckpointEvent, CollectRequest, DiagnosticEvent, RecordEvent, Severity,
};
use fieldnotes_field_sdk::dispatch::read_collect_request;
use fieldnotes_msgraph::clock::RetryClock;
use fieldnotes_msgraph::transport::HttpTransport;
use fieldnotes_msgraph::{
    AccessToken, DeltaStart, GraphClient, GraphError, GraphRequest, RandomSource,
};

use crate::graph::GraphContact;
use crate::photo::PhotoTransport;
use crate::record::RecordContext;

/// Runs the collect operation end to end: reads the request, resolves
/// configuration, obtains credential material on the protected channel, and
/// drives a Graph delta walk against the shipping transport.
///
/// `random` and `clock` are supplied by [`crate::main`], this binary's own
/// composition root: retry-backoff jitter and a tombstone's `observed_at`
/// are the only two places this Field ever needs OS randomness or the wall
/// clock, and library code here reaches both only through the injected
/// traits, never directly.
pub(crate) fn run(
    mut input: impl BufRead,
    random: impl RandomSource,
    clock: &dyn Clock,
) -> ExitCode {
    let request = match read_collect_request(
        &mut input,
        fieldnotes_field_protocol::limits::Limits::ceilings().max_frame_bytes,
        "fieldnotes-field-outlook-contacts",
    ) {
        Ok(request) => request,
        Err(code) => return ExitCode::from(code),
    };

    let mut emitter = Emitter::new(&request);

    let resolved_config = match crate::config::resolve(&request.config) {
        Ok(resolved) => resolved,
        Err(error) => {
            crate::report(&format!("fieldnotes-field-outlook-contacts: {error}"));
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::ConfigInvalid,
                &error.to_string(),
            );
            return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
        }
    };

    let Some(grant) = &request.credential else {
        let message = "this Field's manifest declares protected_channel_required: true, but \
                        the collection request carried no credential grant";
        crate::report(&format!("fieldnotes-field-outlook-contacts: {message}"));
        emitter.diagnostic(
            &request.run_id,
            Severity::Error,
            DiagnosticCode::AuthReauthRequired,
            message,
        );
        return ExitCode::from(ProtocolExit::Authentication.as_raw());
    };

    let channel = match crate::credential::open(&grant.channel) {
        Ok(channel) => channel,
        Err(error) => {
            crate::report(&format!("fieldnotes-field-outlook-contacts: {error}"));
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::AuthReauthRequired,
                &error.to_string(),
            );
            return ExitCode::from(ProtocolExit::Authentication.as_raw());
        }
    };
    let material = match crate::credential::request_access_token(
        channel,
        &request.run_id,
        &grant.grant_id,
        grant.scopes.clone().unwrap_or_default(),
    ) {
        Ok(material) => material,
        Err(error) => {
            crate::report(&format!("fieldnotes-field-outlook-contacts: {error}"));
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::AuthReauthRequired,
                &error.to_string(),
            );
            return ExitCode::from(ProtocolExit::Authentication.as_raw());
        }
    };

    let token = AccessToken::new(material.value.clone());
    let client = GraphClient::new(
        fieldnotes_msgraph::UreqTransport::new(),
        fieldnotes_msgraph::SystemRetryClock::new(),
        random,
    )
    .with_base_url(resolved_config.graph_base_url.clone());
    let photo_transport = crate::photo::UreqPhotoTransport::new();

    let outcome = collect_with(
        &request,
        &resolved_config,
        &client,
        &token,
        &material.value,
        &photo_transport,
        clock,
        &mut emitter,
    );
    ExitCode::from(outcome.as_raw())
}

fn observed_at_from(clock: &dyn Clock) -> Result<OffsetDatetime, String> {
    let millis = i64::try_from(clock.unix_millis()).unwrap_or(i64::MAX);
    let datetime = fieldnotes_domain::Datetime::from_unix_millis(millis, 0)
        .map_err(|error| format!("current instant out of range: {error}"))?;
    OffsetDatetime::parse(&datetime.to_string())
        .map_err(|error| format!("rendered instant failed its own guard: {error}"))
}

/// Runs one delta-driven collection against an already-constructed Graph
/// client, generic over the transport, retry clock, and randomness source so
/// this crate's own tests can drive it against
/// [`fieldnotes_msgraph::testing::ScriptedTransport`] instead of the network.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_with<T, C, R, W>(
    request: &CollectRequest,
    resolved_config: &crate::config::ResolvedConfig,
    client: &GraphClient<T, C, R>,
    token: &AccessToken,
    bearer_token: &str,
    photo_transport: &dyn PhotoTransport,
    clock: &dyn Clock,
    emitter: &mut Emitter<W>,
) -> ProtocolExit
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
    W: std::io::Write,
{
    let source_scope = match crate::scope::compute(&resolved_config.tenant_id) {
        Ok(scope) => scope,
        Err(error) => {
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::ConfigInvalid,
                &error.to_string(),
            );
            return ProtocolExit::ConfigInvalid;
        }
    };

    let context = RecordContext {
        run_id: request.run_id.clone(),
        source_scope,
        staging_dir: std::path::Path::new(&request.artifact_staging_dir),
        limits: request.limits,
        media_policy: &request.artifact_media_types,
        photo_transport,
        bearer_token,
        graph_base_url: &resolved_config.graph_base_url,
        mailbox_resource: &resolved_config.mailbox_resource,
    };

    let previous_cursor = request.cursor.as_ref().and_then(crate::cursor::decode);
    if request.cursor.is_some() && previous_cursor.is_none() {
        emitter.diagnostic(
            &request.run_id,
            Severity::Warning,
            DiagnosticCode::CursorResetRequired,
            "the previous cursor could not be read; starting an unbounded delta collection",
        );
    }

    let start = match previous_cursor {
        Some(delta_token) => DeltaStart::Resume(delta_token),
        // `resolved_config.mailbox_resource` is always `/me` in this
        // release (`crate::config::resolve` refuses a configured
        // `mailbox` outright), so this always builds `/me/contacts`,
        // which `GraphClient::delta` turns into `/me/contacts/delta` --
        // the signed-in user's own contacts-delta feed. See
        // `crate::config::ConfigError::MailboxUnsupported` for why no
        // other segment is ever built here.
        None => DeltaStart::Initial(
            GraphRequest::new(format!("{}/contacts", resolved_config.mailbox_resource)).select([
                "id",
                "displayName",
                "givenName",
                "surname",
                "companyName",
                "jobTitle",
                "emailAddresses",
                "businessPhones",
                "homePhones",
                "mobilePhone",
                "lastModifiedDateTime",
                "createdDateTime",
                "changeKey",
            ]),
        ),
    };

    let mut page_stream = client.delta::<GraphContact>(token, start, "list contacts delta");

    let mut had_error = false;
    let mut exit_code = ProtocolExit::Completed;
    let mut last_record_seq: u64 = 0;
    let mut records_emitted: u64 = 0;

    loop {
        if records_emitted >= request.limits.max_run_records {
            had_error = true;
            break;
        }
        let item = match page_stream.next() {
            Some(item) => item,
            None => break,
        };
        let seq = emitter.next_seq();
        match item {
            Ok(contact) if contact.is_removed() => {
                let observed_at = match observed_at_from(clock) {
                    Ok(instant) => instant,
                    Err(detail) => {
                        had_error = true;
                        exit_code = ProtocolExit::Internal;
                        emitter.diagnostic_at(
                            seq,
                            &request.run_id,
                            Severity::Error,
                            DiagnosticCode::InternalError,
                            &detail,
                        );
                        break;
                    }
                };
                match crate::record::build_delete(&context, seq, &contact, observed_at) {
                    Ok(record) => {
                        if emitter.record(record) {
                            last_record_seq = seq;
                            records_emitted += 1;
                        }
                    }
                    Err(error) => {
                        had_error = true;
                        emitter.diagnostic_at(
                            seq,
                            &request.run_id,
                            Severity::Error,
                            DiagnosticCode::SourceUnavailable,
                            &error.to_string(),
                        );
                    }
                }
            }
            Ok(contact) => match crate::record::build_upsert(&context, seq, &contact) {
                Ok(outcome) => {
                    if emitter.record(outcome.record) {
                        last_record_seq = seq;
                        records_emitted += 1;
                    }
                    for warning in outcome.warnings {
                        emitter.diagnostic(
                            &request.run_id,
                            Severity::Warning,
                            DiagnosticCode::ContentSkipped,
                            &warning,
                        );
                    }
                }
                Err(error) => {
                    had_error = true;
                    emitter.diagnostic_at(
                        seq,
                        &request.run_id,
                        Severity::Error,
                        DiagnosticCode::SourceUnavailable,
                        &error.to_string(),
                    );
                }
            },
            Err(graph_error) => {
                had_error = true;
                let (code, severity, mapped_exit) = classify(&graph_error);
                exit_code = mapped_exit;
                emitter.diagnostic_at(
                    seq,
                    &request.run_id,
                    severity,
                    code,
                    &describe_graph_error(&graph_error),
                );
                break;
            }
        }
    }

    if !had_error && let Some(delta_token) = page_stream.delta_token() {
        match crate::cursor::encode(delta_token) {
            Ok(cursor_token) => {
                let checkpoint_seq = emitter.next_seq();
                let checkpoint = CheckpointEvent {
                    v: ProtocolV1,
                    frame_type: CheckpointTag,
                    run_id: request.run_id.clone(),
                    seq: checkpoint_seq,
                    cursor: cursor_token,
                    cursor_format_version: crate::constants::CURSOR_FORMAT_VERSION,
                    covers_record_seq_through: last_record_seq,
                    records_covered: records_emitted,
                    snapshot: None,
                    window: None,
                    is_final: true,
                };
                emitter.checkpoint(checkpoint);
            }
            Err(error) => {
                // The delta link cannot be expressed as a cursor at all
                // (past the 4 KiB bound): freeze at the previous cursor
                // rather than commit one that will fail to decode later.
                emitter.diagnostic(
                    &request.run_id,
                    Severity::Warning,
                    DiagnosticCode::CursorResetRequired,
                    &format!("the delta link could not be encoded as a cursor: {error}"),
                );
            }
        }
    } else if !had_error {
        // The feed ended with no `deltaLink` at all -- a `nextLink` page cut
        // off unexpectedly. Nothing safe to resume from is offered.
        emitter.diagnostic(
            &request.run_id,
            Severity::Warning,
            DiagnosticCode::CursorResetRequired,
            "the delta feed ended without a resumable delta link; the next run starts \
             unbounded",
        );
    }

    if had_error {
        exit_code
    } else {
        ProtocolExit::Completed
    }
}

/// Renders a Graph failure as this Field's own diagnostic message.
///
/// Deliberately does not delegate to [`GraphError`]'s own [`std::fmt::Display`]:
/// that renders Graph's own short error code as `code=<value>` (see
/// `fieldnotes_msgraph::error::GraphErrorDetail`'s `Display` impl), and
/// core's own second redaction pass
/// (`fieldnotes_field_protocol::redact::Redactor`) treats any `code=`
/// substring as an OAuth authorization-code parameter and blanks it --
/// even though a Graph error code such as `Request_ResourceNotFound` is a
/// short, fixed, non-secret label naming the failure class, exactly the
/// fact a diagnostic exists to surface. A Graph error's free-text
/// `message`, by contrast, can quote request content and is deliberately
/// left out here exactly as `GraphErrorDetail`'s own `Display` already
/// leaves it out.
///
/// Rendering the same fields with a colon instead of `=` (`graph_code: \
/// <value>`) carries identical information without colliding with that
/// redaction pattern, so the code that would have made this Field's own
/// 404 self-diagnosing survives all the way to a reviewer instead of
/// arriving as `graph_code: [redacted]`.
fn describe_graph_error(error: &GraphError) -> String {
    fn with_detail(detail: &fieldnotes_msgraph::GraphErrorDetail, outcome: &str) -> String {
        let mut text = format!(
            "graph request '{}' (status {}",
            detail.operation(),
            detail.status()
        );
        if let Some(code) = detail.code() {
            text.push_str(&format!(", graph_code: {code}"));
        }
        if let Some(request_id) = detail.request_id() {
            text.push_str(&format!(", request_id: {request_id}"));
        }
        text.push(')');
        text.push(' ');
        text.push_str(outcome);
        text
    }
    match error {
        GraphError::ReauthenticationRequired(detail) => {
            with_detail(detail, "requires a fresh access token")
        }
        GraphError::PermissionDenied(detail) => {
            with_detail(detail, "was denied; an administrator must grant consent")
        }
        GraphError::Throttled(detail) => with_detail(detail, "is still throttled after retrying"),
        GraphError::ServiceUnavailable(detail) => with_detail(
            detail,
            "failed with a transient server fault after retrying",
        ),
        GraphError::InvalidRequest(detail) => {
            with_detail(detail, "was rejected and will not succeed by retrying")
        }
        GraphError::UntrustedContinuation { .. }
        | GraphError::MalformedResponse { .. }
        | GraphError::Transport { .. } => error.to_string(),
    }
}

/// Classifies a Graph failure into a diagnostic code, severity, and exit
/// code, so an expired token, a consent problem, and throttling are
/// distinguishable in logs, metrics, and this Field's own exit status.
fn classify(error: &GraphError) -> (DiagnosticCode, Severity, ProtocolExit) {
    match error {
        GraphError::ReauthenticationRequired(_) => (
            DiagnosticCode::AuthReauthRequired,
            Severity::Error,
            ProtocolExit::Authentication,
        ),
        GraphError::PermissionDenied(_) => (
            DiagnosticCode::PermissionDenied,
            Severity::Error,
            ProtocolExit::Authorization,
        ),
        GraphError::Throttled(_) => (
            DiagnosticCode::RateLimitThrottled,
            Severity::Error,
            ProtocolExit::SourceUnavailable,
        ),
        GraphError::ServiceUnavailable(_) | GraphError::Transport { .. } => (
            DiagnosticCode::SourceUnavailable,
            Severity::Error,
            ProtocolExit::SourceUnavailable,
        ),
        GraphError::InvalidRequest(_) => (
            DiagnosticCode::ConfigInvalid,
            Severity::Error,
            ProtocolExit::ConfigInvalid,
        ),
        GraphError::UntrustedContinuation { .. } | GraphError::MalformedResponse { .. } => (
            DiagnosticCode::InternalError,
            Severity::Error,
            ProtocolExit::Internal,
        ),
    }
}

/// Writes protocol frames to standard output.
///
/// Delegates the run's diagnostic-, record-, and cursor-size self-policing,
/// and the "stop after the first write failure" rule, to
/// [`fieldnotes_field_sdk::emit::Emitter`]; this wrapper only adds this
/// Field's own convenience for building a [`DiagnosticEvent`] from primitive
/// severity/code/message arguments, matching `fields/fieldnotes-field-local`.
pub(crate) struct Emitter<W: std::io::Write = std::io::Stdout> {
    inner: fieldnotes_field_sdk::emit::Emitter<W>,
}

impl Emitter<std::io::Stdout> {
    fn new(request: &CollectRequest) -> Self {
        Emitter {
            inner: fieldnotes_field_sdk::emit::Emitter::new(std::io::stdout(), request.limits),
        }
    }
}

impl<W: std::io::Write> Emitter<W> {
    /// Builds an emitter over any sink, for a test that wants to decode
    /// exactly what was written rather than trusting only the exit code.
    #[cfg(test)]
    fn new_with_sink(request: &CollectRequest, sink: W) -> Self {
        Emitter {
            inner: fieldnotes_field_sdk::emit::Emitter::new(sink, request.limits),
        }
    }

    fn next_seq(&mut self) -> u64 {
        self.inner.next_seq()
    }

    fn record(&mut self, record: RecordEvent) -> bool {
        let already_failed = self.inner.write_failed();
        let emitted = self.inner.record(record);
        self.report_if_newly_failed(already_failed);
        emitted
    }

    fn diagnostic(
        &mut self,
        run_id: &RunId,
        severity: Severity,
        code: DiagnosticCode,
        message: &str,
    ) {
        let seq = self.next_seq();
        self.diagnostic_at(seq, run_id, severity, code, message);
    }

    fn diagnostic_at(
        &mut self,
        seq: u64,
        run_id: &RunId,
        severity: Severity,
        code: DiagnosticCode,
        message: &str,
    ) {
        let max_message_bytes = u64::try_from(MessageText::MAX_BYTES).unwrap_or(u64::MAX);
        let (truncated, _) =
            fieldnotes_field_sdk::truncate::truncate_utf8(message, max_message_bytes);
        let Ok(text) = MessageText::parse(&truncated) else {
            return;
        };
        let diagnostic = DiagnosticEvent {
            v: ProtocolV1,
            frame_type: DiagnosticTag,
            run_id: run_id.clone(),
            seq,
            severity,
            code,
            message: text,
            source: None,
            object_kind: None,
            retry_after_seconds: None,
            detail: None,
            redacted: None,
        };
        let already_failed = self.inner.write_failed();
        self.inner.diagnostic(diagnostic);
        self.report_if_newly_failed(already_failed);
    }

    fn checkpoint(&mut self, checkpoint: CheckpointEvent) {
        let already_failed = self.inner.write_failed();
        self.inner.checkpoint(checkpoint);
        self.report_if_newly_failed(already_failed);
    }

    fn report_if_newly_failed(&mut self, already_failed: bool) {
        if !already_failed
            && self.inner.write_failed()
            && let Some(error) = self.inner.last_write_error()
        {
            crate::report(&format!("fieldnotes-field-outlook-contacts: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Emitter, collect_with};
    use crate::config::ResolvedConfig;
    use crate::photo::testing::ScriptedPhotoTransport;
    use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
    use fieldnotes_field_protocol::framing::FrameReader;
    use fieldnotes_field_protocol::grammar::{
        CollectRequestTag, Cursor as CursorToken, FieldIdToken, ProtocolV1, RunId,
    };
    use fieldnotes_field_protocol::limits::{Deadline, Limits, default_artifact_media_types};
    use fieldnotes_field_protocol::message::{
        CollectRequest, CollectionMode, FieldEvent, Severity,
    };
    use fieldnotes_field_protocol::value::ConfigMap;
    use fieldnotes_msgraph::testing::{
        FakeRetryClock, ScriptedTransport, json_response, json_response_with_retry_after,
    };
    use fieldnotes_msgraph::{AccessToken, GraphClient};
    use fieldnotes_test_support::{CountingRandom, FixedClock, TempDir};

    fn run_id(text: &str) -> RunId {
        RunId::parse(text).unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    fn resolved_config() -> ResolvedConfig {
        ResolvedConfig {
            tenant_id: "8d820000-0000-7000-8000-000000000001".to_owned(),
            mailbox_resource: "/me".to_owned(),
            graph_base_url: "https://graph.microsoft.com/v1.0".to_owned(),
        }
    }

    fn request(
        run_id_text: &str,
        staging: &std::path::Path,
        cursor: Option<&str>,
    ) -> CollectRequest {
        CollectRequest {
            v: ProtocolV1,
            frame_type: CollectRequestTag,
            run_id: run_id(run_id_text),
            protocol_version: ProtocolV1,
            protocol_revision: 0,
            field_id: FieldIdToken::parse("outlook_contacts_work")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            mode: CollectionMode::Incremental,
            cursor: cursor.map(|text| {
                CursorToken::parse(text).unwrap_or_else(|error| panic!("must parse: {error}"))
            }),
            cursor_format_version: cursor.map(|_| 1),
            window: None,
            snapshot_scope: None,
            config: ConfigMap::new(),
            credential: None,
            artifact_staging_dir: staging.display().to_string(),
            limits: Limits::ceilings(),
            deadline: Deadline {
                not_after: fieldnotes_field_protocol::grammar::OffsetDatetime::parse(
                    "2099-01-01T00:00:00+00:00",
                )
                .unwrap_or_else(|error| panic!("must parse: {error}")),
                idle_seconds: Deadline::DEFAULT_IDLE_SECONDS,
                cancel_grace_seconds: Deadline::DEFAULT_CANCEL_GRACE_SECONDS,
            },
            artifact_media_types: default_artifact_media_types(),
            recollect_targets: None,
        }
    }

    /// Decodes every event this run wrote to its sink, in emission order.
    fn events_of(sink: &[u8]) -> Vec<FieldEvent> {
        let mut reader = FrameReader::new(sink, Limits::ceilings().max_frame_bytes, u64::MAX);
        let mut events = Vec::new();
        while let Some(raw) = reader
            .next_frame()
            .unwrap_or_else(|error| panic!("must decode: {error}"))
        {
            events.push(
                FieldEvent::decode(raw.value)
                    .unwrap_or_else(|error| panic!("must decode: {error}")),
            );
        }
        events
    }

    fn records_of(events: &[FieldEvent]) -> Vec<&fieldnotes_field_protocol::message::RecordEvent> {
        events
            .iter()
            .filter_map(|event| match event {
                FieldEvent::Record(record) => Some(record.as_ref()),
                _ => None,
            })
            .collect()
    }

    fn checkpoints_of(
        events: &[FieldEvent],
    ) -> Vec<&fieldnotes_field_protocol::message::CheckpointEvent> {
        events
            .iter()
            .filter_map(|event| match event {
                FieldEvent::Checkpoint(checkpoint) => Some(checkpoint.as_ref()),
                _ => None,
            })
            .collect()
    }

    fn diagnostics_of(
        events: &[FieldEvent],
    ) -> Vec<&fieldnotes_field_protocol::message::DiagnosticEvent> {
        events
            .iter()
            .filter_map(|event| match event {
                FieldEvent::Diagnostic(diagnostic) => Some(diagnostic.as_ref()),
                _ => None,
            })
            .collect()
    }

    const ALICE_PAGE: &str = r#"{
        "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/contacts/delta?$deltatoken=next-token",
        "value": [{
            "id": "AAMkAGI2CONTACT01",
            "displayName": "Alice Müller",
            "companyName": "Example AG",
            "jobTitle": "Head of Operations",
            "emailAddresses": [{"address": "alice@example.com"}],
            "businessPhones": ["+41 44 123 45 67"],
            "lastModifiedDateTime": "2026-08-22T08:15:00Z",
            "changeKey": "contact-version-3"
        }]
    }"#;

    #[test]
    fn a_collection_commits_a_checkpoint_carrying_the_delta_link() -> std::io::Result<()> {
        let staging = TempDir::new("collect-checkpoint")?;
        let transport = ScriptedTransport::new(vec![json_response(200, ALICE_PAGE)]);
        let request = request("1a4c9f2e-0000-4000-8000-000000000010", staging.path(), None);
        let mut sink: Vec<u8> = Vec::new();
        let exit = {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client =
                GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1));
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport =
                ScriptedPhotoTransport::new(vec![crate::photo::testing::Scripted::None]);
            let clock = FixedClock(1_755_000_000_000);
            collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            )
        };
        assert_eq!(exit, ProtocolExit::Completed);
        let events = events_of(&sink);
        let records = records_of(&events);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].source.identity.as_str(),
            "contact/AAMkAGI2CONTACT01"
        );
        let checkpoints = checkpoints_of(&events);
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].covers_record_seq_through, records[0].seq);
        assert_eq!(checkpoints[0].records_covered, 1);
        assert!(checkpoints[0].cursor.as_str().contains("next-token"));
        Ok(())
    }

    #[test]
    fn resuming_from_a_delta_token_requests_it_directly_rather_than_the_initial_page()
    -> std::io::Result<()> {
        let staging = TempDir::new("collect-resume")?;
        let final_page = r#"{
            "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/contacts/delta?$deltatoken=resumed-again",
            "value": []
        }"#;
        let transport = ScriptedTransport::new(vec![json_response(200, final_page)]);
        let request = request(
            "1a4c9f2e-0000-4000-8000-000000000011",
            staging.path(),
            Some(
                "outlook-contacts-delta/v1;https://graph.microsoft.com/v1.0/me/contacts/delta?$deltatoken=next-token",
            ),
        );
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client =
                GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1));
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport = ScriptedPhotoTransport::new(vec![]);
            let clock = FixedClock(1_755_000_000_000);
            let exit = collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            );
            assert_eq!(exit, ProtocolExit::Completed);
        }
        let events = events_of(&sink);
        assert!(
            records_of(&events).is_empty(),
            "an empty resumed page re-emits nothing"
        );
        let checkpoints = checkpoints_of(&events);
        assert_eq!(checkpoints.len(), 1);
        assert!(checkpoints[0].cursor.as_str().contains("resumed-again"));
        Ok(())
    }

    #[test]
    fn a_changed_contact_updates_one_record_under_the_same_source_key() -> std::io::Result<()> {
        let staging = TempDir::new("collect-changed")?;
        let second_page = r#"{
            "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/contacts/delta?$deltatoken=third",
            "value": [{
                "id": "AAMkAGI2CONTACT01",
                "displayName": "Alice Müller",
                "companyName": "Example AG",
                "jobTitle": "Chief Operating Officer",
                "lastModifiedDateTime": "2026-08-23T09:00:00Z",
                "changeKey": "contact-version-4"
            }]
        }"#;

        let first_transport = ScriptedTransport::new(vec![json_response(200, ALICE_PAGE)]);
        let first_request = request("1a4c9f2e-0000-4000-8000-000000000012", staging.path(), None);
        let mut first_sink: Vec<u8> = Vec::new();
        {
            let mut emitter = Emitter::new_with_sink(&first_request, &mut first_sink);
            let client = GraphClient::new(
                first_transport,
                FakeRetryClock::new(0),
                CountingRandom::new(1),
            );
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport =
                ScriptedPhotoTransport::new(vec![crate::photo::testing::Scripted::None]);
            let clock = FixedClock(1_755_000_000_000);
            collect_with(
                &first_request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            );
        }
        let first_events = events_of(&first_sink);
        let first_records = records_of(&first_events);
        let original_identity = first_records[0].source.identity.as_str().to_owned();

        let second_transport = ScriptedTransport::new(vec![json_response(200, second_page)]);
        let second_request = request(
            "1a4c9f2e-0000-4000-8000-000000000013",
            staging.path(),
            Some(
                "outlook-contacts-delta/v1;https://graph.microsoft.com/v1.0/me/contacts/delta?$deltatoken=next-token",
            ),
        );
        let mut second_sink: Vec<u8> = Vec::new();
        {
            let mut emitter = Emitter::new_with_sink(&second_request, &mut second_sink);
            let client = GraphClient::new(
                second_transport,
                FakeRetryClock::new(0),
                CountingRandom::new(1),
            );
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport =
                ScriptedPhotoTransport::new(vec![crate::photo::testing::Scripted::None]);
            let clock = FixedClock(1_755_100_000_000);
            collect_with(
                &second_request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            );
        }
        let second_events = events_of(&second_sink);
        let second_records = records_of(&second_events);
        assert_eq!(second_records.len(), 1);
        assert_eq!(
            second_records[0].source.identity.as_str(),
            original_identity
        );
        let job_title = second_records[0]
            .properties
            .as_ref()
            .and_then(|properties| properties.get(crate::constants::PROPERTY_JOB_TITLE))
            .cloned();
        assert_eq!(
            job_title,
            Some(fieldnotes_field_protocol::value::PropertyValue::Text(
                "Chief Operating Officer".to_owned()
            ))
        );
        Ok(())
    }

    #[test]
    fn a_removal_produces_an_authoritative_tombstone_with_no_content() -> std::io::Result<()> {
        let staging = TempDir::new("collect-tombstone")?;
        let page = r#"{
            "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/contacts/delta?$deltatoken=after-delete",
            "value": [{"id": "AAMkAGI2CONTACT01", "@removed": {"reason": "deleted"}}]
        }"#;
        let transport = ScriptedTransport::new(vec![json_response(200, page)]);
        let request = request("1a4c9f2e-0000-4000-8000-000000000014", staging.path(), None);
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client =
                GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1));
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport = ScriptedPhotoTransport::new(vec![]);
            let clock = FixedClock(1_755_000_000_000);
            let exit = collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            );
            assert_eq!(exit, ProtocolExit::Completed);
        }
        let events = events_of(&sink);
        let records = records_of(&events);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].change,
            fieldnotes_field_protocol::message::Change::Delete
        );
        assert!(records[0].note_type.is_none());
        assert!(records[0].body.is_none());
        assert!(records[0].properties.is_none());
        assert_eq!(checkpoints_of(&events).len(), 1);
        Ok(())
    }

    #[test]
    fn a_throttled_response_is_retried_and_the_run_still_completes() -> std::io::Result<()> {
        let staging = TempDir::new("collect-throttled")?;
        let transport = ScriptedTransport::new(vec![
            json_response_with_retry_after(429, 0, r#"{"error":{"code":"TooManyRequests"}}"#),
            json_response(200, ALICE_PAGE),
        ]);
        let request = request("1a4c9f2e-0000-4000-8000-000000000015", staging.path(), None);
        let mut sink: Vec<u8> = Vec::new();
        let exit = {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client =
                GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1));
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport =
                ScriptedPhotoTransport::new(vec![crate::photo::testing::Scripted::None]);
            let clock = FixedClock(1_755_000_000_000);
            collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            )
        };
        assert_eq!(exit, ProtocolExit::Completed);
        let events = events_of(&sink);
        assert_eq!(records_of(&events).len(), 1);
        assert_eq!(
            checkpoints_of(&events).len(),
            1,
            "a transparently retried throttle must still let the run complete and checkpoint"
        );
        Ok(())
    }

    #[test]
    fn an_expired_token_surfaces_actionably_and_commits_no_checkpoint() -> std::io::Result<()> {
        let staging = TempDir::new("collect-expired")?;
        let transport = ScriptedTransport::new(vec![json_response(
            401,
            r#"{"error":{"code":"InvalidAuthenticationToken","message":"Access token has expired"}}"#,
        )]);
        let request = request("1a4c9f2e-0000-4000-8000-000000000016", staging.path(), None);
        let mut sink: Vec<u8> = Vec::new();
        let exit = {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client =
                GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1));
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport = ScriptedPhotoTransport::new(vec![]);
            let clock = FixedClock(1_755_000_000_000);
            collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            )
        };
        assert_eq!(exit, ProtocolExit::Authentication);
        let events = events_of(&sink);
        assert!(checkpoints_of(&events).is_empty());
        let diagnostics = diagnostics_of(&events);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code
            == DiagnosticCode::AuthReauthRequired
            && diagnostic.severity == Severity::Error));
        Ok(())
    }

    #[test]
    fn a_permission_denied_response_never_advances_the_cursor() -> std::io::Result<()> {
        let staging = TempDir::new("collect-consent")?;
        let transport = ScriptedTransport::new(vec![json_response(
            403,
            r#"{"error":{"code":"ErrorAccessDenied","message":"Admin consent required"}}"#,
        )]);
        let request = request("1a4c9f2e-0000-4000-8000-000000000017", staging.path(), None);
        let mut sink: Vec<u8> = Vec::new();
        let exit = {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client =
                GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1));
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport = ScriptedPhotoTransport::new(vec![]);
            let clock = FixedClock(1_755_000_000_000);
            collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            )
        };
        assert_eq!(exit, ProtocolExit::Authorization);
        let events = events_of(&sink);
        assert!(
            checkpoints_of(&events).is_empty(),
            "a partial result must never commit a checkpoint"
        );
        Ok(())
    }

    /// Wraps a borrowed [`ScriptedTransport`] so a test can both drive a
    /// [`GraphClient`] with it and, afterwards, inspect
    /// [`ScriptedTransport::requested_urls`] on the original -- `GraphClient`
    /// otherwise takes its transport by value and never hands it back.
    struct RefTransport<'a>(&'a ScriptedTransport);

    impl fieldnotes_msgraph::transport::HttpTransport for RefTransport<'_> {
        fn execute(
            &self,
            request: &fieldnotes_msgraph::transport::GraphHttpRequest,
        ) -> Result<
            fieldnotes_msgraph::transport::GraphHttpResponse,
            fieldnotes_msgraph::transport::TransportError,
        > {
            self.0.execute(request)
        }
    }

    /// The regression test for this Field's own release-day 404: with no
    /// `mailbox` configured, the very first request this Field makes must be
    /// the signed-in user's own contacts-delta feed, `/me/contacts/delta` --
    /// never a `/users/...`-scoped path -- because that is the one resource
    /// Microsoft Graph actually exposes for this case (see
    /// `crate::config::ConfigError::MailboxUnsupported` for the other half
    /// of this fix).
    #[test]
    fn no_mailbox_requests_the_signed_in_users_own_contacts_delta_path() -> std::io::Result<()> {
        let staging = TempDir::new("collect-default-resource-path")?;
        let transport = ScriptedTransport::new(vec![json_response(200, ALICE_PAGE)]);
        let request = request("1a4c9f2e-0000-4000-8000-000000000020", staging.path(), None);
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client = GraphClient::new(
                RefTransport(&transport),
                FakeRetryClock::new(0),
                CountingRandom::new(1),
            );
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport =
                ScriptedPhotoTransport::new(vec![crate::photo::testing::Scripted::None]);
            let clock = FixedClock(1_755_000_000_000);
            let exit = collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            );
            assert_eq!(exit, ProtocolExit::Completed);
        }
        let urls = transport.requested_urls();
        assert_eq!(urls.len(), 1);
        assert_eq!(
            urls[0],
            "https://graph.microsoft.com/v1.0/me/contacts/delta?$select=id%2CdisplayName%2CgivenName%2Csurname%2CcompanyName%2CjobTitle%2CemailAddresses%2CbusinessPhones%2ChomePhones%2CmobilePhone%2ClastModifiedDateTime%2CcreatedDateTime%2CchangeKey"
        );
        Ok(())
    }

    /// core applies its own second, pattern-based redaction pass over every
    /// diagnostic before display or persistence
    /// (`fieldnotes_field_protocol::redact::Redactor`), and that pass treats
    /// any `code=<value>` substring as an OAuth authorization code. Graph's
    /// own error `code` is a short, fixed, non-secret failure label (see
    /// `describe_graph_error`'s docs) that this Field renders as
    /// `graph_code: <value>` specifically so it is not mistaken for that
    /// pattern and blanked out downstream -- which, for exactly this
    /// Field's own 404, would have destroyed the one detail that made the
    /// failure self-diagnosing.
    #[test]
    fn a_graph_error_code_survives_cores_downstream_redaction() -> std::io::Result<()> {
        let staging = TempDir::new("collect-code-visible")?;
        let transport = ScriptedTransport::new(vec![json_response(
            404,
            r#"{"error":{"code":"Request_ResourceNotFound","message":"Resource not found for the segment 'contacts'."}}"#,
        )]);
        let request = request("1a4c9f2e-0000-4000-8000-000000000021", staging.path(), None);
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut emitter = Emitter::new_with_sink(&request, &mut sink);
            let client =
                GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(1));
            let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned());
            let photo_transport = ScriptedPhotoTransport::new(vec![]);
            let clock = FixedClock(1_755_000_000_000);
            let exit = collect_with(
                &request,
                &resolved_config(),
                &client,
                &token,
                "FIXTURE-NOT-A-REAL-TOKEN-canary",
                &photo_transport,
                &clock,
                &mut emitter,
            );
            assert_eq!(exit, ProtocolExit::ConfigInvalid);
        }
        let events = events_of(&sink);
        let diagnostics = diagnostics_of(&events);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == DiagnosticCode::ConfigInvalid)
            .unwrap_or_else(|| {
                panic!("expected a config-invalid diagnostic among {diagnostics:?}")
            });
        assert!(
            diagnostic
                .message
                .as_str()
                .contains("Request_ResourceNotFound"),
            "the field's own message must carry the graph code: {}",
            diagnostic.message.as_str()
        );
        let redactor = fieldnotes_field_protocol::redact::Redactor::new();
        let redacted = redactor.redact(diagnostic.message.as_str());
        assert!(
            redacted.contains("Request_ResourceNotFound"),
            "the graph error code must survive core's downstream redaction pass: {redacted}"
        );
        Ok(())
    }
}
