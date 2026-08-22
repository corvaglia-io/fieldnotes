//! The records, checkpoints, and diagnostics the fixture Field emits.
//!
//! Every instant, digest, cursor, and property value is a constant taken from
//! the checked-in transcripts, and the two artifact digests plus the 40-byte
//! length are the frozen A1 vectors from `tests/fixtures/hashes/proposed-v1/`.
//! Nothing here reads a clock or a random source, so a scenario is reproducible
//! byte for byte.

use serde_json::{Value, json};

/// The frozen A1 artifact vector: the exact 40 bytes of
/// `tests/fixtures/hashes/proposed-v1/artifact-input.bin`, whose SHA-256 is
/// [`ARTIFACT_DIGEST`].
pub const ARTIFACT_BYTES: &[u8] = b"Fieldnotes artifact bytes.\nSecond line.\n";

/// Core's expected digest over [`ARTIFACT_BYTES`].
pub const ARTIFACT_DIGEST: &str =
    "449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17";

/// The second frozen A1 artifact digest, used for a `digest_only` reference to
/// bytes a notebook does not store.
pub const UNKNOWN_ARTIFACT_DIGEST: &str =
    "cf741b831206259b64c8ec80e25fe3e584e131fd8122b9310fd8feab47dfb36f";

/// A digest that belongs to no bytes at all, for the mismatch case.
pub const WRONG_ARTIFACT_DIGEST: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// The source scope both local records share.
pub const LOCAL_SCOPE: &str = "local-root:reference-library-v1";

/// The mail Field's source scope.
pub const MAIL_SCOPE: &str = "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001";

/// The `readme.md` file record, without artifacts.
#[must_use]
pub fn readme(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": seq,
        "change": "upsert",
        "source": {
            "scope": LOCAL_SCOPE,
            "identity": "file/projects/rollout/readme.md",
            "version": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        },
        "object_kind": "file",
        "note_type": "file",
        "occurred_at": "2026-08-22T09:45:00+02:00",
        "properties": {
            "title": "Rollout reference",
            "local_media_type": "text/markdown",
            "local_relative_path": "projects/rollout/readme.md"
        },
        "body": {
            "format": "markdown",
            "text": "# Rollout reference\n\nLocal reference material describing the rollout checklist.\n"
        },
        "integrity": { "damaged": false, "truncated": false }
    })
}

/// The `readme.md` record with one staged original artifact attached.
#[must_use]
pub fn readme_with_staged_artifact(run_id: &str, seq: u64, handle: &str) -> Value {
    let mut record = readme(run_id, seq);
    set(
        &mut record,
        "artifacts",
        json!([
            {
                "kind": "staged",
                "handle": handle,
                "sha256": ARTIFACT_DIGEST,
                "byte_length": ARTIFACT_BYTES.len(),
                "media_type": "text/markdown",
                "role": "original",
                "source_filename": "readme.md"
            }
        ]),
    );
    record
}

/// The `readme.md` record carrying one arbitrary artifact reference.
#[must_use]
pub fn readme_with_artifact(run_id: &str, seq: u64, reference: Value) -> Value {
    let mut record = readme(run_id, seq);
    set(&mut record, "artifacts", json!([reference]));
    record
}

/// The `readme.md` record with one extra property candidate.
#[must_use]
pub fn readme_with_property(run_id: &str, seq: u64, name: &str, value: Value) -> Value {
    let mut record = readme(run_id, seq);
    if let Some(properties) = record.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(name.to_owned(), value);
    }
    record
}

/// The `readme.md` record declaring a `note_type` that disagrees with its
/// capability slice's declared one. `object_kind` stays `file`, whose
/// declared slice always maps to `note_type: "file"`.
#[must_use]
pub fn readme_with_note_type(run_id: &str, seq: u64, note_type: &str) -> Value {
    let mut record = readme(run_id, seq);
    set(&mut record, "note_type", json!(note_type));
    record
}

/// The master services agreement record with one property candidate replaced,
/// for exercising a declared property's value grammar in isolation.
#[must_use]
pub fn agreement_with_property(run_id: &str, seq: u64, name: &str, value: Value) -> Value {
    let mut record = agreement(run_id, seq);
    if let Some(properties) = record.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(name.to_owned(), value);
    }
    record
}

