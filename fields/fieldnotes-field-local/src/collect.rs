//! The `collect` operation: incremental and snapshot walks of the configured
//! root, emitted as records, checkpoints, and diagnostics.
//!
//! # Snapshot completeness versus a partial result
//!
//! A run's walk either enumerates its whole scope or it does not
//! ([`WalkOutcome::is_complete`]), and building a record for a file this
//! Field could no longer read by the time it tried counts as exactly the
//! same kind of failure. **Only when both hold with zero exceptions does
//! this run advance its resume cursor and, in `snapshot` mode, claim
//! `state: "complete"`.** Any single read error anywhere freezes the cursor
//! at its previous value and, in `snapshot` mode, claims `state: "partial"`
//! instead -- which A2 section 10 already treats as never authorizing
//! deletion, independently of the error-severity diagnostic this Field also
//! emits for the same reason. A partial result is never silently promoted to
//! a complete one.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::Path;
use std::process::ExitCode;

use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
use fieldnotes_field_protocol::framing::FrameWriter;
use fieldnotes_field_protocol::grammar::{
    CheckpointTag, Cursor as CursorToken, DiagnosticTag, MessageText, ProtocolV1, RunId,
    SourceScope,
};
use fieldnotes_field_protocol::host::read_core_frame;
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{
    CheckpointEvent, CollectRequest, CollectionMode, CoreFrame, DiagnosticEvent, FieldEvent,
    RecordEvent, Severity, SnapshotClaim, SnapshotState,
};

use crate::cursor::CursorState;
use crate::record::{RecordContext, build as build_record};
use crate::walk::{WalkEntry, WalkIssue, WalkOutcome, walk};
use crate::{config, scope};

/// Runs the collect operation end to end.
pub(crate) fn run(mut input: impl BufRead) -> ExitCode {
    let request = match read_core_frame(&mut input, Limits::ceilings().max_frame_bytes) {
        Ok(Some(CoreFrame::Collect(request))) => *request,
        Ok(Some(_)) => {
            crate::report(
                "fieldnotes-field-local: a collect run expects a collect_request on standard \
                 input first",
            );
            return ExitCode::from(ProtocolExit::Usage.as_raw());
        }
        Ok(None) => {
            crate::report(
                "fieldnotes-field-local: standard input closed before any request arrived",
            );
            return ExitCode::from(ProtocolExit::Usage.as_raw());
        }
        Err(error) => {
            crate::report(&format!(
                "fieldnotes-field-local: the collection request did not validate: {error}"
            ));
            return ExitCode::from(ProtocolExit::Usage.as_raw());
        }
    };

    let mut emitter = Emitter::new(&request);

    let root = match config::resolve_root(&request.config) {
        Ok(root) => root,
        Err(error) => {
            crate::report(&format!("fieldnotes-field-local: {error}"));
            emitter.diagnostic(
                &request.run_id,
                Severity::Error,
                DiagnosticCode::ConfigInvalid,
                &error.to_string(),
            );
            return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
        }
    };

    let source_scope_text = scope::compute(&root);
    let source_scope = match SourceScope::parse(&source_scope_text) {
        Ok(scope) => scope,
        Err(error) => {
            crate::report(&format!(
                "fieldnotes-field-local: the computed source scope failed its own guard: {error}"
            ));
            return ExitCode::from(ProtocolExit::Internal.as_raw());
        }
    };

    let outcome = walk(&root);

    match request.mode {
        CollectionMode::Incremental => {
            run_walk_mode(&request, &source_scope, outcome, None, &mut emitter)
        }
        CollectionMode::Snapshot => {
            let Some(requested_scope) = request.snapshot_scope.clone() else {
                crate::report(
                    "fieldnotes-field-local: snapshot mode requires an explicit snapshot_scope",
                );
                return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
            };
            run_walk_mode(
                &request,
                &source_scope,
                outcome,
                Some(requested_scope.as_str().to_owned()),
                &mut emitter,
            )
        }
    }
}

