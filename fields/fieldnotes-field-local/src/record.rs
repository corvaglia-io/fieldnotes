//! Mapping one collected file onto a normalized source envelope.
//!
//! Every value this module supplies is post-mapping and pre-serialization,
//! exactly as A2 section 6 requires: it maps vendor structure (a file's
//! bytes, path, and modification time) onto Fieldnotes vocabulary, and does
//! none of the work only core may do. Nothing here computes a record ID, a
//! capture time, a content hash, a canonical key order, a filename, or an
//! artifact path -- the record and artifact types this module builds
//! structurally exclude all of them.

use std::fs;
use std::path::Path;

use fieldnotes_field_protocol::grammar::{
    AttachmentRef, MarkdownTag, MediaType, MediaTypeMatcher, NoteTypeToken, ObjectKind,
    OffsetDatetime, ProtocolV1, RecordTag, RunId, Sha256Hex, SourceIdentity, SourceScope,
};
use fieldnotes_field_protocol::limits::{Limits, artifact_media_type_included};
use fieldnotes_field_protocol::message::{
    ArtifactKind, ArtifactRef, ArtifactRole, Body, Change, Integrity, RecordEvent, SourceRef,
};
use fieldnotes_field_protocol::value::{PropertyValue, RecordProperties};
use sha2::{Digest, Sha256};

use crate::classify;
use crate::walk::WalkEntry;

/// Why one file could not be turned into a record.
///
/// Every reason here is treated as a per-file skip, not a run failure: the
/// caller reports it as a diagnostic and continues with the rest of the
/// walk, since one unreadable file must not cost the whole run.
#[derive(Debug)]
pub(crate) struct RecordError(pub(crate) String);

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordError {}

/// What one record needs beyond the file itself.
pub(crate) struct RecordContext<'a> {
    /// Core's identifier for this run.
    pub(crate) run_id: RunId,
    /// The portable exact-source scope every record in this run shares.
    pub(crate) source_scope: SourceScope,
    /// The per-run staging directory core created and named.
    pub(crate) staging_dir: &'a Path,
    /// The effective bounds for this run.
    pub(crate) limits: Limits,
    /// The effective media-type retention include set.
    pub(crate) media_policy: &'a [MediaTypeMatcher],
}

fn title_of(relative_path: &str) -> String {
    let path = Path::new(relative_path);
    path.file_stem()
        .or_else(|| path.file_name())
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(relative_path)
        .to_owned()
}

fn file_name_of(relative_path: &str) -> Option<String> {
    let name = Path::new(relative_path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)?;
    if name.is_empty() || name.len() > 255 {
        None
    } else {
        Some(name.to_owned())
    }
}

fn occurred_at_from(modified_unix_seconds: i64) -> Result<OffsetDatetime, RecordError> {
    let millis = modified_unix_seconds.saturating_mul(1000);
    let datetime = fieldnotes_domain::Datetime::from_unix_millis(millis, 0)
        .map_err(|error| RecordError(format!("modification time out of range: {error}")))?;
    OffsetDatetime::parse(&datetime.to_string())
        .map_err(|error| RecordError(format!("rendered instant failed its own guard: {error}")))
}

