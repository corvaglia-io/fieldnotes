//! A notebook harness for driving core's `sync` against the fixture Field.
//!
//! The fixture Field can be driven into every failure mode the A2 corpus
//! describes: each malformed output shape, each hostile artifact reference, a
//! hang, a standard-error flood, and a crash on either side of a checkpoint.
//! These cases start it as a **real child process** and assert what a real
//! notebook on disk holds afterwards, which is the only way to show that a
//! committed cursor never precedes an undurable write and that a malformed frame
//! costs a run rather than a notebook.

#![allow(dead_code)]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use fieldnotes_app::{
    FieldRunOutcome, FieldSyncReport, Kernel, SyncMode, SyncOptions, add_field, init, sync,
    validate_field_id,
};
use fieldnotes_domain::{Clock, Scalar, Value};
use fieldnotes_format::{ParsedRecord, parse_record, validate_record};
use fieldnotes_store::{Notebook, StoredCursor, read_cursor, read_last_sync_outcome};
use fieldnotes_test_support::{CountingRandom, TempDir};

/// The fixture Field's pinned absolute path, resolved by Cargo.
pub const FIELD_EXECUTABLE: &str = env!("CARGO_BIN_EXE_fieldnotes-field-fixture");

/// The scenario selector the fixture Field reads.
pub const SCENARIO_VARIABLE: &str = "FIELDNOTES_FIXTURE_SCENARIO";

/// The variable the deliberate-leak scenarios read.
pub const LEAK_VARIABLE: &str = "FIELDNOTES_FIXTURE_LEAK_VALUE";

/// The `local`-stem Field ID the local-flavor scenarios are configured under.
pub const LOCAL_FIELD: &str = "local_fixture";

/// The portable source scope the local-flavor scenarios report.
pub const LOCAL_SCOPE: &str = "local-root:reference-library-v1";

/// A clock that reports a distinct instant on every reading, so a replay's
/// capture time differs from the original's and cannot accidentally make two
/// runs byte-identical for the wrong reason.
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

/// One case: a real notebook with the fixture Field configured under one ID.
pub struct Case {
    temp: TempDir,
    notebook: Notebook,
    field_id: String,
    seed: Cell<u8>,
}

impl Case {
    /// Initializes a notebook and configures the fixture Field under the
    /// `local` stem, which every local-flavor scenario declares.
    pub fn new(label: &str) -> Self {
        Case::with_field(label, "local", "fixture")
    }

    /// Initializes a notebook and configures the fixture Field under an
    /// explicit registered stem and label.
    pub fn with_field(label: &str, stem: &str, field_label: &str) -> Self {
        let temp = TempDir::new(&format!("sync-fixture-{label}"))
            .unwrap_or_else(|error| panic!("a temporary directory is required: {error}"));
        let mut kernel = Kernel::new(AdvancingClock::new(), CountingRandom::new(1), 0)
            .unwrap_or_else(|error| panic!("the kernel must build: {error}"));
        let outcome = init(&mut kernel, &temp.path().join("notebook"), Some(label))
            .unwrap_or_else(|error| panic!("the notebook must initialize: {error}"));
        let notebook = Notebook::open(&outcome.root)
            .unwrap_or_else(|error| panic!("the notebook must open: {error}"));
        let field_id = validate_field_id(stem, field_label)
            .unwrap_or_else(|error| panic!("the Field ID must validate: {error}"));
        add_field(
            &notebook,
            &field_id,
            PathBuf::from(FIELD_EXECUTABLE),
            BTreeMap::new(),
            true,
        )
        .unwrap_or_else(|error| panic!("the Field must be configurable: {error}"));
        Case {
            temp,
            notebook,
            field_id: field_id.as_str().to_owned(),
            seed: Cell::new(2),
        }
    }

    /// The notebook under test.
    pub fn notebook(&self) -> &Notebook {
        &self.notebook
    }

    /// The configured Field ID.
    pub fn field_id(&self) -> &str {
        &self.field_id
    }

    /// Runs one sync driving the named fixture scenario.
    pub fn run(&self, scenario: &str) -> FieldSyncReport {
        self.run_with(scenario, SyncOptions::default())
    }

