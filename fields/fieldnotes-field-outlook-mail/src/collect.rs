//! The `collect` operation: a delta, windowed, or explicitly-recollecting read
//! of one mail folder, emitted as records, diagnostics, and one final
//! checkpoint.
//!
//! # Three plans, one cursor rule
//!
//! - **Delta** (no window, no recollection targets): a Graph delta collection,
//!   resumed from the cursor's delta token when there is one. This is the only
//!   plan that reports removals and the only one that can advance the cursor.
//! - **Windowed**: a `$filter`ed list bounded by the requested window. Graph's
//!   mail delta endpoint admits no `$filter`, so a window is a different read
//!   -- and a bounded one, whose coverage says nothing about the rest of the
//!   mailbox. It therefore re-offers the previous cursor **unchanged**.
//! - **Recollection** (ADR 0007): each named source key refetched by
//!   identifier, with every attachment re-evaluated against the run's
//!   *current* retention policy. Also bounded, so also cursor-freezing.
//!
//! The rule across all three: **the cursor advances only when this run proved
//! it saw everything the token will skip.** Anything else -- a Graph failure, a
//! stream cut short by the run's record bound, a bounded plan -- leaves the
//! cursor exactly where it was, and the next run re-collects. Re-emitting a
//! message already collected is idempotent through the portable exact-source
//! key; skipping one loses it permanently.
//!
//! # Why this run never authorizes deletion by absence
//!
//! This Field's manifest declares `deletion.snapshot: unsupported` and does
//! not offer `snapshot` mode, so no run of it can ever remove a Note by
//! absence, whatever it collected or failed to collect. A removal is only ever
//! reported explicitly, as a tombstone record built from Graph's own
//! `@removed` annotation.

use std::io::BufRead;
use std::process::ExitCode;

use fieldnotes_domain::{Clock, RandomSource};
use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
use fieldnotes_field_protocol::grammar::{
    CheckpointTag, DiagnosticTag, MessageText, ProtocolV1, RunId, SourceScope,
};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{
    CheckpointEvent, CheckpointWindow, CollectRequest, CollectionMode, DiagnosticEvent,
    RecordEvent, Severity, Window,
};
use fieldnotes_field_sdk::dispatch::read_collect_request;
use fieldnotes_msgraph::{AccessToken, GraphClient, GraphError, HttpTransport, RetryClock};

use crate::api::GraphMessage;
use crate::cursor::CursorState;
use crate::mail::MailReader;
use crate::record::{RecordContext, RecordError};

/// What a composition root must supply for a collect run to read Graph: a
/// client over some transport, and the access token for exactly this run's
/// requests.
pub(crate) struct GraphAccess<T, C, R> {
    /// The transport-backed client.
    pub(crate) client: GraphClient<T, C, R>,
    /// The bearer token, obtained on the protected channel by the real
    /// composition root and a non-secret placeholder in fixture mode.
    pub(crate) token: AccessToken,
}

/// Why a collect run could not even begin.
pub(crate) struct SetupFailure {
    /// The diagnostic code to report.
    pub(crate) code: DiagnosticCode,
    /// The exit code to end the run with.
    pub(crate) exit: ProtocolExit,
    /// An actionable, secret-free message.
    pub(crate) message: String,
}

