//! The core-side run state machine: sequence continuity, bound enforcement,
//! checkpoint commit eligibility, deletion authorization, and run outcome.
//!
//! # The one invariant everything here exists to protect
//!
//! **A cursor may advance only at a checkpoint whose covered records are all
//! durable.** A lagging cursor costs work; an advanced cursor loses an object
//! forever. Over-collection is always safe and under-collection never is, so
//! the cursor is required to lag.
//!
//! That is why committing is a two-step handshake rather than a return value.
//! [`CollectSession::accept`] turns a checkpoint frame into a
//! [`CheckpointOffer`] — the Field's *offer* of a resume point — and the offer
//! becomes a committed cursor only when the caller has called
//! [`CollectSession::record_durable`] for every covered record and then calls
//! [`CollectSession::commit`]. A caller that skips a durable write cannot
//! commit past it, because [`CollectSession::commit`] refuses.
//!
//! # Absence is never deletion unless every declared condition holds
//!
//! [`CollectSession::finish`] returns a [`DeletionAuthorization`] that is
//! `Refused` unless *all* of the A2 conditions hold, and it names every reason
//! it refused rather than returning a bare boolean, so a test asserts the
//! reason and not merely the outcome.
//!
//! # Durability is conservative on purpose
//!
//! [`CollectSession::accept`] refuses the next event outright while a
//! checkpoint offer is still awaiting a commit decision (see
//! [`RejectionCode::ProtocolUnexpectedOrder`] at that call site). Core could
//! instead pipeline: keep consuming records while an earlier checkpoint's
//! durability barrier is outstanding, and reconcile later. That is a
//! permitted future optimization, and it is deliberately **not** done in
//! v0.1, so that "which records does a given commit cover" stays a question
//! with one answer instead of a question about in-flight state.
//!
//! # Cross-run idempotence is the store's job, not this one
//!
//! [`RecordDisposition::NoChange`] and [`RejectionCode::RecordDuplicateDivergentInRun`]
//! are both scoped to *one run*: [`CollectSession`] tracks
//! `(source_scope, source_identity) -> semantic fingerprint` only for the
//! events it has itself accepted. Telling a replayed object apart from a
//! genuinely new current state **across runs** requires the notebook's
//! current state — what is actually durable on disk — which this library
//! cannot see; it is handed a starting point via
//! [`CollectSession::with_current_state`] by a caller that owns that state
//! (the conformance kit's [`crate::conformance::CollectRun::current_state`]
//! stands in for a notebook in tests). Cross-run and cross-instance
//! idempotence is therefore guaranteed by the store that reconciles by
//! portable exact-source key, not by the protocol boundary itself. In-run
//! divergence stays a rejected Field defect rather than a conflict for the
//! reason [`RejectionCode::RecordDuplicateDivergentInRun`] documents: one
//! producer asserting two different current states inside a single run, with
//! no declared ordering, is a bug in that Field. Cross-run and
//! cross-instance divergence still becomes a visible conflict at the store,
//! which is where evidence preservation applies.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use fieldnotes_domain::{FieldStemRegistry, NoteType};
use sha2::{Digest, Sha256};

use crate::artifact::{
    ArtifactDigestIndex, ArtifactOutcome, ArtifactRejection, hex, resolve_artifact,
};
use crate::codes::{ExitCode, RejectionCode, RunOutcome};
use crate::declared::{DeclaredPropertyIndex, PropertyRejection};
use crate::grammar::{Cursor, MediaTypeMatcher};
use crate::limits::Limits;
use crate::message::{
    Change, CheckpointEvent, CollectRequest, CollectionMode, DiagnosticEvent, FieldEvent, Manifest,
    RecordEvent, SchemaError, Severity, SnapshotAuthority, SnapshotState, SourceVersionOrdering,
    TombstoneAuthority,
};

/// Why core rejected a frame, a record, or a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// The rejection code core reports.
    pub code: RejectionCode,
    /// Why, in reviewable terms.
    pub detail: String,
}

impl Rejection {
    /// Builds a rejection.
    #[must_use]
    pub fn new(code: RejectionCode, detail: impl Into<String>) -> Self {
        Rejection {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Rejection {}

impl From<SchemaError> for Rejection {
    fn from(error: SchemaError) -> Self {
        Rejection {
            code: error.code,
            detail: error.message,
        }
    }
}

impl From<PropertyRejection> for Rejection {
    fn from(error: PropertyRejection) -> Self {
        Rejection {
            code: error.code,
            detail: format!("{}: {}", error.name, error.detail),
        }
    }
}

impl From<ArtifactRejection> for Rejection {
    fn from(error: ArtifactRejection) -> Self {
        Rejection {
            code: error.code,
            detail: error.detail,
        }
    }
}

/// What core should do with an accepted record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordDisposition {
    /// Install or update the current Note for this source key.
    Upsert,
    /// The source key already has a current Note with an identical semantic
    /// payload, so core rewrites nothing: same Note ID, same bytes, same
    /// filename.
    NoChange,
    /// Remove the current Note for this source key under declared tombstone
    /// authority.
    Delete,
}

/// An accepted record and what to do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedRecord {
    /// The record's sequence number, which the caller passes back to
    /// [`CollectSession::record_durable`] once the write is durable.
    pub seq: u64,
    /// What core should do.
    pub disposition: RecordDisposition,
}

/// A Field's offer of a resume point, not yet committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointOffer {
    /// The checkpoint's own sequence number.
    pub seq: u64,
    /// The offered cursor.
    pub cursor: Cursor,
    /// The cursor's format version.
    pub cursor_format_version: u16,
    /// The last record sequence number this cursor accounts for.
    pub covers_record_seq_through: u64,
    /// Whether the Field declared this the last checkpoint of the run.
    pub is_final: bool,
}

/// A committed cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedCursor {
    /// The committed cursor value, opaque to core.
    pub cursor: Cursor,
    /// The format version it was written at.
    pub cursor_format_version: u16,
    /// The last record sequence number it accounts for.
    pub covers_record_seq_through: u64,
}

