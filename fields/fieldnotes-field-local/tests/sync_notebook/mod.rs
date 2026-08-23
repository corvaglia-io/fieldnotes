//! A notebook harness for driving core's `sync` against the real `local`
//! Field binary.
//!
//! These cases start `fieldnotes-field-local` as a **real child process** over
//! real pipes, exactly as a live sync does, and assert what landed in a real
//! notebook on disk. A test that never starts a process is not evidence about a
//! process boundary, and a test that never writes a notebook is not evidence
//! about durability.

#![allow(dead_code)]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fieldnotes_app::{
    FieldRunOutcome, FieldSyncReport, Kernel, SyncOptions, SyncOutcome, add_field, init, sync,
    validate_field_id,
};
use fieldnotes_domain::{Clock, Scalar, Value};
use fieldnotes_format::{ParsedRecord, parse_record, validate_record};
use fieldnotes_store::{Notebook, StoredCursor, read_cursor};
use fieldnotes_test_support::{CountingRandom, TempDir};

/// The `local` Field's pinned absolute path, resolved by Cargo.
pub const FIELD_EXECUTABLE: &str = env!("CARGO_BIN_EXE_fieldnotes-field-local");

/// The configured Field ID every case uses.
pub const FIELD_ID: &str = "local_work";

/// A clock that reports a distinct instant on every reading.
///
/// Two runs of the same object must produce the same *notebook bytes* even
/// though the second run's capture time differs, which is exactly what the
/// semantic fingerprint's exclusion of `captured_at` guarantees. A clock that
/// never moves would hide a bug there.
#[derive(Debug)]
pub struct AdvancingClock(Cell<u64>);

impl AdvancingClock {
    fn new() -> Self {
        AdvancingClock(Cell::new(1_787_381_100_000))
    }
}

impl Clock for AdvancingClock {
    fn unix_millis(&self) -> u64 {
        let now = self.0.get();
        self.0.set(now + 1000);
        now
    }
}

/// One case: a temporary source directory and a real notebook beside it.
pub struct Case {
    temp: TempDir,
    notebook: Notebook,
    seed: Cell<u8>,
}

