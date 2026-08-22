//! The transcript fixture format, so the checked-in transcripts can be used as
//! test vectors.
//!
//! **The transcript file format is a fixture format, not the wire format.** A
//! single file has to show both directions, two channels, core's own observable
//! behavior, and input that is deliberately not valid protocol — none of which a
//! raw wire capture can carry. So every line is a tagged wrapper: a header, a
//! wire frame with its direction and channel, deliberately invalid raw bytes, an
//! observable core action, or a process exit.
//!
//! A frame line's `valid` member says whether the frame is expected to decode
//! and validate. Its `expect_reject` member names the code core must reject it
//! with, **whether or not a single-frame schema can express why**: a frame may
//! be well formed in isolation and still violate an ordering, authority,
//! registry, declared-typing, or filesystem rule. Both must fail; only the first
//! is a schema matter.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::codes::RejectionCode;

/// Which way a frame travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Core wrote it to the Field's standard input.
    #[serde(rename = "core->field")]
    CoreToField,
    /// The Field wrote it to core.
    #[serde(rename = "field->core")]
    FieldToCore,
}

/// Which channel carried a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// The Field's standard input.
    Stdin,
    /// The Field's standard output, which carries protocol data only.
    Stdout,
    /// The Field's standard error, which carries logs only.
    Stderr,
    /// The protected credential channel.
    Credential,
}

/// Whether a fixture is normative at A2 or illustrative for a later gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    /// Normative for protocol v1 if A2 is approved.
    #[serde(rename = "a2-normative")]
    A2Normative,
    /// Illustrative; it becomes normative at a later release gate.
    #[serde(rename = "later-gate-illustrative")]
    LaterGateIllustrative,
}

/// The operation a transcript demonstrates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptOperation {
    /// A describe run.
    Describe,
    /// A collect run.
    Collect,
}

/// The mode a transcript demonstrates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptMode {
    /// An incremental collect run.
    Incremental,
    /// A reconciling snapshot run.
    Snapshot,
    /// A describe run, which has no mode.
    NotApplicable,
}

/// The outcome a transcript expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedOutcome {
    /// Exit zero, no error diagnostic, and any completeness claim honoured.
    Complete,
    /// Durable work happened and the run did not complete.
    Partial,
    /// A protocol violation, a rejected record, a crash, or a hang.
    Failed,
    /// The frame or run was rejected outright.
    Rejected,
}

/// A transcript's first line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// Always `header`.
    pub line: String,
    /// The transcript's own name.
    pub transcript: String,
    /// What the transcript demonstrates.
    pub demonstrates: String,
    /// Whether it is normative at A2.
    pub gate: Gate,
    /// The operation.
    pub operation: TranscriptOperation,
    /// The mode.
    pub mode: TranscriptMode,
    /// The expected outcome.
    pub expected_outcome: ExpectedOutcome,
    /// The expected process exit code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_exit_code: Option<u16>,
    /// Reviewer notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// One wire frame plus its fixture metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameLine {
    /// Always `frame`.
    pub line: String,
    /// Which way it travelled.
    pub direction: Direction,
    /// Which channel carried it.
    pub channel: Channel,
    /// The wire frame.
    pub frame: serde_json::Value,
    /// Whether the frame is expected to decode and validate.
    pub valid: bool,
    /// The code core must reject it with, when it must be rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_reject: Option<String>,
    /// Reviewer notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl FrameLine {
    /// The `type` discriminator, when the frame has one.
    #[must_use]
    pub fn frame_type(&self) -> Option<&str> {
        self.frame.get("type").and_then(serde_json::Value::as_str)
    }

    /// The expected rejection code, parsed against the closed vocabulary.
    ///
    /// `Some(None)` means the transcript named a code that is **not** in the
    /// closed v1 vocabulary, which is itself a corpus defect.
    #[must_use]
    pub fn expected_rejection(&self) -> Option<Option<RejectionCode>> {
        self.expect_reject.as_deref().map(RejectionCode::parse)
    }
}

/// Bytes that are deliberately not valid protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLine {
    /// Always `raw`.
    pub line: String,
    /// Which way the bytes travelled.
    pub direction: Direction,
    /// Which channel carried them.
    pub channel: Channel,
    /// Literal wire text, when it is representable as a UTF-8 string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_utf8: Option<String>,
    /// A description of bytes that cannot be embedded in a JSON string, such as
    /// invalid UTF-8 or a frame past the frame limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_description: Option<String>,
    /// The code core must reject them with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_reject: Option<String>,
    /// Reviewer notes.
    pub note: String,
}

impl RawLine {
    /// The expected rejection code, parsed against the closed vocabulary.
    #[must_use]
    pub fn expected_rejection(&self) -> Option<Option<RejectionCode>> {
        self.expect_reject.as_deref().map(RejectionCode::parse)
    }
}

