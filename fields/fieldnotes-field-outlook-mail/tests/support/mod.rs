//! Shared harness for the Outlook Mail Field's executable conformance cases.
//!
//! Every case here starts the real `fieldnotes-field-outlook-mail` binary as a
//! **real child process** and talks to it over real pipes through the reusable
//! conformance kit, exercising the same process boundary a live sync run does.
//!
//! There is no tenant, no network, and no credential anywhere in this file.
//! The child is pointed at a **sanitized recorded Graph script** through one
//! environment entry naming a file path, and answers from those recordings.
//! Every address in those recordings is from this repository's fictional cast
//! (`sam@example.net`, `alice@example.com`, `bob@example.net`), every
//! identifier is a fixture constant, and the only token-shaped value anywhere
//! is the self-describing `FIXTURE-NOT-A-REAL-TOKEN-...` placeholder, which is
//! asserted absent from the child's output.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use fieldnotes_field_protocol::conformance::{CollectPlan, CollectRun, FieldUnderTest};
use fieldnotes_field_protocol::grammar::{OffsetDatetime, PropertyNameToken};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{FieldEvent, Manifest, RecordEvent, Severity, Window};
use fieldnotes_field_protocol::value::PropertyValue;
use fieldnotes_test_support::TempDir;

/// The Field's pinned absolute path, resolved by Cargo.
pub const FIELD_EXECUTABLE: &str = env!("CARGO_BIN_EXE_fieldnotes-field-outlook-mail");

/// The environment entry that points the child at a recorded Graph script.
pub const FIXTURE_SCRIPT_VARIABLE: &str = "FIELDNOTES_OUTLOOK_MAIL_FIXTURE_SCRIPT";

/// The non-secret placeholder the child presents in fixture mode, registered
/// with the kit's canary scan so its absence from every output surface is
/// asserted rather than assumed.
pub const PLACEHOLDER_TOKEN: &str = "FIXTURE-NOT-A-REAL-TOKEN-outlook-mail-replay";

/// The configured Field ID every case uses.
pub const FIELD_ID: &str = "outlook_mail_work";

/// The fixture tenant, matching the frozen `outlook_mail_work` Note fixtures.
pub const TENANT_ID: &str = "8d820000-0000-7000-8000-000000000001";

/// The portable exact-source scope those fixtures show.
pub const SOURCE_SCOPE: &str = "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001";

/// A run identifier for a describe run.
pub const DESCRIBE_RUN: &str = "2b5d0a3f-0000-4000-8000-000000000001";

/// A run identifier for the first collect run in a case.
pub const COLLECT_RUN: &str = "2b5d0a3f-0000-4000-8000-000000000002";

/// A run identifier for a second, resuming collect run.
pub const RESUME_RUN: &str = "2b5d0a3f-0000-4000-8000-000000000003";

fn config_key(name: &str) -> PropertyNameToken {
    PropertyNameToken::parse(name).unwrap_or_else(|error| panic!("{name} must be a key: {error}"))
}

fn instant(text: &str) -> OffsetDatetime {
    OffsetDatetime::parse(text).unwrap_or_else(|error| panic!("{text} must parse: {error}"))
}

/// The absolute path of one recorded Graph script.
pub fn script(scenario: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("graph")
        .join(format!("script-{scenario}.json"))
}

/// One conformance case: the pinned executable, a recorded Graph script, and a
/// staging directory core owns.
pub struct Case {
    field: FieldUnderTest,
    staging: TempDir,
}

impl Case {
    /// Builds a case whose child answers from the named recorded script.
    pub fn new(scenario: &str) -> Self {
        let staging = TempDir::new(&format!("outlook-mail-staging-{scenario}"))
            .unwrap_or_else(|error| panic!("a staging directory is required: {error}"));
        Case {
            field: FieldUnderTest::new(FIELD_EXECUTABLE)
                .with_env(
                    FIXTURE_SCRIPT_VARIABLE,
                    script(scenario).display().to_string(),
                )
                .with_secret(PLACEHOLDER_TOKEN)
                // The product's own configured defaults, not the frozen
                // ceilings: the 25 MiB retention threshold is what the
                // attachment cases are about, and the ceiling is 512 MiB.
                .with_limits(Limits::defaults())
                .with_idle(Duration::from_secs(10))
                .with_wait(Duration::from_secs(15)),
            staging,
        }
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

    /// An incremental plan carrying this case's non-secret configuration.
    pub fn plan(&self, run_id: &str) -> CollectPlan {
        let mut plan = CollectPlan::incremental(run_id, FIELD_ID, self.staging_dir())
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"));
        plan.config.insert(
            config_key("tenant_id"),
            PropertyValue::Text(TENANT_ID.to_owned()),
        );
        plan
    }

    /// An incremental plan with no `tenant_id` at all, for the refusal case.
    pub fn plan_without_configuration(&self, run_id: &str) -> CollectPlan {
        CollectPlan::incremental(run_id, FIELD_ID, self.staging_dir())
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"))
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

/// Bounds a plan to the beta's one-week window.
pub fn windowed(mut plan: CollectPlan, from: &str, to: &str) -> CollectPlan {
    plan.window = Some(Window {
        from: instant(from),
        to: instant(to),
    });
    plan
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

/// Every diagnostic event a run emitted, in order.
pub fn diagnostics(run: &CollectRun) -> Vec<&fieldnotes_field_protocol::message::DiagnosticEvent> {
    run.events
        .iter()
        .filter_map(|event| match event {
            FieldEvent::Diagnostic(diagnostic) => Some(diagnostic.as_ref()),
            _ => None,
        })
        .collect()
}

/// Whether the run emitted any diagnostic of `severity` or worse.
pub fn has_severity(run: &CollectRun, severity: Severity) -> bool {
    diagnostics(run)
        .iter()
        .any(|diagnostic| diagnostic.severity >= severity)
}

/// The portable exact-source keys of every record, in order.
pub fn source_keys(run: &CollectRun) -> Vec<(String, String)> {
    record_events(run)
        .iter()
        .map(|record| {
            (
                record.source.scope.as_str().to_owned(),
                record.source.identity.as_str().to_owned(),
            )
        })
        .collect()
}
