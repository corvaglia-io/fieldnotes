//! A frame emitter that self-polices against one run's declared limits.
//!
//! [`fieldnotes_field_protocol::framing::FrameWriter`] already refuses to
//! write a single frame past the per-frame byte ceiling. [`Emitter`] adds the
//! run-level bookkeeping every Field needs on top of that: it assigns the
//! per-run monotonic sequence number, stops counting -- and stops writing --
//! records and diagnostics once the run's declared `max_run_records` or
//! `max_run_diagnostics` is reached, refuses to write a checkpoint whose
//! cursor exceeds the run's declared `max_cursor_bytes`, and, once any frame
//! write fails, stops writing anything else for the rest of the run rather
//! than continuing to call a sink that has already demonstrated it cannot be
//! written to -- [`Emitter::last_write_error`] keeps that failure available
//! for a caller that wants to log or report it.
//!
//! # Escape hatches, deliberately
//!
//! A conformance counterparty needs to emit exactly the malformed, oversized,
//! or otherwise hostile output a well-behaved Field would never produce, so
//! that core's rejection of it is actually exercised. [`Emitter::raw_json`]
//! and [`Emitter::raw_bytes`] write to the same sink without checking
//! anything at all -- not size, not shape, not JSON validity, not any of the
//! self-policing above -- because making that impossible to express would
//! make the misbehaving side of the protocol untestable. A Field's ordinary
//! collection path never needs them; [`Emitter::checked_json`] is the
//! well-behaved middle ground, for a caller that builds a frame as a raw
//! [`serde_json::Value`] but still wants it decoded through the protocol's
//! own types -- and therefore refused if malformed -- before it is written.

use std::io::Write;

use fieldnotes_field_protocol::framing::{FrameError, FrameWriter};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{
    CheckpointEvent, DiagnosticEvent, FieldEvent, RecordEvent,
};

/// Writes Field-to-core frames, self-policing against one run's declared
/// limits.
#[derive(Debug)]
pub struct Emitter<W: Write> {
    sink: W,
    max_frame_bytes: u64,
    seq: u64,
    max_run_diagnostics: u64,
    diagnostics_emitted: u64,
    max_run_records: u64,
    records_emitted: u64,
    max_cursor_bytes: u64,
    write_failed: bool,
    last_write_error: Option<FrameError>,
}

impl<W: Write> Emitter<W> {
    /// Wraps `sink`, self-policing against `limits` for the rest of this run.
    #[must_use]
    pub fn new(sink: W, limits: Limits) -> Self {
        Emitter {
            sink,
            max_frame_bytes: limits.max_frame_bytes,
            seq: 0,
            max_run_diagnostics: limits.max_run_diagnostics,
            diagnostics_emitted: 0,
            max_run_records: limits.max_run_records,
            records_emitted: 0,
            max_cursor_bytes: limits.max_cursor_bytes,
            write_failed: false,
            last_write_error: None,
        }
    }

    /// Allocates the next per-run monotonic sequence number.
    ///
    /// A v1 sequence number starts at 1, so the first call after
    /// construction returns `1`.
    pub fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Writes a record, refusing once the run's declared record ceiling is
    /// already reached or a previous write already failed.
    ///
    /// Returns whether the record was actually emitted, so a caller that
    /// tracks its own "last record covered" bookkeeping for a checkpoint
    /// knows whether this one counts.
    pub fn record(&mut self, record: RecordEvent) -> bool {
        if self.records_emitted >= self.max_run_records {
            return false;
        }
        let emitted = self.write(&FieldEvent::Record(Box::new(record)));
        if emitted {
            self.records_emitted += 1;
        }
        emitted
    }

    /// Writes a diagnostic, refusing once the run's declared diagnostic
    /// ceiling is already reached or a previous write already failed.
    pub fn diagnostic(&mut self, diagnostic: DiagnosticEvent) -> bool {
        if self.diagnostics_emitted >= self.max_run_diagnostics {
            return false;
        }
        let emitted = self.write(&FieldEvent::Diagnostic(Box::new(diagnostic)));
        if emitted {
            self.diagnostics_emitted += 1;
        }
        emitted
    }

