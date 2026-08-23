//! What one `sync` invocation reports, per Field.
//!
//! A multi-Field sync reports every Field individually and never lets one
//! Field's failure abandon the others (A2 section 13). Nothing here is notebook
//! truth: it is the run summary the CLI renders and the reserved
//! `.fieldnotes/state/sync/<field_id>.status.json` file records.

use fieldnotes_field_protocol::codes::{RejectionCode, RunOutcome};
use fieldnotes_field_protocol::session::{DeletionAuthorization, ExitObservation};

/// How one Field's run ended, as reported to a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldRunOutcome {
    /// Exit zero, no error-severity diagnostic, every offered checkpoint
    /// committed, and — in snapshot mode — a completeness claim for exactly the
    /// requested scope. Only this outcome can authorize deletion by absence.
    Complete,
    /// Durable work happened and the run did not complete.
    Partial,
    /// A protocol violation, a rejected record, a crash, a hang, or a refusal
    /// to start at all.
    Failed,
    /// The Field was not run: it is configured disabled.
    Skipped,
}

impl FieldRunOutcome {
    /// A stable lowercase label for machine-readable output and the status
    /// file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FieldRunOutcome::Complete => "complete",
            FieldRunOutcome::Partial => "partial",
            FieldRunOutcome::Failed => "failed",
            FieldRunOutcome::Skipped => "skipped",
        }
    }

    /// The outcome the protocol session classified, mapped onto this report's
    /// vocabulary.
    #[must_use]
    pub fn from_run(outcome: RunOutcome) -> Self {
        match outcome {
            RunOutcome::Complete => FieldRunOutcome::Complete,
            RunOutcome::Partial => FieldRunOutcome::Partial,
            RunOutcome::Failed => FieldRunOutcome::Failed,
        }
    }

    /// Whether this outcome is a success for exit-code purposes.
    #[must_use]
    pub fn is_success(self) -> bool {
        matches!(self, FieldRunOutcome::Complete | FieldRunOutcome::Skipped)
    }
}

impl core::fmt::Display for FieldRunOutcome {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a run did to the notebook.
///
/// `docs/cli.md` asks automation output to distinguish at least collected,
/// updated, unchanged, removed, conflicted, damaged, truncated, and failed;
/// these are those counts plus the artifact and retention outcomes ADR 0007
/// made visible.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncCounts {
    /// Records core accepted from the Field.
    pub records_accepted: u64,
    /// Notes minted under a new Note ID, because no active Note held their
    /// portable source key.
    pub created: u64,
    /// Notes atomically rewritten under their existing Note ID.
    pub updated: u64,
    /// Records whose current state already matched the notebook byte for byte,
    /// so nothing was rewritten.
    pub unchanged: u64,
    /// Notes removed under an authoritative tombstone.
    pub removed_by_tombstone: u64,
    /// Notes removed because a completed authoritative snapshot proved their
    /// absence inside its declared scope.
    pub removed_by_snapshot: u64,
    /// Notes whose event-time filename changed, installed under the new name
    /// before the old one was removed.
    pub renamed: u64,
    /// Original artifacts whose bytes core hashed and installed.
    pub artifacts_stored: u64,
    /// Artifact references core satisfied from bytes it already stored.
    pub artifacts_reused: u64,
    /// Attachments a Field deliberately did not retain, projected onto the
    /// `skipped_attachments` Note property.
    pub attachments_skipped: u64,
    /// Records the Field reported as damaged.
    pub damaged: u64,
    /// Records the Field reported as truncated.
    pub truncated: u64,
    /// Durable writes that did not complete, each of which independently stops
    /// the cursor from advancing past it.
    pub durable_write_failures: u64,
}

/// One diagnostic the Field emitted, after core's own redaction pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncDiagnostic {
    /// `info`, `warning`, or `error`. An `error` disqualifies completeness for
    /// the whole run.
    pub severity: String,
    /// The closed-vocabulary diagnostic code.
    pub code: String,
    /// The message, redacted by the Field and then again by core.
    pub message: String,
    /// The portable source identity the diagnostic concerns, when it named one.
    pub source_identity: Option<String>,
}

/// The rejection that failed a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRejection {
    /// The closed-vocabulary rejection code.
    pub code: String,
    /// Why, in reviewable terms.
    pub detail: String,
}

impl SyncRejection {
    /// Builds a rejection report from a code and a detail.
    #[must_use]
    pub fn new(code: RejectionCode, detail: impl Into<String>) -> Self {
        SyncRejection {
            code: code.as_str().to_owned(),
            detail: detail.into(),
        }
    }
}

