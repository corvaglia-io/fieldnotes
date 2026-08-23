//! Turning one accepted `record` event into canonical Note bytes.
//!
//! A record is a **normalized source envelope**: post-mapping and
//! pre-serialization (A2 section 6). The Field has already mapped vendor
//! structure onto A1 vocabulary; everything this module adds is what only core
//! may decide — the Note ID, producer provenance, the capture time, the content
//! hash, the projected `artifacts`/`attachments`/`skipped_attachments`/
//! `identities` lists, the hoisted source key and integrity members, the
//! canonical key order and scalar spelling, and the filename.
//!
//! Nothing here writes anything. It produces a
//! [`fieldnotes_format::CanonicalRecord`], which cannot exist unless the format
//! crate has already emitted canonical bytes, re-parsed them, and validated them
//! with the public validator. Only then does a durable writer see them.
//!
//! # The body's attachment link (ADR 0007 ruling 2)
//!
//! A Field cannot render the attachment link itself: the local artifact path is
//! derived from **core's own** digest and A1's extension registry, which the
//! Field never sees. So core appends one deterministic artifact section to the
//! Field's body evidence. Within it, a retained artifact links to its derived
//! notebook-relative path and a `not_retained` one names its source location
//! instead, while `source_url` stays in frontmatter either way, so provenance
//! never depends on a retention outcome that can later change.

use fieldnotes_domain::ScalarKind;
use fieldnotes_domain::{Date, Datetime, FieldId, NoteType, RecordId, Scalar, Value};
use fieldnotes_field_protocol::codes::RejectionCode;
use fieldnotes_field_protocol::declared::{Cardinality, DeclaredPropertyIndex, PropertyShape};
use fieldnotes_field_protocol::message::{ArtifactRole, RecordEvent};
use fieldnotes_field_protocol::session::Rejection;
use fieldnotes_field_protocol::value::PropertyValue;
use fieldnotes_format::{
    CanonicalRecord, PropertyRegistry, RecordBuilder, content_hash_value, normalize_body_str,
};

/// One artifact whose bytes are durable in this notebook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetainedArtifact {
    /// The A1 content-addressed artifact ID, derived from core's own digest.
    pub(crate) artifact_id: String,
    /// The notebook-relative stored path, which the body links to.
    pub(crate) relative_path: String,
    /// The reference's role in its record.
    pub(crate) role: ArtifactRole,
    /// Display metadata only, never a path component.
    pub(crate) source_filename: Option<String>,
}

/// One artifact the Field saw at the source and declined to retain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkippedArtifact {
    /// The stable connector-namespaced upstream attachment reference: the only
    /// identity a declined artifact carries, since it has no bytes and no
    /// digest.
    pub(crate) attachment_ref: String,
    /// Display metadata only.
    pub(crate) source_filename: Option<String>,
}

/// Every artifact outcome for one record, in the order the record listed them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ArtifactProjection {
    /// Artifacts whose bytes are durable.
    pub(crate) retained: Vec<RetainedArtifact>,
    /// Artifacts the Field declined to retain.
    pub(crate) skipped: Vec<SkippedArtifact>,
}