/// Why core refused to commit an offered checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitRefusal {
    /// Some covered record is not durable yet, so committing would advance the
    /// cursor past an undurable write.
    RecordsNotDurable {
        /// The coverage the checkpoint claimed.
        covers_record_seq_through: u64,
        /// The highest contiguous sequence number that is actually durable.
        durable_through: u64,
    },
    /// Core rejected a record in this run, so it commits no further checkpoint.
    RunHasRejectedRecord,
    /// The offer is not the one this session is waiting on.
    UnknownOffer,
    /// Committing would move the cursor backwards within the run.
    NotMonotonic {
        /// The coverage already committed.
        committed_through: u64,
        /// The coverage this offer claims.
        offered_through: u64,
    },
}

impl fmt::Display for CommitRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommitRefusal::RecordsNotDurable {
                covers_record_seq_through,
                durable_through,
            } => write!(
                f,
                "the checkpoint covers records through {covers_record_seq_through} but only \
                 {durable_through} is durable; a cursor never advances past an undurable write"
            ),
            CommitRefusal::RunHasRejectedRecord => write!(
                f,
                "core rejected a record in this run, so it commits no further checkpoint"
            ),
            CommitRefusal::UnknownOffer => {
                write!(f, "no such checkpoint offer is awaiting a commit decision")
            }
            CommitRefusal::NotMonotonic {
                committed_through,
                offered_through,
            } => write!(
                f,
                "a committed cursor already covers records through {committed_through}; \
                 {offered_through} would move it backwards"
            ),
        }
    }
}

impl std::error::Error for CommitRefusal {}

/// One accepted event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptedEvent {
    /// A record, with what core should do about it.
    Record(AcceptedRecord),
    /// An offered resume point awaiting a commit decision.
    Checkpoint(CheckpointOffer),
    /// A diagnostic.
    Diagnostic {
        /// The diagnostic's sequence number.
        seq: u64,
        /// Its severity. `Error` disqualifies completeness for the whole run.
        severity: Severity,
    },
}

/// How the child process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitObservation {
    /// The process exited with a status code.
    Exited(u8),
    /// The process was terminated by a POSIX signal, reported as 128 plus the
    /// signal number.
    ///
    /// Core normalizes any signal termination to a failed run rather than
    /// inventing a Field-level meaning for it.
    Signalled(u8),
    /// The process ended on Windows by unhandled structured exception rather
    /// than by exiting with an ordinary status code, carrying the full
    /// NTSTATUS-shaped 32-bit value Windows reports.
    ///
    /// Windows has no POSIX-style signal number, so this is not "the Windows
    /// equivalent of [`ExitObservation::Signalled`]" in the sense of also
    /// fitting a `u8`: `std::process::abort()` on Windows surfaces as
    /// `0xC0000409` (`STATUS_STACK_BUFFER_OVERRUN`), whose low byte alone is
    /// `0x09` — indistinguishable from ordinary exit code 9, "configuration
    /// invalid," if naively narrowed. Core normalizes this to a failed run
    /// exactly like a POSIX signal termination, without ever attempting that
    /// narrowing.
    WindowsAbnormalTermination(u32),
    /// Core terminated the child, after a cancellation grace period or a bound.
    TerminatedByCore,
    /// The run exceeded its wall clock.
    Timeout,
    /// The run produced neither a frame nor artifact progress within the idle
    /// bound.
    IdleTimeout,
}

impl ExitObservation {
    /// The classified exit code, when the process ended on its own.
    #[must_use]
    pub fn exit_code(self) -> Option<ExitCode> {
        match self {
            ExitObservation::Exited(code) => Some(ExitCode::from_raw(code)),
            _ => None,
        }
    }

    /// Whether the process ended normally with exit zero.
    #[must_use]
    pub fn is_normal(self) -> bool {
        matches!(self, ExitObservation::Exited(0))
    }
}

/// Why core refused to treat absence as deletion.
///
/// Each reason is independently sufficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeletionRefusal {
    /// The manifest does not declare authoritative snapshots.
    AuthorityNotDeclared,
    /// The run was not a snapshot run.
    NotSnapshotMode,
    /// The Field made no completeness claim.
    NoCompletenessClaim,
    /// The Field claimed only a partial enumeration.
    ClaimIsPartial,
    /// The claim named a scope other than the one core requested.
    ClaimScopeMismatch,
    /// A diagnostic of severity `error` appeared somewhere in the run.
    ErrorDiagnostic,
    /// The process did not exit zero.
    NonZeroExit,
    /// Core rejected a frame or a record.
    ProtocolViolation,
    /// The run was bounded by a window, so it is bounded evidence.
    WindowedRun,
    /// The claim was not carried by the run's final checkpoint.
    ClaimNotFinal,
    /// The cursor covering the claim was never committed.
    ClaimNotCommitted,
}

impl DeletionRefusal {
    /// A stable label for reporting.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DeletionRefusal::AuthorityNotDeclared => "the manifest declares no snapshot authority",
            DeletionRefusal::NotSnapshotMode => "the run was not a snapshot run",
            DeletionRefusal::NoCompletenessClaim => "the Field claimed no completeness",
            DeletionRefusal::ClaimIsPartial => "the completeness claim is partial",
            DeletionRefusal::ClaimScopeMismatch => {
                "the completeness claim names a different scope than core requested"
            }
            DeletionRefusal::ErrorDiagnostic => "an error-severity diagnostic appeared in the run",
            DeletionRefusal::NonZeroExit => "the process did not exit zero",
            DeletionRefusal::ProtocolViolation => "core rejected a frame or a record",
            DeletionRefusal::WindowedRun => "the run was bounded by a window",
            DeletionRefusal::ClaimNotFinal => {
                "the completeness claim was not carried by the run's final checkpoint"
            }
            DeletionRefusal::ClaimNotCommitted => {
                "the checkpoint carrying the claim was never committed"
            }
        }
    }
}

impl fmt::Display for DeletionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether this run may remove Notes it did not report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeletionAuthorization {
    /// Core may remove Notes whose portable source key falls inside exactly
    /// this scope and which the run did not report. Notes outside the scope,
    /// including every `self` Note and every Note from another Field, are
    /// untouched.
    Authorized {
        /// The declared scope, and nothing wider.
        scope: String,
    },
    /// Absence proves nothing. Every reason is listed.
    Refused {
        /// Every independently sufficient reason to refuse.
        reasons: Vec<DeletionRefusal>,
    },
}