/// Whether the run may remove Notes it did not report, and why not when it may
/// not.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeletionReport {
    /// The declared scope removal was authorized for, and nothing wider.
    pub authorized_scope: Option<String>,
    /// Every independently sufficient reason core refused removal by absence.
    pub refusals: Vec<String>,
}

impl DeletionReport {
    /// Summarizes a protocol-level deletion authorization.
    #[must_use]
    pub fn from_authorization(authorization: &DeletionAuthorization) -> Self {
        match authorization {
            DeletionAuthorization::Authorized { scope } => DeletionReport {
                authorized_scope: Some(scope.clone()),
                refusals: Vec::new(),
            },
            DeletionAuthorization::Refused { reasons } => DeletionReport {
                authorized_scope: None,
                refusals: reasons
                    .iter()
                    .map(|reason| reason.as_str().to_owned())
                    .collect(),
            },
        }
    }
}

/// One Field's whole run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSyncReport {
    /// The configured Field ID.
    pub field_id: String,
    /// `incremental` or `snapshot`.
    pub mode: String,
    /// How the run ended.
    pub outcome: FieldRunOutcome,
    /// What it did to the notebook.
    pub counts: SyncCounts,
    /// Every diagnostic, redacted.
    pub diagnostics: Vec<SyncDiagnostic>,
    /// Whether this run committed a cursor at all.
    pub cursor_committed: bool,
    /// The record coverage the committed cursor accounts for, when one was
    /// committed.
    pub cursor_coverage: Option<u64>,
    /// Checkpoint offers core declined to commit, with the reason, in order.
    pub withheld_checkpoints: Vec<String>,
    /// Whether a stored cursor could not be replayed because the Field's
    /// declared cursor format version no longer matches the version it was
    /// stored at. The run then starts unbounded, which is a recovery gap the
    /// user should see rather than a silent full re-collection.
    pub cursor_recovery_gap: bool,
    /// Deletion authority for this run.
    pub deletion: DeletionReport,
    /// The rejection that failed the run, when one did.
    pub rejection: Option<SyncRejection>,
    /// A refusal to start, or a failure that is not a protocol rejection: a
    /// version mismatch, a required migration, a Field that needs
    /// authentication, an unusable configuration, or a durable-write failure.
    pub failure: Option<String>,
    /// How the child process ended.
    pub exit: String,
    /// Captured standard error, after core's redaction pass, when the Field
    /// wrote any. Core never persists raw standard error.
    pub stderr: Option<String>,
    /// Portable source keys more than one active Note claims, rendered as
    /// `<scope>\t<identity>`.
    ///
    /// A1 section 7 admits this only "while a visible conflict is unresolved",
    /// and conflict bundles are `0.1.2` work, so reconciliation reports the
    /// boundary instead of inventing behavior at it.
    pub conflicts: Vec<String>,
}

impl FieldSyncReport {
    /// A report for a Field that was not run at all.
    #[must_use]
    pub fn not_run(
        field_id: impl Into<String>,
        mode: &str,
        outcome: FieldRunOutcome,
        failure: impl Into<String>,
    ) -> Self {
        FieldSyncReport {
            field_id: field_id.into(),
            mode: mode.to_owned(),
            outcome,
            counts: SyncCounts::default(),
            diagnostics: Vec::new(),
            cursor_committed: false,
            cursor_coverage: None,
            withheld_checkpoints: Vec::new(),
            cursor_recovery_gap: false,
            deletion: DeletionReport::default(),
            rejection: None,
            failure: Some(failure.into()),
            exit: "not_started".to_owned(),
            stderr: None,
            conflicts: Vec::new(),
        }
    }
}

/// Every Field one `sync` invocation touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// One report per Field, in ascending Field ID order.
    pub fields: Vec<FieldSyncReport>,
}

impl SyncOutcome {
    /// Whether every Field succeeded.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.fields.iter().all(|report| report.outcome.is_success())
    }
}

/// A stable label for how a child process ended.
#[must_use]
pub fn exit_label(exit: ExitObservation) -> String {
    match exit {
        ExitObservation::Exited(code) => format!("exited {code}"),
        ExitObservation::Signalled(code) => format!("signalled {code}"),
        ExitObservation::WindowsAbnormalTermination(code) => {
            format!("windows abnormal termination {code:#010x}")
        }
        ExitObservation::TerminatedByCore => "terminated by core".to_owned(),
        ExitObservation::Timeout => "run wall clock exceeded".to_owned(),
        ExitObservation::IdleTimeout => "idle without progress".to_owned(),
    }
}