    /// Writes a checkpoint, refusing one whose cursor exceeds the run's
    /// declared `max_cursor_bytes` rather than emit a frame the run's own
    /// limits already rule out.
    ///
    /// A Field with its own cursor-shrinking strategy -- widening a tie-break
    /// set to a single flag, say -- applies that strategy itself before
    /// calling this; this check is the backstop for when it did not, not the
    /// mechanism for choosing what to drop.
    pub fn checkpoint(&mut self, checkpoint: CheckpointEvent) -> bool {
        let cursor_bytes = u64::try_from(checkpoint.cursor.as_str().len()).unwrap_or(u64::MAX);
        if cursor_bytes > self.max_cursor_bytes {
            return false;
        }
        self.write(&FieldEvent::Checkpoint(Box::new(checkpoint)))
    }

    /// Writes a manifest, the sole answer to a describe run.
    pub fn manifest(&mut self, manifest: fieldnotes_field_protocol::message::Manifest) -> bool {
        self.write(&FieldEvent::Manifest(Box::new(manifest)))
    }

    /// Whether a frame write has already failed.
    ///
    /// Once true, every further call to [`Emitter::record`],
    /// [`Emitter::diagnostic`], [`Emitter::checkpoint`], [`Emitter::manifest`],
    /// or [`Emitter::checked_json`] returns `false` without attempting to
    /// write, and every count-based limit above stays frozen at whatever it
    /// last reached.
    #[must_use]
    pub fn write_failed(&self) -> bool {
        self.write_failed
    }

    /// The error from the most recent failed write, for a caller that wants
    /// to log or report it. `None` until the first write fails, and
    /// unchanged afterward, since no further write is even attempted.
    #[must_use]
    pub fn last_write_error(&self) -> Option<&FrameError> {
        self.last_write_error.as_ref()
    }

    /// Decodes `value` through [`FieldEvent::decode`] and writes the
    /// re-encoded, self-policed frame, so a caller that builds a frame as raw
    /// JSON -- for terser test fixtures, say -- still cannot emit one the
    /// protocol's own types would refuse.
    ///
    /// Dispatches to [`Emitter::record`], [`Emitter::diagnostic`],
    /// [`Emitter::checkpoint`], or [`Emitter::manifest`] once decoded, so the
    /// same limits and bookkeeping apply as if the caller had built the typed
    /// event itself.
    pub fn checked_json(&mut self, value: serde_json::Value) -> bool {
        match FieldEvent::decode(value) {
            Ok(FieldEvent::Record(record)) => self.record(*record),
            Ok(FieldEvent::Diagnostic(diagnostic)) => self.diagnostic(*diagnostic),
            Ok(FieldEvent::Checkpoint(checkpoint)) => self.checkpoint(*checkpoint),
            Ok(FieldEvent::Manifest(manifest)) => self.manifest(*manifest),
            Err(_) => false,
        }
    }

    /// Writes `value` verbatim: JSON-encoded and newline-terminated, but with
    /// no size check, no schema validation, and none of this type's
    /// self-policing. See the module documentation: this is how a
    /// deliberately misbehaving Field expresses that on purpose.
    pub fn raw_json(&mut self, value: &serde_json::Value) {
        if let Ok(mut bytes) = serde_json::to_vec(value) {
            bytes.push(b'\n');
            self.raw_bytes(&bytes);
        }
    }

    /// Writes `bytes` verbatim: no framing, no terminator, no validation of
    /// any kind. See the module documentation.
    pub fn raw_bytes(&mut self, bytes: &[u8]) {
        let _ = self.sink.write_all(bytes);
        let _ = self.sink.flush();
    }

