//! The scenario catalogue: every behavior the transcripts describe, including
//! the pathological ones.
//!
//! The conformance tests need a counterparty that misbehaves on purpose, so a
//! scenario may emit a frame this Field's own types would refuse. Well-formed
//! frames are built as JSON and then **decoded through the protocol crate's own
//! types** before they are written, which proves the data-transfer objects
//! accept them; deliberately malformed output bypasses that step and is written
//! as raw bytes, which is exactly what an untrusted child process can do.

use std::io::{BufRead, Write};
use std::path::Path;

use fieldnotes_field_protocol::artifact::ArtifactHandle;
use fieldnotes_field_protocol::codes::ExitCode as ProtocolExit;
use fieldnotes_field_protocol::framing::FrameWriter;
use fieldnotes_field_protocol::host::read_core_frame;
use fieldnotes_field_protocol::message::{
    CollectRequest, CollectionMode, CoreFrame, DescribeRequest, FieldEvent,
};
use fieldnotes_field_protocol::version::select_version;
use serde_json::{Value, json};

use crate::records::{self, ARTIFACT_BYTES, ARTIFACT_DIGEST};
use crate::{LEAK_VARIABLE, manifests, report};

/// What a scenario run ends with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioOutcome {
    /// The process exit code.
    pub exit_code: u8,
}

impl ScenarioOutcome {
    /// A run that completed normally.
    #[must_use]
    pub fn completed() -> Self {
        ScenarioOutcome {
            exit_code: ProtocolExit::Completed.as_raw(),
        }
    }

    /// A run that ended with a classified failure.
    #[must_use]
    pub fn failed(code: ProtocolExit) -> Self {
        ScenarioOutcome {
            exit_code: code.as_raw(),
        }
    }
}

/// Which manifest a scenario answers a describe request with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Local,
    LocalCursorFormat2,
    LocalRetypedProperty,
    Mail,
    MailWithoutTombstones,
    FutureVersion,
    NoSharedVersion,
}

macro_rules! scenarios {
    ($( $variant:ident => $name:literal, $flavor:ident ; )+) => {
        /// Every behavior the fixture Field can be driven into.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum Scenario {
            $(
                #[doc = concat!("The `", $name, "` scenario.")]
                $variant,
            )+
        }

        impl Scenario {
            /// Parses a scenario name. Unknown names are refused rather than
            /// defaulted, so a typo in a test is a failure and not a silently
            /// different run.
            #[must_use]
            pub fn parse(name: &str) -> Option<Self> {
                match name {
                    $( $name => Some(Scenario::$variant), )+
                    _ => None,
                }
            }

            /// The scenario's name.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $( Scenario::$variant => $name, )+
                }
            }

            /// Every scenario name, for an actionable error message.
            #[must_use]
            pub fn names() -> Vec<&'static str> {
                vec![$( $name, )+]
            }

            fn flavor(self) -> Flavor {
                match self {
                    $( Scenario::$variant => Flavor::$flavor, )+
                }
            }
        }
    };
}