/// Builds the canonical Note for one accepted upsert record.
pub(crate) fn build_note(
    note_id: &RecordId,
    instance_id: &RecordId,
    field_id: &FieldId,
    record: &RecordEvent,
    artifacts: &ArtifactProjection,
    captured_at: Datetime,
    declared: &DeclaredPropertyIndex<'_>,
) -> Result<CanonicalRecord, Rejection> {
    let Some(note_type_token) = &record.note_type else {
        return Err(Rejection::new(
            RejectionCode::RecordInvalidNoteType,
            "an upsert record states the primary Note type it maps to; core will not guess one",
        ));
    };
    let Some(note_type) = NoteType::parse(note_type_token.as_str()) else {
        return Err(Rejection::new(
            RejectionCode::RecordInvalidNoteType,
            format!("'{note_type_token}' is not one of A1's eleven approved primary Note types"),
        ));
    };
    let Some(occurred_at) = record.occurred_at else {
        return Err(Rejection::new(
            RejectionCode::ProtocolSchemaInvalid,
            "an upsert record states occurred_at; A1 requires it on every Note and core will not \
             substitute its own capture time for a source event time",
        ));
    };

    let body = compose_body(record, artifacts);
    let body = normalize_body_str(&body);
    if body.trim().is_empty() {
        return Err(Rejection::new(
            RejectionCode::ProtocolSchemaInvalid,
            "the record carries neither body evidence nor an artifact reference, so there is no \
             Note content to write",
        ));
    }

    let mut builder = RecordBuilder::note(
        note_id,
        instance_id,
        field_id,
        note_type,
        occurred_at.datetime(),
    );
    builder.set_datetime("captured_at", captured_at);
    builder.set_text("content_hash", content_hash_value(&body));
    builder.set_body(body);

    // The hoisted portable exact-source key. The record schema structurally
    // excludes these names from `properties`, so they can only come from here.
    builder.set_text("source_scope", record.source.scope.as_str().to_owned());
    builder.set_text(
        "source_identity",
        record.source.identity.as_str().to_owned(),
    );
    if let Some(version) = &record.source.version {
        builder.set_text("source_version", version.as_str().to_owned());
    }
    if let Some(url) = &record.source.url {
        builder.set_text("source_url", url.clone());
    }
    if let Some(parent) = &record.source.parent_identity {
        builder.set_text("source_parent_id", parent.as_str().to_owned());
    }

    if let Some(integrity) = &record.integrity {
        // A1 omits missing values, so a false flag is absent rather than
        // spelled out: `damaged: false` on every healthy Note would be noise.
        if integrity.damaged {
            builder.set_bool("damaged", true);
        }
        if integrity.truncated {
            builder.set_bool("truncated", true);
        }
        if let Some(lost) = integrity.lost_characters.filter(|lost| *lost > 0) {
            builder.set_number("lost_characters", lost_characters_as_number(lost)?);
        }
    }

    builder.set_text_list(
        "artifacts",
        artifacts
            .retained
            .iter()
            .map(|artifact| artifact.artifact_id.clone()),
    );
    builder.set_text_list(
        "attachments",
        artifacts
            .retained
            .iter()
            .filter(|artifact| artifact.role == ArtifactRole::Attachment)
            .map(|artifact| artifact.artifact_id.clone()),
    );
    builder.set_text_list(
        "skipped_attachments",
        artifacts
            .skipped
            .iter()
            .map(|artifact| artifact.attachment_ref.clone()),
    );
    if let Some(anchors) = &record.identity_anchors {
        builder.set_text_list(
            "identities",
            anchors
                .iter()
                .map(|anchor| format!("{}:{}", anchor.namespace, anchor.value)),
        );
    }

    if let Some(properties) = &record.properties {
        for (name, value) in properties.iter() {
            let shape = shape_of(declared, name)?;
            builder.set(name, convert(name, shape, value)?);
        }
    }

    // Emitted, re-parsed, and validated here; only a value that survives all
    // three reaches a durable writer.
    builder.build().map_err(|error| {
        Rejection::new(
            RejectionCode::ProtocolSchemaInvalid,
            format!("the projected Note did not satisfy the A1 validator: {error}"),
        )
    })
}

/// A byte/character count as an exactly representable binary64 number.
fn lost_characters_as_number(lost: u64) -> Result<f64, Rejection> {
    // A1 rejects an integer outside the exactly representable binary64 range
    // rather than rounding it, so the guard belongs here rather than in the
    // emitter, which would only see an already-rounded value.
    if i128::from(lost) > fieldnotes_domain::value::MAX_EXACT_INTEGER {
        return Err(Rejection::new(
            RejectionCode::ProtocolLimitExceeded,
            format!("lost_characters is {lost}, outside the exactly representable range"),
        ));
    }
    Ok(lost as f64)
}

/// The approved shape of one property candidate: the declaring manifest's for a
/// prefixed name, A1's closed shared registry for an unprefixed one.
fn shape_of(declared: &DeclaredPropertyIndex<'_>, name: &str) -> Result<PropertyShape, Rejection> {
    if let Some(shape) = declared.declared_shape(name) {
        return Ok(shape);
    }
    match PropertyRegistry::v1().lookup(name) {
        Some(entry) => Ok(PropertyShape::from_registry(entry)),
        None => Err(Rejection::new(
            RejectionCode::RecordUnknownProperty,
            format!(
                "'{name}' is neither a declared prefixed property of this Field nor a name in \
                 A1's closed shared registry, so core has no approved type for it"
            ),
        )),
    }
}