/// The `readme.md` record with a divergent body, for the in-run divergence case.
#[must_use]
pub fn readme_divergent(run_id: &str, seq: u64) -> Value {
    let mut record = readme(run_id, seq);
    set(
        &mut record,
        "body",
        json!({
            "format": "markdown",
            "text": "# Rollout reference\n\nA different current state for the same source object.\n"
        }),
    );
    record
}

/// The `timeline.md` file record, which transcript 02 collects on resumption.
#[must_use]
pub fn timeline(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": seq,
        "change": "upsert",
        "source": {
            "scope": LOCAL_SCOPE,
            "identity": "file/projects/rollout/timeline.md",
            "version": "sha256:1111111111111111111111111111111111111111111111111111111111111111"
        },
        "object_kind": "file",
        "note_type": "file",
        "occurred_at": "2026-08-22T12:05:00+02:00",
        "properties": {
            "title": "Rollout timeline",
            "local_media_type": "text/markdown",
            "local_relative_path": "projects/rollout/timeline.md"
        },
        "body": {
            "format": "markdown",
            "text": "# Rollout timeline\n\nWave 1 on Thursday, wave 2 the following Monday.\n"
        },
        "integrity": { "damaged": false, "truncated": false }
    })
}

/// The master services agreement document record, whose declared prefixed
/// properties exercise a date, an opaque text flag, and a set-like list.
#[must_use]
pub fn agreement(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": seq,
        "change": "upsert",
        "source": {
            "scope": LOCAL_SCOPE,
            "identity": "document/contracts/2026-08-msa.md",
            "version": "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
        },
        "object_kind": "document",
        "note_type": "document",
        "occurred_at": "2026-08-22T20:00:00-05:00",
        "properties": {
            "title": "Master services agreement",
            "local_media_type": "text/markdown",
            "local_relative_path": "contracts/2026-08-msa.md",
            "local_document_date": "2026-08-20",
            "local_document_flag": "true",
            "local_tags": ["contracts", "legal"]
        },
        "body": {
            "format": "markdown",
            "text": "# Master services agreement\n\nSigned copy of the August agreement.\n"
        },
        "integrity": { "damaged": false, "truncated": false }
    })
}

/// An authoritative tombstone for one mail message.
///
/// It carries the portable source key, its authority, and the observation
/// instant, and no content at all.
#[must_use]
pub fn mail_tombstone(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": seq,
        "change": "delete",
        "source": {
            "scope": MAIL_SCOPE,
            "identity": "mail-message/AAMkAGI2TQABAAAA"
        },
        "object_kind": "mail-message",
        "authority": "tombstone",
        "observed_at": "2026-08-22T12:40:11+02:00"
    })
}

/// One mail message record with identity anchors, as transcript 06 shows it.
#[must_use]
pub fn mail_message(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": seq,
        "change": "upsert",
        "source": {
            "scope": MAIL_SCOPE,
            "identity": "mail-message/AAMkAGI2TQABAAAA",
            "version": "CQAAABYAAAC1",
            "url": "https://outlook.office.com/mail/id/AAMkAGI2TQABAAAA"
        },
        "object_kind": "mail-message",
        "note_type": "mail",
        "occurred_at": "2026-08-22T10:00:00+02:00",
        "properties": {
            "subject": "Migration Thursday",
            "from": "alice@example.com",
            "to": ["joe@example.net"],
            "cc": ["bob@example.net"],
            "participants": ["alice@example.com", "bob@example.net", "joe@example.net"],
            "conversation_id": "AAQkAGI2TQ",
            "thread_id": "outlook-thread/AAQkAGI2TQ",
            "outlook_mail_importance": "normal",
            "outlook_mail_internet_message_id": "<migration-thursday@example.com>",
            "outlook_mail_folder_path": "Inbox",
            "outlook_mail_has_attachments": true,
            "outlook_mail_categories": ["migration", "tenant"]
        },
        "body": {
            "format": "markdown",
            "text": "# Migration Thursday\n\nHi Joe,\n\nWe move the tenant on Thursday at 09:00.\n\nAlice\n"
        },
        "identity_anchors": [
            {
                "namespace": "email",
                "value": "alice@example.com",
                "scope_class": "normalized_channel",
                "normalization_rule": "email_v1",
                "normalization_version": 1,
                "role": "sender"
            },
            {
                "namespace": "microsoft-graph-user-id",
                "value": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
                "scope_class": "authority_scoped",
                "scope": MAIL_SCOPE,
                "role": "sender"
            }
        ],
        "integrity": { "damaged": false, "truncated": false }
    })
}