/// Runs the collect operation end to end.
///
/// `setup` is called once, after the collection request has been read and
/// validated, because the credential grant it needs is *in* that request. It
/// is the only place a token is obtained, and this function never sees the
/// token's value: it only hands the resulting [`AccessToken`] to the reader.
pub(crate) fn run<T, C, R, W, F>(mut input: impl BufRead, wall: &W, setup: F) -> ExitCode
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
    W: Clock,
    F: FnOnce(&CollectRequest) -> Result<GraphAccess<T, C, R>, SetupFailure>,
{
    let request = match read_collect_request(
        &mut input,
        Limits::ceilings().max_frame_bytes,
        crate::FIELD_NAME,
    ) {
        Ok(request) => request,
        Err(code) => return ExitCode::from(code),
    };

    let mut emitter = Emitter::new(request.limits);

    let config = match crate::config::resolve(&request.config) {
        Ok(config) => config,
        Err(error) => {
            return fail_early(
                &mut emitter,
                &request.run_id,
                DiagnosticCode::ConfigInvalid,
                ProtocolExit::ConfigInvalid,
                &error.to_string(),
            );
        }
    };

    if request.mode != CollectionMode::Incremental {
        return fail_early(
            &mut emitter,
            &request.run_id,
            DiagnosticCode::CapabilityUnsupportedObject,
            ProtocolExit::ConfigInvalid,
            "this Field declares only incremental mode: a Graph delta collection reports removals \
             explicitly, so it never needs -- and never claims -- deletion by absence",
        );
    }

    let source_scope = match SourceScope::parse(&crate::scope::compute(&config.tenant_id)) {
        Ok(scope) => scope,
        Err(error) => {
            return fail_early(
                &mut emitter,
                &request.run_id,
                DiagnosticCode::InternalError,
                ProtocolExit::Internal,
                &format!("the computed source scope failed its own guard: {error}"),
            );
        }
    };

    let access = match setup(&request) {
        Ok(access) => access,
        Err(failure) => {
            return fail_early(
                &mut emitter,
                &request.run_id,
                failure.code,
                failure.exit,
                &failure.message,
            );
        }
    };

    let previous = match &request.cursor {
        Some(token) => match CursorState::decode(token) {
            Some(state) => state,
            None => {
                emitter.diagnostic(
                    &request.run_id,
                    Severity::Warning,
                    DiagnosticCode::CursorResetRequired,
                    "the previous cursor could not be read, so this run starts an unbounded delta \
                     collection: over-collection is safe and idempotent through the portable \
                     source key",
                    None,
                );
                CursorState::empty()
            }
        },
        None => CursorState::empty(),
    };

    let reader = MailReader::new(&access.client, &access.token, &config.mail_folder);
    let context = RecordContext {
        run_id: request.run_id.clone(),
        source_scope,
        limits: request.limits,
    };
    let staging_dir = std::path::PathBuf::from(&request.artifact_staging_dir);

    let mut state = RunState {
        had_error: false,
        staged_bytes: 0,
        exit: ProtocolExit::Completed,
    };

    let adopted = if let Some(targets) = request.recollect_targets.clone() {
        recollect(
            &reader,
            &context,
            &request,
            &staging_dir,
            &targets,
            &mut emitter,
            &mut state,
        );
        None
    } else if let Some(window) = request.window {
        collect_window(
            &reader,
            &context,
            &request,
            &staging_dir,
            window,
            &mut emitter,
            &mut state,
        );
        None
    } else {
        collect_delta(
            &reader,
            &context,
            &request,
            &staging_dir,
            &previous,
            wall,
            &mut emitter,
            &mut state,
        )
    };

    // The cursor advances only for an unwindowed delta collection that
    // reached its final page with zero errors. Everything else re-offers the
    // previous cursor exactly as it arrived.
    let next = match adopted {
        Some(token) if !state.had_error => CursorState::adopt(&token),
        _ => previous.clone(),
    };
    let cursor = match crate::cursor::encode_within_limit(&next, request.limits.max_cursor_bytes) {
        Some(cursor) => cursor,
        None => {
            return fail_early(
                &mut emitter,
                &request.run_id,
                DiagnosticCode::InternalError,
                ProtocolExit::Internal,
                "the resume cursor could not be encoded at all",
            );
        }
    };
    if next.is_resumable()
        && !CursorState::decode(&cursor).is_some_and(|state| state.is_resumable())
    {
        emitter.diagnostic(
            &request.run_id,
            Severity::Warning,
            DiagnosticCode::CursorResetRequired,
            "the delta token Graph returned does not fit this run's cursor bound, so it was \
             dropped rather than truncated; the next run starts an unbounded delta collection",
            None,
        );
    }

    emitter.checkpoint(
        &request.run_id,
        cursor,
        request.window.map(|window| CheckpointWindow {
            from: window.from,
            through: window.to,
        }),
    );

    ExitCode::from(state.exit.as_raw())
}