/// One observable core action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreAction {
    /// Version and revision negotiation settled.
    NegotiateProtocol,
    /// Core aborted before any credential grant existed.
    AbortBeforeCredentialDelivery,
    /// Core started the pinned executable.
    StartField,
    /// Core resolved a staged handle inside the staging directory.
    StageArtifact,
    /// Core installed an artifact at its content-addressed path.
    InstallArtifact,
    /// Core reused bytes it already stored.
    ReuseArtifact,
    /// The Field declined to retain an artifact; core stored no bytes and
    /// computed no digest for it.
    DeclineArtifact,
    /// Core installed a new Note.
    WriteNote,
    /// Core updated a Note in place under the same Note ID.
    UpdateNote,
    /// Core removed a Note.
    RemoveNote,
    /// Core rewrote nothing.
    NoChange,
    /// Core refused to remove anything.
    NoDeletion,
    /// Core committed a cursor.
    CommitCheckpoint,
    /// Core declined to commit a cursor.
    WithholdCheckpoint,
    /// Core rejected one record.
    RejectRecord,
    /// Core failed the run.
    RejectRun,
    /// Core applied its own redaction pass.
    Redact,
    /// Core terminated the child.
    TerminateChild,
    /// The fixture simulated a crash.
    SimulatedCrash,
    /// Core resumed from a committed cursor.
    Resume,
    /// Core reported the run's outcome.
    ReportOutcome,
}

/// One observable core action line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreLine {
    /// Always `core`.
    pub line: String,
    /// What core did.
    pub action: CoreAction,
    /// Core's rejection code, when the action is a rejection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// What happened, in reviewable terms.
    pub detail: String,
}

impl CoreLine {
    /// The rejection code, parsed against the closed vocabulary.
    #[must_use]
    pub fn rejection(&self) -> Option<Option<RejectionCode>> {
        self.error_code.as_deref().map(RejectionCode::parse)
    }
}

/// One process exit line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExitLine {
    /// Always `exit`.
    pub line: String,
    /// The raw exit code.
    pub code: u16,
    /// What the code means.
    pub meaning: String,
    /// The resulting run outcome.
    pub outcome: ExpectedOutcome,
}

/// One transcript line.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptLine {
    /// The header.
    Header(Box<Header>),
    /// A wire frame.
    Frame(Box<FrameLine>),
    /// Deliberately invalid bytes.
    Raw(Box<RawLine>),
    /// An observable core action.
    Core(Box<CoreLine>),
    /// A process exit.
    Exit(Box<ExitLine>),
}

/// Why a transcript could not be read.
#[derive(Debug)]
pub enum TranscriptError {
    /// The file could not be read.
    Io(std::io::Error),
    /// A line is not JSON, or not a transcript line.
    Line {
        /// The one-based line number.
        number: usize,
        /// What was wrong.
        detail: String,
    },
}

impl std::fmt::Display for TranscriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranscriptError::Io(error) => write!(f, "transcript could not be read: {error}"),
            TranscriptError::Line { number, detail } => {
                write!(f, "transcript line {number}: {detail}")
            }
        }
    }
}

impl std::error::Error for TranscriptError {}

impl From<std::io::Error> for TranscriptError {
    fn from(error: std::io::Error) -> Self {
        TranscriptError::Io(error)
    }
}

/// One parsed transcript file.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// The transcript's file stem, for reporting.
    pub name: String,
    /// Every line in file order.
    pub lines: Vec<TranscriptLine>,
}