impl DeletionAuthorization {
    /// Whether removal by proven absence is authorized.
    #[must_use]
    pub fn is_authorized(&self) -> bool {
        matches!(self, DeletionAuthorization::Authorized { .. })
    }

    /// Whether a particular reason contributed to a refusal.
    #[must_use]
    pub fn refused_because(&self, reason: DeletionRefusal) -> bool {
        match self {
            DeletionAuthorization::Refused { reasons } => reasons.contains(&reason),
            DeletionAuthorization::Authorized { .. } => false,
        }
    }
}

/// The outcome of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// Complete, partial, or failed.
    pub outcome: RunOutcome,
    /// The cursor core committed, if any. Durable work committed before a
    /// failure remains, and it is correct because it was committed only after
    /// it was durable.
    pub committed_cursor: Option<CommittedCursor>,
    /// Whether absence may remove Notes, and why not when it may not.
    pub deletion: DeletionAuthorization,
    /// How many records core accepted.
    pub records_accepted: u64,
    /// How many diagnostics core accepted.
    pub diagnostics_accepted: u64,
}

/// The semantic payload of one record, used to tell an identical duplicate from
/// a divergent one.
///
/// Computed over the record with its volatile transport members removed, so a
/// replay across runs with a different `run_id` and `seq` still compares equal.
/// That is what makes replay after a crash idempotent: the same upstream object
/// re-emitted in a later run has the same fingerprint.
pub fn semantic_fingerprint(record: &RecordEvent) -> Result<String, Rejection> {
    let mut value = serde_json::to_value(record).map_err(|error| {
        Rejection::new(
            RejectionCode::ProtocolSchemaInvalid,
            format!("record could not be re-encoded for duplicate detection: {error}"),
        )
    })?;
    if let Some(object) = value.as_object_mut() {
        for volatile in ["v", "type", "run_id", "seq"] {
            object.remove(volatile);
        }
    }
    let encoded = serde_json::to_vec(&value).map_err(|error| {
        Rejection::new(
            RejectionCode::ProtocolSchemaInvalid,
            format!("record could not be encoded for duplicate detection: {error}"),
        )
    })?;
    Ok(hex(&Sha256::digest(&encoded)))
}

/// One collect run, from core's side.
#[derive(Debug)]
pub struct CollectSession<'a> {
    manifest: &'a Manifest,
    properties: DeclaredPropertyIndex<'a>,
    mode: CollectionMode,
    snapshot_scope: Option<String>,
    windowed: bool,
    limits: Limits,
    media_type_policy: Vec<MediaTypeMatcher>,
    run_id: String,
    last_seq: u64,
    record_count: u64,
    diagnostic_count: u64,
    records_by_seq: BTreeSet<u64>,
    durable_seqs: BTreeSet<u64>,
    committed: Option<CommittedCursor>,
    last_committed_coverage: u64,
    last_checkpoint_coverage: u64,
    pending_offer: Option<CheckpointOffer>,
    seen_keys: BTreeMap<(String, String), String>,
    error_diagnostic: bool,
    rejected: bool,
    final_checkpoint_seq: Option<u64>,
    final_claim: Option<(String, SnapshotState)>,
    final_claim_committed: bool,
    staged_bytes: u64,
}

impl<'a> CollectSession<'a> {
    /// Starts a session for one collect run.
    ///
    /// Rejects a manifest whose declared properties are not its own, which is
    /// checked once here rather than per record.
    pub fn new(
        request: &CollectRequest,
        manifest: &'a Manifest,
        stems: &'a FieldStemRegistry,
    ) -> Result<Self, Rejection> {
        let properties = DeclaredPropertyIndex::new(manifest, stems)?;
        Ok(CollectSession {
            manifest,
            properties,
            mode: request.mode,
            snapshot_scope: request
                .snapshot_scope
                .as_ref()
                .map(|scope| scope.as_str().to_owned()),
            windowed: request.window.is_some(),
            limits: request.limits,
            media_type_policy: request.artifact_media_types.clone(),
            run_id: request.run_id.as_str().to_owned(),
            last_seq: 0,
            record_count: 0,
            diagnostic_count: 0,
            records_by_seq: BTreeSet::new(),
            durable_seqs: BTreeSet::new(),
            committed: None,
            last_committed_coverage: 0,
            last_checkpoint_coverage: 0,
            pending_offer: None,
            seen_keys: BTreeMap::new(),
            error_diagnostic: false,
            rejected: false,
            final_checkpoint_seq: None,
            final_claim: None,
            final_claim_committed: false,
            staged_bytes: 0,
        })
    }

    /// The cursor committed so far.
    #[must_use]
    pub fn committed_cursor(&self) -> Option<&CommittedCursor> {
        self.committed.as_ref()
    }

    /// The highest record sequence number that is durable with no undurable
    /// record below it.
    ///
    /// Note that this is **not** a contiguous watermark over all sequence
    /// numbers: `seq` is shared by records, checkpoints, and diagnostics, so the
    /// record sequence numbers of one run are not consecutive integers.
    /// Durability is tracked per accepted record instead.
    #[must_use]
    pub fn durable_through(&self) -> u64 {
        let mut highest = 0;
        for seq in &self.records_by_seq {
            if self.durable_seqs.contains(seq) {
                highest = *seq;
            } else {
                break;
            }
        }
        highest
    }

    /// The first accepted record at or below `covers` whose durable write has
    /// not completed.
    fn first_undurable_through(&self, covers: u64) -> Option<u64> {
        self.records_by_seq
            .range(..=covers)
            .find(|seq| !self.durable_seqs.contains(seq))
            .copied()
    }

    /// The semantic fingerprint of every source key this session has seen.
    ///
    /// Core seeds the next run with this so a replayed object is recognised as
    /// the same current state rather than rewritten. Within one process this is
    /// what the conformance kit uses in place of a notebook.
    #[must_use]
    pub fn current_state(&self) -> BTreeMap<(String, String), String> {
        self.seen_keys.clone()
    }

    /// Seeds the current state core already holds for this Field.
    #[must_use]
    pub fn with_current_state(mut self, state: BTreeMap<(String, String), String>) -> Self {
        self.seen_keys = state;
        self
    }

    fn fail(&mut self, code: RejectionCode, detail: impl Into<String>) -> Rejection {
        self.rejected = true;
        Rejection::new(code, detail)
    }

