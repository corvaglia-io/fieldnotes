//! Shared harness for the `local` Field's executable conformance cases.
//!
//! Every case here starts the real `fieldnotes-field-local` binary as a
//! **real child process** and talks to it over real pipes through the
//! reusable conformance kit, exercising the same process boundary a live
//! sync run does.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use fieldnotes_field_protocol::conformance::{CollectPlan, CollectRun, FieldUnderTest};
use fieldnotes_field_protocol::grammar::PropertyNameToken;
use fieldnotes_field_protocol::message::{FieldEvent, Manifest, RecordEvent};
use fieldnotes_field_protocol::value::PropertyValue;
use fieldnotes_test_support::TempDir;

/// The `local` Field's pinned absolute path, resolved by Cargo.
pub const FIELD_EXECUTABLE: &str = env!("CARGO_BIN_EXE_fieldnotes-field-local");

/// The configured Field ID every case uses.
pub const FIELD_ID: &str = "local_work";

/// A run identifier for a describe run.
pub const DESCRIBE_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000001";

/// A run identifier for the first collect run in a case.
pub const COLLECT_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000002";

/// A run identifier for a second, resuming collect run.
pub const RESUME_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000003";

/// A run identifier for a snapshot run.
pub const SNAPSHOT_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000004";

fn config_key(name: &str) -> PropertyNameToken {
    PropertyNameToken::parse(name).unwrap_or_else(|error| panic!("{name} must be a key: {error}"))
}

/// One conformance case: a pinned executable, a real configured root
/// directory, and a staging directory core owns.
pub struct Case {
    field: FieldUnderTest,
    staging: TempDir,
    root: TempDir,
}

impl Case {
    /// Builds a case with a fresh, empty configured root and staging
    /// directory.
    pub fn new(label: &str) -> Self {
        let staging = TempDir::new(&format!("local-staging-{label}"))
            .unwrap_or_else(|error| panic!("a staging directory is required: {error}"));
        let root = TempDir::new(&format!("local-root-{label}"))
            .unwrap_or_else(|error| panic!("a configured root is required: {error}"));
        Case {
            field: FieldUnderTest::new(FIELD_EXECUTABLE)
                .with_idle(Duration::from_secs(10))
                .with_wait(Duration::from_secs(15)),
            staging,
            root,
        }
    }

    /// The configured root directory this case collects from.
    pub fn root_dir(&self) -> &Path {
        self.root.path()
    }

    /// The staging directory core created and will name in the request.
    pub fn staging_dir(&self) -> PathBuf {
        self.staging.path().to_path_buf()
    }

    /// Runs a describe run and requires a manifest.
    pub fn manifest(&self) -> Manifest {
        match self.field.describe(DESCRIBE_RUN, Some(FIELD_ID)) {
            Ok(run) => run.manifest.unwrap_or_else(|| {
                panic!(
                    "the describe run produced no manifest: {:?} {:?}",
                    run.rejection, run.detail
                )
            }),
            Err(error) => panic!("the describe run could not start: {error}"),
        }
    }

    fn with_root_config(&self, mut plan: CollectPlan) -> CollectPlan {
        plan.config.insert(
            config_key("root_path"),
            PropertyValue::Text(self.root_dir().display().to_string()),
        );
        plan
    }

    /// An incremental plan configured to collect from this case's root.
    pub fn incremental_plan(&self, run_id: &str) -> CollectPlan {
        let plan = CollectPlan::incremental(run_id, FIELD_ID, self.staging_dir())
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"));
        self.with_root_config(plan)
    }

    /// A snapshot plan for one declared scope, configured to collect from
    /// this case's root.
    pub fn snapshot_plan(&self, run_id: &str, scope: &str) -> CollectPlan {
        let plan = CollectPlan::snapshot(run_id, FIELD_ID, self.staging_dir(), scope)
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"));
        self.with_root_config(plan)
    }

    /// Runs a collect run against a manifest already obtained from
    /// [`Case::manifest`].
    pub fn collect(&self, manifest: &Manifest, plan: &CollectPlan) -> CollectRun {
        match self.field.collect(manifest, plan) {
            Ok(run) => run,
            Err(error) => panic!("the collect run could not start: {error}"),
        }
    }
}

/// Requires a plan to carry a replayed cursor.
pub fn with_cursor(plan: CollectPlan, cursor: &str, format_version: u16) -> CollectPlan {
    match plan.with_cursor(cursor, format_version) {
        Ok(plan) => plan,
        Err(error) => panic!("the cursor is invalid: {error}"),
    }
}

/// Every record event a run accepted, in order.
pub fn record_events(run: &CollectRun) -> Vec<&RecordEvent> {
    run.events
        .iter()
        .filter_map(|event| match event {
            FieldEvent::Record(record) => Some(record.as_ref()),
            _ => None,
        })
        .collect()
}

/// The portable exact-source scope of the first record event, panicking if
/// the run reported none.
pub fn first_scope(run: &CollectRun) -> String {
    record_events(run)
        .first()
        .map(|record| record.source.scope.as_str().to_owned())
        .unwrap_or_else(|| panic!("the run must have reported at least one record"))
}

/// Pushes a file's modification time forward by `seconds`, so a rewritten
/// file's mtime is unambiguously newer than before even at one-second
/// filesystem mtime resolution.
pub fn touch_forward(path: &Path, seconds: u64) {
    let metadata = std::fs::metadata(path).unwrap_or_else(|error| panic!("metadata: {error}"));
    let modified = metadata
        .modified()
        .unwrap_or_else(|error| panic!("modified: {error}"));
    let bumped = modified + Duration::from_secs(seconds);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|error| panic!("open: {error}"));
    file.set_modified(bumped)
        .unwrap_or_else(|error| panic!("set_modified: {error}"));
}
