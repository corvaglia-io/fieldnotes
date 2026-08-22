//! `status`: a summary of local notebook health.
//!
//! Everything reported here comes from the public files plus instance
//! metadata, so a notebook that was copied, merged, or edited by hand reports
//! what it actually contains.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fieldnotes_format::instance::InstanceMetadata;
use fieldnotes_store::{Notebook, ScanOptions, read_instance, scan};

use crate::error::AppError;
use crate::kernel::SELF_FIELD;

/// A notebook summary.
#[derive(Debug, Clone)]
pub struct StatusReport {
    /// The notebook root.
    pub root: PathBuf,
    /// The notebook's instance identity.
    pub instance: InstanceMetadata,
    /// The configured Field IDs. `0.1.0` has only the built-in `self` Field.
    pub fields: Vec<String>,
    /// How many Note files exist.
    pub notes_total: usize,
    /// How many Note files are fully valid.
    pub notes_valid: usize,
    /// How many Note files carry a problem.
    pub notes_invalid: usize,
    /// Note counts by primary type, in ascending type order.
    pub notes_by_type: BTreeMap<String, usize>,
    /// How many original artifacts are stored.
    pub artifacts_total: usize,
    /// Total stored artifact bytes.
    pub artifact_bytes: u64,
    /// Stored artifacts no retained Note references.
    pub artifacts_unreferenced: usize,
    /// Artifact references with no stored bytes.
    pub missing_artifact_references: usize,
    /// Leftover staging files, which indicate an interrupted write.
    pub interrupted_writes: usize,
    /// The earliest and latest `occurred_at` among valid Notes, as canonical
    /// datetimes.
    pub occurred_range: Option<(String, String)>,
}

/// Summarizes a notebook.
pub fn status(notebook: &Notebook) -> Result<StatusReport, AppError> {
    let instance = read_instance(notebook)?;
    let scanned = scan(notebook, ScanOptions::default())?;

    let mut occurred: Vec<&fieldnotes_domain::Datetime> = scanned
        .notes
        .iter()
        .filter(|note| note.is_valid())
        .filter_map(|note| note.record.as_ref()?.occurred_at())
        .collect();
    occurred.sort_by(|left, right| left.cmp_instant(right));
    let occurred_range = match (occurred.first(), occurred.last()) {
        (Some(first), Some(last)) => Some((first.to_string(), last.to_string())),
        _ => None,
    };

    let missing_artifact_references = scanned
        .notes
        .iter()
        .flat_map(|note| note.problems.iter())
        .filter(|problem| matches!(problem, fieldnotes_store::Problem::MissingArtifact { .. }))
        .count();

    Ok(StatusReport {
        root: notebook.root().to_path_buf(),
        instance,
        fields: vec![SELF_FIELD.to_owned()],
        notes_total: scanned.notes.len(),
        notes_valid: scanned.valid_notes(),
        notes_invalid: scanned.notes.len() - scanned.valid_notes(),
        notes_by_type: scanned
            .notes_by_type()
            .into_iter()
            .map(|(key, count)| (key.to_owned(), count))
            .collect(),
        artifacts_total: scanned.artifacts.len(),
        artifact_bytes: scanned.artifact_bytes(),
        artifacts_unreferenced: scanned
            .artifacts
            .iter()
            .filter(|artifact| !artifact.referenced)
            .count(),
        missing_artifact_references,
        interrupted_writes: scanned.stray_staged_files.len(),
        occurred_range,
    })
}
