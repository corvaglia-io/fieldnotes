//! The closed v1 rejection, diagnostic, and exit-code vocabularies.
//!
//! All three are closed by the proposed A2 package. Adding a code is an
//! additive protocol revision; silently ignoring an unknown one is never
//! permitted, which is why every enum here is exhaustive and parsing an
//! unknown spelling fails rather than falling back.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A code core rejects a frame, a record, or a run with.
///
/// The vocabulary is closed at v1 and grouped by what went wrong. These codes
/// are core's, not a Field's: a Field never emits one. Compare
/// [`DiagnosticCode`], which a Field does emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RejectionCode {
    /// A frame's bytes are not valid UTF-8.
    ProtocolInvalidUtf8,
    /// A frame is not a JSON object.
    ProtocolNotJson,
    /// A frame exceeded the effective frame ceiling.
    ProtocolOversizedFrame,
    /// Standard output ended in the middle of a frame.
    ProtocolTruncatedFrame,
    /// A frame does not validate against the schema its `type` selects.
    ProtocolSchemaInvalid,
    /// A frame carries a `type` v1 does not define.
    ProtocolUnknownEvent,
    /// A frame arrived where the protocol does not allow it.
    ProtocolUnexpectedOrder,
    /// Two events shared one sequence number.
    ProtocolDuplicateSeq,
    /// A sequence number went backwards.
    ProtocolSeqRegression,
    /// A sequence number skipped ahead, neither repeating nor regressing.
    /// Distinct from [`RejectionCode::ProtocolUnexpectedOrder`], which is
    /// reserved for a frame arriving somewhere the protocol does not allow it
    /// at all, not for a hole in an otherwise well-ordered stream.
    ProtocolSeqGap,
    /// A declared bound other than the frame ceiling was exceeded.
    ProtocolLimitExceeded,
    /// The run exceeded its wall-clock deadline.
    ProtocolTimeout,
    /// The run produced neither a frame nor artifact progress within the idle
    /// bound.
    ProtocolIdleTimeout,
    /// Standard error exceeded its per-run ceiling.
    ProtocolStderrFlood,
    /// No protocol version is shared by the two peers, or a peer answered with
    /// a version that was not offered.
    ProtocolVersionUnsupported,
    /// A checkpoint's `records_covered` disagrees with what core actually
    /// received for the range it claims to cover. The two sides disagree
    /// about what was transferred.
    ProtocolCoverageMismatch,
    /// An unprefixed property name outside A1's closed shared registry,
    /// including a name the registry types for a derived record only, which a
    /// Field collecting a Note may not emit.
    RecordUnknownProperty,
    /// A prefixed property the declaring manifest does not list.
    RecordUndeclaredProperty,
    /// A prefixed property belonging to another Field's registered stem.
    RecordForeignPrefix,
    /// A property value whose JSON shape contradicts its declared or
    /// registered type or cardinality.
    RecordPropertyTypeMismatch,
    /// A primary Note type the A1 registry does not contain.
    RecordInvalidNoteType,
    /// A record's `note_type` differs from the `note_type` its capability
    /// slice declared in the manifest. Declaration before exercise: without
    /// this check a slice's declared `note_type` is decoration, not a bound.
    RecordNoteTypeNotDeclared,
    /// A `date` value that is not a well-formed A1 date-only value. Distinct
    /// from [`RejectionCode::RecordInvalidDatetime`] so a reviewer can tell
    /// which grammar failed without opening the record.
    RecordInvalidDate,
    /// A datetime that is not an explicit-offset RFC 3339 value.
    RecordInvalidDatetime,
    /// A record without a usable portable exact-source key.
    RecordMissingSourceKey,
    /// One source key asserted twice in one run with divergent payloads and no
    /// declared version ordering.
    RecordDuplicateDivergentInRun,
    /// An artifact handle that violates the single-segment handle grammar.
    /// Distinct from [`RejectionCode::ArtifactNotRegularFile`], which is a
    /// filesystem-shape failure rather than a grammar failure.
    ArtifactInvalidHandle,
    /// A staged entry that is a symlink, a directory, or any other non-regular
    /// file. The handle itself was grammatically valid; what is on the
    /// filesystem is not what a staged artifact must be.
    ArtifactNotRegularFile,
    /// Core's own digest over the staged bytes disagrees with the declared one.
    ArtifactDigestMismatch,
    /// The staged byte count disagrees with the declared length.
    ArtifactLengthMismatch,
    /// A staged handle names nothing inside the staging directory.
    ArtifactMissingStagedFile,
    /// A `digest_only` reference for bytes the notebook does not store.
    ArtifactUnknownDigest,
    /// A staged artifact exceeds the effective artifact ceiling.
    ArtifactOversized,
    /// A staged artifact's declared media type is excluded by the effective
    /// media-type retention policy. Distinct from
    /// [`RejectionCode::ArtifactOversized`] so the two kinds of retention
    /// refusal remain distinguishable in logs, metrics, and tests.
    ArtifactTypeExcluded,
    /// A deletion signal the manifest declared no authority for.
    DeletionUnauthorized,
    /// A completeness claim contradicted by the run's own evidence.
    SnapshotCompletenessContradicted,
    /// A completeness claim wider than the requested snapshot scope.
    SnapshotScopeWidened,
    /// A credential request naming a grant core did not issue.
    CredentialUnknownGrant,
    /// A credential request after its grant expired.
    CredentialGrantExpired,
    /// A credential request after core closed the channel.
    CredentialChannelClosed,
    /// A manifest that changed a declared property's type or cardinality.
    ManifestPropertyTypeChanged,
    /// A manifest that changed its declared cursor format version.
    ManifestCursorFormatChanged,
    /// A record naming an object kind the manifest does not declare.
    ManifestUndeclaredCapability,
}