/// Converts one wire property candidate into a typed A1 frontmatter value.
///
/// A number keeps the Field's wire spelling until this point; A1 alone owns the
/// canonical spelling, which the emitter applies to the binary64 value.
fn convert(name: &str, shape: PropertyShape, value: &PropertyValue) -> Result<Value, Rejection> {
    let mismatch = || {
        Rejection::new(
            RejectionCode::RecordPropertyTypeMismatch,
            format!(
                "property '{name}' is approved as {} but arrived as a {}",
                shape.value_type,
                value.shape()
            ),
        )
    };
    let scalars: Vec<Scalar> = match (shape.value_type.kind(), value) {
        (ScalarKind::Text, PropertyValue::Text(text)) => vec![Scalar::Text(text.clone())],
        (ScalarKind::Text, PropertyValue::TextList(members)) => {
            members.iter().cloned().map(Scalar::Text).collect()
        }
        (ScalarKind::Bool, PropertyValue::Boolean(flag)) => vec![Scalar::Bool(*flag)],
        (ScalarKind::Bool, PropertyValue::BooleanList(members)) => {
            members.iter().copied().map(Scalar::Bool).collect()
        }
        (ScalarKind::Number, PropertyValue::Number(number)) => {
            vec![Scalar::Number(number_of(name, number)?)]
        }
        (ScalarKind::Number, PropertyValue::NumberList(members)) => members
            .iter()
            .map(|number| number_of(name, number).map(Scalar::Number))
            .collect::<Result<Vec<Scalar>, Rejection>>()?,
        (ScalarKind::Date, PropertyValue::Text(text)) => vec![Scalar::Date(date_of(name, text)?)],
        (ScalarKind::Date, PropertyValue::TextList(members)) => members
            .iter()
            .map(|text| date_of(name, text).map(Scalar::Date))
            .collect::<Result<Vec<Scalar>, Rejection>>()?,
        (ScalarKind::Datetime, PropertyValue::Text(text)) => {
            vec![Scalar::Datetime(datetime_of(name, text)?)]
        }
        (ScalarKind::Datetime, PropertyValue::TextList(members)) => members
            .iter()
            .map(|text| datetime_of(name, text).map(Scalar::Datetime))
            .collect::<Result<Vec<Scalar>, Rejection>>()?,
        _ => return Err(mismatch()),
    };
    match shape.cardinality {
        Cardinality::List => Ok(Value::List(scalars)),
        Cardinality::Scalar => match scalars.into_iter().next() {
            Some(scalar) => Ok(Value::Scalar(scalar)),
            None => Err(mismatch()),
        },
    }
}

fn number_of(name: &str, number: &serde_json::Number) -> Result<f64, Rejection> {
    number
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            Rejection::new(
                RejectionCode::RecordPropertyTypeMismatch,
                format!(
                    "property '{name}' carries {number}, which is not a finite binary64 number"
                ),
            )
        })
}

fn date_of(name: &str, text: &str) -> Result<Date, Rejection> {
    Date::parse(text).map_err(|error| {
        Rejection::new(
            RejectionCode::RecordInvalidDate,
            format!("property '{name}' is approved as a date: {error}"),
        )
    })
}

fn datetime_of(name: &str, text: &str) -> Result<Datetime, Rejection> {
    Datetime::parse(text).map_err(|error| {
        Rejection::new(
            RejectionCode::RecordInvalidDatetime,
            format!("property '{name}' is approved as a datetime: {error}"),
        )
    })
}

/// Composes the Note body: the Field's deterministic source evidence, then
/// core's own artifact section.
fn compose_body(record: &RecordEvent, artifacts: &ArtifactProjection) -> String {
    let mut sections: Vec<String> = Vec::new();
    if let Some(body) = &record.body {
        let text = body.text.trim_end();
        if !text.is_empty() {
            sections.push(text.to_owned());
        }
    }
    if let Some(section) = artifact_section(record, artifacts) {
        sections.push(section);
    }
    let mut body = sections.join("\n\n");
    body.push('\n');
    body
}

/// The deterministic artifact section, or `None` when the record referenced no
/// artifact at all.
fn artifact_section(record: &RecordEvent, artifacts: &ArtifactProjection) -> Option<String> {
    if artifacts.retained.is_empty() && artifacts.skipped.is_empty() {
        return None;
    }
    let mut section = String::from("Artifacts:\n");
    for artifact in &artifacts.retained {
        // The body lives in `notes/`, so the link is relative to it.
        section.push_str(&format!(
            "\n- {}: retained at `../{}`",
            label(
                artifact.source_filename.as_deref(),
                role_label(artifact.role)
            ),
            artifact.relative_path
        ));
    }
    for artifact in &artifacts.skipped {
        section.push_str(&format!(
            "\n- {}: not retained; stays at its source {}",
            label(artifact.source_filename.as_deref(), "attachment"),
            source_location(record)
        ));
    }
    section.push('\n');
    Some(section)
}