scenarios! {
    DescribeLocal => "describe-local", Local;
    DescribeMail => "describe-mail", Mail;
    DescribeVersionMismatch => "describe-version-mismatch", NoSharedVersion;
    DescribeFutureVersion => "describe-future-version", FutureVersion;
    DescribeCursorFormat2 => "describe-cursor-format-2", LocalCursorFormat2;
    DescribeRetypedProperty => "describe-retyped-property", LocalRetypedProperty;
    Incremental => "incremental", Local;
    Resume => "resume", Local;
    DuplicateReplay => "duplicate-replay", Local;
    DuplicateDivergent => "duplicate-divergent", Local;
    Tombstone => "tombstone", Mail;
    TombstoneUnauthorized => "tombstone-unauthorized", MailWithoutTombstones;
    SnapshotComplete => "snapshot-complete", Local;
    SnapshotPartial => "snapshot-partial", Local;
    SnapshotScopeWidened => "snapshot-scope-widened", Local;
    RedactedDiagnostic => "redacted-diagnostic", Mail;
    MalformedNotJson => "malformed-not-json", Local;
    MalformedUnknownEvent => "malformed-unknown-event", Local;
    MalformedSeqRegression => "malformed-seq-regression", Local;
    MalformedDuplicateSeq => "malformed-duplicate-seq", Local;
    MalformedSeqGap => "malformed-seq-gap", Local;
    MalformedInvalidUtf8 => "malformed-invalid-utf8", Local;
    MalformedOversizedFrame => "malformed-oversized-frame", Local;
    MalformedTruncatedFrame => "malformed-truncated-frame", Local;
    ArtifactDigestOnly => "artifact-digest-only", Local;
    ArtifactTraversalHandle => "artifact-traversal-handle", Local;
    ArtifactAbsoluteHandle => "artifact-absolute-handle", Local;
    ArtifactDeviceNameHandle => "artifact-device-name-handle", Local;
    ArtifactSymlinkEscape => "artifact-symlink-escape", Local;
    ArtifactDigestMismatch => "artifact-digest-mismatch", Local;
    ArtifactUnknownDigest => "artifact-unknown-digest", Local;
    ArtifactMissingStagedFile => "artifact-missing-staged-file", Local;
    ArtifactOversized => "artifact-oversized", Local;
    ArtifactLengthMismatch => "artifact-length-mismatch", Local;
    PropertyUndeclared => "property-undeclared", Local;
    PropertyForeignPrefix => "property-foreign-prefix", Local;
    PropertyUnknown => "property-unknown", Local;
    PropertyTypeMismatch => "property-type-mismatch", Local;
    PropertyCoreOwned => "property-core-owned", Local;
    PropertyInvalidDate => "property-invalid-date", Local;
    PropertyDerivedRecordOnly => "property-derived-record-only", Local;
    NoteTypeInvalid => "note-type-invalid", Local;
    NoteTypeNotDeclared => "note-type-not-declared", Local;
    CapabilityUndeclared => "capability-undeclared", Local;
    ArtifactNotRetained => "artifact-not-retained", Local;
    Cancel => "cancel", Local;
    CrashBeforeCheckpoint => "crash-before-checkpoint", Local;
    CrashAfterCheckpoint => "crash-after-checkpoint", Local;
    ExitBeforeCheckpoint => "exit-before-checkpoint", Local;
    ResumeAfterCrash => "resume-after-crash", Local;
    StderrFlood => "stderr-flood", Local;
    Hang => "hang", Local;
    LeakSecretInDiagnostic => "leak-secret-in-diagnostic", Local;
    LeakSecretInCursor => "leak-secret-in-cursor", Local;
}

/// Answers a describe request.
pub fn describe(scenario: Scenario, request: &DescribeRequest) -> ScenarioOutcome {
    let run_id = request.run_id.as_str();
    // A describe run almost never states limits, since it has almost nothing
    // to bound; fall back to the frozen ceiling when core omitted them.
    let max_frame_bytes = request.limits.map_or(
        fieldnotes_field_protocol::limits::Limits::ceilings().max_frame_bytes,
        |limits| limits.max_frame_bytes,
    );
    let mut emitter = Emitter::new(max_frame_bytes);
    match scenario.flavor() {
        Flavor::NoSharedVersion => {
            // A Field that supports no version core offered emits no manifest:
            // a manifest it cannot express correctly is worse than none.
            let supported = [2_u16, 3];
            if select_version(request.supported_protocol_versions.as_slice(), &supported).is_some()
            {
                report(
                    "fieldnotes-field-fixture: the mismatch scenario requires a core that offers \
                     neither version 2 nor version 3",
                );
                return ScenarioOutcome::failed(ProtocolExit::ConfigInvalid);
            }
            report(&format!(
                "fieldnotes-field-fixture: protocol version mismatch: core offered {:?}, this \
                 build supports {supported:?}. Upgrade Fieldnotes or install the matching Field \
                 build.",
                request.supported_protocol_versions.as_slice()
            ));
            ScenarioOutcome::failed(ProtocolExit::Negotiation)
        }
        Flavor::FutureVersion => {
            emitter.raw_json(&manifests::future_version(run_id));
            ScenarioOutcome::completed()
        }
        flavor => {
            if select_version(request.supported_protocol_versions.as_slice(), &[1]).is_none() {
                report(&format!(
                    "fieldnotes-field-fixture: protocol version mismatch: core offered {:?}, this \
                     build supports [1].",
                    request.supported_protocol_versions.as_slice()
                ));
                return ScenarioOutcome::failed(ProtocolExit::Negotiation);
            }
            let manifest = match flavor {
                Flavor::Local => manifests::local(run_id),
                Flavor::LocalCursorFormat2 => manifests::local_with_cursor_format(run_id, 2),
                Flavor::LocalRetypedProperty => manifests::local_with_retyped_property(run_id),
                Flavor::Mail => manifests::mail(run_id),
                Flavor::MailWithoutTombstones => {
                    manifests::mail_without_tombstone_authority(run_id)
                }
                Flavor::FutureVersion | Flavor::NoSharedVersion => manifests::local(run_id),
            };
            if emitter.frame(manifest) {
                ScenarioOutcome::completed()
            } else {
                ScenarioOutcome::failed(ProtocolExit::Internal)
            }
        }
    }
}

