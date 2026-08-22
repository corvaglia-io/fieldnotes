//! The manifests the fixture Field declares.
//!
//! Every value here mirrors the checked-in transcripts, so a conformance case
//! and a reviewer are looking at the same bytes. Manifests are built as JSON and
//! then decoded through the protocol crate's own types before they are written,
//! which means the fixture cannot emit a manifest the DTOs would reject —
//! except in the scenarios where emitting exactly that is the point.
//!
//! These declared property names, capability slices, and scopes are
//! **illustrative**. Each becomes normative only when its Field's release gate
//! approves its manifest and fixtures.

use serde_json::{Value, json};

/// The `local` driver's manifest, as transcript 01 shows it.
///
/// Declares authoritative snapshots and no tombstones: a completed walk of the
/// configured root is authoritative for absence inside that root and nowhere
/// else.
#[must_use]
pub fn local(run_id: &str) -> Value {
    json!({
        "v": 1,
        "type": "manifest",
        "run_id": run_id,
        "protocol_version": 1,
        "protocol_revision": 0,
        "supported_protocol_versions": [1],
        "driver": "local-reference",
        "driver_version": "0.1.1",
        "field_stem": "local",
        "property_prefix": "local_",
        "declared_properties": local_declared_properties(),
        "capabilities": [
            {
                "object_kind": "file",
                "note_type": "file",
                "emits_artifacts": true,
                "emits_identity_anchors": false,
                "description": "Generic file collected without modification from the configured root."
            },
            {
                "object_kind": "document",
                "note_type": "document",
                "emits_artifacts": true,
                "emits_identity_anchors": false,
                "description": "Text-bearing source document whose document identity is primary."
            }
        ],
        "source_key": {
            "scope_rule": "local_root_id",
            "scope_rule_version": 1,
            "scope_shape": "local-root:<configured-root-id>",
            "scope_depends_on_field_label": false,
            "identity_shape": "<object-kind>/<root-relative-path>",
            "identity_includes_object_kind": true,
            "source_version_ordering": "unsupported",
            "stable_across_instances": true
        },
        "identity_anchors": [],
        "auth": {
            "kind": "none",
            "credential_profile_required": false,
            "protected_channel_required": false,
            "refresh_owner": "not_applicable",
            "writes_to_source": false
        },
        "collection": local_collection(1),
        "limitations": [
            {
                "code": "content.unsupported_format",
                "message": "Archive members are not expanded; an archive is collected as one original file."
            }
        ]
    })
}

fn local_declared_properties() -> Value {
    json!([
        {
            "name": "local_relative_path",
            "value_type": "text",
            "cardinality": "scalar",
            "description": "Path of the collected file relative to the configured root, retained as display evidence and never used as a path component by core."
        },
        {
            "name": "local_media_type",
            "value_type": "text",
            "cardinality": "scalar",
            "description": "Media type detected for the collected file."
        },
        {
            "name": "local_document_date",
            "value_type": "date",
            "cardinality": "scalar",
            "description": "Document date stated by a text-bearing source document."
        },
        {
            "name": "local_document_flag",
            "value_type": "text",
            "cardinality": "scalar",
            "description": "Source-declared document flag retained as opaque text, never coerced to a boolean."
        },
        {
            "name": "local_tags",
            "value_type": "text",
            "cardinality": "list",
            "list_semantics": "set",
            "description": "Deduplicated tags declared by the configured root manifest."
        }
    ])
}

fn local_collection(cursor_format_version: u16) -> Value {
    json!({
        "incremental": true,
        "cursor_format_version": cursor_format_version,
        "supported_modes": ["incremental", "snapshot"],
        "window_supported": false,
        "refetch": "supported",
        "refetch_note": "The configured root is re-readable, so a lost cursor is recovered by a full snapshot walk.",
        "deletion": {
            "tombstones": "unsupported",
            "snapshot": "authoritative",
            "note": "A completed walk of the configured root is authoritative for absence inside that root and nowhere else."
        }
    })
}

