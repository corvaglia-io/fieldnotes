//! The `collect` operation: a windowed, delta-resumable Graph `calendarView`
//! collection, emitted as records, checkpoints, and diagnostics.
//!
//! # Windowing and resumption
//!
//! A first (cursor-less) run requires `collect_request.window` and starts a
//! fresh Graph delta bounded to it (A2 section 5: "a windowed run is never a
//! complete snapshot and can never authorize deletion by absence" -- exactly
//! why this Field never declares snapshot authority at all). A resumed run
//! ignores any window the request carries -- Graph's own delta link already
//! fixes the bounds the moment it is minted, and there is no way to widen or
//! move them mid-stream -- and continues from the stored delta token
//! instead.
//!
//! # What "complete" means here
//!
//! This run advances its cursor only when the whole Graph page sequence for
//! this request was fetched without error and every item on it became an
//! accepted record. Any error anywhere -- a Graph failure, or one event this
//! Field could not map -- freezes the cursor at its previous value (or, on a
//! first run that got nowhere, offers no checkpoint at all) and reports a
//! non-zero exit, exactly mirroring `fieldnotes-field-local`'s "a partial
//! result is never silently promoted to a complete one".

use std::io::BufRead;
use std::process::ExitCode;

use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
use fieldnotes_field_protocol::grammar::{
    CheckpointTag, DiagnosticTag, MessageText, OffsetDatetime, ProtocolV1, RunId,
};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{
    CheckpointEvent, CheckpointWindow, CollectRequest, CollectionMode, DiagnosticEvent,
    RecordEvent, Severity,
};
use fieldnotes_field_sdk::dispatch::read_collect_request;
use fieldnotes_msgraph::{
    AccessToken, DeltaStart, GraphClient, GraphError, HttpTransport, RandomSource, RetryClock,
};

use crate::cursor::CursorState;
use crate::graph::{GraphEvent, initial_delta_request};
use crate::record::{RecordContext, build_delete, build_upsert};
use crate::{config, credential};

/// Runs the collect operation end to end.
///
/// `transport`, `clock`, and `random` are supplied by the composition root
/// (`main`); this function never touches the network, a wall clock, or an
/// OS random source directly. `observed_now_millis` is the one wall-clock
/// reading this run needs -- the instant a `@removed` item is reported as
/// deleted -- read once by `main` rather than by this library logic.
pub(crate) fn run<T, C, R>(
    mut input: impl BufRead,
    transport: T,
    clock: C,
    random: R,
    observed_now_millis: u64,
) -> ExitCode
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    let request = match read_collect_request(
        &mut input,
        Limits::ceilings().max_frame_bytes,
        "fieldnotes-field-outlook-calendar",
    ) {
        Ok(request) => request,
        Err(code) => return ExitCode::from(code),
    };

    let mut emitter = Emitter::new(&request);

    if request.mode != CollectionMode::Incremental {
        crate::report(
            "fieldnotes-field-outlook-calendar: only incremental mode is supported; a windowed \
             calendarView collection can never prove it enumerated the whole calendar",
        );
        emitter.diagnostic(
            &request.run_id,
            Severity::Error,
            DiagnosticCode::ConfigInvalid,
            "snapshot mode is not supported by this Field's manifest",
        );
        return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
    }

    let source_scope = match config::resolve_scope(&request.config) {
        Ok(scope) => scope,
        Err(error) => {
            crate::report(&format!("fieldnotes-field-outlook-calendar: {error}"));
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::ConfigInvalid,
                &error.to_string(),
            );
            return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
        }
    };

    let token = match credential::obtain(&request) {
        Ok(token) => token,
        Err(failure) => {
            crate::report(&format!("fieldnotes-field-outlook-calendar: {failure}"));
            let (exit, code) = classify_credential_failure(&failure);
            emitter.diagnostic(&request.run_id, Severity::Error, code, &failure.to_string());
            return ExitCode::from(exit.as_raw());
        }
    };

    let observed_at = match observed_instant(observed_now_millis) {
        Ok(instant) => instant,
        Err(error) => {
            crate::report(&format!("fieldnotes-field-outlook-calendar: {error}"));
            return ExitCode::from(ProtocolExit::Internal.as_raw());
        }
    };

    let client = GraphClient::new(transport, clock, random);
    run_collection(
        &request,
        &source_scope,
        &token,
        &client,
        observed_at,
        &mut emitter,
    )
}

