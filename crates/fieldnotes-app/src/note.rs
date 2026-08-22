//! `note`: create a `self`-Field Note, optionally importing a file.
//!
//! A file or voice import stores the original bytes as a content-addressed
//! artifact and references it by artifact ID. `attachments` stays empty because
//! a user import is the Note's own original, not a role-specific attachment
//! received with a message.

use std::path::{Path, PathBuf};

use fieldnotes_domain::{Clock, Datetime, NoteType, RandomSource, RecordId, RecordKind};
use fieldnotes_format::{RecordBuilder, content_hash_value, detect_media_type, normalize_body_str};
use fieldnotes_store::{
    NoteWrite, Notebook, StoreError, StoredArtifact, read_instance, store_artifact, write_note,
};

use crate::error::AppError;
use crate::kernel::{Kernel, self_field};

/// What the Note is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteSource {
    /// A text Note.
    Text,
    /// A copied file import, stored as an original artifact.
    File(PathBuf),
    /// A voice-recording import, stored as an original artifact.
    Voice(PathBuf),
}

/// A request to create one Note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRequest {
    /// What the Note is made of.
    pub source: NoteSource,
    /// The Note text, which becomes the Markdown body.
    pub text: Option<String>,
    /// An optional title, emitted as the `title` property and as the body's
    /// first heading.
    pub title: Option<String>,
    /// The event time. Defaults to the Note's capture time.
    pub occurred_at: Option<Datetime>,
}

impl NoteRequest {
    /// A text Note carrying `text`.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        NoteRequest {
            source: NoteSource::Text,
            text: Some(text.into()),
            title: None,
            occurred_at: None,
        }
    }
}

/// What `note` wrote.
#[derive(Debug, Clone)]
pub struct NoteOutcome {
    /// The new Note's ID.
    pub note_id: RecordId,
    /// The primary Note type.
    pub note_type: NoteType,
    /// The event time in the frontmatter.
    pub occurred_at: Datetime,
    /// The capture time in the frontmatter.
    pub captured_at: Datetime,
    /// The installed file.
    pub write: NoteWrite,
    /// The notebook-relative Note path.
    pub relative_path: String,
    /// The imported original, when the Note imported one.
    pub artifact: Option<StoredArtifact>,
    /// The Note's `fn-content-v1` body hash.
    pub content_hash: String,
}

/// Creates one `self` Note and installs it durably.
///
/// The order is deliberate: the original artifact becomes durable first, then
/// the Note that references it. A crash between the two leaves an unreferenced
/// artifact, which is harmless, rather than a Note pointing at bytes that do
/// not exist.
pub fn create_note<C: Clock, R: RandomSource>(
    kernel: &mut Kernel<C, R>,
    notebook: &Notebook,
    request: &NoteRequest,
) -> Result<NoteOutcome, AppError> {
    let instance = read_instance(notebook)?;
    let (note_id, captured_at) = kernel.new_record(RecordKind::Note)?;
    let occurred_at = request.occurred_at.unwrap_or(captured_at);
    let field = self_field()?;

    let import = match &request.source {
        NoteSource::Text => None,
        NoteSource::File(path) => Some(store_import(notebook, path, false)?),
        NoteSource::Voice(path) => Some(store_import(notebook, path, true)?),
    };
    let note_type = match &request.source {
        NoteSource::Text => NoteType::Text,
        NoteSource::File(_) => NoteType::File,
        NoteSource::Voice(_) => NoteType::Voice,
    };

    let title = request.title.clone().or_else(|| {
        import
            .as_ref()
            .and_then(|import| import.source_name.clone())
    });
    let body = compose_body(
        title.as_deref(),
        request.text.as_deref(),
        import.as_ref().map(|import| {
            (
                if matches!(request.source, NoteSource::Voice(_)) {
                    "Original audio path"
                } else {
                    "Original artifact path"
                },
                import.artifact.relative_path.as_str(),
            )
        }),
    );
    let body = normalize_body_str(&body);
    if body.trim().is_empty() {
        return Err(AppError::EmptyNote);
    }
    let content_hash = content_hash_value(&body);

    let mut builder = RecordBuilder::note(
        &note_id,
        &instance.instance_id,
        &field,
        note_type,
        occurred_at,
    );
    builder.set_datetime("captured_at", captured_at);
    builder.set_text("content_hash", content_hash.clone());
    builder.set_body(body);
    if let Some(title) = &title {
        builder.set_text("title", title.clone());
    }
    if let Some(import) = &import {
        builder.set_text_list("artifacts", [import.artifact.id.to_string()]);
        if let Some(audio) = &import.audio_media_type {
            builder.set_text("audio_media_type", audio.clone());
        }
    }
    // The record is emitted, re-parsed, and validated here; only then is it
    // handed to a durable writer.
    let record = builder.build()?;
    let write = write_note(notebook, &record)?;

    Ok(NoteOutcome {
        note_id,
        note_type,
        occurred_at,
        captured_at,
        relative_path: notebook.relative_display(&write.path),
        write,
        artifact: import.map(|import| import.artifact),
        content_hash,
    })
}

/// An imported original and the metadata the Note derives from it.
struct Import {
    artifact: StoredArtifact,
    source_name: Option<String>,
    audio_media_type: Option<String>,
}

/// Reads a file and stores its exact bytes as a content-addressed original.
fn store_import(notebook: &Notebook, path: &Path, require_audio: bool) -> Result<Import, AppError> {
    let bytes = std::fs::read(path).map_err(|error| StoreError::io("read import", path, error))?;
    // Detection reads content only: A1 forbids a source filename from choosing
    // the stored extension.
    let media_type = detect_media_type(&bytes);
    if require_audio && !media_type.is_some_and(|value| value.starts_with("audio/")) {
        return Err(AppError::NotAudio {
            path: path.to_path_buf(),
            detected: media_type.map(str::to_owned),
        });
    }
    let artifact = store_artifact(notebook, &bytes, media_type)?;
    Ok(Import {
        artifact,
        source_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned),
        audio_media_type: media_type
            .filter(|value| value.starts_with("audio/"))
            .map(str::to_owned),
    })
}

/// Builds the Markdown body from its optional parts.
///
/// Sections are separated by one blank line, in a fixed order, so the same
/// inputs always produce the same bytes.
fn compose_body(title: Option<&str>, text: Option<&str>, artifact: Option<(&str, &str)>) -> String {
    let mut sections: Vec<String> = Vec::new();
    if let Some(title) = title.map(str::trim).filter(|value| !value.is_empty()) {
        sections.push(format!("# {title}"));
    }
    if let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) {
        sections.push(text.to_owned());
    }
    if let Some((label, relative_path)) = artifact {
        // The body links relative to `notes/`, where the file lives.
        sections.push(format!("{label}:\n`../{relative_path}`"));
    }
    let mut body = sections.join("\n\n");
    body.push('\n');
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_sections_are_ordered_and_blank_separated() {
        assert_eq!(
            compose_body(
                Some("Whiteboard"),
                Some("Imported photo.\n"),
                Some(("Original artifact path", "artifacts/artifact_sha256_ab.png"))
            ),
            "# Whiteboard\n\nImported photo.\n\nOriginal artifact path:\n`../artifacts/artifact_sha256_ab.png`\n"
        );
        assert_eq!(compose_body(None, Some("Just text"), None), "Just text\n");
        assert_eq!(compose_body(None, None, None), "\n");
    }
}