/// The display label for one artifact: its source filename when the Field
/// supplied one, otherwise its role. Display evidence only, never a path
/// component and never the stored extension.
fn label(source_filename: Option<&str>, fallback: &str) -> String {
    match source_filename
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => format!("`{name}`"),
        None => fallback.to_owned(),
    }
}

fn role_label(role: ArtifactRole) -> &'static str {
    match role {
        ArtifactRole::Attachment => "attachment",
        ArtifactRole::Original => "original",
        ArtifactRole::Embedded => "embedded content",
        ArtifactRole::Other => "artifact",
    }
}

/// Where the original material lives, for a declined artifact.
///
/// The Field's `source.url` when it supplied one; otherwise the portable
/// exact-source key, which is the only source location core is ever given.
fn source_location(record: &RecordEvent) -> String {
    match &record.source.url {
        Some(url) => format!("<{url}>"),
        None => format!("(`{}` `{}`)", record.source.scope, record.source.identity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(json: serde_json::Value) -> RecordEvent {
        match fieldnotes_field_protocol::message::FieldEvent::decode(json) {
            Ok(fieldnotes_field_protocol::message::FieldEvent::Record(record)) => *record,
            other => panic!("the fixture must decode as a record: {other:?}"),
        }
    }

    fn base(body: &str) -> serde_json::Value {
        serde_json::json!({
            "v": 1, "type": "record", "run_id": "1a4c9f2e-0000-4000-8000-000000000002",
            "seq": 1, "change": "upsert",
            "source": {
                "scope": "local-root:one", "identity": "file/a.md",
                "url": "https://example.test/a.md"
            },
            "object_kind": "file",
            "note_type": "file",
            "occurred_at": "2026-08-22T09:45:00+02:00",
            "body": {"format": "markdown", "text": body}
        })
    }

    #[test]
    fn a_retained_artifact_links_to_its_derived_notebook_path() {
        let event = record(base("# A\n\nEvidence.\n"));
        let projection = ArtifactProjection {
            retained: vec![RetainedArtifact {
                artifact_id: "artifact_sha256_00".to_owned(),
                relative_path: "artifacts/artifact_sha256_00.md".to_owned(),
                role: ArtifactRole::Original,
                source_filename: Some("a.md".to_owned()),
            }],
            skipped: Vec::new(),
        };
        let body = compose_body(&event, &projection);
        assert!(body.contains("`a.md`: retained at `../artifacts/artifact_sha256_00.md`"));
        assert!(!body.contains("not retained"));
    }

    #[test]
    fn a_declined_artifact_links_to_the_source_and_never_to_a_local_path() {
        let event = record(base("# A\n\nEvidence.\n"));
        let projection = ArtifactProjection {
            retained: Vec::new(),
            skipped: vec![SkippedArtifact {
                attachment_ref: "file-attachment/export-01".to_owned(),
                source_filename: Some("full-export.zip".to_owned()),
            }],
        };
        let body = compose_body(&event, &projection);
        assert!(body.contains(
            "`full-export.zip`: not retained; stays at its source \
             <https://example.test/a.md>"
        ));
        assert!(!body.contains("../artifacts/"));
    }

    #[test]
    fn with_no_source_url_a_declined_artifact_names_the_portable_source_key() {
        let mut value = base("Evidence.\n");
        if let Some(source) = value
            .get_mut("source")
            .and_then(serde_json::Value::as_object_mut)
        {
            source.remove("url");
        }
        let event = record(value);
        let projection = ArtifactProjection {
            retained: Vec::new(),
            skipped: vec![SkippedArtifact {
                attachment_ref: "file-attachment/export-01".to_owned(),
                source_filename: None,
            }],
        };
        let body = compose_body(&event, &projection);
        assert!(body.contains("(`local-root:one` `file/a.md`)"));
    }

    #[test]
    fn a_record_with_blank_body_evidence_and_no_artifact_has_no_note_content() {
        // The transport schema requires an upsert to carry a body at all, but
        // not that its text says anything, so this is the reachable shape core
        // must refuse rather than write an empty Note for.
        let event = record(base("   \n\n"));
        assert_eq!(compose_body(&event, &ArtifactProjection::default()), "\n");
    }
}
