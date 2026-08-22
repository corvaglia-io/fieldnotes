//! Reading a notebook back: parse, validate, and report problems per file.
//!
//! The scan never hides a bad file. An unparseable or invalid Note becomes a
//! reported problem attached to its path, so `status` can count it and
//! `inspect` can explain it, instead of the file silently disappearing from
//! every view.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fieldnotes_domain::{Scalar, Value};
use fieldnotes_format::{
    ParsedRecord, ValidationError, content_hash_value, expected_note_filename, parse_record,
    validate_record,
};

use crate::artifact::verify_artifact;
use crate::atomic;
use crate::error::StoreError;
use crate::layout::Notebook;

/// One thing wrong with a file in the notebook.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// The file does not parse or does not validate.
    Invalid(ValidationError),
    /// The filename disagrees with the name computed from the frontmatter.
    FilenameMismatch {
        /// The canonical name computed from frontmatter.
        expected: String,
    },
    /// The declared `content_hash` does not match the body.
    ContentHashMismatch {
        /// The value in the file.
        declared: String,
        /// The value recomputed from the normalized body.
        computed: String,
    },
    /// A referenced artifact is not stored in the notebook.
    MissingArtifact {
        /// The referenced artifact ID.
        id: String,
    },
    /// Two files claim the same Note ID.
    DuplicateNoteId {
        /// The duplicated Note ID.
        id: String,
    },
    /// A stored artifact's bytes no longer match its content address.
    ArtifactCorrupt,
}

impl Problem {
    /// A stable lowercase label for machine-readable output.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Problem::Invalid(_) => "invalid",
            Problem::FilenameMismatch { .. } => "filename_mismatch",
            Problem::ContentHashMismatch { .. } => "content_hash_mismatch",
            Problem::MissingArtifact { .. } => "missing_artifact",
            Problem::DuplicateNoteId { .. } => "duplicate_note_id",
            Problem::ArtifactCorrupt => "artifact_corrupt",
        }
    }

    /// A human-readable explanation.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Problem::Invalid(error) => error.to_string(),
            Problem::FilenameMismatch { expected } => {
                format!("filename should be `{expected}`")
            }
            Problem::ContentHashMismatch { declared, computed } => {
                format!("content_hash is `{declared}` but the body hashes to `{computed}`")
            }
            Problem::MissingArtifact { id } => {
                format!("referenced artifact `{id}` is not stored in this notebook")
            }
            Problem::DuplicateNoteId { id } => {
                format!("another file already claims Note ID `{id}`")
            }
            Problem::ArtifactCorrupt => {
                "stored bytes no longer match the artifact's content address".to_owned()
            }
        }
    }
}

/// One scanned Note file.
#[derive(Debug, Clone)]
pub struct ScannedNote {
    /// The file path.
    pub path: PathBuf,
    /// The bare filename.
    pub filename: String,
    /// The parsed record, when the file parsed at all.
    pub record: Option<ParsedRecord>,
    /// Everything wrong with this file; empty means healthy.
    pub problems: Vec<Problem>,
}

impl ScannedNote {
    /// Whether the file is fully valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.problems.is_empty()
    }

    /// The Note's `type` value, when parsed.
    #[must_use]
    pub fn note_type(&self) -> Option<&str> {
        match self.record.as_ref()?.get("type") {
            Some(Value::Scalar(Scalar::Text(text))) => Some(text.as_str()),
            _ => None,
        }
    }
}

/// One scanned artifact file.
#[derive(Debug, Clone)]
pub struct ScannedArtifact {
    /// The file path.
    pub path: PathBuf,
    /// The bare filename.
    pub filename: String,
    /// The artifact ID taken from the filename stem, when it is well formed.
    pub id: Option<String>,
    /// The stored byte length.
    pub bytes: u64,
    /// Whether any retained Note references this artifact.
    pub referenced: bool,
    /// Everything wrong with this file; empty means healthy.
    pub problems: Vec<Problem>,
}

/// A complete notebook scan.
#[derive(Debug, Clone, Default)]
pub struct NotebookScan {
    /// Every `.md` file in `notes/`, in filename order.
    pub notes: Vec<ScannedNote>,
    /// Every file in `artifacts/`, in filename order.
    pub artifacts: Vec<ScannedArtifact>,
    /// Leftover staging files, which indicate an interrupted write.
    pub stray_staged_files: Vec<PathBuf>,
}

impl NotebookScan {
    /// Note counts by `type`, in ascending type order.
    #[must_use]
    pub fn notes_by_type(&self) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        for note in &self.notes {
            *counts
                .entry(note.note_type().unwrap_or("unknown"))
                .or_insert(0) += 1;
        }
        counts
    }

    /// How many Note files are fully valid.
    #[must_use]
    pub fn valid_notes(&self) -> usize {
        self.notes.iter().filter(|note| note.is_valid()).count()
    }

    /// How many files of any kind carry at least one problem.
    #[must_use]
    pub fn problem_files(&self) -> usize {
        self.notes.iter().filter(|n| !n.problems.is_empty()).count()
            + self
                .artifacts
                .iter()
                .filter(|a| !a.problems.is_empty())
                .count()
    }

    /// Total stored artifact bytes.
    #[must_use]
    pub fn artifact_bytes(&self) -> u64 {
        self.artifacts.iter().map(|artifact| artifact.bytes).sum()
    }
}

