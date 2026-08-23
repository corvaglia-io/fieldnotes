//! `inspect`: validate notebook contents, or render one record.
//!
//! Invalid files are surfaced, never hidden: a file that fails to parse or
//! validate appears in the report with the reason, so a damaged notebook looks
//! damaged instead of looking small.

use std::path::{Path, PathBuf};

use fieldnotes_domain::{Scalar, Value};
use fieldnotes_format::instance::InstanceMetadata;
use fieldnotes_store::{Notebook, ScanOptions, ScannedNote, read_instance, scan};

use crate::error::AppError;

/// One problem, flattened for presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedProblem {
    /// A stable lowercase kind label.
    pub kind: String,
    /// A human-readable explanation.
    pub message: String,
}

/// One inspected record file.
#[derive(Debug, Clone)]
pub struct InspectedRecord {
    /// The notebook-relative path.
    pub path: String,
    /// The record ID, when the file parsed.
    pub id: Option<String>,
    /// The `field_id`, when present.
    pub field_id: Option<String>,
    /// The record `type`, when present.
    pub record_type: Option<String>,
    /// The `occurred_at` value, when present.
    pub occurred_at: Option<String>,
    /// Referenced artifact IDs.
    pub artifacts: Vec<String>,
    /// Whether the file is fully valid.
    pub valid: bool,
    /// Everything wrong with the file.
    pub problems: Vec<ReportedProblem>,
    /// The normalized Markdown body, included only for a single targeted
    /// record so bulk reports stay compact.
    pub body: Option<String>,
}

/// One inspected artifact file.
#[derive(Debug, Clone)]
pub struct InspectedArtifact {
    /// The notebook-relative path.
    pub path: String,
    /// The stored byte length.
    pub bytes: u64,
    /// Whether a retained Note references it.
    pub referenced: bool,
    /// Whether the stored bytes still match the content address.
    pub valid: bool,
    /// Everything wrong with the file.
    pub problems: Vec<ReportedProblem>,
}

/// An inspection result.
#[derive(Debug, Clone)]
pub struct InspectReport {
    /// The notebook root.
    pub root: PathBuf,
    /// The notebook's instance identity.
    pub instance: InstanceMetadata,
    /// The inspected records.
    pub records: Vec<InspectedRecord>,
    /// The inspected artifacts. Empty when one record was targeted.
    pub artifacts: Vec<InspectedArtifact>,
    /// Leftover staging files from an interrupted write, notebook-relative.
    pub interrupted_writes: Vec<String>,
    /// Whether every inspected file is valid and nothing is left over.
    pub healthy: bool,
}

/// Validates a notebook, or one record identified by ID, filename, or path.
pub fn inspect(notebook: &Notebook, target: Option<&str>) -> Result<InspectReport, AppError> {
    let instance = read_instance(notebook)?;
    let scanned = scan(
        notebook,
        ScanOptions {
            verify_artifact_bytes: true,
        },
    )?;

    let mut records: Vec<InspectedRecord> = Vec::new();
    let mut artifacts = Vec::new();
    match target {
        Some(target) => {
            let matched = scanned
                .notes
                .iter()
                .find(|note| matches_target(notebook, note, target))
                .ok_or_else(|| AppError::UnknownTarget {
                    target: target.to_owned(),
                })?;
            records.push(describe(notebook, matched, true));
        }
        None => {
            for note in &scanned.notes {
                records.push(describe(notebook, note, false));
            }
            for artifact in &scanned.artifacts {
                artifacts.push(InspectedArtifact {
                    path: notebook.relative_display(&artifact.path),
                    bytes: artifact.bytes,
                    referenced: artifact.referenced,
                    valid: artifact.problems.is_empty(),
                    problems: artifact.problems.iter().map(reported).collect(),
                });
            }
        }
    }

    let interrupted_writes: Vec<String> = scanned
        .stray_staged_files
        .iter()
        .map(|path| notebook.relative_display(path))
        .collect();
    let healthy = records.iter().all(|record| record.valid)
        && artifacts.iter().all(|artifact| artifact.valid)
        && interrupted_writes.is_empty();

    Ok(InspectReport {
        root: notebook.root().to_path_buf(),
        instance,
        records,
        artifacts,
        interrupted_writes,
        healthy,
    })
}

/// Whether a scanned Note is the requested target.
///
/// A record is addressable by its logical ID independently of its current
/// filename, and by filename or path for convenience.
///
/// Path comparison goes through [`crate::paths`] rather than `==`, because one
/// file has more than one spelling: a macOS temporary directory reached through
/// a symlink, and on Windows an 8.3 short component, a different letter case,
/// or the verbatim `\\?\` prefix. A user who pasted any of those spellings
/// means the file it names, and `==` would answer "no such record".
fn matches_target(notebook: &Notebook, note: &ScannedNote, target: &str) -> bool {
    if note.filename == target {
        return true;
    }
    if let Some(record) = note.record.as_ref()
        && record.id().to_string() == target
    {
        return true;
    }
    let target_path = Path::new(target);
    if crate::paths::same_path(&note.path, target_path) {
        return true;
    }
    crate::paths::same_relative_display(
        &notebook_relative(notebook, &note.path),
        &notebook_relative(notebook, target_path),
    )
}

/// One path's notebook-relative display, falling back to the notebook's own
/// rendering for a path that is not inside the notebook (a
/// working-directory-relative target a user typed, most often).
fn notebook_relative(notebook: &Notebook, path: &Path) -> String {
    crate::paths::relative_to(path, notebook.root())
        .map(|relative| crate::paths::slash_display(&relative))
        .unwrap_or_else(|| notebook.relative_display(path))
}

fn text_value(record: &fieldnotes_format::ParsedRecord, key: &str) -> Option<String> {
    match record.get(key) {
        Some(Value::Scalar(Scalar::Text(text))) => Some(text.clone()),
        _ => None,
    }
}

fn describe(notebook: &Notebook, note: &ScannedNote, include_body: bool) -> InspectedRecord {
    let record = note.record.as_ref();
    InspectedRecord {
        path: notebook.relative_display(&note.path),
        id: record.map(|record| record.id().to_string()),
        field_id: record.and_then(|record| text_value(record, "field_id")),
        record_type: record.and_then(|record| text_value(record, "type")),
        occurred_at: record
            .and_then(|record| record.occurred_at())
            .map(ToString::to_string),
        artifacts: record
            .and_then(|record| match record.get("artifacts") {
                Some(Value::List(items)) => Some(
                    items
                        .iter()
                        .filter_map(|item| match item {
                            Scalar::Text(text) => Some(text.clone()),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default(),
        valid: note.is_valid(),
        problems: note.problems.iter().map(reported).collect(),
        body: if include_body {
            record.map(|record| record.body().to_owned())
        } else {
            None
        },
    }
}

fn reported(problem: &fieldnotes_store::Problem) -> ReportedProblem {
    ReportedProblem {
        kind: problem.kind().to_owned(),
        message: problem.message(),
    }
}