impl RejectionCode {
    /// Every v1 rejection code, in declaration order.
    pub const ALL: [RejectionCode; 43] = [
        RejectionCode::ProtocolInvalidUtf8,
        RejectionCode::ProtocolNotJson,
        RejectionCode::ProtocolOversizedFrame,
        RejectionCode::ProtocolTruncatedFrame,
        RejectionCode::ProtocolSchemaInvalid,
        RejectionCode::ProtocolUnknownEvent,
        RejectionCode::ProtocolUnexpectedOrder,
        RejectionCode::ProtocolDuplicateSeq,
        RejectionCode::ProtocolSeqRegression,
        RejectionCode::ProtocolSeqGap,
        RejectionCode::ProtocolLimitExceeded,
        RejectionCode::ProtocolTimeout,
        RejectionCode::ProtocolIdleTimeout,
        RejectionCode::ProtocolStderrFlood,
        RejectionCode::ProtocolVersionUnsupported,
        RejectionCode::ProtocolCoverageMismatch,
        RejectionCode::RecordUnknownProperty,
        RejectionCode::RecordUndeclaredProperty,
        RejectionCode::RecordForeignPrefix,
        RejectionCode::RecordPropertyTypeMismatch,
        RejectionCode::RecordInvalidNoteType,
        RejectionCode::RecordNoteTypeNotDeclared,
        RejectionCode::RecordInvalidDate,
        RejectionCode::RecordInvalidDatetime,
        RejectionCode::RecordMissingSourceKey,
        RejectionCode::RecordDuplicateDivergentInRun,
        RejectionCode::ArtifactInvalidHandle,
        RejectionCode::ArtifactNotRegularFile,
        RejectionCode::ArtifactDigestMismatch,
        RejectionCode::ArtifactLengthMismatch,
        RejectionCode::ArtifactMissingStagedFile,
        RejectionCode::ArtifactUnknownDigest,
        RejectionCode::ArtifactOversized,
        RejectionCode::ArtifactTypeExcluded,
        RejectionCode::DeletionUnauthorized,
        RejectionCode::SnapshotCompletenessContradicted,
        RejectionCode::SnapshotScopeWidened,
        RejectionCode::CredentialUnknownGrant,
        RejectionCode::CredentialGrantExpired,
        RejectionCode::CredentialChannelClosed,
        RejectionCode::ManifestPropertyTypeChanged,
        RejectionCode::ManifestCursorFormatChanged,
        RejectionCode::ManifestUndeclaredCapability,
    ];