/// Truncates `text` to at most `max_bytes`, on a valid UTF-8 boundary,
/// returning the truncated text and the number of characters removed.
pub(crate) fn truncate(text: &str, max_bytes: u64) -> (String, u64) {
    let max = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    if text.len() <= max {
        return (text.to_owned(), 0);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let kept = &text[..end];
    let original_chars = text.chars().count();
    let kept_chars = kept.chars().count();
    (
        kept.to_owned(),
        u64::try_from(original_chars.saturating_sub(kept_chars)).unwrap_or(0),
    )
}

fn media_type_of(text: &str) -> Result<MediaType, RecordError> {
    MediaType::parse(text).map_err(|error| RecordError(format!("detected media type: {error}")))
}

fn sha256_of(text: &str) -> Result<Sha256Hex, RecordError> {
    Sha256Hex::parse(text).map_err(|error| RecordError(format!("digest guard: {error}")))
}

fn identity_of(object_kind: &str, relative_path: &str) -> Result<SourceIdentity, RecordError> {
    SourceIdentity::parse(&format!("{object_kind}/{relative_path}"))
        .map_err(|error| RecordError(format!("source identity guard: {error}")))
}

fn attachment_ref_of(object_kind: &str, relative_path: &str) -> Result<AttachmentRef, RecordError> {
    AttachmentRef::parse(&format!("{object_kind}/{relative_path}"))
        .map_err(|error| RecordError(format!("attachment reference guard: {error}")))
}

/// What building one artifact reference produced.
struct BuiltArtifact {
    reference: ArtifactRef,
    /// The opaque source-version token, present only when this Field
    /// actually read and hashed the bytes.
    version: Option<String>,
    /// The body text to use when the file's content was not otherwise
    /// readable as evidence.
    fallback_body: Option<String>,
    /// The detected media type, for the `local_media_type` property,
    /// regardless of whether the bytes were retained.
    media_type: Option<&'static str>,
    /// The exact file content, when it was read and is valid UTF-8 text.
    text_content: Option<String>,
}

fn build_artifact(
    context: &RecordContext<'_>,
    seq: u64,
    capability: &classify::Capability,
    entry: &WalkEntry,
) -> Result<BuiltArtifact, RecordError> {
    let title = title_of(&entry.relative_path);
    let source_filename = file_name_of(&entry.relative_path);

    // Re-checked immediately before reading, narrowing the window in which a
    // racing filesystem could swap a symlink in after `crate::walk` listed
    // this entry.
    let metadata = fs::symlink_metadata(&entry.absolute_path).map_err(|error| {
        RecordError(format!(
            "could not examine {}: {error}",
            entry.relative_path
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(RecordError(format!(
            "{} changed to a non-regular entry before it could be read",
            entry.relative_path
        )));
    }

    if metadata.len() > context.limits.max_artifact_bytes {
        let attachment_ref = attachment_ref_of(capability.object_kind, &entry.relative_path)?;
        let reference = ArtifactRef {
            kind: ArtifactKind::NotRetained,
            handle: None,
            sha256: None,
            byte_length: Some(metadata.len()),
            media_type: None,
            role: ArtifactRole::Original,
            source_filename,
            attachment_ref: Some(attachment_ref),
        };
        return Ok(BuiltArtifact {
            reference,
            version: None,
            fallback_body: Some(format!(
                "# {title}\n\nContent not retained: {} bytes exceeds the configured retention \
                 threshold.\n",
                metadata.len()
            )),
            media_type: None,
            text_content: None,
        });
    }

    let bytes = fs::read(&entry.absolute_path)
        .map_err(|error| RecordError(format!("could not read {}: {error}", entry.relative_path)))?;
    let byte_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = crate::hexutil::to_hex(&hasher.finalize());
    let media_type = classify::sniff_media_type(&bytes);
    let text_content = media_type
        .filter(|kind| *kind == "text/plain")
        .and_then(|_| String::from_utf8(bytes.clone()).ok());

    let policy_excluded =
        media_type.is_some_and(|kind| !artifact_media_type_included(context.media_policy, kind));

    if policy_excluded {
        let attachment_ref = attachment_ref_of(capability.object_kind, &entry.relative_path)?;
        let reference = ArtifactRef {
            kind: ArtifactKind::NotRetained,
            handle: None,
            sha256: None,
            byte_length: Some(byte_length),
            media_type: media_type.map(media_type_of).transpose()?,
            role: ArtifactRole::Original,
            source_filename,
            attachment_ref: Some(attachment_ref),
        };
        return Ok(BuiltArtifact {
            reference,
            version: Some(format!("sha256:{digest}")),
            fallback_body: Some(format!(
                "# {title}\n\nContent not retained: media type {} is excluded by the run's \
                 retention policy.\n",
                media_type.unwrap_or("unknown")
            )),
            media_type,
            text_content,
        });
    }

    let handle = format!("a{seq:07}");
    fs::write(context.staging_dir.join(&handle), &bytes).map_err(|error| {
        RecordError(format!("could not stage {}: {error}", entry.relative_path))
    })?;
    let reference = ArtifactRef {
        kind: ArtifactKind::Staged,
        handle: Some(handle),
        sha256: Some(sha256_of(&digest)?),
        byte_length: Some(byte_length),
        media_type: media_type.map(media_type_of).transpose()?,
        role: ArtifactRole::Original,
        source_filename,
        attachment_ref: None,
    };
    Ok(BuiltArtifact {
        reference,
        version: Some(format!("sha256:{digest}")),
        fallback_body: Some(format!(
            "# {title}\n\nBinary content collected as an attachment; not decoded to text.\n"
        )),
        media_type,
        text_content,
    })
}

/// Builds one record from a walked file.
///
/// Returns [`RecordError`] for a file this Field could no longer read by the
/// time it got here (for example, removed or replaced between the walk and
/// this call); the caller treats that as a per-file skip, not a run failure.
pub(crate) fn build(
    context: &RecordContext<'_>,
    seq: u64,
    entry: &WalkEntry,
) -> Result<RecordEvent, RecordError> {
    let capability = classify::classify(&entry.relative_path);
    let identity = identity_of(capability.object_kind, &entry.relative_path)?;
    let occurred_at = occurred_at_from(entry.modified_unix_seconds)?;
    let artifact = build_artifact(context, seq, &capability, entry)?;

    let mut properties = RecordProperties::new();
    let title = title_of(&entry.relative_path);
    insert(&mut properties, "title", PropertyValue::Text(title))?;
    insert(
        &mut properties,
        "local_relative_path",
        PropertyValue::Text(entry.relative_path.clone()),
    )?;
    if let Some(media_type) = artifact.media_type {
        insert(
            &mut properties,
            "local_media_type",
            PropertyValue::Text(media_type.to_owned()),
        )?;
    }

    let max_body_bytes = context.limits.max_body_bytes;
    let (body_text, lost_characters) = match artifact.text_content {
        Some(content) => truncate(&content, max_body_bytes),
        None => truncate(
            artifact
                .fallback_body
                .as_deref()
                .unwrap_or("Content unavailable.\n"),
            max_body_bytes,
        ),
    };
    let integrity = Integrity {
        damaged: false,
        truncated: lost_characters > 0,
        lost_characters: (lost_characters > 0).then_some(lost_characters),
    };

    Ok(RecordEvent {
        v: ProtocolV1,
        frame_type: RecordTag,
        run_id: context.run_id.clone(),
        seq,
        change: Change::Upsert,
        source: SourceRef {
            scope: context.source_scope.clone(),
            identity,
            version: artifact
                .version
                .as_deref()
                .map(parse_source_version)
                .transpose()?,
            url: None,
            parent_identity: None,
        },
        object_kind: Some(parse_object_kind(capability.object_kind)?),
        note_type: Some(parse_note_type(capability.note_type)?),
        occurred_at: Some(occurred_at),
        properties: Some(properties),
        body: Some(Body {
            format: MarkdownTag,
            text: body_text,
        }),
        artifacts: Some(vec![artifact.reference]),
        identity_anchors: None,
        integrity: Some(integrity),
        authority: None,
        observed_at: None,
    })
}

fn insert(
    properties: &mut RecordProperties,
    name: &str,
    value: PropertyValue,
) -> Result<(), RecordError> {
    properties
        .insert(name, value)
        .map_err(|reason| RecordError(format!("property {name}: {reason}")))
}

fn parse_object_kind(text: &str) -> Result<ObjectKind, RecordError> {
    ObjectKind::parse(text).map_err(|error| RecordError(format!("object kind guard: {error}")))
}

fn parse_note_type(text: &str) -> Result<NoteTypeToken, RecordError> {
    NoteTypeToken::parse(text).map_err(|error| RecordError(format!("Note type guard: {error}")))
}

fn parse_source_version(
    text: &str,
) -> Result<fieldnotes_field_protocol::grammar::SourceVersion, RecordError> {
    fieldnotes_field_protocol::grammar::SourceVersion::parse(text)
        .map_err(|error| RecordError(format!("source version guard: {error}")))
}

#[cfg(test)]
mod tests {
    use super::title_of;

    #[test]
    fn title_prefers_the_file_stem() {
        assert_eq!(title_of("projects/rollout/readme.md"), "readme");
        assert_eq!(title_of("no-extension"), "no-extension");
    }
}