    fn advance_seq(&mut self, seq: u64) -> Result<(), Rejection> {
        if seq == self.last_seq {
            return Err(self.fail(
                RejectionCode::ProtocolDuplicateSeq,
                format!("sequence number {seq} was already used in this run"),
            ));
        }
        if seq < self.last_seq {
            return Err(self.fail(
                RejectionCode::ProtocolSeqRegression,
                format!(
                    "sequence number regressed from {} to {seq}; a repeat or a regression is the \
                     cheapest possible detection of a truncated, interleaved, or reordered stream",
                    self.last_seq
                ),
            ));
        }
        if seq != self.last_seq + 1 {
            // A gap has its own code, distinct from `protocol.unexpected_order`:
            // it is a well-formed stream with a hole in it, not a frame that
            // arrived somewhere the protocol forbids it outright.
            return Err(self.fail(
                RejectionCode::ProtocolSeqGap,
                format!(
                    "sequence number jumped from {} to {seq}; seq increases by exactly 1 across \
                     every event of one run",
                    self.last_seq
                ),
            ));
        }
        self.last_seq = seq;
        Ok(())
    }

    fn check_run_id(&mut self, run_id: &str) -> Result<(), Rejection> {
        if run_id == self.run_id {
            return Ok(());
        }
        Err(self.fail(
            RejectionCode::ProtocolUnexpectedOrder,
            format!(
                "frame declares run {run_id}, but this run is {}",
                self.run_id
            ),
        ))
    }

    fn check_not_after_final(&mut self) -> Result<(), Rejection> {
        if let Some(seq) = self.final_checkpoint_seq {
            return Err(self.fail(
                RejectionCode::ProtocolUnexpectedOrder,
                format!("the final checkpoint at seq {seq} already closed this run"),
            ));
        }
        Ok(())
    }

    /// Accepts one Field-to-core event.
    ///
    /// A rejection fails the run: core stops consuming output, terminates the
    /// child, removes the staging directory, and commits no further checkpoint.
    pub fn accept(&mut self, event: FieldEvent) -> Result<AcceptedEvent, Rejection> {
        if self.rejected {
            return Err(Rejection::new(
                RejectionCode::ProtocolUnexpectedOrder,
                "the run already failed, so core consumes no further output",
            ));
        }
        if self.pending_offer.is_some() {
            return Err(self.fail(
                RejectionCode::ProtocolUnexpectedOrder,
                "a checkpoint offer is still awaiting a commit decision; core resolves it before \
                 accepting the next event",
            ));
        }
        match event {
            FieldEvent::Manifest(_) => Err(self.fail(
                RejectionCode::ProtocolUnexpectedOrder,
                "a manifest belongs to the describe run, not to a collect run",
            )),
            FieldEvent::Record(record) => self.accept_record(&record).map(AcceptedEvent::Record),
            FieldEvent::Checkpoint(checkpoint) => self
                .accept_checkpoint(&checkpoint)
                .map(AcceptedEvent::Checkpoint),
            FieldEvent::Diagnostic(diagnostic) => self.accept_diagnostic(&diagnostic),
        }
    }

    fn accept_record(&mut self, record: &RecordEvent) -> Result<AcceptedRecord, Rejection> {
        self.check_run_id(record.run_id.as_str())?;
        self.check_not_after_final()?;
        self.advance_seq(record.seq)?;
        if self.record_count + 1 > self.limits.max_run_records {
            return Err(self.fail(
                RejectionCode::ProtocolLimitExceeded,
                format!(
                    "the run exceeded its {} record bound",
                    self.limits.max_run_records
                ),
            ));
        }

        if let Some(object_kind) = &record.object_kind
            && !self.manifest.declares_object_kind(object_kind.as_str())
        {
            let detail = format!(
                "object kind '{object_kind}' is not a capability slice this manifest declares; a \
                 Field cannot acquire a capability by emitting a frame"
            );
            return Err(self.fail(RejectionCode::ManifestUndeclaredCapability, detail));
        }

        if let Some(note_type) = &record.note_type {
            if NoteType::parse(note_type.as_str()).is_none() {
                let detail = format!(
                    "'{note_type}' is not one of A1's eleven approved primary Note types; the \
                     protocol does not restate that vocabulary and core validates against the A1 \
                     registry"
                );
                return Err(self.fail(RejectionCode::RecordInvalidNoteType, detail));
            }
            // Declare before exercise: a capability slice's declared
            // note_type is a bound, not decoration. The object-kind check
            // above already rejected an undeclared slice, so a slice is found
            // here whenever object_kind was supplied at all.
            if let Some(object_kind) = &record.object_kind
                && let Some(declared) = self.manifest.declared_note_type_for(object_kind.as_str())
                && declared.as_str() != note_type.as_str()
            {
                let detail = format!(
                    "record declares note_type '{note_type}' but the manifest's capability slice \
                     for object kind '{object_kind}' declares '{declared}'; a slice's declared \
                     note_type is a bound the Field must honour, not a suggestion"
                );
                return Err(self.fail(RejectionCode::RecordNoteTypeNotDeclared, detail));
            }
        }

        if let Some(body) = &record.body {
            let length = u64::try_from(body.text.len()).unwrap_or(u64::MAX);
            if length > self.limits.max_body_bytes {
                return Err(self.fail(
                    RejectionCode::ProtocolLimitExceeded,
                    format!(
                        "body.text is {length} bytes, past the run's {} byte bound",
                        self.limits.max_body_bytes
                    ),
                ));
            }
        }

        if let Some(properties) = &record.properties {
            let count = u64::try_from(properties.len()).unwrap_or(u64::MAX);
            if count > self.limits.max_properties_per_record {
                return Err(self.fail(
                    RejectionCode::ProtocolLimitExceeded,
                    format!(
                        "{count} property candidates, past the run's {} bound",
                        self.limits.max_properties_per_record
                    ),
                ));
            }
            for (name, value) in properties.iter() {
                let members = u64::try_from(value.member_count()).unwrap_or(u64::MAX);
                if members > self.limits.max_list_members {
                    return Err(self.fail(
                        RejectionCode::ProtocolLimitExceeded,
                        format!(
                            "property {name} has {members} members, past the run's {} bound",
                            self.limits.max_list_members
                        ),
                    ));
                }
                let bytes = u64::try_from(value.max_member_bytes()).unwrap_or(u64::MAX);
                if bytes > self.limits.max_property_value_bytes {
                    return Err(self.fail(
                        RejectionCode::ProtocolLimitExceeded,
                        format!(
                            "property {name} has a {bytes} byte value, past the run's {} bound",
                            self.limits.max_property_value_bytes
                        ),
                    ));
                }
            }
            // Collect the ruling 4 verdict without borrowing self mutably while
            // the index is borrowed.
            let mut verdict = Ok(());
            for (name, value) in properties.iter() {
                if let Err(rejection) = self.properties.check(name, value) {
                    verdict = Err(Rejection::from(rejection));
                    break;
                }
            }
            if let Err(rejection) = verdict {
                self.rejected = true;
                return Err(rejection);
            }
        }

        if let Some(artifacts) = &record.artifacts {
            let count = u64::try_from(artifacts.len()).unwrap_or(u64::MAX);
            if count > self.limits.max_artifacts_per_record {
                return Err(self.fail(
                    RejectionCode::ProtocolLimitExceeded,
                    format!(
                        "{count} artifact references, past the run's {} bound",
                        self.limits.max_artifacts_per_record
                    ),
                ));
            }
        }

        if record.change == Change::Delete
            && self.manifest.collection.deletion.tombstones != TombstoneAuthority::Authoritative
        {
            return Err(self.fail(
                RejectionCode::DeletionUnauthorized,
                "this manifest declares 'deletion.tombstones: unsupported', so a delete record is \
                 rejected: a connector cannot acquire deletion power by emitting a frame",
            ));
        }

        let fingerprint = semantic_fingerprint(record)?;
        let key = (
            record.source.scope.as_str().to_owned(),
            record.source.identity.as_str().to_owned(),
        );
        let disposition = match self.seen_keys.get(&key) {
            Some(previous) if *previous == fingerprint => RecordDisposition::NoChange,
            Some(_)
                if self.manifest.source_key.source_version_ordering
                    == SourceVersionOrdering::Unsupported =>
            {
                return Err(self.fail(
                    RejectionCode::RecordDuplicateDivergentInRun,
                    format!(
                        "source key ({}, {}) was asserted twice in one run with divergent \
                         payloads and no declared version ordering: one producer asserting two \
                         different current states for one object is a Field defect, not a conflict",
                        key.0, key.1
                    ),
                ));
            }
            _ => match record.change {
                Change::Upsert => RecordDisposition::Upsert,
                Change::Delete => RecordDisposition::Delete,
            },
        };
        self.seen_keys.insert(key, fingerprint);
        self.record_count += 1;
        self.records_by_seq.insert(record.seq);
        Ok(AcceptedRecord {
            seq: record.seq,
            disposition,
        })
    }