    /// The wire spelling of this code.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RejectionCode::ProtocolInvalidUtf8 => "protocol.invalid_utf8",
            RejectionCode::ProtocolNotJson => "protocol.not_json",
            RejectionCode::ProtocolOversizedFrame => "protocol.oversized_frame",
            RejectionCode::ProtocolTruncatedFrame => "protocol.truncated_frame",
            RejectionCode::ProtocolSchemaInvalid => "protocol.schema_invalid",
            RejectionCode::ProtocolUnknownEvent => "protocol.unknown_event",
            RejectionCode::ProtocolUnexpectedOrder => "protocol.unexpected_order",
            RejectionCode::ProtocolDuplicateSeq => "protocol.duplicate_seq",
            RejectionCode::ProtocolSeqRegression => "protocol.seq_regression",
            RejectionCode::ProtocolSeqGap => "protocol.seq_gap",
            RejectionCode::ProtocolLimitExceeded => "protocol.limit_exceeded",
            RejectionCode::ProtocolTimeout => "protocol.timeout",
            RejectionCode::ProtocolIdleTimeout => "protocol.idle_timeout",
            RejectionCode::ProtocolStderrFlood => "protocol.stderr_flood",
            RejectionCode::ProtocolVersionUnsupported => "protocol.version_unsupported",
            RejectionCode::ProtocolCoverageMismatch => "protocol.coverage_mismatch",
            RejectionCode::RecordUnknownProperty => "record.unknown_property",
            RejectionCode::RecordUndeclaredProperty => "record.undeclared_property",
            RejectionCode::RecordForeignPrefix => "record.foreign_prefix",
            RejectionCode::RecordPropertyTypeMismatch => "record.property_type_mismatch",
            RejectionCode::RecordInvalidNoteType => "record.invalid_note_type",
            RejectionCode::RecordNoteTypeNotDeclared => "record.note_type_not_declared",
            RejectionCode::RecordInvalidDate => "record.invalid_date",
            RejectionCode::RecordInvalidDatetime => "record.invalid_datetime",
            RejectionCode::RecordMissingSourceKey => "record.missing_source_key",
            RejectionCode::RecordDuplicateDivergentInRun => "record.duplicate_divergent_in_run",
            RejectionCode::ArtifactInvalidHandle => "artifact.invalid_handle",
            RejectionCode::ArtifactNotRegularFile => "artifact.not_regular_file",
            RejectionCode::ArtifactDigestMismatch => "artifact.digest_mismatch",
            RejectionCode::ArtifactLengthMismatch => "artifact.length_mismatch",
            RejectionCode::ArtifactMissingStagedFile => "artifact.missing_staged_file",
            RejectionCode::ArtifactUnknownDigest => "artifact.unknown_digest",
            RejectionCode::ArtifactOversized => "artifact.oversized",
            RejectionCode::ArtifactTypeExcluded => "artifact.type_excluded",
            RejectionCode::DeletionUnauthorized => "deletion.unauthorized",
            RejectionCode::SnapshotCompletenessContradicted => "snapshot.completeness_contradicted",
            RejectionCode::SnapshotScopeWidened => "snapshot.scope_widened",
            RejectionCode::CredentialUnknownGrant => "credential.unknown_grant",
            RejectionCode::CredentialGrantExpired => "credential.grant_expired",
            RejectionCode::CredentialChannelClosed => "credential.channel_closed",
            RejectionCode::ManifestPropertyTypeChanged => "manifest.property_type_changed",
            RejectionCode::ManifestCursorFormatChanged => "manifest.cursor_format_changed",
            RejectionCode::ManifestUndeclaredCapability => "manifest.undeclared_capability",
        }
    }

    /// Parses a wire spelling. An unknown spelling is `None`: the vocabulary is
    /// closed, so there is no permissive branch.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        RejectionCode::ALL
            .into_iter()
            .find(|code| code.as_str() == text)
    }

    /// Whether this code says the frame's own sequence number is untrustworthy,
    /// which is every `protocol.` code. A checker must exclude such a frame
    /// from per-run sequence continuity.
    #[must_use]
    pub fn is_transport_level(self) -> bool {
        self.as_str().starts_with("protocol.")
    }
}