    /// Runs one snapshot-mode sync driving the named fixture scenario.
    pub fn run_snapshot(&self, scenario: &str, scope: &str) -> FieldSyncReport {
        self.run_with(
            scenario,
            SyncOptions {
                mode: SyncMode::Snapshot,
                snapshot_scope: Some(scope.to_owned()),
                ..SyncOptions::default()
            },
        )
    }

    /// Runs one sync driving the named fixture scenario with explicit options.
    pub fn run_with(&self, scenario: &str, mut options: SyncOptions) -> FieldSyncReport {
        options
            .field_environment
            .insert(SCENARIO_VARIABLE.to_owned(), scenario.to_owned());
        let seed = self.seed.get();
        self.seed.set(seed.wrapping_add(23));
        let mut kernel = Kernel::new(AdvancingClock::new(), CountingRandom::new(seed), 0)
            .unwrap_or_else(|error| panic!("the kernel must build: {error}"));
        let outcome = sync(&mut kernel, &self.notebook, Some(&self.field_id), &options)
            .unwrap_or_else(|error| panic!("sync must not fail at the notebook level: {error}"));
        match outcome.fields.as_slice() {
            [report] => report.clone(),
            other => panic!("expected exactly one Field report, got {}", other.len()),
        }
    }

    /// The committed cursor, when one is recorded.
    pub fn cursor(&self) -> Option<StoredCursor> {
        read_cursor(&self.notebook, &self.field_id)
            .unwrap_or_else(|error| panic!("the cursor must be readable: {error}"))
    }

    /// The recorded last-sync outcome label, when one is recorded.
    pub fn last_sync_outcome(&self) -> Option<String> {
        read_last_sync_outcome(&self.notebook, &self.field_id)
            .unwrap_or_else(|error| panic!("the status file must be readable: {error}"))
            .map(|outcome| outcome.outcome)
    }

    /// A fingerprint of every public notebook file: names and exact bytes.
    ///
    /// Comparing this before and after a run is what "byte-identical" means.
    pub fn notebook_state(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        for directory in [self.notebook.notes_dir(), self.notebook.artifacts_dir()] {
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = self.notebook.relative_display(&path);
                let bytes = std::fs::read(&path)
                    .unwrap_or_else(|error| panic!("a notebook file must be readable: {error}"));
                files.push((name, bytes));
            }
        }
        files.sort();
        files
    }

    /// Every Note file, parsed and validated with the format crate's own public
    /// validator.
    pub fn validated_notes(&self) -> Vec<(String, ParsedRecord)> {
        let mut notes = Vec::new();
        let entries = match std::fs::read_dir(self.notebook.notes_dir()) {
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
            let record =
                parse_record(&bytes).unwrap_or_else(|error| panic!("{name} must parse: {error}"));
            validate_record(&record)
                .unwrap_or_else(|error| panic!("{name} must validate: {error}"));
            notes.push((name.to_owned(), record));
        }
        notes.sort_by(|left, right| left.0.cmp(&right.0));
        notes
    }

    /// Whether an active Note carries `source_identity`.
    pub fn has_note_for(&self, source_identity: &str) -> bool {
        self.validated_notes().iter().any(|(_, record)| {
            text_of(record, "source_identity").as_deref() == Some(source_identity)
        })
    }

    /// The one Note carrying `source_identity`.
    pub fn note_for(&self, source_identity: &str) -> ParsedRecord {
        let matches: Vec<ParsedRecord> = self
            .validated_notes()
            .into_iter()
            .filter(|(_, record)| {
                text_of(record, "source_identity").as_deref() == Some(source_identity)
            })
            .map(|(_, record)| record)
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

/// Requires a run to have failed with a specific rejection code.
pub fn require_rejection(report: &FieldSyncReport, code: &str) {
    let rejection = report
        .rejection
        .as_ref()
        .unwrap_or_else(|| panic!("the run must have been rejected: {report:?}"));
    assert_eq!(
        rejection.code, code,
        "unexpected rejection code; detail was {}",
        rejection.detail
    );
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
}

/// Requires a run to have completed.
pub fn require_complete(report: &FieldSyncReport) {
    assert_eq!(
        report.outcome,
        FieldRunOutcome::Complete,
        "the run should have completed: rejection {:?}, failure {:?}",
        report.rejection,
        report.failure
    );
}