/// Renders the composition root's one wall-clock reading as an explicit-offset
/// instant, for a `@removed` item's `observed_at`.
fn observed_instant(unix_millis: u64) -> Result<OffsetDatetime, String> {
    let millis = i64::try_from(unix_millis).unwrap_or(i64::MAX);
    let datetime = fieldnotes_domain::Datetime::from_unix_millis(millis, 0)
        .map_err(|error| format!("the observed-at instant is out of range: {error}"))?;
    OffsetDatetime::parse(&datetime.to_string())
        .map_err(|error| format!("rendered observed-at instant failed its own guard: {error}"))
}

fn classify_credential_failure(
    failure: &credential::CredentialFailure,
) -> (ProtocolExit, DiagnosticCode) {
    use credential::CredentialFailure as F;
    match failure {
        F::NoGrant | F::MalformedChannel(_) | F::UnsupportedChannel(_) => {
            (ProtocolExit::ConfigInvalid, DiagnosticCode::ConfigInvalid)
        }
        F::Denied(_) => (
            ProtocolExit::Authorization,
            DiagnosticCode::PermissionAdminConsentRequired,
        ),
        F::Expired(_) => (ProtocolExit::Authentication, DiagnosticCode::AuthExpired),
        F::UnknownGrant(_) => (
            ProtocolExit::Authentication,
            DiagnosticCode::AuthReauthRequired,
        ),
        F::Unavailable(_) => (
            ProtocolExit::SourceUnavailable,
            DiagnosticCode::SourceUnavailable,
        ),
        F::Io(_) | F::Malformed(_) | F::UnexpectedMaterialKind => {
            (ProtocolExit::Internal, DiagnosticCode::InternalError)
        }
    }
}

fn classify_graph_error(error: &GraphError) -> (ProtocolExit, DiagnosticCode) {
    match error {
        GraphError::ReauthenticationRequired(_) => (
            ProtocolExit::Authentication,
            DiagnosticCode::AuthReauthRequired,
        ),
        GraphError::PermissionDenied(_) => (
            ProtocolExit::Authorization,
            DiagnosticCode::PermissionAdminConsentRequired,
        ),
        GraphError::Throttled(_) => (
            ProtocolExit::SourceUnavailable,
            DiagnosticCode::RateLimitThrottled,
        ),
        GraphError::ServiceUnavailable(_) | GraphError::Transport { .. } => (
            ProtocolExit::SourceUnavailable,
            DiagnosticCode::SourceUnavailable,
        ),
        GraphError::InvalidRequest(_)
        | GraphError::UntrustedContinuation { .. }
        | GraphError::MalformedResponse { .. } => {
            (ProtocolExit::Internal, DiagnosticCode::InternalError)
        }
    }
}