/// The `local` manifest with a different declared cursor format version.
///
/// Core compares this against the version the stored cursor was written at and
/// refuses to replay a token the Field may misread.
#[must_use]
pub fn local_with_cursor_format(run_id: &str, cursor_format_version: u16) -> Value {
    let mut manifest = local(run_id);
    if let Some(object) = manifest.as_object_mut() {
        object.insert(
            "collection".to_owned(),
            local_collection(cursor_format_version),
        );
    }
    manifest
}

/// The `local` manifest with one declared property retyped between runs.
///
/// Core treats a changed declared type as a migration that blocks sync, rather
/// than retyping notebook data in place.
#[must_use]
pub fn local_with_retyped_property(run_id: &str) -> Value {
    let mut manifest = local(run_id);
    if let Some(object) = manifest.as_object_mut() {
        object.insert(
            "declared_properties".to_owned(),
            json!([
                {
                    "name": "local_relative_path",
                    "value_type": "text",
                    "cardinality": "scalar",
                    "description": "Path of the collected file relative to the configured root."
                },
                {
                    "name": "local_media_type",
                    "value_type": "text",
                    "cardinality": "scalar",
                    "description": "Media type detected for the collected file."
                },
                {
                    "name": "local_document_date",
                    "value_type": "date",
                    "cardinality": "scalar",
                    "description": "Document date stated by a text-bearing source document."
                },
                {
                    "name": "local_document_flag",
                    "value_type": "boolean",
                    "cardinality": "scalar",
                    "description": "Retyped between releases, which core must treat as a migration."
                },
                {
                    "name": "local_tags",
                    "value_type": "text",
                    "cardinality": "list",
                    "list_semantics": "set",
                    "description": "Deduplicated tags declared by the configured root manifest."
                }
            ]),
        );
    }
    manifest
}

/// The `outlook_mail` driver's manifest, as transcript 04 shows it.
///
/// Declares authoritative tombstones and no snapshot authority: the change feed
/// reports removals explicitly, so absence alone never deletes.
#[must_use]
pub fn mail(run_id: &str) -> Value {
    json!({
        "v": 1,
        "type": "manifest",
        "run_id": run_id,
        "protocol_version": 1,
        "protocol_revision": 0,
        "supported_protocol_versions": [1],
        "driver": "outlook-mail",
        "driver_version": "0.1.3",
        "field_stem": "outlook_mail",
        "property_prefix": "outlook_mail_",
        "declared_properties": [
            {
                "name": "outlook_mail_importance",
                "value_type": "text",
                "cardinality": "scalar",
                "description": "Source-declared importance value, retained verbatim; Fieldnotes never assigns importance."
            },
            {
                "name": "outlook_mail_internet_message_id",
                "value_type": "text",
                "cardinality": "scalar",
                "description": "RFC 5322 Message-ID as supplied by the source."
            },
            {
                "name": "outlook_mail_folder_path",
                "value_type": "text",
                "cardinality": "scalar",
                "description": "Mailbox folder path of the message as display evidence only."
            },
            {
                "name": "outlook_mail_has_attachments",
                "value_type": "boolean",
                "cardinality": "scalar",
                "description": "Source-declared attachment flag, which may be true even when no attachment was collectable."
            },
            {
                "name": "outlook_mail_categories",
                "value_type": "text",
                "cardinality": "list",
                "list_semantics": "set",
                "description": "Source-assigned categories, deduplicated and sorted by core."
            }
        ],
        "capabilities": [
            {
                "object_kind": "mail-message",
                "note_type": "mail",
                "emits_artifacts": true,
                "emits_identity_anchors": true,
                "description": "Mail messages, participants, conversation identity, and collectable attachments."
            }
        ],
        "source_key": {
            "scope_rule": "graph_tenant_id",
            "scope_rule_version": 1,
            "scope_shape": "microsoft-graph:tenant/<tenant-guid>",
            "scope_depends_on_field_label": false,
            "identity_shape": "mail-message/<immutable-graph-item-id>",
            "identity_includes_object_kind": true,
            "source_version_ordering": "unsupported",
            "stable_across_instances": true
        },
        "identity_anchors": [
            {
                "namespace": "email",
                "scope_class": "normalized_channel",
                "normalization_rule": "email_v1",
                "normalization_version": 1,
                "substitutes_for_source_key": false,
                "description": "Normalized mail addresses observed as sender, recipient, or copy recipient."
            },
            {
                "namespace": "microsoft-graph-user-id",
                "scope_class": "authority_scoped",
                "substitutes_for_source_key": false,
                "description": "Directory user identifier, exact only within the declared tenant scope."
            }
        ],
        "auth": {
            "kind": "oauth_authorization_code",
            "credential_profile_required": true,
            "protected_channel_required": true,
            "scopes": ["Mail.Read", "User.Read"],
            "refresh_owner": "core",
            "writes_to_source": false
        },
        "collection": {
            "incremental": true,
            "cursor_format_version": 1,
            "supported_modes": ["incremental"],
            "window_supported": true,
            "refetch": "bounded",
            "refetch_note": "Refetch is bounded by the mailbox retention and by any folder the granted scope cannot read.",
            "deletion": {
                "tombstones": "authoritative",
                "snapshot": "unsupported",
                "note": "The change feed reports removals explicitly; no complete-mailbox snapshot is claimed, so absence alone never deletes."
            }
        },
        "limitations": [
            {
                "code": "source.history_unavailable",
                "message": "Messages purged from the mailbox before the first sync are not recoverable."
            }
        ]
    })
}

