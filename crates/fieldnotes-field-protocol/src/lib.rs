//! The **proposed** Fieldnotes Field process protocol, version 1.
//!
//! # This implements a proposal, not an approved contract
//!
//! Approval gate A2 — the Field process protocol — is **prepared but not
//! approved**. The proposal lives in `docs/approvals/A2-field-protocol.md`
//! with candidate JSON Schemas and transcripts under
//! `tests/fixtures/protocol/proposed-v1/`. Everything in this crate implements
//! those candidate bytes so that A2 can be reviewed against working code
//! instead of prose alone. **Every type, code, bound, and rule here is subject
//! to change on review**, and none of it is in force until the user approves
//! A2 explicitly. Nothing here may be treated as settled, and no connector may
//! amend it privately: an implementation finding returns to the A2 gate as a
//! recorded finding and a coordinator ruling.
//!
//! # What this crate is
//!
//! The vendor-neutral half of the process boundary, usable from both sides:
//!
//! - [`message`] — Rust data-transfer objects mirroring every candidate
//!   schema: the [`DescribeRequest`] and [`Manifest`], the [`CollectRequest`]
//!   and [`Cancel`] control frames, the [`RecordEvent`], [`CheckpointEvent`]
//!   and [`DiagnosticEvent`] events, and the protected-channel
//!   [`CredentialRequest`] and [`CredentialResponse`];
//! - [`framing`] — newline-delimited JSON framing over standard output with a
//!   hard byte ceiling, plus the bounded standard-error ring buffer;
//! - [`version`] — protocol-version negotiation that fails closed;
//! - [`limits`] — the frozen v1 bound ceilings and the per-run effective
//!   limits;
//! - [`codes`] — the closed rejection, diagnostic, and exit-code vocabularies;
//! - [`declared`] — the `describe` manifest's declared-property mechanism and
//!   the ruling 4 enforcement rules;
//! - [`artifact`] — staged-artifact handle resolution and digest verification;
//! - [`session`] — the core-side run state machine: sequence continuity,
//!   checkpoint commit eligibility, deletion authorization, and run outcome;
//! - [`host`] — enough child-process support to drive a Field executable and
//!   observe its protocol behavior;
//! - [`conformance`] — the transcript fixture format, so the checked-in
//!   transcripts can be used as test vectors.
//!
//! # What this crate is not
//!
//! It contains **no vendor logic**, no credential storage, no notebook
//! writing, and no canonical serialization. A record is a normalized source
//! envelope: post-mapping and pre-serialization. Record IDs, producer
//! provenance, capture time, content hashes, canonical key order and scalar
//! spelling, filenames, and every durable write stay with core, and the record
//! types here structurally exclude them.
//!
//! Field process output is untrusted input. Every reader in this crate bounds
//! what it will hold, rejects malformed and oversized input, and refuses an
//! artifact handle that violates the handle grammar rather than sanitizing it.

pub mod artifact;
pub mod codes;
pub mod conformance;
pub mod declared;
pub mod framing;
pub mod grammar;
pub mod host;
pub mod limits;
pub mod message;
pub mod redact;
pub mod session;
pub mod value;
pub mod version;

pub use artifact::{ArtifactDigestIndex, ArtifactHandle, ResolvedArtifact, resolve_artifact};
pub use codes::{DiagnosticCode, ExitCode, RejectionCode, RunOutcome};
pub use conformance::{
    CollectPlan, CollectRun, CoreObservation, DescribeRun, DriverError, DurabilityPolicy,
    FieldUnderTest, Transcript, TranscriptLine,
};
pub use declared::{Cardinality, DeclaredPropertyIndex, ListSemantics, ScalarType};
pub use framing::{FrameReader, FrameWriter, StderrCapture};
pub use grammar::{
    Cursor, DriverName, DriverVersion, GrammarError, IdentityNamespace, MediaType, ObjectKind,
    PropertyPrefix, ProtocolV1, RunId, Sha256Hex, SourceIdentity, SourceScope,
};
pub use limits::{Deadline, Limits};
pub use message::{
    Cancel, CheckpointEvent, CollectRequest, CoreFrame, CredentialRequest, CredentialResponse,
    DescribeRequest, DiagnosticEvent, FieldEvent, Manifest, RecordEvent,
};
pub use redact::{REDACTION_MARKER, Redactor};
pub use session::{
    AcceptedEvent, CheckpointOffer, CollectSession, CommitRefusal, DeletionAuthorization,
    ExitObservation, Rejection, RunReport,
};
pub use version::{Negotiation, NegotiationError, PROTOCOL_VERSION, ProtocolRevision};