/// Runs one windowed-or-resumed delta collection against Graph.
fn run_collection<T, C, R>(
    request: &CollectRequest,
    source_scope: &fieldnotes_field_protocol::grammar::SourceScope,
    token: &AccessToken,
    client: &GraphClient<T, C, R>,
    observed_at: OffsetDatetime,
    emitter: &mut Emitter,
) -> ExitCode
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    let previous = request.cursor.as_ref().and_then(CursorState::decode);
    if request.cursor.is_some() && previous.is_none() {
        emitter.diagnostic(
            &request.run_id,
            Severity::Warning,
            DiagnosticCode::CursorResetRequired,
            "the previous cursor could not be read as this Field's own delta wrapper; a window \
             is required to start a fresh delta",
        );
    }

    let mut window_report: Option<(OffsetDatetime, OffsetDatetime)> = None;
    let mut stream = if let Some(state) = &previous {
        client.delta::<GraphEvent>(
            token,
            DeltaStart::Resume(state.delta_token.clone()),
            "list calendar events (resume)",
        )
    } else {
        let Some(window) = &request.window else {
            crate::report(
                "fieldnotes-field-outlook-calendar: a first (cursor-less) run requires an \
                 explicit window; this Field never enumerates the whole calendar unbounded",
            );
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::ConfigInvalid,
                "a first run requires collect_request.window",
            );
            return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
        };
        window_report = Some((window.from, window.to));
        let graph_request = initial_delta_request(window.from, window.to);
        client.list::<GraphEvent>(token, graph_request, "list calendar events (initial)")
    };

    let context = RecordContext {
        run_id: request.run_id.clone(),
        source_scope: source_scope.clone(),
    };

    let mut had_error = false;
    let mut graph_failure: Option<(ProtocolExit, DiagnosticCode)> = None;
    let mut last_record_seq: u64 = 0;
    let mut records_emitted: u64 = 0;

    for item in &mut stream {
        if records_emitted >= request.limits.max_run_records {
            break;
        }
        match item {
            Ok(event) => {
                let seq = emitter.next_seq();
                let built = if event.is_removed() {
                    build_delete(&context, seq, &event.id, observed_at)
                } else {
                    build_upsert(&context, seq, &event)
                };
                match built {
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
                            DiagnosticCode::ContentUnsupportedFormat,
                            &error.to_string(),
                        );
                    }
                }
            }
            Err(error) => {
                had_error = true;
                let classification = classify_graph_error(&error);
                graph_failure = Some(classification);
                emitter.diagnostic(
                    &request.run_id,
                    Severity::Error,
                    classification.1,
                    &error.to_string(),
                );
                break;
            }
        }
    }

    let is_complete = !had_error;
    if is_complete && stream.delta_token().is_none() {
        emitter.diagnostic(
            &request.run_id,
            Severity::Warning,
            DiagnosticCode::CursorResetRequired,
            "Graph's final page carried no @odata.deltaLink; the next run will start unbounded \
             again unless a previous cursor can still be re-offered",
        );
    }

    let new_state = if is_complete {
        stream
            .delta_token()
            .map(|token| CursorState {
                delta_token: token.clone(),
            })
            .or_else(|| previous.clone())
    } else {
        previous.clone()
    };

    let exit_code = if is_complete {
        ProtocolExit::Completed
    } else {
        graph_failure.map_or(ProtocolExit::Unclassified, |(exit, _)| exit)
    };

    if let Some(state) = new_state {
        let cursor_token = match encode_within_limit(&state, request.limits.max_cursor_bytes) {
            Some(token) => token,
            None => {
                emitter.diagnostic(
                    &request.run_id,
                    Severity::Error,
                    DiagnosticCode::InternalError,
                    "the delta cursor could not be encoded within this run's cursor bound",
                );
                return ExitCode::from(ProtocolExit::Internal.as_raw());
            }
        };
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
            window: window_report.map(|(from, to)| CheckpointWindow { from, through: to }),
            is_final: true,
        };
        emitter.checkpoint(checkpoint);
    }

    ExitCode::from(exit_code.as_raw())
}

/// Encodes `state`, self-policing against the run's cursor-byte bound.
///
/// Unlike the `local` Field's cursor, there is no meaningful way to shrink a
/// Graph delta link -- it is Graph's own opaque continuation, not a
/// tie-break set this Field controls the shape of -- so a delta link past
/// the bound is reported rather than truncated, which would corrupt it.
fn encode_within_limit(
    state: &CursorState,
    max_cursor_bytes: u64,
) -> Option<fieldnotes_field_protocol::grammar::Cursor> {
    let token = state.encode().ok()?;
    if u64::try_from(token.as_str().len()).unwrap_or(u64::MAX) <= max_cursor_bytes {
        Some(token)
    } else {
        None
    }
}

/// Writes protocol frames to standard output.
///
/// Delegates the run's diagnostic-, record-, and cursor-size self-policing,
/// and the "stop after the first write failure" rule, to
/// [`fieldnotes_field_sdk::emit::Emitter`]; this wrapper only adds this
/// Field's own convenience for building a [`DiagnosticEvent`] from primitive
/// severity/code/message arguments.
struct Emitter {
    inner: fieldnotes_field_sdk::emit::Emitter<std::io::Stdout>,
}

impl Emitter {
    fn new(request: &CollectRequest) -> Self {
        Emitter {
            inner: fieldnotes_field_sdk::emit::Emitter::new(std::io::stdout(), request.limits),
        }
    }

    fn next_seq(&mut self) -> u64 {
        self.inner.next_seq()
    }

    /// Writes a record, returning whether it was actually emitted.
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
            crate::report(&format!("fieldnotes-field-outlook-calendar: {error}"));
        }
    }
}