/// The mutable bookkeeping one run carries across its plan.
struct RunState {
    /// Whether anything failed in a way that must freeze the cursor.
    had_error: bool,
    /// Attachment bytes staged so far this run.
    staged_bytes: u64,
    /// The exit code the run will end with.
    exit: ProtocolExit,
}

impl RunState {
    /// Records a failure that freezes the cursor and ends the run non-zero.
    fn fail(&mut self, exit: ProtocolExit) {
        self.had_error = true;
        if self.exit == ProtocolExit::Completed {
            self.exit = exit;
        }
    }
}

/// Reports a failure that happened before any collection, and ends the run.
fn fail_early(
    emitter: &mut Emitter,
    run_id: &RunId,
    code: DiagnosticCode,
    exit: ProtocolExit,
    message: &str,
) -> ExitCode {
    crate::report(&format!("{}: {message}", crate::FIELD_NAME));
    emitter.diagnostic(run_id, Severity::Error, code, message, None);
    ExitCode::from(exit.as_raw())
}

/// Collects the delta stream, returning the delta token to adopt when the
/// stream completed.
#[allow(clippy::too_many_arguments)]
fn collect_delta<T, C, R, W>(
    reader: &MailReader<'_, T, C, R>,
    context: &RecordContext,
    request: &CollectRequest,
    staging_dir: &std::path::Path,
    previous: &CursorState,
    wall: &W,
    emitter: &mut Emitter,
    state: &mut RunState,
) -> Option<fieldnotes_msgraph::DeltaToken>
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
    W: Clock,
{
    let mut stream = reader.delta(previous.resume_token());
    let mut exhausted = true;
    loop {
        if !emitter.can_record() {
            // The run's record ceiling stopped this stream short, so it has
            // not seen the whole delta and must not adopt a token.
            emitter.diagnostic(
                &request.run_id,
                Severity::Info,
                DiagnosticCode::ContentSkipped,
                "this run reached its record bound, so the cursor was not advanced and the next \
                 run resumes from the same point",
                None,
            );
            exhausted = false;
            break;
        }
        let Some(item) = stream.next() else { break };
        match item {
            Ok(message) => {
                if message.is_removed() {
                    if message.is_authoritative_deletion() {
                        emit_tombstone(&message, context, request, wall, emitter, state);
                    } else {
                        // The item left the collected folder without Graph
                        // stating that it was deleted, so it is very likely
                        // still in the mailbox under another folder. Absence
                        // from this folder is not deletion.
                        emitter.diagnostic(
                            &request.run_id,
                            Severity::Info,
                            DiagnosticCode::ContentSkipped,
                            "a message left the collected folder without Graph reporting it as \
                             deleted, so no tombstone was emitted: absence from one folder is not \
                             deletion",
                            None,
                        );
                    }
                } else {
                    emit_upsert(
                        &message,
                        reader,
                        context,
                        request,
                        staging_dir,
                        emitter,
                        state,
                    );
                }
            }
            Err(error) => {
                report_graph_failure(&error, request, emitter, state);
                exhausted = false;
                break;
            }
        }
    }
    if exhausted {
        stream.delta_token().cloned()
    } else {
        None
    }
}