    fn write(&mut self, event: &FieldEvent) -> bool {
        if self.write_failed {
            return false;
        }
        // A fresh `FrameWriter` borrowing the sink for one call, rather than
        // one stored for the `Emitter`'s lifetime, so `raw_bytes` above can
        // still reach the same sink directly without the two writers
        // fighting over ownership of it.
        let mut writer = FrameWriter::new(&mut self.sink, self.max_frame_bytes);
        match writer.write_event(event) {
            Ok(_) => true,
            Err(error) => {
                self.write_failed = true;
                self.last_write_error = Some(error);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Emitter;
    use fieldnotes_field_protocol::codes::DiagnosticCode;
    use fieldnotes_field_protocol::grammar::{
        CheckpointTag, Cursor, DiagnosticTag, ProtocolV1, RunId,
    };
    use fieldnotes_field_protocol::limits::Limits;
    use fieldnotes_field_protocol::message::{CheckpointEvent, DiagnosticEvent, Severity};

    fn run_id() -> RunId {
        RunId::parse("1a4c9f2e-0000-4000-8000-000000000001")
            .unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    fn diagnostic(seq: u64) -> DiagnosticEvent {
        DiagnosticEvent {
            v: ProtocolV1,
            frame_type: DiagnosticTag,
            run_id: run_id(),
            seq,
            severity: Severity::Info,
            code: DiagnosticCode::ContentSkipped,
            message: fieldnotes_field_protocol::grammar::MessageText::parse("skipped")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            source: None,
            object_kind: None,
            retry_after_seconds: None,
            detail: None,
            redacted: None,
        }
    }

    fn checkpoint(seq: u64, cursor: &str) -> CheckpointEvent {
        CheckpointEvent {
            v: ProtocolV1,
            frame_type: CheckpointTag,
            run_id: run_id(),
            seq,
            cursor: Cursor::parse(cursor).unwrap_or_else(|error| panic!("must parse: {error}")),
            cursor_format_version: 1,
            covers_record_seq_through: 0,
            records_covered: 0,
            snapshot: None,
            window: None,
            is_final: true,
        }
    }

    #[test]
    fn seq_starts_at_one_and_increments() {
        let mut emitter = Emitter::new(Vec::new(), Limits::ceilings());
        assert_eq!(emitter.next_seq(), 1);
        assert_eq!(emitter.next_seq(), 2);
        assert_eq!(emitter.next_seq(), 3);
    }

    #[test]
    fn a_diagnostic_past_the_run_ceiling_is_refused() {
        let limits = Limits {
            max_run_diagnostics: 1,
            ..Limits::ceilings()
        };
        let mut emitter = Emitter::new(Vec::new(), limits);
        assert!(emitter.diagnostic(diagnostic(1)));
        assert!(!emitter.diagnostic(diagnostic(2)));
    }

    #[test]
    fn a_checkpoint_whose_cursor_exceeds_the_run_bound_is_refused() {
        let limits = Limits {
            max_cursor_bytes: 4,
            ..Limits::ceilings()
        };
        let mut emitter = Emitter::new(Vec::new(), limits);
        assert!(!emitter.checkpoint(checkpoint(1, "way-too-long-for-four-bytes")));
    }

    #[test]
    fn a_checkpoint_within_the_cursor_bound_is_written() {
        let limits = Limits {
            max_cursor_bytes: 64,
            ..Limits::ceilings()
        };
        let mut sink = Vec::new();
        {
            let mut emitter = Emitter::new(&mut sink, limits);
            assert!(emitter.checkpoint(checkpoint(1, "short")));
        }
        assert!(!sink.is_empty());
    }

    #[test]
    fn writing_stops_after_the_first_failure() {
        // A frame ceiling of zero bytes fails every write immediately.
        let limits = Limits {
            max_frame_bytes: 4096,
            ..Limits::ceilings()
        };
        let mut emitter = Emitter::new(Vec::new(), limits);
        assert!(!emitter.write_failed());
        // A cursor at exactly the frame ceiling still fails once JSON framing
        // overhead is added, which is enough to exercise the failure path
        // without constructing an oversized cursor by hand.
        let oversized_cursor = "x".repeat(4096);
        assert!(!emitter.checkpoint(checkpoint(1, &oversized_cursor)));
        assert!(emitter.write_failed());
        assert!(
            !emitter.diagnostic(diagnostic(2)),
            "no further frame is written once failed"
        );
    }

    #[test]
    fn checked_json_dispatches_through_the_same_self_policing() {
        let limits = Limits {
            max_run_diagnostics: 0,
            ..Limits::ceilings()
        };
        let mut emitter = Emitter::new(Vec::new(), limits);
        let value = serde_json::json!({
            "v": 1,
            "type": "diagnostic",
            "run_id": run_id().as_str(),
            "seq": 1,
            "severity": "info",
            "code": "content.skipped",
            "message": "skipped"
        });
        assert!(
            !emitter.checked_json(value),
            "the run's zero-diagnostic ceiling must still apply"
        );
    }

    #[test]
    fn checked_json_refuses_a_frame_the_protocol_s_own_types_reject() {
        let mut emitter = Emitter::new(Vec::new(), Limits::ceilings());
        let value = serde_json::json!({ "v": 1, "type": "not_a_real_event" });
        assert!(!emitter.checked_json(value));
    }

    #[test]
    fn raw_bytes_writes_unconditionally_even_after_a_self_policed_refusal() {
        let mut sink = Vec::new();
        {
            let mut emitter = Emitter::new(
                &mut sink,
                Limits {
                    max_run_diagnostics: 0,
                    ..Limits::ceilings()
                },
            );
            assert!(!emitter.diagnostic(diagnostic(1)));
            emitter.raw_bytes(b"not json at all\n");
        }
        assert_eq!(sink, b"not json at all\n");
    }
}
