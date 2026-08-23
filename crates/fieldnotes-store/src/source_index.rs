//! Finding the one active Note for a portable exact-source key.
//!
//! A1 section 7 allows at most one active Note per exact portable source key
//! `(source_scope, source_identity)`, and requires an update to that source
//! object to preserve the existing Note's ID rather than mint a second Note.
//! Reconciliation therefore has to be able to answer "which Note is this
//! object, if any?" before it writes anything.
//!
//! # How the lookup is built, and what it costs
//!
//! **`0.1.1` builds this by scanning `notes/` once per sync run.** Every `.md`
//! file is read and parsed, and the ones that carry both halves of the source
//! key are indexed by `(field_id, source_scope, source_identity)`. The cost is
//! one pass over the notes directory and one frontmatter parse per Note per
//! run — linear in notebook size, paid once per run rather than once per
//! record, and entirely sequential I/O.
//!
//! That is a deliberate, stated choice rather than an oversight. The
//! alternative is a persistent side index, and `docs/roadmap.md`'s invariants
//! make every cache and index disposable and rebuildable from the canonical
//! files, which means a persistent index needs its own staleness story,
//! invalidation rules, and rebuild command. `0.1.2` builds exactly that
//! machinery for the deterministic graph; until then a scan is honest, cannot
//! go stale, and costs one linear pass. A notebook large enough for the scan to
//! dominate a sync is the signal to move this lookup onto `0.1.2`'s
//! rebuildable index.
//!
//! # Parsing, not validating
//!
//! The index parses; it does not validate. A Note whose frontmatter no longer
//! satisfies the full A1 validator still occupies its source key, and treating
//! it as absent would mint a second Note for the same upstream object — the
//! exact duplicate A1 section 7 forbids. Unparseable files are reported
//! instead, so `inspect` and `status` still surface them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fieldnotes_domain::{RecordId, Scalar, Value};
use fieldnotes_format::{ParsedRecord, parse_record};

use crate::atomic;
use crate::error::StoreError;
use crate::layout::Notebook;

/// One indexed Note: where it lives, what its ID is, and its parsed content.
#[derive(Debug, Clone)]
pub struct IndexedNote {
    /// The Note's absolute path.
    pub path: PathBuf,
    /// The Note's current filename, which reconciliation needs so it can
    /// remove a superseded name after installing the new one.
    pub filename: String,
    /// The Note's logical ID, which an update preserves.
    pub note_id: RecordId,
    /// The parsed record, for comparing against a freshly collected candidate.
    pub record: ParsedRecord,
}

/// The portable exact-source key, qualified by the producing Field.
///
/// The Field ID is part of the lookup key rather than part of the identity: the
/// portable key alone is what collapses independently collected copies of one
/// upstream object, and `0.1.2`'s merge work is what unions producers across
/// Fields and instances. Within one sync run, core reconciles the Notes its own
/// `(instance_id, field_id)` produced, and leaves another producer's Notes for
/// that merge pass rather than silently rewriting them.
pub type SourceKey = (String, String, String);

/// Every Note in the notebook that carries a portable exact-source key.
#[derive(Debug, Clone, Default)]
pub struct SourceIndex {
    notes: BTreeMap<SourceKey, IndexedNote>,
    duplicates: Vec<SourceKey>,
    unparseable: Vec<PathBuf>,
}

impl SourceIndex {
    /// Looks up the active Note for one Field's portable source key.
    #[must_use]
    pub fn get(&self, field_id: &str, scope: &str, identity: &str) -> Option<&IndexedNote> {
        self.notes
            .get(&(field_id.to_owned(), scope.to_owned(), identity.to_owned()))
    }

    /// Every source key inside one Field's `scope`, in ascending key order.
    ///
    /// This is what an authoritative snapshot reconciles against: a Note whose
    /// key falls inside the declared scope and which the run did not report may
    /// be removed, and nothing outside that scope is ever considered.
    #[must_use]
    pub fn keys_in_scope(&self, field_id: &str, scope: &str) -> Vec<(&SourceKey, &IndexedNote)> {
        self.notes
            .iter()
            .filter(|((field, note_scope, _), _)| field == field_id && note_scope == scope)
            .collect()
    }

    /// Every distinct `source_scope` one Field's Notes carry, in ascending
    /// order.
    ///
    /// A snapshot run needs an explicit scope, and a Field's scope value is
    /// computed by the Field at run time rather than declared in its manifest,
    /// so the notebook's own Notes are the only place core can learn it from
    /// before starting a run.
    #[must_use]
    pub fn scopes_for(&self, field_id: &str) -> Vec<&str> {
        let mut scopes: Vec<&str> = self
            .notes
            .keys()
            .filter(|(field, _, _)| field == field_id)
            .map(|(_, scope, _)| scope.as_str())
            .collect();
        scopes.dedup();
        scopes
    }

    /// Records a Note this run installed, so a later record in the same run
    /// finds it without a second scan.
    pub fn insert(&mut self, field_id: &str, scope: &str, identity: &str, note: IndexedNote) {
        self.notes.insert(
            (field_id.to_owned(), scope.to_owned(), identity.to_owned()),
            note,
        );
    }

    /// Forgets a Note this run removed.
    pub fn remove(&mut self, field_id: &str, scope: &str, identity: &str) {
        self.notes
            .remove(&(field_id.to_owned(), scope.to_owned(), identity.to_owned()));
    }

    /// Source keys that more than one Note claims.
    ///
    /// A1 section 7 admits at most one active Note per exact portable source
    /// key "except while a visible conflict is unresolved", and conflict
    /// bundles are `0.1.2` work. Reconciliation therefore reports this rather
    /// than choosing a winner.
    #[must_use]
    pub fn duplicate_keys(&self) -> &[SourceKey] {
        &self.duplicates
    }