impl Transcript {
    /// Reads and parses one transcript file.
    pub fn load(path: &Path) -> Result<Self, TranscriptError> {
        let text = fs::read_to_string(path)?;
        let name = path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("transcript")
            .to_owned();
        let mut lines = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            if raw.is_empty() {
                return Err(TranscriptError::Line {
                    number,
                    detail: "a transcript has no blank lines".to_owned(),
                });
            }
            let value: serde_json::Value =
                serde_json::from_str(raw).map_err(|error| TranscriptError::Line {
                    number,
                    detail: format!("not JSON: {error}"),
                })?;
            let tag = value
                .get("line")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| TranscriptError::Line {
                    number,
                    detail: "every transcript line carries a string 'line' tag".to_owned(),
                })?
                .to_owned();
            let parsed = match tag.as_str() {
                "header" => decode_line(value, number).map(TranscriptLine::Header),
                "frame" => decode_line(value, number).map(TranscriptLine::Frame),
                "raw" => decode_line(value, number).map(TranscriptLine::Raw),
                "core" => decode_line(value, number).map(TranscriptLine::Core),
                "exit" => decode_line(value, number).map(TranscriptLine::Exit),
                other => Err(TranscriptError::Line {
                    number,
                    detail: format!("'{other}' is not a transcript line kind"),
                }),
            }?;
            lines.push(parsed);
        }
        Ok(Transcript { name, lines })
    }

    /// Reads every transcript in a directory, in file-name order.
    pub fn load_directory(directory: &Path) -> Result<Vec<Self>, TranscriptError> {
        let mut paths: Vec<_> = fs::read_dir(directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("ndjson"))
            .collect();
        paths.sort();
        paths.iter().map(|path| Transcript::load(path)).collect()
    }

    /// The header, which is always the first line.
    #[must_use]
    pub fn header(&self) -> Option<&Header> {
        match self.lines.first() {
            Some(TranscriptLine::Header(header)) => Some(header),
            _ => None,
        }
    }

    /// Every frame line.
    pub fn frames(&self) -> impl Iterator<Item = &FrameLine> {
        self.lines.iter().filter_map(|line| match line {
            TranscriptLine::Frame(frame) => Some(frame.as_ref()),
            _ => None,
        })
    }

    /// Every raw line.
    pub fn raw_lines(&self) -> impl Iterator<Item = &RawLine> {
        self.lines.iter().filter_map(|line| match line {
            TranscriptLine::Raw(raw) => Some(raw.as_ref()),
            _ => None,
        })
    }

    /// Every core-action line.
    pub fn core_lines(&self) -> impl Iterator<Item = &CoreLine> {
        self.lines.iter().filter_map(|line| match line {
            TranscriptLine::Core(core) => Some(core.as_ref()),
            _ => None,
        })
    }

    /// Every exit line.
    pub fn exits(&self) -> impl Iterator<Item = &ExitLine> {
        self.lines.iter().filter_map(|line| match line {
            TranscriptLine::Exit(exit) => Some(exit.as_ref()),
            _ => None,
        })
    }
}

fn decode_line<T: for<'de> Deserialize<'de>>(
    value: serde_json::Value,
    number: usize,
) -> Result<Box<T>, TranscriptError> {
    serde_json::from_value(value)
        .map(Box::new)
        .map_err(|error| TranscriptError::Line {
            number,
            detail: format!("does not match its line schema: {error}"),
        })
}

/// The repository-relative location of the proposed protocol corpus.
pub const CORPUS_RELATIVE_PATH: &str = "tests/fixtures/protocol/proposed-v1";

// ---------------------------------------------------------------------------
// The conformance kit: a reusable driver that runs a Field as a child process
// ---------------------------------------------------------------------------

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use crate::artifact::ArtifactDigestIndex;
use crate::codes::RunOutcome;
use crate::grammar::{
    CancelTag, CollectRequestTag, Cursor, DescribeRequestTag, FieldIdToken, OffsetDatetime,
    PropertyPrefix, ProtocolV1, RunId, SnapshotScope,
};
use crate::host::{FieldProcess, FieldSpawn, Operation};
use crate::limits::{Deadline, Limits};
use crate::message::{
    Cancel, CancelReason, CollectRequest, CollectionMode, CoreFrame, CredentialGrant,
    DescribeRequest, FieldEvent, Manifest, RecordEvent, Severity, VersionList, Window,
};
use crate::redact::Redactor;
use crate::session::{
    AcceptedEvent, CollectSession, CommittedCursor, DeletionAuthorization, ExitObservation,
    Rejection, RunReport,
};
use crate::value::ConfigMap;
use crate::version::{Negotiation, PROTOCOL_REVISION, PROTOCOL_VERSION, negotiate};

/// Why the kit could not run a Field at all.
///
/// Distinct from a protocol failure: this is core's own problem, not the
/// Field's.
#[derive(Debug)]
pub enum DriverError {
    /// The child could not be started, or a pipe failed.
    Io(std::io::Error),
    /// A fixture value in the plan was not well formed.
    Plan(String),
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriverError::Io(error) => write!(f, "the Field could not be run: {error}"),
            DriverError::Plan(detail) => write!(f, "the conformance plan is invalid: {detail}"),
        }
    }
}

impl std::error::Error for DriverError {}

impl From<std::io::Error> for DriverError {
    fn from(error: std::io::Error) -> Self {
        DriverError::Io(error)
    }
}

/// How core's own durable writes behave during a run.
///
/// The kit needs to be able to *fail* a durable write, because the invariant
/// under test is that a cursor never advances past one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DurabilityPolicy {
    /// Every durable write succeeds.
    #[default]
    AllSucceed,
    /// The durable write for this record sequence number does not complete.
    FailAt(u64),
}

impl DurabilityPolicy {
    fn succeeds(self, seq: u64) -> bool {
        match self {
            DurabilityPolicy::AllSucceed => true,
            DurabilityPolicy::FailAt(failing) => seq != failing,
        }
    }
}