/// Collects the messages inside one bounded window.
#[allow(clippy::too_many_arguments)]
fn collect_window<T, C, R>(
    reader: &MailReader<'_, T, C, R>,
    context: &RecordContext,
    request: &CollectRequest,
    staging_dir: &std::path::Path,
    window: Window,
    emitter: &mut Emitter,
    state: &mut RunState,
) where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    let stream = match reader.window(&window) {
        Ok(stream) => stream,
        Err(error) => {
            state.fail(ProtocolExit::Internal);
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::InternalError,
                &format!("the windowed request could not be built: {error}"),
                None,
            );
            return;
        }
    };
    for item in stream {
        if !emitter.can_record() {
            emitter.diagnostic(
                &request.run_id,
                Severity::Info,
                DiagnosticCode::ContentSkipped,
                "this run reached its record bound before the end of the requested window",
                None,
            );
            break;
        }
        match item {
            // A plain list never reports a removal; a `@removed` annotation
            // cannot appear outside a delta response. If one somehow did, it
            // is not authoritative evidence from a bounded read, so it is
            // ignored rather than acted on.
            Ok(message) if message.is_removed() => {}
            Ok(message) => {
                emit_upsert(
                    &message,
                    reader,
                    context,
                    request,
                    staging_dir,
                    emitter,
                    state,
                );
            }
            Err(error) => {
                report_graph_failure(&error, request, emitter, state);
                break;
            }
        }
    }
}

/// Refetches each explicitly named source key (ADR 0007).
#[allow(clippy::too_many_arguments)]
fn recollect<T, C, R>(
    reader: &MailReader<'_, T, C, R>,
    context: &RecordContext,
    request: &CollectRequest,
    staging_dir: &std::path::Path,
    targets: &[fieldnotes_field_protocol::message::RecollectTarget],
    emitter: &mut Emitter,
    state: &mut RunState,
) where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    for target in targets {
        if !emitter.can_record() {
            break;
        }
        if target.scope.as_str() != context.source_scope.as_str() {
            emitter.diagnostic(
                &request.run_id,
                Severity::Warning,
                DiagnosticCode::ContentSkipped,
                "a recollection target named a source scope this run does not cover",
                None,
            );
            continue;
        }
        let prefix = format!("{}/", crate::constants::OBJECT_KIND_MAIL_MESSAGE);
        let Some(message_id) = target.identity.as_str().strip_prefix(&prefix) else {
            emitter.diagnostic(
                &request.run_id,
                Severity::Warning,
                DiagnosticCode::ContentSkipped,
                "a recollection target named an identity outside this Field's object kind",
                None,
            );
            continue;
        };
        match reader.message(message_id) {
            Ok(message) => {
                emit_upsert(
                    &message,
                    reader,
                    context,
                    request,
                    staging_dir,
                    emitter,
                    state,
                );
            }
            Err(error) => {
                // A message that can no longer be read is **not** evidence
                // that it was deleted: only a delta `@removed` annotation is.
                report_graph_failure(&error, request, emitter, state);
            }
        }
    }
}

/// Emits one message's upsert record, collecting its attachments first so the
/// record can reference them.
fn emit_upsert<T, C, R>(
    message: &GraphMessage,
    reader: &MailReader<'_, T, C, R>,
    context: &RecordContext,
    request: &CollectRequest,
    staging_dir: &std::path::Path,
    emitter: &mut Emitter,
    state: &mut RunState,
) where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    let Some(seq) = emitter.reserve() else {
        return;
    };
    let Some(message_id) = message.id.as_deref().filter(|id| !id.is_empty()) else {
        emitter.diagnostic_at(
            seq,
            &request.run_id,
            Severity::Warning,
            DiagnosticCode::ContentSkipped,
            "a message arrived with no identifier, so it has no portable source key and was \
             skipped",
            None,
        );
        return;
    };

    let outcome = if message.has_attachments == Some(true) {
        crate::attachment::collect(
            reader,
            message_id,
            seq,
            staging_dir,
            &request.limits,
            &request.artifact_media_types,
            state.staged_bytes,
        )
    } else {
        crate::attachment::AttachmentOutcome::default()
    };
    state.staged_bytes = state.staged_bytes.saturating_add(outcome.staged_bytes);

    match crate::record::build_upsert(context, seq, message, outcome.artifacts, &outcome.evidence) {
        Ok(record) => {
            emitter.record(record);
            report_attachment_issues(&outcome.issues, request, emitter, state);
        }
        Err(RecordError(reason)) => {
            emitter.diagnostic_at(
                seq,
                &request.run_id,
                Severity::Warning,
                DiagnosticCode::ContentSkipped,
                &reason,
                None,
            );
            report_attachment_issues(&outcome.issues, request, emitter, state);
        }
    }
}