impl fmt::Display for RejectionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The closed v1 diagnostic vocabulary a Field may emit.
///
/// Core must be able to *act* on some of these — `error` severity blocks
/// deletion, throttling drives backoff, [`DiagnosticCode::CursorResetRequired`]
/// triggers recovery — so it cannot act on a string it does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// Interactive re-authentication is required.
    #[serde(rename = "auth.reauth_required")]
    AuthReauthRequired,
    /// Held credential material expired.
    #[serde(rename = "auth.expired")]
    AuthExpired,
    /// The granted scopes do not cover what the Field needs.
    #[serde(rename = "auth.scope_insufficient")]
    AuthScopeInsufficient,
    /// The source denied access.
    #[serde(rename = "permission.denied")]
    PermissionDenied,
    /// An administrator must consent before access is possible.
    #[serde(rename = "permission.admin_consent_required")]
    PermissionAdminConsentRequired,
    /// The source throttled the Field.
    #[serde(rename = "rate_limit.throttled")]
    RateLimitThrottled,
    /// The source is unavailable.
    #[serde(rename = "source.unavailable")]
    SourceUnavailable,
    /// History the Field would need is not retrievable.
    #[serde(rename = "source.history_unavailable")]
    SourceHistoryUnavailable,
    /// Collected content was truncated.
    #[serde(rename = "content.truncated")]
    ContentTruncated,
    /// Collected content was damaged.
    #[serde(rename = "content.damaged")]
    ContentDamaged,
    /// A content format the Field does not support.
    #[serde(rename = "content.unsupported_format")]
    ContentUnsupportedFormat,
    /// Content deliberately skipped.
    #[serde(rename = "content.skipped")]
    ContentSkipped,
    /// An object kind this release does not support.
    #[serde(rename = "capability.unsupported_object")]
    CapabilityUnsupportedObject,
    /// The cursor cannot be used and must be reset.
    #[serde(rename = "cursor.reset_required")]
    CursorResetRequired,
    /// The cursor encoding changed.
    #[serde(rename = "cursor.format_changed")]
    CursorFormatChanged,
    /// A snapshot run did not enumerate its whole scope.
    #[serde(rename = "snapshot.partial")]
    SnapshotPartial,
    /// A refetch is required to recover.
    #[serde(rename = "refetch.required")]
    RefetchRequired,
    /// Refetch is not possible for this source.
    #[serde(rename = "refetch.unsupported")]
    RefetchUnsupported,
    /// The run was cancelled.
    #[serde(rename = "run.cancelled")]
    RunCancelled,
    /// The run hit its deadline.
    #[serde(rename = "run.deadline_exceeded")]
    RunDeadlineExceeded,
    /// Configuration is invalid.
    #[serde(rename = "config.invalid")]
    ConfigInvalid,
    /// An internal Field error.
    #[serde(rename = "internal.error")]
    InternalError,
}

impl DiagnosticCode {
    /// Every v1 diagnostic code, in schema order.
    pub const ALL: [DiagnosticCode; 22] = [
        DiagnosticCode::AuthReauthRequired,
        DiagnosticCode::AuthExpired,
        DiagnosticCode::AuthScopeInsufficient,
        DiagnosticCode::PermissionDenied,
        DiagnosticCode::PermissionAdminConsentRequired,
        DiagnosticCode::RateLimitThrottled,
        DiagnosticCode::SourceUnavailable,
        DiagnosticCode::SourceHistoryUnavailable,
        DiagnosticCode::ContentTruncated,
        DiagnosticCode::ContentDamaged,
        DiagnosticCode::ContentUnsupportedFormat,
        DiagnosticCode::ContentSkipped,
        DiagnosticCode::CapabilityUnsupportedObject,
        DiagnosticCode::CursorResetRequired,
        DiagnosticCode::CursorFormatChanged,
        DiagnosticCode::SnapshotPartial,
        DiagnosticCode::RefetchRequired,
        DiagnosticCode::RefetchUnsupported,
        DiagnosticCode::RunCancelled,
        DiagnosticCode::RunDeadlineExceeded,
        DiagnosticCode::ConfigInvalid,
        DiagnosticCode::InternalError,
    ];
}

/// A Field's exit code, which is the one signal that survives a crashed or
/// hung process.
///
/// Codes 11 to 63 are reserved for additive protocol revisions, 64 to 125 must
/// not be used by a Field, and 126 to 255 belong to the operating system,
/// shell, and signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExitCode {
    /// 0: the run completed normally.
    Completed,
    /// 1: unclassified Field failure; partial results possible.
    Unclassified,
    /// 2: usage or invocation error, such as an unknown operation.
    Usage,
    /// 3: protocol version negotiation failure.
    Negotiation,
    /// 4: authentication or credential failure; re-authentication needed.
    Authentication,
    /// 5: authorization or permission denied by the source.
    Authorization,
    /// 6: source unavailable or throttled beyond the retry budget.
    SourceUnavailable,
    /// 7: cursor unusable; resume impossible without reset or backfill.
    CursorUnusable,
    /// 8: cancelled by core, acknowledged within the grace period.
    Cancelled,
    /// 9: configuration invalid.
    ConfigInvalid,
    /// 10: internal Field error.
    Internal,
    /// 11 to 63: reserved for additive protocol revisions.
    Reserved(u8),
    /// 64 to 125: reserved by convention to shells and `sysexits`; a Field must
    /// not use these.
    ShellReserved(u8),
    /// 126 to 255: operating system, shell, and signal territory.
    PlatformReserved(u8),
}