impl Case {
    /// Initializes a notebook and configures the `local` Field against a fresh
    /// source directory.
    pub fn new(label: &str) -> Self {
        let temp = TempDir::new(&format!("sync-local-{label}"))
            .unwrap_or_else(|error| panic!("a temporary directory is required: {error}"));
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source)
            .unwrap_or_else(|error| panic!("a source directory is required: {error}"));
        let mut kernel = Kernel::new(AdvancingClock::new(), CountingRandom::new(1), 0)
            .unwrap_or_else(|error| panic!("the kernel must build: {error}"));
        let outcome = init(&mut kernel, &temp.path().join("notebook"), Some(label))
            .unwrap_or_else(|error| panic!("the notebook must initialize: {error}"));
        let notebook = Notebook::open(&outcome.root)
            .unwrap_or_else(|error| panic!("the notebook must open: {error}"));
        let field_id = validate_field_id("local", "work")
            .unwrap_or_else(|error| panic!("the Field ID must validate: {error}"));
        let mut config = BTreeMap::new();
        config.insert("root_path".to_owned(), source.display().to_string());
        add_field(
            &notebook,
            &field_id,
            PathBuf::from(FIELD_EXECUTABLE),
            config,
            true,
        )
        .unwrap_or_else(|error| panic!("the Field must be configurable: {error}"));
        Case {
            temp,
            notebook,
            seed: Cell::new(2),
        }
    }

    /// The configured source directory.
    pub fn source(&self) -> PathBuf {
        self.temp.path().join("source")
    }

    /// The notebook under test.
    pub fn notebook(&self) -> &Notebook {
        &self.notebook
    }

    /// Writes a file into the source directory, creating parent directories.
    pub fn write_source(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.source().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("a parent directory is required: {error}"));
        }
        std::fs::write(&path, contents)
            .unwrap_or_else(|error| panic!("the source file must be writable: {error}"));
        path
    }

    /// Removes a file from the source directory.
    pub fn remove_source(&self, relative: &str) {
        let path = self.source().join(relative);
        std::fs::remove_file(&path)
            .unwrap_or_else(|error| panic!("the source file must be removable: {error}"));
    }

    /// Runs one sync with the given options, returning the whole outcome.
    ///
    /// A fresh kernel per run keeps IDs deterministic per run while the
    /// advancing clock still moves capture times between runs.
    pub fn sync_with(&self, options: &SyncOptions) -> SyncOutcome {
        let seed = self.seed.get();
        self.seed.set(seed.wrapping_add(17));
        let mut kernel = Kernel::new(AdvancingClock::new(), CountingRandom::new(seed), 0)
            .unwrap_or_else(|error| panic!("the kernel must build: {error}"));
        sync(&mut kernel, &self.notebook, Some(FIELD_ID), options)
            .unwrap_or_else(|error| panic!("sync must not fail at the notebook level: {error}"))
    }

    /// Runs one sync and requires exactly one Field report.
    pub fn sync_one(&self, options: &SyncOptions) -> FieldSyncReport {
        let outcome = self.sync_with(options);
        match outcome.fields.as_slice() {
            [report] => report.clone(),
            other => panic!("expected exactly one Field report, got {}", other.len()),
        }
    }

    /// The committed cursor, when one is recorded.
    pub fn cursor(&self) -> Option<StoredCursor> {
        read_cursor(&self.notebook, FIELD_ID)
            .unwrap_or_else(|error| panic!("the cursor must be readable: {error}"))
    }

    /// Every Note file's name and bytes, in filename order.
    pub fn notes(&self) -> Vec<(String, Vec<u8>)> {
        let mut notes = Vec::new();
        let directory = self.notebook.notes_dir();
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => return notes,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.ends_with(".md") {
                continue;
            }
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("a Note must be readable: {error}"));
            notes.push((name.to_owned(), bytes));
        }
        notes.sort();
        notes
    }

    /// Every Note file, parsed and validated with the format crate's own
    /// public validator, so the bytes core wrote are provably canonical.
    pub fn validated_notes(&self) -> Vec<(String, ParsedRecord)> {
        self.notes()
            .into_iter()
            .map(|(name, bytes)| {
                let record = parse_record(&bytes)
                    .unwrap_or_else(|error| panic!("{name} must parse: {error}"));
                validate_record(&record)
                    .unwrap_or_else(|error| panic!("{name} must validate: {error}"));
                (name, record)
            })
            .collect()
    }

    /// Every artifact file's name, in filename order.
    pub fn artifacts(&self) -> Vec<String> {
        let mut names = Vec::new();
        let entries = match std::fs::read_dir(self.notebook.artifacts_dir()) {
            Ok(entries) => entries,
            Err(_) => return names,
        };
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_owned());
            }
        }
        names.sort();
        names
    }

    /// The one Note carrying `source_identity`, panicking if there is not
    /// exactly one.
    pub fn note_for(&self, source_identity: &str) -> (String, ParsedRecord) {
        let matches: Vec<(String, ParsedRecord)> = self
            .validated_notes()
            .into_iter()
            .filter(|(_, record)| {
                text_of(record, "source_identity").as_deref() == Some(source_identity)
            })
            .collect();
        match matches.len() {
            1 => matches
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("a single match must be available")),
            other => panic!("expected exactly one Note for {source_identity}, found {other}"),
        }
    }
}

/// A text property value, when present.
pub fn text_of(record: &ParsedRecord, key: &str) -> Option<String> {
    match record.get(key) {
        Some(Value::Scalar(Scalar::Text(text))) => Some(text.clone()),
        _ => None,
    }
}

/// A text-list property value, when present.
pub fn list_of(record: &ParsedRecord, key: &str) -> Vec<String> {
    match record.get(key) {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                Scalar::Text(text) => Some(text.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Pushes a file's modification time forward, so a rewritten file's mtime is
/// unambiguously newer even at one-second filesystem resolution.
pub fn touch_forward(path: &Path, seconds: u64) {
    let metadata = std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("metadata must be readable: {error}"));
    let modified = metadata
        .modified()
        .unwrap_or_else(|error| panic!("a modification time is required: {error}"));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|error| panic!("the file must be openable: {error}"));
    file.set_modified(modified + std::time::Duration::from_secs(seconds))
        .unwrap_or_else(|error| panic!("the modification time must be settable: {error}"));
}

/// Requires a run to have completed.
pub fn require_complete(report: &FieldSyncReport) {
    assert_eq!(
        report.outcome,
        FieldRunOutcome::Complete,
        "the run should have completed: rejection {:?}, failure {:?}, stderr {:?}",
        report.rejection,
        report.failure,
        report.stderr
    );
}