/// Emits one authoritative tombstone for a message Graph reported as removed.
fn emit_tombstone<W: Clock>(
    message: &GraphMessage,
    context: &RecordContext,
    request: &CollectRequest,
    wall: &W,
    emitter: &mut Emitter,
    state: &mut RunState,
) {
    let Some(seq) = emitter.reserve() else {
        return;
    };
    let Some(message_id) = message.id.as_deref().filter(|id| !id.is_empty()) else {
        emitter.diagnostic_at(
            seq,
            &request.run_id,
            Severity::Warning,
            DiagnosticCode::ContentSkipped,
            "a removal annotation arrived with no identifier, so there is no source key to remove",
            None,
        );
        return;
    };
    match crate::record::build_tombstone(context, seq, message_id, wall.unix_millis()) {
        Ok(record) => {
            emitter.record(record);
        }
        Err(RecordError(reason)) => {
            state.fail(ProtocolExit::Internal);
            emitter.diagnostic_at(
                seq,
                &request.run_id,
                Severity::Error,
                DiagnosticCode::InternalError,
                &reason,
                None,
            );
        }
    }
}

/// Reports one Graph failure, freezing the cursor and setting the run's exit
/// code.
fn report_graph_failure(
    error: &GraphError,
    request: &CollectRequest,
    emitter: &mut Emitter,
    state: &mut RunState,
) {
    let classified = crate::errors::classify(error);
    state.fail(classified.exit);
    crate::report(&format!("{}: {}", crate::FIELD_NAME, classified.message));
    emitter.diagnostic(
        &request.run_id,
        classified.severity,
        classified.code,
        &classified.message,
        classified.retry_after_seconds,
    );
}

/// Reports the per-attachment problems one message produced.
///
/// A transport failure freezes the cursor, because the next run can and should
/// try those bytes again. A permanent problem -- an undecodable payload, an
/// attachment with no identifier, more attachments than the run's bound admits
/// -- does not: the attachment is already recorded as not retained, so nothing
/// is lost silently, re-collecting the message would produce the identical
/// result, and ADR 0007's explicit recollection exists precisely to revisit an
/// attachment later. A cursor frozen on a permanently-undecodable attachment
/// would never advance again.
fn report_attachment_issues(
    issues: &[crate::attachment::AttachmentError],
    request: &CollectRequest,
    emitter: &mut Emitter,
    state: &mut RunState,
) {
    for issue in issues {
        match issue {
            crate::attachment::AttachmentError::Graph(error) => {
                report_graph_failure(error, request, emitter, state);
            }
            other => {
                emitter.diagnostic(
                    &request.run_id,
                    Severity::Warning,
                    DiagnosticCode::ContentSkipped,
                    &other.to_string(),
                    None,
                );
            }
        }
    }
}

/// Writes protocol frames to standard output.
///
/// Beyond [`fieldnotes_field_sdk::emit::Emitter`]'s own self-policing, this
/// wrapper owns the **sequence-number discipline** A2 requires: `seq` increases
/// by exactly one across every event of a run, so a sequence number must never
/// be allocated for a frame that then does not get written. Every count-based
/// ceiling is therefore checked *before* a number is taken, and the inner
/// emitter is constructed with those counts raised so it can never refuse a
/// frame this wrapper has already committed to writing.
struct Emitter {
    inner: fieldnotes_field_sdk::emit::Emitter<std::io::Stdout>,
    max_run_records: u64,
    records_emitted: u64,
    max_run_diagnostics: u64,
    diagnostics_emitted: u64,
    last_record_seq: u64,
    max_message_bytes: u64,
}