/// A mail record with one retained text attachment and one video attachment
/// the Field declines to retain, per the run's default media-type retention
/// policy (ADR 0007). `attachment_ref` is the only stable identity a declined
/// artifact carries, since it has no bytes and no digest.
#[must_use]
pub fn standup_recording_declined(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": seq,
        "change": "upsert",
        "source": {
            "scope": MAIL_SCOPE,
            "identity": "mail-message/AAMkAGI2TQABAAAF",
            "version": "CQAAABYAAAC5",
            "url": "https://outlook.office.com/mail/id/AAMkAGI2TQABAAAF"
        },
        "object_kind": "mail-message",
        "note_type": "mail",
        "occurred_at": "2026-08-22T11:20:00+02:00",
        "properties": {
            "subject": "Team standup recording",
            "from": "alice@example.com",
            "to": ["joe@example.net"],
            "participants": ["alice@example.com", "joe@example.net"],
            "conversation_id": "AAQkAGI2TS",
            "thread_id": "outlook-thread/AAQkAGI2TS",
            "outlook_mail_importance": "normal",
            "outlook_mail_internet_message_id": "<standup-recording@example.com>",
            "outlook_mail_folder_path": "Inbox",
            "outlook_mail_has_attachments": true
        },
        "body": {
            "format": "markdown",
            "text": "# Team standup recording\n\nHi Joe,\n\nSharing this week's standup notes and the recording.\n\nAlice\n"
        },
        "artifacts": [
            {
                "kind": "staged",
                "handle": "a0021",
                "sha256": ARTIFACT_DIGEST,
                "byte_length": ARTIFACT_BYTES.len(),
                "media_type": "text/plain",
                "role": "attachment",
                "source_filename": "notes.txt"
            },
            {
                "kind": "not_retained",
                "byte_length": 641_728_512,
                "media_type": "video/mp4",
                "role": "attachment",
                "source_filename": "team-standup-recording.mp4",
                "attachment_ref": "mail-attachment/AAMkAGI2TQABAAACattach02"
            }
        ],
        "integrity": { "damaged": false, "truncated": false }
    })
}

/// The same message, misbehaving: a hostile Field stages the excluded video
/// type anyway instead of declining it. Core must reject this before it
/// fails for any other reason.
#[must_use]
pub fn standup_recording_hostile_staged_video(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": seq,
        "change": "upsert",
        "source": {
            "scope": MAIL_SCOPE,
            "identity": "mail-message/AAMkAGI2TQABAAAG",
            "version": "CQAAABYAAAC6",
            "url": "https://outlook.office.com/mail/id/AAMkAGI2TQABAAAG"
        },
        "object_kind": "mail-message",
        "note_type": "mail",
        "occurred_at": "2026-08-22T11:25:00+02:00",
        "properties": {
            "subject": "Second copy of the recording",
            "from": "alice@example.com",
            "to": ["joe@example.net"],
            "participants": ["alice@example.com", "joe@example.net"],
            "conversation_id": "AAQkAGI2TS",
            "thread_id": "outlook-thread/AAQkAGI2TS",
            "outlook_mail_importance": "normal",
            "outlook_mail_internet_message_id": "<standup-recording-2@example.com>",
            "outlook_mail_folder_path": "Inbox",
            "outlook_mail_has_attachments": true
        },
        "body": {
            "format": "markdown",
            "text": "# Second copy of the recording\n\nA hostile Field stages the same excluded type instead of declining it.\n"
        },
        "artifacts": [
            {
                "kind": "staged",
                "handle": "a0022",
                "sha256": ARTIFACT_DIGEST,
                "byte_length": ARTIFACT_BYTES.len(),
                "media_type": "video/mp4",
                "role": "attachment",
                "source_filename": "team-standup-recording-2.mp4"
            }
        ],
        "integrity": { "damaged": false, "truncated": false }
    })
}

