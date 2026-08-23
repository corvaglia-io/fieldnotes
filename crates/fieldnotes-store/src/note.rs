//! Durable Note installation, including the contract's filename rename.
//!
//! A Note's filename is computed from its own validated frontmatter, never from
//! caller-supplied text. When a Note's event time changes, the new filename is
//! installed first and the old one is removed only afterwards, so a crash in
//! the middle leaves the Note present under one name or the other and never
//! absent.

use std::path::PathBuf;

use fieldnotes_format::CanonicalRecord;

use crate::atomic::{self, StagedFile};
use crate::error::StoreError;
use crate::layout::Notebook;

/// The outcome of installing a Note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteWrite {
    /// The installed file path.
    pub path: PathBuf,
    /// The canonical filename computed from the record's frontmatter.
    pub filename: String,
    /// Whether an existing file with that name was replaced.
    pub replaced: bool,
    /// The previous filename removed after a successful event-time change.
    pub removed_previous: Option<String>,
}

/// Installs a validated Note under its canonical filename.
///
/// Writing the same record twice converges: the filename is derived from the
/// record, the bytes are byte-identical, and the rename replaces rather than
/// duplicates.
pub fn write_note(notebook: &Notebook, record: &CanonicalRecord) -> Result<NoteWrite, StoreError> {
    install(notebook, record, None)
}

/// Installs a validated Note and removes a superseded filename.
///
/// `previous_filename` is the name the Note currently occupies. When the
/// canonical name has changed — the only cause in v0.1 is a corrected
/// `occurred_at` — the new name is installed first and the old file is removed
/// afterwards, preserving the Note's logical ID throughout.
pub fn replace_note(
    notebook: &Notebook,
    record: &CanonicalRecord,
    previous_filename: &str,
) -> Result<NoteWrite, StoreError> {
    install(notebook, record, Some(previous_filename))
}

/// Removes one Note file, for an authoritative source deletion.
///
/// Returns whether a file existed to remove. No tombstone Note and no revision
/// entry is written: A1 section 7 keeps no deletion ledger, so a later refetch
/// simply recreates the Note under a new Note ID. Shared artifacts are left
/// alone; reclaiming them needs the separate verified reference-analysis pass.
pub fn remove_note(notebook: &Notebook, filename: &str) -> Result<bool, StoreError> {
    let directory = notebook.notes_dir();
    let path = directory.join(filename);
    match std::fs::remove_file(&path) {
        Ok(()) => {
            atomic::sync_directory(&directory)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(StoreError::io("remove Note", &path, error)),
    }
}

fn install(
    notebook: &Notebook,
    record: &CanonicalRecord,
    previous_filename: Option<&str>,
) -> Result<NoteWrite, StoreError> {
    let directory = notebook.notes_dir();
    let filename = record
        .note_filename()
        .map_err(|source| StoreError::Invalid {
            path: directory.clone(),
            source,
        })?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| StoreError::io("create directory", &directory, error))?;
    let replaced = directory.join(&filename).is_file();
    let staged = StagedFile::create(&directory, record.bytes())?;
    let path = staged.install(&filename)?;

    // Only now that the replacement is durable may the superseded name go.
    let mut removed_previous = None;
    if let Some(previous) = previous_filename
        && previous != filename
    {
        let stale = directory.join(previous);
        if stale.is_file() {
            std::fs::remove_file(&stale)
                .map_err(|error| StoreError::io("remove superseded Note", &stale, error))?;
            atomic::sync_directory(&directory)?;
            removed_previous = Some(previous.to_owned());
        }
    }

    Ok(NoteWrite {
        path,
        filename,
        replaced,
        removed_previous,
    })
}