impl Emitter {
    fn new(limits: Limits) -> Self {
        let uncounted = Limits {
            max_run_records: u64::MAX,
            max_run_diagnostics: u64::MAX,
            ..limits
        };
        Emitter {
            inner: fieldnotes_field_sdk::emit::Emitter::new(std::io::stdout(), uncounted),
            max_run_records: limits.max_run_records,
            records_emitted: 0,
            max_run_diagnostics: limits.max_run_diagnostics,
            diagnostics_emitted: 0,
            last_record_seq: 0,
            max_message_bytes: u64::try_from(MessageText::MAX_BYTES).unwrap_or(u64::MAX),
        }
    }

    /// Whether another record may still be emitted this run.
    fn can_record(&self) -> bool {
        self.records_emitted < self.max_run_records
            && self.diagnostics_emitted < self.max_run_diagnostics
    }

    /// Reserves the sequence number for one record-or-its-diagnostic.
    ///
    /// Both outcomes write exactly one frame, so the number is never wasted.
    fn reserve(&mut self) -> Option<u64> {
        if !self.can_record() {
            return None;
        }
        Some(self.inner.next_seq())
    }

    fn record(&mut self, record: RecordEvent) {
        let seq = record.seq;
        let already_failed = self.inner.write_failed();
        if self.inner.record(record) {
            self.records_emitted += 1;
            self.last_record_seq = seq;
        }
        self.report_if_newly_failed(already_failed);
    }

    fn diagnostic(
        &mut self,
        run_id: &RunId,
        severity: Severity,
        code: DiagnosticCode,
        message: &str,
        retry_after_seconds: Option<u32>,
    ) {
        if self.diagnostics_emitted >= self.max_run_diagnostics {
            return;
        }
        let seq = self.inner.next_seq();
        self.diagnostic_at(seq, run_id, severity, code, message, retry_after_seconds);
    }

    fn diagnostic_at(
        &mut self,
        seq: u64,
        run_id: &RunId,
        severity: Severity,
        code: DiagnosticCode,
        message: &str,
        retry_after_seconds: Option<u32>,
    ) {
        let (truncated, _) =
            fieldnotes_field_sdk::truncate::truncate_utf8(message, self.max_message_bytes);
        // `MessageText` admits no empty string, and a reserved sequence number
        // must always be filled, so an empty message becomes a fixed
        // non-empty one rather than a skipped frame and a sequence gap.
        let text = match MessageText::parse(&truncated) {
            Ok(text) => text,
            Err(_) => {
                match MessageText::parse("this Field reported a diagnostic it could not render") {
                    Ok(text) => text,
                    Err(_) => return,
                }
            }
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
            retry_after_seconds,
            detail: None,
            redacted: None,
        };
        let already_failed = self.inner.write_failed();
        if self.inner.diagnostic(diagnostic) {
            self.diagnostics_emitted += 1;
        }
        self.report_if_newly_failed(already_failed);
    }

    /// Emits the run's single final checkpoint.
    ///
    /// One checkpoint, not several: a Graph delta token exists only once the
    /// final page has arrived, so an intermediate checkpoint could offer
    /// nothing but the cursor this run started from, which advances nothing and
    /// costs core a durability barrier for no gain.
    fn checkpoint(
        &mut self,
        run_id: &RunId,
        cursor: fieldnotes_field_protocol::grammar::Cursor,
        window: Option<CheckpointWindow>,
    ) {
        let seq = self.inner.next_seq();
        let checkpoint = CheckpointEvent {
            v: ProtocolV1,
            frame_type: CheckpointTag,
            run_id: run_id.clone(),
            seq,
            cursor,
            cursor_format_version: crate::constants::CURSOR_FORMAT_VERSION,
            covers_record_seq_through: self.last_record_seq,
            records_covered: self.records_emitted,
            snapshot: None,
            window,
            is_final: true,
        };
        let already_failed = self.inner.write_failed();
        self.inner.checkpoint(checkpoint);
        self.report_if_newly_failed(already_failed);
    }

    fn report_if_newly_failed(&mut self, already_failed: bool) {
        if !already_failed
            && self.inner.write_failed()
            && let Some(error) = self.inner.last_write_error()
        {
            crate::report(&format!("{}: {error}", crate::FIELD_NAME));
        }
    }
}