/// Runs one walk-driven collection, shared by `incremental` and `snapshot`
/// mode. `snapshot_scope` is `Some` exactly for a snapshot run.
fn run_walk_mode(
    request: &CollectRequest,
    source_scope: &SourceScope,
    outcome: WalkOutcome,
    snapshot_scope: Option<String>,
    emitter: &mut Emitter,
) -> ExitCode {
    let is_snapshot = snapshot_scope.is_some();
    let previous = match &request.cursor {
        Some(token) => match CursorState::decode(token) {
            Some(state) => state,
            None => {
                emitter.diagnostic(
                    &request.run_id,
                    Severity::Warning,
                    DiagnosticCode::CursorResetRequired,
                    "the previous cursor could not be read; starting an unbounded walk",
                );
                CursorState::default()
            }
        },
        None => CursorState::default(),
    };

    let mut had_error = !outcome.is_complete();
    emit_walk_diagnostics(emitter, &request.run_id, &outcome.issues);

    let media_policy = request.artifact_media_types.clone();
    let context = RecordContext {
        run_id: request.run_id.clone(),
        source_scope: source_scope.clone(),
        staging_dir: Path::new(&request.artifact_staging_dir),
        limits: request.limits,
        media_policy: &media_policy,
    };

    let due: Vec<&WalkEntry> = outcome
        .entries
        .iter()
        .filter(|entry| {
            is_snapshot || previous.is_due(entry.modified_unix_seconds, &entry.relative_path)
        })
        .collect();

    let mut last_record_seq: u64 = 0;
    let mut records_emitted: u64 = 0;
    for entry in due {
        if records_emitted >= request.limits.max_run_records {
            break;
        }
        let seq = emitter.next_seq();
        match build_record(&context, seq, entry) {
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

    let is_complete = !had_error;
    let observed = observed_from(&outcome.entries);
    let new_state = if is_complete {
        CursorState::advance(&previous, observed)
    } else {
        previous.clone()
    };
    let cursor_token = encode_within_limit(&new_state, request.limits.max_cursor_bytes);

    let snapshot_claim = snapshot_scope.map(|scope_text| SnapshotClaim {
        scope: parse_snapshot_scope(&scope_text),
        state: if is_complete {
            SnapshotState::Complete
        } else {
            SnapshotState::Partial
        },
        objects_enumerated: Some(u64::try_from(outcome.entries.len()).unwrap_or(u64::MAX)),
    });

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
        snapshot: snapshot_claim,
        window: None,
        is_final: true,
    };
    emitter.checkpoint(checkpoint);

    if is_complete {
        ExitCode::from(ProtocolExit::Completed.as_raw())
    } else {
        ExitCode::from(ProtocolExit::Unclassified.as_raw())
    }
}

fn parse_snapshot_scope(text: &str) -> fieldnotes_field_protocol::grammar::SnapshotScope {
    fieldnotes_field_protocol::grammar::SnapshotScope::parse(text).unwrap_or_else(|error| {
        panic!("a snapshot scope core itself sent must satisfy its own guard: {error}")
    })
}

/// Emits one diagnostic per walk issue, honouring the run's diagnostic bound.
fn emit_walk_diagnostics(emitter: &mut Emitter, run_id: &RunId, issues: &[WalkIssue]) {
    for issue in issues {
        match issue {
            WalkIssue::SymlinkSkipped { relative_path } => {
                emitter.diagnostic(
                    run_id,
                    Severity::Info,
                    DiagnosticCode::ContentSkipped,
                    &format!(
                        "skipped symlink at {relative_path}: this Field never follows a symlink \
                         out of the configured root"
                    ),
                );
            }
            WalkIssue::Unreadable {
                relative_path,
                reason,
            } => {
                let where_ = relative_path.as_deref().unwrap_or("<root>");
                emitter.diagnostic(
                    run_id,
                    Severity::Error,
                    DiagnosticCode::SourceUnavailable,
                    &format!("could not read {where_}: {reason}"),
                );
            }
        }
    }
}

/// The maximum modification instant observed across every walked file, and
/// every relative path sitting exactly at it.
fn observed_from(entries: &[WalkEntry]) -> Option<(i64, BTreeSet<String>)> {
    let max = entries
        .iter()
        .map(|entry| entry.modified_unix_seconds)
        .max()?;
    let at_max = entries
        .iter()
        .filter(|entry| entry.modified_unix_seconds == max)
        .map(|entry| entry.relative_path.clone())
        .collect();
    Some((max, at_max))
}

/// Encodes `state`, self-policing against the run's cursor-byte bound rather
/// than discovering the limit by being rejected.
fn encode_within_limit(state: &CursorState, max_cursor_bytes: u64) -> CursorToken {
    if let Ok(token) = state.encode()
        && u64::try_from(token.as_str().len()).unwrap_or(u64::MAX) <= max_cursor_bytes
    {
        return token;
    }
    let widened = CursorState {
        high_water: state.high_water,
        at_high_water: BTreeSet::new(),
        wide: true,
    };
    widened
        .encode()
        .unwrap_or_else(|error| panic!("a widened cursor must always encode: {error}"))
}

/// Writes protocol frames to standard output, self-policing against the
/// run's declared diagnostic and record bounds.
struct Emitter {
    writer: FrameWriter<std::io::Stdout>,
    seq: u64,
    max_diagnostics: u64,
    diagnostics_emitted: u64,
    write_failed: bool,
}

impl Emitter {
    fn new(request: &CollectRequest) -> Self {
        Emitter {
            writer: FrameWriter::new(std::io::stdout(), request.limits.max_frame_bytes),
            seq: 0,
            max_diagnostics: request.limits.max_run_diagnostics,
            diagnostics_emitted: 0,
            write_failed: false,
        }
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Writes a record, returning whether it was actually emitted.
    fn record(&mut self, record: RecordEvent) -> bool {
        self.write(FieldEvent::Record(Box::new(record)))
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
        if self.diagnostics_emitted >= self.max_diagnostics {
            return;
        }
        let (truncated, _) = crate::record::truncate(message, 4096);
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
        if self.write(FieldEvent::Diagnostic(Box::new(diagnostic))) {
            self.diagnostics_emitted += 1;
        }
    }

    fn checkpoint(&mut self, checkpoint: CheckpointEvent) {
        self.write(FieldEvent::Checkpoint(Box::new(checkpoint)));
    }

    fn write(&mut self, event: FieldEvent) -> bool {
        if self.write_failed {
            return false;
        }
        if let Err(error) = self.writer.write_event(&event) {
            crate::report(&format!("fieldnotes-field-local: {error}"));
            self.write_failed = true;
            return false;
        }
        true
    }
}