    fn accept_checkpoint(
        &mut self,
        checkpoint: &CheckpointEvent,
    ) -> Result<CheckpointOffer, Rejection> {
        self.check_run_id(checkpoint.run_id.as_str())?;
        self.check_not_after_final()?;
        self.advance_seq(checkpoint.seq)?;

        if checkpoint.covers_record_seq_through >= checkpoint.seq {
            return Err(self.fail(
                RejectionCode::ProtocolSchemaInvalid,
                format!(
                    "a checkpoint covers records through {}, which must be below its own sequence \
                     number {}",
                    checkpoint.covers_record_seq_through, checkpoint.seq
                ),
            ));
        }
        if checkpoint.covers_record_seq_through < self.last_checkpoint_coverage {
            return Err(self.fail(
                RejectionCode::ProtocolUnexpectedOrder,
                format!(
                    "checkpoint coverage regressed from {} to {}",
                    self.last_checkpoint_coverage, checkpoint.covers_record_seq_through
                ),
            ));
        }
        // A checkpoint whose coverage equals the previous one is a repeated,
        // already-covered range: legal, and a no-op that accounts for no new
        // records. The empty range must not be constructed backwards — a
        // naive `(previous + 1)..=covers` panics on `BTreeSet::range` when
        // `covers` has not advanced past `previous`, since that is a start
        // bound greater than the end bound — so the range is only ever built
        // in the branch where `covers` has actually advanced.
        let actual = if checkpoint.covers_record_seq_through > self.last_checkpoint_coverage {
            u64::try_from(
                self.records_by_seq
                    .range(
                        (self.last_checkpoint_coverage + 1)..=checkpoint.covers_record_seq_through,
                    )
                    .count(),
            )
            .unwrap_or(u64::MAX)
        } else {
            0
        };
        if actual != checkpoint.records_covered {
            return Err(self.fail(
                RejectionCode::ProtocolCoverageMismatch,
                format!(
                    "the checkpoint accounts for {} records through seq {} but core received {}; \
                     the two sides disagree about what was transferred",
                    checkpoint.records_covered, checkpoint.covers_record_seq_through, actual
                ),
            ));
        }
        let cursor_bytes = u64::try_from(checkpoint.cursor.as_str().len()).unwrap_or(u64::MAX);
        if cursor_bytes > self.limits.max_cursor_bytes {
            return Err(self.fail(
                RejectionCode::ProtocolLimitExceeded,
                format!(
                    "the offered cursor is {cursor_bytes} bytes, past the run's {} byte bound",
                    self.limits.max_cursor_bytes
                ),
            ));
        }
        if checkpoint.cursor_format_version != self.manifest.collection.cursor_format_version {
            return Err(self.fail(
                RejectionCode::ManifestCursorFormatChanged,
                format!(
                    "the checkpoint offers a cursor at format version {} but the manifest declares \
                     {}",
                    checkpoint.cursor_format_version,
                    self.manifest.collection.cursor_format_version
                ),
            ));
        }

        // Precedence, when both `snapshot.completeness_contradicted` and
        // `snapshot.scope_widened` could apply to the same claim: whether the
        // run and manifest even admit a completeness claim at all is checked
        // first, and only a claim that clears that bar is then checked
        // against the requested scope. An incremental-mode run offering a
        // claim whose scope also happens to differ from some other run's
        // scope is reported as "contradicted", not "widened" — the more
        // fundamental defect wins the report.
        if let Some(claim) = &checkpoint.snapshot {
            if self.mode != CollectionMode::Snapshot {
                return Err(self.fail(
                    RejectionCode::SnapshotCompletenessContradicted,
                    "a completeness claim belongs to a snapshot-mode run only",
                ));
            }
            match self.snapshot_scope.as_deref() {
                Some(requested) if requested == claim.scope.as_str() => {}
                Some(requested) => {
                    return Err(self.fail(
                        RejectionCode::SnapshotScopeWidened,
                        format!(
                            "the claim covers scope '{}' but core requested '{requested}'; a claim \
                             narrower than the notebook can never reach beyond its declared scope, \
                             and a wider one is rejected",
                            claim.scope
                        ),
                    ));
                }
                None => {
                    return Err(self.fail(
                        RejectionCode::SnapshotCompletenessContradicted,
                        "core requested no snapshot scope, so there is nothing to claim",
                    ));
                }
            }
            self.final_claim = Some((claim.scope.as_str().to_owned(), claim.state));
        }

        self.last_checkpoint_coverage = checkpoint.covers_record_seq_through;
        if checkpoint.is_final {
            self.final_checkpoint_seq = Some(checkpoint.seq);
        }
        let offer = CheckpointOffer {
            seq: checkpoint.seq,
            cursor: checkpoint.cursor.clone(),
            cursor_format_version: checkpoint.cursor_format_version,
            covers_record_seq_through: checkpoint.covers_record_seq_through,
            is_final: checkpoint.is_final,
        };
        self.pending_offer = Some(offer.clone());
        Ok(offer)
    }