/// One observable core action, so a test asserts what core did rather than that
/// nothing errored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreObservation {
    /// Version and revision negotiation settled.
    NegotiatedProtocol(Negotiation),
    /// Core resolved and installed a staged artifact under its own digest.
    InstalledArtifact {
        /// Core's own digest.
        digest: String,
    },
    /// Core reused bytes it already stored.
    ReusedArtifact {
        /// The digest it matched.
        digest: String,
    },
    /// The Field declined to retain an artifact; core stored no bytes and
    /// computed no digest for it, and the record was accepted anyway.
    DeclinedArtifact {
        /// The declined artifact's role.
        role: crate::message::ArtifactRole,
    },
    /// Core installed or updated the current Note for a source key.
    WroteNote {
        /// The record's sequence number.
        seq: u64,
    },
    /// Core rewrote nothing, because the payload was identical.
    NoChange {
        /// The record's sequence number.
        seq: u64,
    },
    /// Core removed a Note under declared tombstone authority.
    RemovedNote {
        /// The record's sequence number.
        seq: u64,
    },
    /// Core's durable write did not complete.
    DurableWriteFailed {
        /// The record's sequence number.
        seq: u64,
    },
    /// Core accepted a diagnostic.
    Diagnostic {
        /// The diagnostic's severity.
        severity: Severity,
    },
    /// Core committed a cursor.
    CommittedCheckpoint {
        /// The coverage the committed cursor accounts for.
        covers_record_seq_through: u64,
    },
    /// Core declined to commit an offered cursor.
    WithheldCheckpoint {
        /// Why it declined.
        reason: String,
    },
    /// Core wrote a cancel frame.
    Cancelled,
    /// Core failed the run.
    RejectedRun {
        /// The code core rejected it with.
        code: RejectionCode,
    },
    /// Core reported the run's outcome.
    ReportedOutcome(RunOutcome),
}

/// What core asks a Field to collect.
#[derive(Debug, Clone)]
pub struct CollectPlan {
    /// Core's identifier for the run.
    pub run_id: RunId,
    /// The configured Field ID.
    pub field_id: FieldIdToken,
    /// Incremental or snapshot.
    pub mode: CollectionMode,
    /// The last committed cursor and the format version it was stored at.
    pub cursor: Option<(Cursor, u16)>,
    /// The scope a snapshot run claims to cover.
    pub snapshot_scope: Option<SnapshotScope>,
    /// An optional bounded window.
    pub window: Option<Window>,
    /// The per-run staging directory core creates and names.
    pub staging_dir: PathBuf,
    /// Digests the notebook already stores, for `digest_only` references.
    pub known_digests: BTreeSet<String>,
    /// How core's durable writes behave.
    pub durability: DurabilityPolicy,
    /// Write a cancel frame after this many accepted records.
    pub cancel_after_records: Option<u64>,
    /// A credential reference, never a value.
    pub credential: Option<CredentialGrant>,
    /// Non-secret connector configuration.
    pub config: ConfigMap,
    /// The current state core already holds for this Field, keyed by portable
    /// exact-source key.
    ///
    /// Seeding this is how the kit models "locate the current Note by portable
    /// source key" without a notebook: a replayed object is then recognised as
    /// the same current state rather than rewritten, which is what makes replay
    /// after a crash idempotent.
    pub current_state: BTreeMap<(String, String), String>,
}

impl CollectPlan {
    /// A plain incremental plan.
    pub fn incremental(
        run_id: &str,
        field_id: &str,
        staging_dir: PathBuf,
    ) -> Result<Self, DriverError> {
        Ok(CollectPlan {
            run_id: parse_run_id(run_id)?,
            field_id: FieldIdToken::parse(field_id)
                .map_err(|error| DriverError::Plan(error.to_string()))?,
            mode: CollectionMode::Incremental,
            cursor: None,
            snapshot_scope: None,
            window: None,
            staging_dir,
            known_digests: BTreeSet::new(),
            durability: DurabilityPolicy::AllSucceed,
            cancel_after_records: None,
            credential: None,
            config: ConfigMap::new(),
            current_state: BTreeMap::new(),
        })
    }

    /// A snapshot plan for one declared scope.
    pub fn snapshot(
        run_id: &str,
        field_id: &str,
        staging_dir: PathBuf,
        scope: &str,
    ) -> Result<Self, DriverError> {
        let mut plan = CollectPlan::incremental(run_id, field_id, staging_dir)?;
        plan.mode = CollectionMode::Snapshot;
        plan.snapshot_scope = Some(
            SnapshotScope::parse(scope).map_err(|error| DriverError::Plan(error.to_string()))?,
        );
        Ok(plan)
    }

    /// Replays a committed cursor at its stored format version.
    pub fn with_cursor(mut self, cursor: &str, format_version: u16) -> Result<Self, DriverError> {
        self.cursor = Some((
            Cursor::parse(cursor).map_err(|error| DriverError::Plan(error.to_string()))?,
            format_version,
        ));
        Ok(self)
    }