/// How much work a scan should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanOptions {
    /// Re-hash every stored artifact to prove it still matches its ID.
    ///
    /// Off by default: `status` only counts files, while `inspect` pays for
    /// the proof.
    pub verify_artifact_bytes: bool,
}

/// Lists the files of one notebook directory in filename order.
fn list_files(directory: &Path) -> Result<Vec<(String, PathBuf)>, StoreError> {
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(StoreError::io("read directory", directory, error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io("read directory", directory, error))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if atomic::is_staged_name(name) {
            continue;
        }
        files.push((name.to_owned(), path));
    }
    files.sort();
    Ok(files)
}

/// Every artifact ID referenced by a Note's `artifacts` or `attachments` list.
fn referenced_artifacts(notes: &[ScannedNote]) -> BTreeSet<String> {
    let mut referenced = BTreeSet::new();
    for note in notes {
        let Some(record) = note.record.as_ref() else {
            continue;
        };
        for key in ["artifacts", "attachments"] {
            if let Some(Value::List(items)) = record.get(key) {
                for item in items {
                    if let Scalar::Text(text) = item {
                        referenced.insert(text.clone());
                    }
                }
            }
        }
    }
    referenced
}

/// Scans a notebook's public files.
pub fn scan(notebook: &Notebook, options: ScanOptions) -> Result<NotebookScan, StoreError> {
    let notes_dir = notebook.notes_dir();
    let artifacts_dir = notebook.artifacts_dir();

    // Stored artifact IDs, so a Note's references can be checked.
    let artifact_files = list_files(&artifacts_dir)?;
    let stored_ids: BTreeSet<String> = artifact_files
        .iter()
        .filter_map(|(_, path)| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
        })
        .collect();

    let mut notes = Vec::new();
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();
    for (filename, path) in list_files(&notes_dir)? {
        if !filename.ends_with(".md") {
            continue;
        }
        let mut problems = Vec::new();
        let bytes =
            std::fs::read(&path).map_err(|error| StoreError::io("read Note", &path, error))?;
        let record = match parse_record(&bytes) {
            Ok(record) => match validate_record(&record) {
                Ok(()) => Some(record),
                Err(error) => {
                    problems.push(Problem::Invalid(error));
                    Some(record)
                }
            },
            Err(error) => {
                problems.push(Problem::Invalid(error));
                None
            }
        };
        if let Some(record) = record.as_ref()
            && problems.is_empty()
        {
            match expected_note_filename(record) {
                Ok(expected) if expected != filename => {
                    problems.push(Problem::FilenameMismatch { expected });
                }
                Ok(_) => {}
                Err(error) => problems.push(Problem::Invalid(error)),
            }
            if let Some(Value::Scalar(Scalar::Text(declared))) = record.get("content_hash") {
                let computed = content_hash_value(record.body());
                if *declared != computed {
                    problems.push(Problem::ContentHashMismatch {
                        declared: declared.clone(),
                        computed,
                    });
                }
            }
            for key in ["artifacts", "attachments"] {
                if let Some(Value::List(items)) = record.get(key) {
                    for item in items {
                        if let Scalar::Text(id) = item
                            && !stored_ids.contains(id.as_str())
                        {
                            problems.push(Problem::MissingArtifact { id: id.clone() });
                        }
                    }
                }
            }
            let id = record.id().to_string();
            if !seen_ids.insert(id.clone()) {
                problems.push(Problem::DuplicateNoteId { id });
            }
        }
        notes.push(ScannedNote {
            path,
            filename,
            record,
            problems,
        });
    }

    let referenced = referenced_artifacts(&notes);
    let mut artifacts = Vec::new();
    for (filename, path) in artifact_files {
        let mut problems = Vec::new();
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_owned);
        let mut bytes = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .map_err(|error| StoreError::io("read artifact metadata", &path, error))?;
        if options.verify_artifact_bytes {
            match verify_artifact(&path) {
                Ok(length) => bytes = length,
                Err(StoreError::ArtifactCorrupt { .. }) => problems.push(Problem::ArtifactCorrupt),
                Err(error) => return Err(error),
            }
        }
        let is_referenced = stem
            .as_deref()
            .is_some_and(|stem| referenced.contains(stem));
        artifacts.push(ScannedArtifact {
            path,
            filename,
            id: stem,
            bytes,
            referenced: is_referenced,
            problems,
        });
    }

    let mut stray_staged_files = atomic::staged_files(&notes_dir)?;
    stray_staged_files.extend(atomic::staged_files(&artifacts_dir)?);
    stray_staged_files.extend(atomic::staged_files(&notebook.private_dir())?);
    stray_staged_files.sort();

    Ok(NotebookScan {
        notes,
        artifacts,
        stray_staged_files,
    })
}