    fn accept_diagnostic(
        &mut self,
        diagnostic: &DiagnosticEvent,
    ) -> Result<AcceptedEvent, Rejection> {
        self.check_run_id(diagnostic.run_id.as_str())?;
        self.check_not_after_final()?;
        self.advance_seq(diagnostic.seq)?;
        if self.diagnostic_count + 1 > self.limits.max_run_diagnostics {
            return Err(self.fail(
                RejectionCode::ProtocolLimitExceeded,
                format!(
                    "the run exceeded its {} diagnostic bound",
                    self.limits.max_run_diagnostics
                ),
            ));
        }
        self.diagnostic_count += 1;
        if diagnostic.severity == Severity::Error {
            self.error_diagnostic = true;
        }
        Ok(AcceptedEvent::Diagnostic {
            seq: diagnostic.seq,
            severity: diagnostic.severity,
        })
    }

    /// Resolves a record's artifact references, computing core's own digest.
    ///
    /// Artifacts become durable before any Note that references them, so a
    /// caller calls this before its Note write and only calls
    /// [`CollectSession::record_durable`] once both are durable. A
    /// `not_retained` reference resolves to [`ArtifactOutcome::Declined`]:
    /// it never touches the run's staged-byte budget, because core reads
    /// nothing for it, and it never fails the record — the whole point of
    /// "stays at source" is that declining to retain something is a policy
    /// decision, not a violation.
    pub fn resolve_artifacts(
        &mut self,
        record: &RecordEvent,
        staging_dir: &Path,
        index: &dyn ArtifactDigestIndex,
    ) -> Result<Vec<ArtifactOutcome>, Rejection> {
        let mut resolved = Vec::new();
        let Some(references) = &record.artifacts else {
            return Ok(resolved);
        };
        for reference in references {
            match resolve_artifact(
                staging_dir,
                reference,
                &self.limits,
                &self.media_type_policy,
                index,
            ) {
                Ok(ArtifactOutcome::Resolved(artifact)) => {
                    if !artifact.reused {
                        self.staged_bytes = self.staged_bytes.saturating_add(artifact.byte_length);
                        if self.staged_bytes > self.limits.max_run_artifact_bytes {
                            return Err(self.fail(
                                RejectionCode::ProtocolLimitExceeded,
                                format!(
                                    "the run staged {} bytes, past its {} byte bound",
                                    self.staged_bytes, self.limits.max_run_artifact_bytes
                                ),
                            ));
                        }
                    }
                    resolved.push(ArtifactOutcome::Resolved(artifact));
                }
                Ok(declined @ ArtifactOutcome::Declined(_)) => {
                    resolved.push(declined);
                }
                Err(rejection) => {
                    self.rejected = true;
                    return Err(Rejection::from(rejection));
                }
            }
        }
        Ok(resolved)
    }

    /// Records that every durable write for `seq` completed and the store's
    /// durability barrier returned.
    pub fn record_durable(&mut self, seq: u64) {
        self.durable_seqs.insert(seq);
    }

    /// Commits an offered checkpoint, if and only if every covered record is
    /// durable.
    pub fn commit(&mut self, offer: &CheckpointOffer) -> Result<CommittedCursor, CommitRefusal> {
        if self.pending_offer.as_ref() != Some(offer) {
            return Err(CommitRefusal::UnknownOffer);
        }
        if self.rejected {
            self.pending_offer = None;
            return Err(CommitRefusal::RunHasRejectedRecord);
        }
        if let Some(undurable) = self.first_undurable_through(offer.covers_record_seq_through) {
            return Err(CommitRefusal::RecordsNotDurable {
                covers_record_seq_through: offer.covers_record_seq_through,
                durable_through: undurable.saturating_sub(1),
            });
        }
        if offer.covers_record_seq_through < self.last_committed_coverage {
            return Err(CommitRefusal::NotMonotonic {
                committed_through: self.last_committed_coverage,
                offered_through: offer.covers_record_seq_through,
            });
        }
        let committed = CommittedCursor {
            cursor: offer.cursor.clone(),
            cursor_format_version: offer.cursor_format_version,
            covers_record_seq_through: offer.covers_record_seq_through,
        };
        self.last_committed_coverage = offer.covers_record_seq_through;
        self.committed = Some(committed.clone());
        if self.final_checkpoint_seq == Some(offer.seq) {
            self.final_claim_committed = true;
        }
        self.pending_offer = None;
        Ok(committed)
    }

    /// Declines to commit an offered checkpoint, without failing the run.
    ///
    /// A durable write that did not complete leaves the cursor where it was.
    /// The next run replays from there and the replay is a no-op.
    pub fn withhold(&mut self, offer: &CheckpointOffer) -> Result<(), CommitRefusal> {
        if self.pending_offer.as_ref() != Some(offer) {
            return Err(CommitRefusal::UnknownOffer);
        }
        self.pending_offer = None;
        Ok(())
    }