/// The `outlook_mail` manifest with tombstone authority withdrawn.
///
/// A delete record from this Field must be rejected: a connector cannot acquire
/// deletion power by emitting a frame.
#[must_use]
pub fn mail_without_tombstone_authority(run_id: &str) -> Value {
    let mut manifest = mail(run_id);
    if let Some(deletion) = manifest
        .get_mut("collection")
        .and_then(|collection| collection.get_mut("deletion"))
        .and_then(Value::as_object_mut)
    {
        deletion.insert("tombstones".to_owned(), json!("unsupported"));
        deletion.insert(
            "note".to_owned(),
            json!("This build claims no deletion authority at all."),
        );
    }
    manifest
}

/// A manifest answering with a protocol version core did not offer.
///
/// Invalid against the v1 manifest schema, because `v` and `protocol_version`
/// are fixed at 1 in v1. A v1 core rejects the frame instead of guessing which
/// members it still understands.
#[must_use]
pub fn future_version(run_id: &str) -> Value {
    json!({
        "v": 2,
        "type": "manifest",
        "run_id": run_id,
        "protocol_version": 2,
        "protocol_revision": 0,
        "supported_protocol_versions": [2],
        "driver": "outlook-mail",
        "driver_version": "0.2.0",
        "field_stem": "outlook_mail",
        "property_prefix": "outlook_mail_",
        "declared_properties": [],
        "capabilities": [
            {
                "object_kind": "mail-message",
                "note_type": "mail",
                "emits_artifacts": true,
                "emits_identity_anchors": true,
                "description": "Mail messages."
            }
        ],
        "source_key": {
            "scope_rule": "graph_tenant_id",
            "scope_rule_version": 1,
            "scope_shape": "microsoft-graph:tenant/<tenant-guid>",
            "scope_depends_on_field_label": false,
            "identity_shape": "mail-message/<immutable-graph-item-id>",
            "identity_includes_object_kind": true,
            "source_version_ordering": "unsupported",
            "stable_across_instances": true
        },
        "auth": {
            "kind": "oauth_authorization_code",
            "credential_profile_required": true,
            "protected_channel_required": true,
            "refresh_owner": "core",
            "writes_to_source": false
        },
        "collection": {
            "incremental": true,
            "cursor_format_version": 2,
            "supported_modes": ["incremental"],
            "window_supported": true,
            "refetch": "bounded",
            "deletion": {
                "tombstones": "authoritative",
                "snapshot": "unsupported"
            }
        }
    })
}