    /// Declares a digest the notebook already stores.
    #[must_use]
    pub fn with_known_digest(mut self, digest: &str) -> Self {
        self.known_digests.insert(digest.to_owned());
        self
    }

    /// Makes one durable write fail.
    #[must_use]
    pub fn with_durability(mut self, policy: DurabilityPolicy) -> Self {
        self.durability = policy;
        self
    }

    /// Cancels the run after this many accepted records.
    #[must_use]
    pub fn cancelling_after(mut self, records: u64) -> Self {
        self.cancel_after_records = Some(records);
        self
    }

    /// Seeds the current state a previous run left behind.
    #[must_use]
    pub fn resuming_state(mut self, state: BTreeMap<(String, String), String>) -> Self {
        self.current_state = state;
        self
    }
}

fn parse_run_id(text: &str) -> Result<RunId, DriverError> {
    RunId::parse(text).map_err(|error| DriverError::Plan(error.to_string()))
}

struct DigestIndex(BTreeSet<String>);

impl ArtifactDigestIndex for DigestIndex {
    fn contains_digest(&self, digest: &str) -> bool {
        self.0.contains(digest)
    }
}

/// One describe run, as core observed it.
#[derive(Debug, Clone)]
pub struct DescribeRun {
    /// The manifest, when the Field produced a valid one.
    pub manifest: Option<Manifest>,
    /// The settled negotiation, when there was one.
    pub negotiation: Option<Negotiation>,
    /// The code core rejected the run with, when it did.
    pub rejection: Option<RejectionCode>,
    /// Why, in reviewable terms.
    pub detail: Option<String>,
    /// How the child ended.
    pub exit: ExitObservation,
    /// Captured standard error, after core's redaction pass.
    pub stderr: String,
    /// The exact argv the child was given.
    pub argv: Vec<String>,
    /// The exact environment the child was given.
    pub environment: BTreeMap<String, String>,
}

/// One collect run, as core observed it.
#[derive(Debug, Clone)]
pub struct CollectRun {
    /// Core's classification of the run.
    pub report: RunReport,
    /// Every event core accepted, in order.
    pub events: Vec<FieldEvent>,
    /// Every cursor core committed, in order.
    pub committed_cursors: Vec<CommittedCursor>,
    /// The rejection that failed the run, when one did.
    pub rejection: Option<Rejection>,
    /// Every observable core action, in order.
    pub actions: Vec<CoreObservation>,
    /// How the child ended.
    pub exit: ExitObservation,
    /// Captured standard error, after core's redaction pass.
    pub stderr: String,
    /// Captured standard error before core's redaction pass, so a test can prove
    /// the pass is what removed a secret rather than luck.
    pub raw_stderr: String,
    /// Whether the standard-error ring buffer overflowed.
    pub stderr_truncated: bool,
    /// The exact argv the child was given.
    pub argv: Vec<String>,
    /// The exact environment the child was given.
    pub environment: BTreeMap<String, String>,
    /// The current state this run leaves behind, keyed by portable exact-source
    /// key, for seeding the next run.
    pub current_state: BTreeMap<(String, String), String>,
}

impl CollectRun {
    /// The code core rejected the run with, when it did.
    #[must_use]
    pub fn rejection_code(&self) -> Option<RejectionCode> {
        self.rejection.as_ref().map(|rejection| rejection.code)
    }

    /// Whether absence may remove Notes.
    #[must_use]
    pub fn deletion(&self) -> &DeletionAuthorization {
        &self.report.deletion
    }

    /// The last committed cursor value, as text.
    #[must_use]
    pub fn last_cursor(&self) -> Option<&str> {
        self.committed_cursors
            .last()
            .map(|committed| committed.cursor.as_str())
    }

    /// Everywhere `secret` appears in something core produced, held, or was
    /// given.
    ///
    /// The secret-canary assertion: a unique canary must be absent from argv,
    /// the inherited environment, standard output, standard error, logs,
    /// diagnostics, and cursors. The scan deliberately covers the **unredacted**
    /// standard error too, because redaction is defense in depth and not
    /// permission to log a secret first.
    #[must_use]
    pub fn secret_locations(&self, secret: &str) -> Vec<&'static str> {
        let mut found = Vec::new();
        if secret.is_empty() {
            return found;
        }
        if self.argv.iter().any(|token| token.contains(secret)) {
            found.push("argv");
        }
        if self
            .environment
            .iter()
            .any(|(name, value)| name.contains(secret) || value.contains(secret))
        {
            found.push("environment");
        }
        let events_carry = self.events.iter().any(|event| {
            event
                .to_json()
                .ok()
                .and_then(|value| serde_json::to_string(&value).ok())
                .is_some_and(|text| text.contains(secret))
        });
        if events_carry {
            found.push("events");
        }
        if self
            .committed_cursors
            .iter()
            .any(|committed| committed.cursor.as_str().contains(secret))
        {
            found.push("cursors");
        }
        if self.raw_stderr.contains(secret) {
            found.push("stderr");
        }
        if self.stderr.contains(secret) {
            found.push("redacted-stderr");
        }
        found
    }
}