    /// Records that core rejected a frame outside [`CollectSession::accept`],
    /// such as a framing failure, so no further checkpoint is committed.
    pub fn note_rejection(&mut self) {
        self.rejected = true;
    }

    /// Finishes the run and classifies its outcome and deletion authority.
    #[must_use]
    pub fn finish(&self, exit: ExitObservation) -> RunReport {
        let violated = self.rejected;
        let normal_exit = exit.is_normal();
        let snapshot_ok = match self.mode {
            CollectionMode::Incremental => true,
            CollectionMode::Snapshot => matches!(
                (&self.final_claim, &self.snapshot_scope),
                (Some((scope, SnapshotState::Complete)), Some(requested)) if scope == requested
            ),
        };
        let outcome = if violated || !matches!(exit, ExitObservation::Exited(_)) {
            RunOutcome::Failed
        } else if normal_exit && !self.error_diagnostic && snapshot_ok {
            RunOutcome::Complete
        } else if self.committed.is_some() || self.record_count > 0 {
            RunOutcome::Partial
        } else {
            RunOutcome::Failed
        };

        let mut reasons = Vec::new();
        if self.manifest.collection.deletion.snapshot != SnapshotAuthority::Authoritative {
            reasons.push(DeletionRefusal::AuthorityNotDeclared);
        }
        if self.mode != CollectionMode::Snapshot || self.snapshot_scope.is_none() {
            reasons.push(DeletionRefusal::NotSnapshotMode);
        }
        match &self.final_claim {
            None => reasons.push(DeletionRefusal::NoCompletenessClaim),
            Some((scope, state)) => {
                if *state != SnapshotState::Complete {
                    reasons.push(DeletionRefusal::ClaimIsPartial);
                }
                if self.snapshot_scope.as_deref() != Some(scope.as_str()) {
                    reasons.push(DeletionRefusal::ClaimScopeMismatch);
                }
                if self.final_checkpoint_seq.is_none() {
                    reasons.push(DeletionRefusal::ClaimNotFinal);
                }
                if !self.final_claim_committed {
                    reasons.push(DeletionRefusal::ClaimNotCommitted);
                }
            }
        }
        if self.error_diagnostic {
            reasons.push(DeletionRefusal::ErrorDiagnostic);
        }
        if !normal_exit {
            reasons.push(DeletionRefusal::NonZeroExit);
        }
        if violated {
            reasons.push(DeletionRefusal::ProtocolViolation);
        }
        if self.windowed {
            reasons.push(DeletionRefusal::WindowedRun);
        }
        reasons.sort_unstable();
        reasons.dedup();

        let deletion = match (reasons.is_empty(), &self.snapshot_scope) {
            (true, Some(scope)) => DeletionAuthorization::Authorized {
                scope: scope.clone(),
            },
            (true, None) => DeletionAuthorization::Refused {
                reasons: vec![DeletionRefusal::NotSnapshotMode],
            },
            (false, _) => DeletionAuthorization::Refused { reasons },
        };

        RunReport {
            outcome,
            committed_cursor: self.committed.clone(),
            deletion,
            records_accepted: self.record_count,
            diagnostics_accepted: self.diagnostic_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::FieldEvent;

    fn manifest() -> Manifest {
        let value = serde_json::json!({
            "v": 1, "type": "manifest", "run_id": "1a4c9f2e-0000-4000-8000-000000000001",
            "protocol_version": 1, "protocol_revision": 0, "supported_protocol_versions": [1],
            "driver": "local-reference", "driver_version": "0.1.1", "field_stem": "local",
            "property_prefix": "local_",
            "declared_properties": [],
            "capabilities": [
                {"object_kind": "file", "note_type": "file", "emits_artifacts": true,
                 "emits_identity_anchors": false, "description": "d"},
                {"object_kind": "document", "note_type": "document", "emits_artifacts": false,
                 "emits_identity_anchors": false, "description": "d"}
            ],
            "source_key": {
                "scope_rule": "local_root_id", "scope_rule_version": 1,
                "scope_shape": "s", "scope_depends_on_field_label": false,
                "identity_shape": "i", "identity_includes_object_kind": true,
                "source_version_ordering": "unsupported", "stable_across_instances": true
            },
            "auth": {
                "kind": "none", "credential_profile_required": false,
                "protected_channel_required": false, "refresh_owner": "not_applicable",
                "writes_to_source": false
            },
            "collection": {
                "incremental": true, "cursor_format_version": 1,
                "supported_modes": ["incremental", "snapshot"], "window_supported": false,
                "refetch": "unsupported",
                "deletion": { "tombstones": "unsupported", "snapshot": "authoritative" }
            }
        });
        serde_json::from_value(value)
            .unwrap_or_else(|error| panic!("manifest fixture must parse: {error}"))
    }

    fn collect_request(mode: &str, snapshot_scope: Option<&str>) -> CollectRequest {
        let mut value = serde_json::json!({
            "v": 1, "type": "collect_request", "run_id": "1a4c9f2e-0000-4000-8000-000000000002",
            "protocol_version": 1, "protocol_revision": 0, "field_id": "local_work",
            "mode": mode, "config": {},
            "artifact_staging_dir": "/tmp/staging",
            "artifact_media_types": ["application/pdf", "image/*"],
            "limits": serde_json::to_value(Limits::ceilings())
                .unwrap_or_else(|error| panic!("limits must encode: {error}")),
            "deadline": {
                "not_after": "2099-01-01T00:00:00+00:00",
                "idle_seconds": 120,
                "cancel_grace_seconds": 10
            }
        });
        if let Some(scope) = snapshot_scope {
            value["snapshot_scope"] = serde_json::json!(scope);
        }
        serde_json::from_value(value)
            .unwrap_or_else(|error| panic!("collect_request fixture must parse: {error}"))
    }

    fn session(mode: &str, snapshot_scope: Option<&str>) -> (CollectSession<'static>, Manifest) {
        let manifest: &'static Manifest = Box::leak(Box::new(manifest()));
        let stems: &'static FieldStemRegistry = Box::leak(Box::new(FieldStemRegistry::v1()));
        let request = collect_request(mode, snapshot_scope);
        let session = CollectSession::new(&request, manifest, stems)
            .unwrap_or_else(|error| panic!("the session must start: {error}"));
        (session, manifest.clone())
    }

    fn record_event(seq: u64, object_kind: &str, note_type: &str) -> FieldEvent {
        let value = serde_json::json!({
            "v": 1, "type": "record", "run_id": "1a4c9f2e-0000-4000-8000-000000000002",
            "seq": seq, "change": "upsert",
            "source": {"scope": "local-root:reference-library-v1", "identity": format!("file/{seq}")},
            "object_kind": object_kind, "note_type": note_type,
            "occurred_at": "2026-08-22T09:45:00+02:00",
            "body": {"format": "markdown", "text": "x"}
        });
        FieldEvent::decode(value)
            .unwrap_or_else(|error| panic!("the record fixture must decode: {error}"))
    }

    fn checkpoint_event(
        seq: u64,
        covers: u64,
        records_covered: u64,
        snapshot: Option<serde_json::Value>,
    ) -> FieldEvent {
        let mut value = serde_json::json!({
            "v": 1, "type": "checkpoint", "run_id": "1a4c9f2e-0000-4000-8000-000000000002",
            "seq": seq, "cursor": "walk:v1:seq", "cursor_format_version": 1,
            "covers_record_seq_through": covers, "records_covered": records_covered,
            "final": false
        });
        if let Some(claim) = snapshot {
            value["snapshot"] = claim;
        }
        FieldEvent::decode(value)
            .unwrap_or_else(|error| panic!("the checkpoint fixture must decode: {error}"))
    }

    #[test]
    fn a_sequence_gap_is_its_own_code_distinct_from_unexpected_order() {
        let (mut harness, _manifest) = session("incremental", None);
        harness
            .accept(record_event(1, "file", "file"))
            .unwrap_or_else(|error| panic!("seq 1 must be accepted: {error}"));
        match harness.accept(record_event(3, "file", "file")) {
            Err(rejection) => assert_eq!(rejection.code, RejectionCode::ProtocolSeqGap),
            Ok(_) => panic!("a gap from 1 to 3 must be rejected"),
        }
    }

    #[test]
    fn a_checkpoint_coverage_disagreement_is_its_own_code() {
        let (mut harness, _manifest) = session("incremental", None);
        harness
            .accept(record_event(1, "file", "file"))
            .unwrap_or_else(|error| panic!("seq 1 must be accepted: {error}"));
        // The checkpoint claims to cover two records through seq 1, but only
        // one record exists in that range.
        match harness.accept(checkpoint_event(2, 1, 2, None)) {
            Err(rejection) => assert_eq!(rejection.code, RejectionCode::ProtocolCoverageMismatch),
            Ok(_) => panic!("a coverage disagreement must be rejected"),
        }
    }

    #[test]
    fn covers_zero_means_no_records_are_covered() {
        let (mut harness, _manifest) = session("incremental", None);
        // A checkpoint at seq 1 covering through 0 with zero records claimed
        // is legal: it advances the cursor without any record in between.
        match harness.accept(checkpoint_event(1, 0, 0, None)) {
            Ok(AcceptedEvent::Checkpoint(offer)) => {
                assert_eq!(offer.covers_record_seq_through, 0);
            }
            other => panic!("covers=0 with records_covered=0 must be accepted: {other:?}"),
        }
    }

    #[test]
    fn repeated_coverage_of_an_already_covered_range_is_a_legal_no_op() {
        let (mut harness, _manifest) = session("incremental", None);
        harness
            .accept(record_event(1, "file", "file"))
            .unwrap_or_else(|error| panic!("seq 1 must be accepted: {error}"));
        let first = match harness.accept(checkpoint_event(2, 1, 1, None)) {
            Ok(AcceptedEvent::Checkpoint(offer)) => offer,
            other => panic!("the first checkpoint must be accepted: {other:?}"),
        };
        harness.record_durable(1);
        harness
            .commit(&first)
            .unwrap_or_else(|error| panic!("the first checkpoint must commit: {error}"));

        // A second checkpoint repeating the same coverage claims zero new
        // records. A naive `(previous + 1)..=covers` range would try to
        // range a BTreeSet backwards here and panic; this must not.
        let repeat = match harness.accept(checkpoint_event(3, 1, 0, None)) {
            Ok(AcceptedEvent::Checkpoint(offer)) => offer,
            other => panic!("repeated coverage must be accepted as a no-op: {other:?}"),
        };
        assert!(harness.commit(&repeat).is_ok());
    }

    #[test]
    fn contradicted_takes_precedence_over_widened_when_both_could_apply() {
        // An incremental-mode run never admits a completeness claim at all,
        // regardless of what scope the claim names, so this must report
        // `snapshot.completeness_contradicted`, not `snapshot.scope_widened`.
        let (mut harness, _manifest) = session("incremental", None);
        harness
            .accept(record_event(1, "file", "file"))
            .unwrap_or_else(|error| panic!("seq 1 must be accepted: {error}"));
        let claim = serde_json::json!({
            "scope": "local-root:everything", "state": "complete", "objects_enumerated": 1
        });
        match harness.accept(checkpoint_event(2, 1, 1, Some(claim))) {
            Err(rejection) => assert_eq!(
                rejection.code,
                RejectionCode::SnapshotCompletenessContradicted
            ),
            Ok(_) => panic!("a claim in an incremental run must be rejected"),
        }
    }

    #[test]
    fn a_note_type_disagreeing_with_the_declared_capability_slice_is_rejected() {
        let (mut harness, _manifest) = session("incremental", None);
        // object_kind "file" declares note_type "file" in the fixture
        // manifest; "document" disagrees with that declaration.
        match harness.accept(record_event(1, "file", "document")) {
            Err(rejection) => {
                assert_eq!(rejection.code, RejectionCode::RecordNoteTypeNotDeclared);
            }
            Ok(_) => panic!("a mismatched note_type must be rejected"),
        }
        // The matching slice's own note_type is still accepted.
        let (mut matching, _manifest) = session("incremental", None);
        assert!(
            matching
                .accept(record_event(1, "document", "document"))
                .is_ok()
        );
    }
}