    /// Note files that could not be parsed at all, so their source key is
    /// unknown.
    #[must_use]
    pub fn unparseable(&self) -> &[PathBuf] {
        &self.unparseable
    }

    /// How many Notes carry a portable source key.
    #[must_use]
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// Whether no Note carries a portable source key.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

/// Builds the source-key index by scanning `notes/` once.
pub fn build_source_index(notebook: &Notebook) -> Result<SourceIndex, StoreError> {
    let directory = notebook.notes_dir();
    let mut index = SourceIndex::default();
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(index),
        Err(error) => return Err(StoreError::io("read directory", &directory, error)),
    };
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io("read directory", &directory, error))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if atomic::is_staged_name(name) || !name.ends_with(".md") {
            continue;
        }
        files.push((name.to_owned(), path));
    }
    // Deterministic order, so a duplicate key always reports the same survivor.
    files.sort();
    for (filename, path) in files {
        index_one(&mut index, &filename, &path)?;
    }
    Ok(index)
}

fn index_one(index: &mut SourceIndex, filename: &str, path: &Path) -> Result<(), StoreError> {
    let bytes = std::fs::read(path).map_err(|error| StoreError::io("read Note", path, error))?;
    let Ok(record) = parse_record(&bytes) else {
        index.unparseable.push(path.to_path_buf());
        return Ok(());
    };
    let (Some(field_id), Some(scope), Some(identity)) = (
        text_of(&record, "field_id"),
        text_of(&record, "source_scope"),
        text_of(&record, "source_identity"),
    ) else {
        return Ok(());
    };
    let key = (field_id, scope, identity);
    let note = IndexedNote {
        path: path.to_path_buf(),
        filename: filename.to_owned(),
        note_id: *record.id(),
        record,
    };
    if index.notes.insert(key.clone(), note).is_some() {
        index.duplicates.push(key);
    }
    Ok(())
}

fn text_of(record: &ParsedRecord, key: &str) -> Option<String> {
    match record.get(key) {
        Some(Value::Scalar(Scalar::Text(text))) => Some(text.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_test_support::TempDir;

    fn notebook(label: &str) -> Result<(TempDir, Notebook), StoreError> {
        let temp = TempDir::new(label)
            .map_err(|error| StoreError::io("create temporary directory", ".", error))?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        Ok((temp, notebook))
    }

    fn write(notebook: &Notebook, filename: &str, text: &str) -> Result<(), StoreError> {
        let directory = notebook.notes_dir();
        std::fs::create_dir_all(&directory)
            .map_err(|error| StoreError::io("create", &directory, error))?;
        let path = directory.join(filename);
        std::fs::write(&path, text).map_err(|error| StoreError::io("write", &path, error))
    }

    fn note_text(id: &str, identity: &str) -> String {
        format!(
            "---\nid: {id}\ninstance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a\nfield_id: \
             local_work\ntype: file\noccurred_at: 2026-08-22T09:45:00+02:00\nsource_identity: \
             {identity}\nsource_scope: \"local-root:one\"\n---\n\nBody.\n"
        )
    }

    #[test]
    fn an_empty_notebook_indexes_nothing() -> Result<(), StoreError> {
        let (_temp, notebook) = notebook("index-empty")?;
        let index = build_source_index(&notebook)?;
        assert!(index.is_empty());
        assert!(
            index
                .get("local_work", "local-root:one", "file/a")
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn a_note_is_found_by_its_portable_source_key() -> Result<(), StoreError> {
        let (_temp, notebook) = notebook("index-found")?;
        write(
            &notebook,
            "20260822T074500Z_local_work_file_note_01a0286e-33d0-7000-8000-000000000004.md",
            &note_text("note_01a0286e-33d0-7000-8000-000000000004", "file/a.md"),
        )?;
        let index = build_source_index(&notebook)?;
        let found = index
            .get("local_work", "local-root:one", "file/a.md")
            .ok_or_else(|| StoreError::ArtifactCorrupt {
                path: notebook.notes_dir(),
            })?;
        assert_eq!(
            found.note_id.to_string(),
            "note_01a0286e-33d0-7000-8000-000000000004"
        );
        // A different Field never matches the same portable key.
        assert!(
            index
                .get("teams_work", "local-root:one", "file/a.md")
                .is_none()
        );
        assert_eq!(index.scopes_for("local_work"), vec!["local-root:one"]);
        assert_eq!(index.keys_in_scope("local_work", "local-root:one").len(), 1);
        Ok(())
    }

    #[test]
    fn two_notes_claiming_one_key_are_reported_rather_than_resolved() -> Result<(), StoreError> {
        let (_temp, notebook) = notebook("index-duplicate")?;
        write(
            &notebook,
            "20260822T074500Z_local_work_file_note_01a0286e-33d0-7000-8000-000000000004.md",
            &note_text("note_01a0286e-33d0-7000-8000-000000000004", "file/a.md"),
        )?;
        write(
            &notebook,
            "20260822T074500Z_local_work_file_note_01a0286e-33d0-7000-8000-000000000005.md",
            &note_text("note_01a0286e-33d0-7000-8000-000000000005", "file/a.md"),
        )?;
        let index = build_source_index(&notebook)?;
        assert_eq!(index.duplicate_keys().len(), 1);
        Ok(())
    }

    #[test]
    fn an_unparseable_note_is_reported_not_silently_absent() -> Result<(), StoreError> {
        let (_temp, notebook) = notebook("index-unparseable")?;
        write(&notebook, "20260822T074500Z_broken.md", "not frontmatter")?;
        let index = build_source_index(&notebook)?;
        assert_eq!(index.unparseable().len(), 1);
        assert!(index.is_empty());
        Ok(())
    }
}