impl ExitCode {
    /// Classifies a raw process exit status code.
    #[must_use]
    pub fn from_raw(code: u8) -> Self {
        match code {
            0 => ExitCode::Completed,
            1 => ExitCode::Unclassified,
            2 => ExitCode::Usage,
            3 => ExitCode::Negotiation,
            4 => ExitCode::Authentication,
            5 => ExitCode::Authorization,
            6 => ExitCode::SourceUnavailable,
            7 => ExitCode::CursorUnusable,
            8 => ExitCode::Cancelled,
            9 => ExitCode::ConfigInvalid,
            10 => ExitCode::Internal,
            11..=63 => ExitCode::Reserved(code),
            64..=125 => ExitCode::ShellReserved(code),
            126..=255 => ExitCode::PlatformReserved(code),
        }
    }

    /// The raw numeric code.
    #[must_use]
    pub fn as_raw(self) -> u8 {
        match self {
            ExitCode::Completed => 0,
            ExitCode::Unclassified => 1,
            ExitCode::Usage => 2,
            ExitCode::Negotiation => 3,
            ExitCode::Authentication => 4,
            ExitCode::Authorization => 5,
            ExitCode::SourceUnavailable => 6,
            ExitCode::CursorUnusable => 7,
            ExitCode::Cancelled => 8,
            ExitCode::ConfigInvalid => 9,
            ExitCode::Internal => 10,
            ExitCode::Reserved(code)
            | ExitCode::ShellReserved(code)
            | ExitCode::PlatformReserved(code) => code,
        }
    }

    /// Whether this code means the process ended normally.
    ///
    /// Exit zero means the process ended normally; it never means the run
    /// enumerated everything. Completeness is a separate explicit claim.
    #[must_use]
    pub fn is_normal(self) -> bool {
        self == ExitCode::Completed
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_raw())
    }
}

/// The outcome of one Field run.
///
/// Only [`RunOutcome::Complete`] can authorize deletion by absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunOutcome {
    /// Exit 0, no error-severity diagnostic, and in snapshot mode a
    /// completeness claim for the requested scope.
    Complete,
    /// Durable work happened and the run did not complete.
    Partial,
    /// A protocol violation, a rejected record, a crash, or a hang.
    Failed,
}

impl RunOutcome {
    /// A stable lowercase label matching the transcript fixture vocabulary.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RunOutcome::Complete => "complete",
            RunOutcome::Partial => "partial",
            RunOutcome::Failed => "failed",
        }
    }
}

impl fmt::Display for RunOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_vocabulary_round_trips_and_is_closed() {
        for code in RejectionCode::ALL {
            assert_eq!(RejectionCode::parse(code.as_str()), Some(code));
        }
        assert_eq!(RejectionCode::parse("protocol.made_up"), None);
        assert_eq!(RejectionCode::parse(""), None);
    }

    #[test]
    fn transport_codes_are_the_protocol_group() {
        assert!(RejectionCode::ProtocolSeqRegression.is_transport_level());
        assert!(!RejectionCode::ArtifactInvalidHandle.is_transport_level());
    }

    #[test]
    fn diagnostic_codes_use_their_dotted_wire_spelling() -> Result<(), serde_json::Error> {
        let text = serde_json::to_string(&DiagnosticCode::CursorResetRequired)?;
        assert_eq!(text, "\"cursor.reset_required\"");
        let parsed: DiagnosticCode = serde_json::from_str("\"run.cancelled\"")?;
        assert_eq!(parsed, DiagnosticCode::RunCancelled);
        assert!(serde_json::from_str::<DiagnosticCode>("\"made.up\"").is_err());
        Ok(())
    }

    #[test]
    fn exit_codes_classify_reserved_ranges() {
        assert_eq!(ExitCode::from_raw(3), ExitCode::Negotiation);
        assert_eq!(ExitCode::from_raw(20), ExitCode::Reserved(20));
        assert_eq!(ExitCode::from_raw(70), ExitCode::ShellReserved(70));
        assert_eq!(ExitCode::from_raw(137), ExitCode::PlatformReserved(137));
        assert!(ExitCode::from_raw(0).is_normal());
        assert!(!ExitCode::from_raw(8).is_normal());
    }
}