/// A Field executable under test, plus the bounds core imposes on it.
///
/// Reusable by every later connector: the `local` Field and every live
/// connector is meant to be run against this kit rather than against
/// throwaway test code.
#[derive(Debug, Clone)]
pub struct FieldUnderTest {
    executable: PathBuf,
    environment: BTreeMap<String, String>,
    limits: Limits,
    idle: Duration,
    wait: Duration,
    secrets: Vec<String>,
}

impl FieldUnderTest {
    /// Pins a Field executable, which must be an absolute path.
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        FieldUnderTest {
            executable: executable.into(),
            environment: BTreeMap::new(),
            limits: Limits::ceilings(),
            idle: Duration::from_secs(10),
            wait: Duration::from_secs(20),
            secrets: Vec::new(),
        }
    }

    /// Adds one allowlisted environment entry for the child.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    /// Lowers the effective bounds for the run.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets how long core waits for a frame before calling the run idle.
    #[must_use]
    pub fn with_idle(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    /// Sets how long core waits for the child to end before terminating it.
    ///
    /// Every wait in the kit is bounded, so a non-terminating child is killed
    /// and reaped rather than leaked.
    #[must_use]
    pub fn with_wait(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }

    /// Registers a secret core is holding, for the canary scan and for core's
    /// own redaction pass.
    #[must_use]
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        self.secrets.push(secret.into());
        self
    }

    fn redactor(&self) -> Redactor {
        let mut redactor = Redactor::new();
        for secret in &self.secrets {
            redactor.register_secret(secret);
        }
        redactor
    }

    fn deadline(&self) -> Result<Deadline, DriverError> {
        Ok(Deadline {
            // A fixture instant, never a wall clock: the kit is deterministic.
            not_after: OffsetDatetime::parse("2099-01-01T00:00:00+00:00")
                .map_err(|error| DriverError::Plan(error.to_string()))?,
            idle_seconds: Deadline::DEFAULT_IDLE_SECONDS,
            cancel_grace_seconds: Deadline::DEFAULT_CANCEL_GRACE_SECONDS,
        })
    }

    fn spawn(&self, operation: Operation) -> Result<(FieldSpawn, FieldProcess), DriverError> {
        let mut spawn = FieldSpawn::new(self.executable.clone(), operation)?;
        for (name, value) in &self.environment {
            spawn = spawn.with_env(name, value);
        }
        let process = spawn.spawn(self.limits)?;
        Ok((spawn, process))
    }

    /// Runs a describe run and settles negotiation.
    pub fn describe(
        &self,
        run_id: &str,
        field_id: Option<&str>,
    ) -> Result<DescribeRun, DriverError> {
        let run_id = parse_run_id(run_id)?;
        let (spawn, mut process) = self.spawn(Operation::Describe)?;
        let request = DescribeRequest {
            v: ProtocolV1,
            frame_type: DescribeRequestTag,
            run_id,
            supported_protocol_versions: VersionList::new([PROTOCOL_VERSION])
                .map_err(|error| DriverError::Plan(error.message))?,
            max_protocol_revision: PROTOCOL_REVISION,
            field_id: match field_id {
                Some(text) => Some(
                    FieldIdToken::parse(text)
                        .map_err(|error| DriverError::Plan(error.to_string()))?,
                ),
                None => None,
            },
            limits: Some(self.limits),
            deadline: self.deadline()?,
        };
        let mut rejection = None;
        let mut detail = None;
        if let Err(error) = process.send(&CoreFrame::Describe(Box::new(request.clone()))) {
            rejection = Some(error.code);
            detail = Some(error.detail);
        }
        process.close_stdin();

        let mut manifest = None;
        let mut negotiation = None;
        if rejection.is_none() {
            match process.next_event(self.idle) {
                Ok(Some(FieldEvent::Manifest(frame))) => {
                    match negotiate(
                        request.supported_protocol_versions.as_slice(),
                        request.max_protocol_revision,
                        PROTOCOL_VERSION,
                        frame.protocol_revision,
                        frame.supported_protocol_versions.as_slice(),
                    ) {
                        Ok(settled) => {
                            negotiation = Some(settled);
                            manifest = Some(*frame);
                        }
                        Err(error) => {
                            rejection = Some(error.code());
                            detail = Some(error.to_string());
                        }
                    }
                }
                Ok(Some(_)) => {
                    rejection = Some(RejectionCode::ProtocolUnexpectedOrder);
                    detail = Some("a describe run answers with exactly one manifest".to_owned());
                }
                Ok(None) => {
                    // A Field that supports no version core offered emits no
                    // manifest at all, which is the correct fail-closed shape.
                    rejection = Some(RejectionCode::ProtocolVersionUnsupported);
                    detail = Some(
                        "the Field emitted no manifest; a manifest it cannot express correctly is \
                         worse than none"
                            .to_owned(),
                    );
                }
                Err(error) => {
                    rejection = Some(error.code);
                    detail = Some(error.detail);
                }
            }
        }

        let exit = process.wait(self.wait)?;
        process.join_stderr();
        let redactor = self.redactor();
        let stderr = redactor.redact_log(&process.captured_stderr());
        Ok(DescribeRun {
            manifest,
            negotiation,
            rejection,
            detail,
            exit,
            stderr,
            argv: spawn.argv(),
            environment: spawn.environment().clone(),
        })
    }

    /// Runs a collect run against a manifest already obtained from a describe
    /// run.
    ///
    /// The persistence order A2 fixes is followed: an artifact is staged and
    /// verified, then made durable, then the Note is installed, and only then is
    /// a cursor eligible for commit.
    pub fn collect(
        &self,
        manifest: &Manifest,
        plan: &CollectPlan,
    ) -> Result<CollectRun, DriverError> {
        let stems = fieldnotes_domain::FieldStemRegistry::v1();
        let request = self.build_request(plan)?;
        let (spawn, mut process) = self.spawn(Operation::Collect)?;
        let mut actions = Vec::new();
        let mut events = Vec::new();
        let mut committed_cursors = Vec::new();
        let mut rejection = None;
        let index = DigestIndex(plan.known_digests.clone());

        let mut session = match CollectSession::new(&request, manifest, stems)
            .map(|session| session.with_current_state(plan.current_state.clone()))
        {
            Ok(session) => session,
            Err(error) => {
                let exit = process.terminate()?;
                let redactor = self.redactor();
                let raw = process.captured_stderr();
                return Ok(CollectRun {
                    report: RunReport {
                        outcome: RunOutcome::Failed,
                        committed_cursor: None,
                        deletion: DeletionAuthorization::Refused {
                            reasons: Vec::new(),
                        },
                        records_accepted: 0,
                        diagnostics_accepted: 0,
                    },
                    events,
                    committed_cursors,
                    actions: vec![CoreObservation::RejectedRun { code: error.code }],
                    rejection: Some(error),
                    exit,
                    stderr: redactor.redact_log(&raw),
                    raw_stderr: raw,
                    stderr_truncated: process.stderr_truncated(),
                    argv: spawn.argv(),
                    environment: spawn.environment().clone(),
                    current_state: plan.current_state.clone(),
                });
            }
        };

        if let Err(error) = process.send(&CoreFrame::Collect(Box::new(request.clone()))) {
            session.note_rejection();
            rejection = Some(Rejection::new(error.code, error.detail));
        }

        let mut records_accepted: u64 = 0;
        let mut cancelled = false;
        while rejection.is_none() {
            let raw = match process.next_frame(self.idle) {
                Ok(Some(raw)) => raw,
                Ok(None) => break,
                Err(error) => {
                    session.note_rejection();
                    rejection = Some(Rejection::new(error.code, error.detail));
                    break;
                }
            };
            let event = match FieldEvent::decode(raw.value) {
                Ok(event) => event,
                Err(error) => {
                    session.note_rejection();
                    rejection = Some(Rejection::new(error.code, error.message));
                    break;
                }
            };
            let record = match &event {
                FieldEvent::Record(frame) => Some(frame.as_ref().clone()),
                _ => None,
            };
            match session.accept(event.clone()) {
                Err(error) => {
                    rejection = Some(error);
                    break;
                }
                Ok(AcceptedEvent::Record(accepted)) => {
                    events.push(event);
                    let Some(record) = record else {
                        rejection = Some(Rejection::new(
                            RejectionCode::ProtocolSchemaInvalid,
                            "an accepted record must be a record frame",
                        ));
                        break;
                    };
                    if let Err(error) =
                        self.install(&mut session, &record, &accepted, plan, &index, &mut actions)
                    {
                        rejection = Some(error);
                        break;
                    }
                    records_accepted += 1;
                    if !cancelled && plan.cancel_after_records == Some(records_accepted) {
                        let cancel = Cancel {
                            v: ProtocolV1,
                            frame_type: CancelTag,
                            run_id: plan.run_id.clone(),
                            reason: CancelReason::UserRequested,
                            grace_seconds: Deadline::DEFAULT_CANCEL_GRACE_SECONDS,
                        };
                        if process.send(&CoreFrame::Cancel(Box::new(cancel))).is_ok() {
                            actions.push(CoreObservation::Cancelled);
                            cancelled = true;
                        }
                    }
                }
                Ok(AcceptedEvent::Checkpoint(offer)) => {
                    events.push(event);
                    match session.commit(&offer) {
                        Ok(committed) => {
                            actions.push(CoreObservation::CommittedCheckpoint {
                                covers_record_seq_through: committed.covers_record_seq_through,
                            });
                            committed_cursors.push(committed);
                        }
                        Err(refusal) => {
                            actions.push(CoreObservation::WithheldCheckpoint {
                                reason: refusal.to_string(),
                            });
                            let _ = session.withhold(&offer);
                        }
                    }
                }
                Ok(AcceptedEvent::Diagnostic { severity, .. }) => {
                    events.push(event);
                    actions.push(CoreObservation::Diagnostic { severity });
                }
            }
        }

        if let Some(failure) = &rejection {
            actions.push(CoreObservation::RejectedRun { code: failure.code });
        }
        let exit = process.wait(self.wait)?;
        process.join_stderr();
        let current_state = session.current_state();
        let report = session.finish(exit);
        actions.push(CoreObservation::ReportedOutcome(report.outcome));
        let redactor = self.redactor();
        let raw_stderr = process.captured_stderr();
        Ok(CollectRun {
            report,
            events,
            committed_cursors,
            rejection,
            actions,
            exit,
            stderr: redactor.redact_log(&raw_stderr),
            raw_stderr,
            stderr_truncated: process.stderr_truncated(),
            argv: spawn.argv(),
            environment: spawn.environment().clone(),
            current_state,
        })
    }

    fn install(
        &self,
        session: &mut CollectSession<'_>,
        record: &RecordEvent,
        accepted: &crate::session::AcceptedRecord,
        plan: &CollectPlan,
        index: &DigestIndex,
        actions: &mut Vec<CoreObservation>,
    ) -> Result<(), Rejection> {
        // Stage, verify, and install or reuse original artifacts, and make them
        // durable, before the Note that references them.
        let resolved = session.resolve_artifacts(record, &plan.staging_dir, index)?;
        for outcome in resolved {
            match outcome {
                crate::artifact::ArtifactOutcome::Resolved(artifact) if artifact.reused => {
                    actions.push(CoreObservation::ReusedArtifact {
                        digest: artifact.digest,
                    });
                }
                crate::artifact::ArtifactOutcome::Resolved(artifact) => {
                    actions.push(CoreObservation::InstalledArtifact {
                        digest: artifact.digest,
                    });
                }
                crate::artifact::ArtifactOutcome::Declined(declined) => {
                    actions.push(CoreObservation::DeclinedArtifact {
                        role: declined.role,
                    });
                }
            }
        }
        if plan.durability.succeeds(accepted.seq) {
            match accepted.disposition {
                crate::session::RecordDisposition::Upsert => {
                    actions.push(CoreObservation::WroteNote { seq: accepted.seq });
                }
                crate::session::RecordDisposition::NoChange => {
                    actions.push(CoreObservation::NoChange { seq: accepted.seq });
                }
                crate::session::RecordDisposition::Delete => {
                    actions.push(CoreObservation::RemovedNote { seq: accepted.seq });
                }
            }
            session.record_durable(accepted.seq);
        } else {
            // The durable write did not complete, so the cursor must not be
            // allowed past it.
            actions.push(CoreObservation::DurableWriteFailed { seq: accepted.seq });
        }
        Ok(())
    }

    fn build_request(&self, plan: &CollectPlan) -> Result<CollectRequest, DriverError> {
        let staging = plan
            .staging_dir
            .to_str()
            .ok_or_else(|| {
                DriverError::Plan("the staging directory path is not valid UTF-8".to_owned())
            })?
            .to_owned();
        Ok(CollectRequest {
            v: ProtocolV1,
            frame_type: CollectRequestTag,
            run_id: plan.run_id.clone(),
            protocol_version: ProtocolV1,
            protocol_revision: PROTOCOL_REVISION,
            field_id: plan.field_id.clone(),
            mode: plan.mode,
            cursor: plan.cursor.as_ref().map(|(cursor, _)| cursor.clone()),
            cursor_format_version: plan.cursor.as_ref().map(|(_, version)| *version),
            window: plan.window,
            snapshot_scope: plan.snapshot_scope.clone(),
            config: plan.config.clone(),
            credential: plan.credential.clone(),
            artifact_staging_dir: staging,
            limits: self.limits,
            deadline: self.deadline()?,
        })
    }
}

/// A manifest core snapshots and compares between runs, for the migration
/// check.
#[must_use]
pub fn manifest_snapshot(manifest: &Manifest) -> crate::declared::ManifestSnapshot {
    crate::declared::ManifestSnapshot::of(manifest)
}

/// The registered property prefix a manifest declares, or `None` for a Field
/// that contributes none.
#[must_use]
pub fn manifest_prefix(manifest: &Manifest) -> Option<&str> {
    manifest
        .property_prefix
        .as_ref()
        .map(PropertyPrefix::as_str)
}