/// A checkpoint offering a resume point.
#[must_use]
pub fn checkpoint(
    run_id: &str,
    seq: u64,
    cursor: &str,
    covers: u64,
    records_covered: u64,
    is_final: bool,
) -> Value {
    json!({
        "v": 1,
        "type": "checkpoint",
        "run_id": run_id,
        "seq": seq,
        "cursor": cursor,
        "cursor_format_version": 1,
        "covers_record_seq_through": covers,
        "records_covered": records_covered,
        "final": is_final
    })
}

/// A snapshot completeness claim, for [`snapshot_checkpoint`].
///
/// `complete` is the Field's explicit claim that it enumerated the whole
/// declared scope; anything else leaves absence meaningless.
#[must_use]
pub fn claim(scope: &str, state: &str, enumerated: u64) -> Value {
    json!({ "scope": scope, "state": state, "objects_enumerated": enumerated })
}

/// A final checkpoint carrying a snapshot completeness claim.
#[must_use]
pub fn snapshot_checkpoint(
    run_id: &str,
    seq: u64,
    cursor: &str,
    covers: u64,
    records_covered: u64,
    claim: Value,
) -> Value {
    let mut frame = checkpoint(run_id, seq, cursor, covers, records_covered, true);
    set(&mut frame, "snapshot", claim);
    frame
}

/// An informational diagnostic.
#[must_use]
pub fn skipped_diagnostic(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "diagnostic",
        "run_id": run_id,
        "seq": seq,
        "severity": "info",
        "code": "content.skipped",
        "message": "Skipped 1 file above the configured size bound.",
        "detail": { "skipped_count": 1, "reason": "size_bound" }
    })
}

/// An error diagnostic that disqualifies completeness for the whole run.
#[must_use]
pub fn unavailable_diagnostic(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "diagnostic",
        "run_id": run_id,
        "seq": seq,
        "severity": "error",
        "code": "source.unavailable",
        "message": "Stopped walking 'contracts/' after repeated read errors; the subtree was not enumerated.",
        "object_kind": "file",
        "detail": { "enumerated_paths": 1, "unreadable_subtrees": 1 }
    })
}

/// The redacted authentication diagnostic transcript 06 shows.
///
/// The Field classified and sanitized first, replaced each removed value with
/// the exact marker, and named what it removed so a reviewer can see that
/// redaction happened rather than guessing.
#[must_use]
pub fn redacted_auth_diagnostic(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "diagnostic",
        "run_id": run_id,
        "seq": seq,
        "severity": "error",
        "code": "auth.reauth_required",
        "message": "Mailbox read failed: the tenant requires interactive re-authentication. Authorization header and signed continuation URL were removed before emission. Run 'fieldnotes fields auth outlook_mail_work'.",
        "detail": {
            "http_status": 401,
            "authorization": "[redacted]",
            "continuation_url": "[redacted]",
            "www_authenticate_realm": "[redacted]"
        },
        "redacted": ["authorization", "continuation_url", "www_authenticate_realm"]
    })
}

/// A cancellation acknowledgement diagnostic.
#[must_use]
pub fn cancelled_diagnostic(run_id: &str, seq: u64) -> Value {
    json!({
        "v": 1,
        "type": "diagnostic",
        "run_id": run_id,
        "seq": seq,
        "severity": "warning",
        "code": "run.cancelled",
        "message": "Cancelled after enumerating 1 of an unknown number of files; the scope was not completed."
    })
}

/// A diagnostic whose message deliberately carries `value`.
///
/// The negative control for the secret-canary scan: a scan that cannot fail
/// proves nothing, so one scenario leaks on purpose and the conformance case
/// asserts the scan catches it.
#[must_use]
pub fn leaking_diagnostic(run_id: &str, seq: u64, value: &str) -> Value {
    json!({
        "v": 1,
        "type": "diagnostic",
        "run_id": run_id,
        "seq": seq,
        "severity": "warning",
        "code": "internal.error",
        "message": format!("deliberate fixture leak for the canary negative control: {value}")
    })
}

fn set(frame: &mut Value, key: &str, value: Value) {
    if let Some(object) = frame.as_object_mut() {
        object.insert(key.to_owned(), value);
    }
}