/// Runs a collect scenario.
pub fn collect<R: BufRead>(
    scenario: Scenario,
    request: &CollectRequest,
    input: &mut R,
) -> ScenarioOutcome {
    let run_id = request.run_id.as_str();
    let limits = request.limits;
    let staging = Path::new(&request.artifact_staging_dir);
    let mut out = Emitter::new(limits.max_frame_bytes);
    let scope = request.snapshot_scope.as_ref().map_or_else(
        || records::LOCAL_SCOPE.to_owned(),
        |scope| scope.as_str().to_owned(),
    );

    match scenario {
        Scenario::Incremental => {
            if !stage(staging, "a0001", ARTIFACT_BYTES) {
                return ScenarioOutcome::failed(ProtocolExit::Internal);
            }
            out.frame(records::readme_with_staged_artifact(run_id, 1, "a0001"));
            out.frame(records::agreement(run_id, 2));
            out.frame(records::checkpoint(
                run_id,
                3,
                "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
                2,
                2,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::Resume => {
            // Resumption is observable: with a replayable cursor the Field
            // returns only newer material, and without one it starts unbounded.
            match &request.cursor {
                Some(_) => {
                    out.frame(records::skipped_diagnostic(run_id, 1));
                    out.frame(records::timeline(run_id, 2));
                    out.frame(records::checkpoint(
                        run_id,
                        3,
                        "walk:v1:seq=3;mtime=2026-08-22T12:05:00Z",
                        2,
                        1,
                        true,
                    ));
                }
                None => {
                    out.frame(records::readme(run_id, 1));
                    out.frame(records::agreement(run_id, 2));
                    out.frame(records::timeline(run_id, 3));
                    out.frame(records::checkpoint(
                        run_id,
                        4,
                        "walk:v1:seq=3;mtime=2026-08-22T12:05:00Z",
                        3,
                        3,
                        true,
                    ));
                }
            }
            ScenarioOutcome::completed()
        }
        Scenario::DuplicateReplay => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::readme(run_id, 2));
            out.frame(records::checkpoint(
                run_id,
                3,
                "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
                2,
                2,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::DuplicateDivergent => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::readme_divergent(run_id, 2));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::Tombstone | Scenario::TombstoneUnauthorized => {
            out.frame(records::mail_tombstone(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "graph-delta:v1:eyJ0b2tlbiI6IjAyIn0",
                1,
                1,
                true,
            ));
            if scenario == Scenario::Tombstone {
                ScenarioOutcome::completed()
            } else {
                ScenarioOutcome::failed(ProtocolExit::Internal)
            }
        }
        Scenario::SnapshotComplete => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::agreement(run_id, 2));
            out.frame(records::snapshot_checkpoint(
                run_id,
                3,
                "walk:v1:snapshot;generation=7",
                2,
                2,
                records::claim(&scope, "complete", 2),
            ));
            ScenarioOutcome::completed()
        }
        Scenario::SnapshotPartial => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::unavailable_diagnostic(run_id, 2));
            out.frame(records::snapshot_checkpoint(
                run_id,
                3,
                "walk:v1:partial;prefix=projects/",
                1,
                1,
                records::claim(&scope, "partial", 1),
            ));
            ScenarioOutcome::failed(ProtocolExit::SourceUnavailable)
        }
        Scenario::SnapshotScopeWidened => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::snapshot_checkpoint(
                run_id,
                2,
                "walk:v1:snapshot;generation=8",
                1,
                1,
                records::claim("local-root:everything", "complete", 1),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::RedactedDiagnostic => {
            out.frame(records::mail_message(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "graph-delta:v1:eyJ0b2tlbiI6IjAzIn0",
                1,
                1,
                false,
            ));
            out.frame(records::redacted_auth_diagnostic(run_id, 3));
            report(
                "2026-08-22T12:20:04+02:00 WARN outlook-mail: token refresh failed for profile \
                 microsoft_work; giving up after 1 attempt",
            );
            ScenarioOutcome::failed(ProtocolExit::Authentication)
        }
        Scenario::MalformedNotJson => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                false,
            ));
            out.raw_bytes(
                b"Traceback (most recent call last): NullPointerException in mail mapper\n",
            );
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::MalformedUnknownEvent => {
            out.raw_json(&json!({ "v": 1, "type": "note", "run_id": run_id, "seq": 1 }));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::MalformedSeqRegression => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                false,
            ));
            out.frame(records::readme(run_id, 1));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::MalformedDuplicateSeq => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::agreement(run_id, 1));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::MalformedSeqGap => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::agreement(run_id, 3));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::MalformedInvalidUtf8 => {
            let mut bytes = br#"{"v":1,"type":"record","body":""#.to_vec();
            bytes.extend_from_slice(&[0xc3, 0x28]);
            bytes.extend_from_slice(b"\"}\n");
            out.raw_bytes(&bytes);
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::MalformedOversizedFrame => {
            let ceiling = usize::try_from(limits.max_frame_bytes).unwrap_or(1 << 20);
            let padding = "A".repeat(ceiling + 1024);
            let mut frame = records::readme(run_id, 1);
            if let Some(body) = frame.get_mut("body").and_then(Value::as_object_mut) {
                body.insert("text".to_owned(), json!(padding));
            }
            out.raw_json(&frame);
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::MalformedTruncatedFrame => {
            out.raw_bytes(
                format!(
                    "{{\"v\":1,\"type\":\"checkpoint\",\"run_id\":\"{run_id}\",\"seq\":9,\"cur"
                )
                .as_bytes(),
            );
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactDigestOnly => {
            out.frame(records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "digest_only",
                    "sha256": ARTIFACT_DIGEST,
                    "media_type": "text/plain",
                    "role": "attachment",
                    "source_filename": "migration notes.txt"
                }),
            ));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=1;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::ArtifactTraversalHandle => {
            out.raw_json(&records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "staged",
                    "handle": "../../../../etc/passwd",
                    "byte_length": 4096,
                    "role": "original"
                }),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactAbsoluteHandle => {
            out.raw_json(&records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "staged",
                    "handle": "/Users/joe/.ssh/id_ed25519",
                    "byte_length": 464,
                    "role": "original"
                }),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactDeviceNameHandle => {
            out.raw_json(&records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "staged",
                    "handle": "nul",
                    "byte_length": 0,
                    "role": "original"
                }),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactSymlinkEscape => {
            if !stage_escape(staging, "a0004") {
                return ScenarioOutcome::failed(ProtocolExit::Internal);
            }
            out.frame(records::readme_with_staged_artifact(run_id, 1, "a0004"));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactDigestMismatch => {
            if !stage(staging, "a0005", ARTIFACT_BYTES) {
                return ScenarioOutcome::failed(ProtocolExit::Internal);
            }
            out.frame(records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "staged",
                    "handle": "a0005",
                    "sha256": records::WRONG_ARTIFACT_DIGEST,
                    "byte_length": ARTIFACT_BYTES.len(),
                    "media_type": "text/markdown",
                    "role": "original"
                }),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactUnknownDigest => {
            out.frame(records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "digest_only",
                    "sha256": records::UNKNOWN_ARTIFACT_DIGEST,
                    "media_type": "application/octet-stream",
                    "role": "original"
                }),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactMissingStagedFile => {
            out.frame(records::readme_with_staged_artifact(run_id, 1, "a0009"));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactOversized => {
            out.frame(records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "staged",
                    "handle": "a0010",
                    "byte_length": limits.max_artifact_bytes.saturating_add(1),
                    "media_type": "application/octet-stream",
                    "role": "original"
                }),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactLengthMismatch => {
            if !stage(staging, "a0011", ARTIFACT_BYTES) {
                return ScenarioOutcome::failed(ProtocolExit::Internal);
            }
            out.frame(records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "staged",
                    "handle": "a0011",
                    "byte_length": ARTIFACT_BYTES.len() - 1,
                    "media_type": "text/markdown",
                    "role": "original"
                }),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::PropertyUndeclared => {
            out.frame(records::readme_with_property(
                run_id,
                1,
                "local_inode_number",
                json!(8_814_423),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::PropertyForeignPrefix => {
            out.frame(records::readme_with_property(
                run_id,
                1,
                "teams_chat_id",
                json!("19:abc"),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::PropertyUnknown => {
            out.frame(records::readme_with_property(
                run_id,
                1,
                "checksum_kind",
                json!("sha256"),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::PropertyTypeMismatch => {
            let mut record = records::agreement(run_id, 1);
            if let Some(properties) = record.get_mut("properties").and_then(Value::as_object_mut) {
                properties.insert("local_document_flag".to_owned(), json!(true));
                properties.insert("local_tags".to_owned(), json!("contracts"));
            }
            out.frame(record);
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::PropertyCoreOwned => {
            let mut record = records::readme(run_id, 1);
            if let Some(properties) = record.get_mut("properties").and_then(Value::as_object_mut) {
                properties.insert(
                    "id".to_owned(),
                    json!("note_01a02844-f150-7000-8000-000000000001"),
                );
                properties.insert(
                    "content_hash".to_owned(),
                    json!("fn-content-v1-sha256:0000"),
                );
            }
            out.raw_json(&record);
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::NoteTypeInvalid => {
            let mut record = records::readme(run_id, 1);
            if let Some(object) = record.as_object_mut() {
                object.insert("note_type".to_owned(), json!("email"));
            }
            out.frame(record);
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::NoteTypeNotDeclared => {
            // "document" is one of A1's eleven approved types, so it passes
            // the registry check; it simply is not what the "file" capability
            // slice declares. Declaration before exercise: a slice's
            // note_type is a bound, not decoration.
            out.frame(records::readme_with_note_type(run_id, 1, "document"));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::PropertyInvalidDate => {
            out.frame(records::agreement_with_property(
                run_id,
                1,
                "local_document_date",
                json!("not-a-date"),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::PropertyDerivedRecordOnly => {
            // 'confidence' is a registered A1 property, but only for a
            // derived record generated from other Notes. A Field collects a
            // Note and must not be able to emit it.
            out.frame(records::readme_with_property(
                run_id,
                1,
                "confidence",
                json!(0.9),
            ));
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::ArtifactNotRetained => {
            // Bytes this large stay at the source by policy: the Field
            // declines to stage them, and the run still completes.
            out.frame(records::readme_with_artifact(
                run_id,
                1,
                json!({
                    "kind": "not_retained",
                    "byte_length": 1_073_741_824_u64,
                    "media_type": "application/zip",
                    "role": "attachment",
                    "source_filename": "full-export.zip"
                }),
            ));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=1;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::CapabilityUndeclared => {
            let mut record = records::readme(run_id, 1);
            if let Some(object) = record.as_object_mut() {
                object.insert("object_kind".to_owned(), json!("mailbox"));
            }
            out.frame(record);
            ScenarioOutcome::failed(ProtocolExit::Internal)
        }
        Scenario::Cancel => {
            out.frame(records::readme(run_id, 1));
            // Cooperative cancellation: stop starting new work, offer one final
            // checkpoint for what was already emitted, and exit with code 8.
            match read_core_frame(input, limits.max_frame_bytes) {
                Ok(Some(CoreFrame::Cancel(cancel))) => {
                    report(&format!(
                        "fieldnotes-field-fixture: cancelled ({:?}), {} second grace",
                        cancel.reason, cancel.grace_seconds
                    ));
                }
                other => {
                    report(&format!(
                        "fieldnotes-field-fixture: expected a cancel frame, got {other:?}"
                    ));
                    return ScenarioOutcome::failed(ProtocolExit::Usage);
                }
            }
            out.frame(records::cancelled_diagnostic(run_id, 2));
            if request.mode == CollectionMode::Snapshot {
                out.frame(records::snapshot_checkpoint(
                    run_id,
                    3,
                    "walk:v1:partial;prefix=projects/rollout/readme.md",
                    1,
                    1,
                    records::claim(&scope, "partial", 1),
                ));
            } else {
                out.frame(records::checkpoint(
                    run_id,
                    3,
                    "walk:v1:partial;prefix=projects/rollout/readme.md",
                    1,
                    1,
                    true,
                ));
            }
            ScenarioOutcome::failed(ProtocolExit::Cancelled)
        }
        Scenario::CrashAfterCheckpoint => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                false,
            ));
            report("fieldnotes-field-fixture: simulated crash after the checkpoint");
            std::process::abort();
        }
        Scenario::CrashBeforeCheckpoint => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                false,
            ));
            out.frame(records::agreement(run_id, 3));
            report(
                "fieldnotes-field-fixture: simulated crash after a durable write and before the \
                 checkpoint covering it",
            );
            std::process::abort();
        }
        Scenario::ExitBeforeCheckpoint => {
            out.frame(records::readme(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=2;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                false,
            ));
            out.frame(records::agreement(run_id, 3));
            ScenarioOutcome::failed(ProtocolExit::Unclassified)
        }
        Scenario::ResumeAfterCrash => {
            if request.cursor.is_none() {
                report(
                    "fieldnotes-field-fixture: the resume-after-crash scenario expects the cursor \
                     committed before the crash",
                );
                return ScenarioOutcome::failed(ProtocolExit::CursorUnusable);
            }
            out.frame(records::agreement(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=3;mtime=2026-08-22T20:05:00Z",
                1,
                1,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::StderrFlood => {
            // Standard error carries logs, never protocol data, and core keeps
            // only a bounded ring buffer of it.
            let line = "x".repeat(120);
            for index in 0..4096 {
                report(&format!("{index:06} {line}"));
            }
            out.frame(records::readme(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                "walk:v1:seq=1;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::Hang => {
            // A connector that stops making progress: core's idle bound, not
            // the connector's good behavior, is what ends the run.
            std::thread::sleep(std::time::Duration::from_secs(30));
            ScenarioOutcome::completed()
        }
        Scenario::LeakSecretInDiagnostic => {
            let leaked = std::env::var(LEAK_VARIABLE).unwrap_or_default();
            out.frame(records::readme(run_id, 1));
            out.frame(records::leaking_diagnostic(run_id, 2, &leaked));
            out.frame(records::checkpoint(
                run_id,
                3,
                "walk:v1:seq=1;mtime=2026-08-22T09:45:00Z",
                1,
                1,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::LeakSecretInCursor => {
            let leaked = std::env::var(LEAK_VARIABLE).unwrap_or_default();
            out.frame(records::readme(run_id, 1));
            out.frame(records::checkpoint(
                run_id,
                2,
                &format!("walk:v1:token={leaked}"),
                1,
                1,
                true,
            ));
            ScenarioOutcome::completed()
        }
        Scenario::DescribeLocal
        | Scenario::DescribeMail
        | Scenario::DescribeVersionMismatch
        | Scenario::DescribeFutureVersion
        | Scenario::DescribeCursorFormat2
        | Scenario::DescribeRetypedProperty => {
            report(&format!(
                "fieldnotes-field-fixture: scenario {} is a describe-run scenario",
                scenario.as_str()
            ));
            ScenarioOutcome::failed(ProtocolExit::Usage)
        }
    }
}

/// Writes `bytes` into the run's staging directory under `handle`.
///
/// The handle is validated against the closed grammar and joined with
/// [`Path::join`], never concatenated, so even the fixture cannot write outside
/// the directory core named.
fn stage(staging: &Path, handle: &str, bytes: &[u8]) -> bool {
    let Ok(handle) = ArtifactHandle::parse(handle) else {
        report("fieldnotes-field-fixture: refusing to stage under an invalid handle");
        return false;
    };
    if std::fs::create_dir_all(staging).is_err() {
        report("fieldnotes-field-fixture: the staging directory could not be created");
        return false;
    }
    match std::fs::write(handle.resolve_in(staging), bytes) {
        Ok(()) => true,
        Err(error) => {
            report(&format!(
                "fieldnotes-field-fixture: staging failed for {handle}: {error}"
            ));
            false
        }
    }
}

/// Stages something that is not a regular file, so core must refuse to read it.
///
/// A symlink out of the staging directory where the platform allows one, and a
/// directory otherwise: creating a symlink on Windows needs a privilege a test
/// process may not hold, and a directory exercises the same rule that the staged
/// entry must be a regular file.
fn stage_escape(staging: &Path, handle: &str) -> bool {
    let Ok(handle) = ArtifactHandle::parse(handle) else {
        return false;
    };
    if std::fs::create_dir_all(staging).is_err() {
        return false;
    }
    let path = handle.resolve_in(staging);
    #[cfg(unix)]
    {
        let target = Path::new("/etc/passwd");
        if std::os::unix::fs::symlink(target, &path).is_ok() {
            return true;
        }
    }
    match std::fs::create_dir(&path) {
        Ok(()) => true,
        Err(error) => {
            report(&format!(
                "fieldnotes-field-fixture: could not stage a non-regular entry: {error}"
            ));
            false
        }
    }
}

/// Writes frames to standard output, which carries protocol data and nothing
/// else.
struct Emitter {
    writer: FrameWriter<std::io::Stdout>,
}

impl Emitter {
    fn new(max_frame_bytes: u64) -> Self {
        Emitter {
            writer: FrameWriter::new(std::io::stdout(), max_frame_bytes),
        }
    }

    /// Decodes the value through the protocol crate's own types and writes the
    /// re-encoded frame, so a well-formed scenario cannot emit something the
    /// data-transfer objects would refuse.
    fn frame(&mut self, value: Value) -> bool {
        match FieldEvent::decode(value) {
            Ok(event) => match self.writer.write_event(&event) {
                Ok(_) => true,
                Err(error) => {
                    report(&format!("fieldnotes-field-fixture: {error}"));
                    false
                }
            },
            Err(error) => {
                report(&format!(
                    "fieldnotes-field-fixture: refusing to emit a frame its own types reject: \
                     {error}"
                ));
                false
            }
        }
    }

    /// Writes a value verbatim, without validating it.
    ///
    /// This is how the fixture misbehaves on purpose: an untrusted child process
    /// is under no obligation to self-police, and core must not depend on it.
    fn raw_json(&mut self, value: &Value) {
        match serde_json::to_vec(value) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                self.raw_bytes(&bytes);
            }
            Err(error) => report(&format!("fieldnotes-field-fixture: {error}")),
        }
    }

    /// Writes bytes verbatim, terminating nothing and validating nothing.
    fn raw_bytes(&mut self, bytes: &[u8]) {
        let mut out = std::io::stdout();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }
}
