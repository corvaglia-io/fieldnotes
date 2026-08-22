//! Shared harness for the executable conformance cases.
//!
//! Every case here starts the fixture Field as a **real child process** and
//! talks to it over real pipes, because A2 exists to approve a *process*
//! boundary: a test that never starts a process is not evidence about one.
//!
//! Every wait is bounded, and [`fieldnotes_field_protocol::host::FieldProcess`]
//! kills and reaps a child that has not ended, so a non-terminating scenario
//! cannot hang CI.
//!
//! Cargo compiles this module separately into each integration-test binary, so
//! a helper used by one binary is dead code in the other. That is a property of
//! how integration tests are built, not a sign of an unused helper.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use fieldnotes_field_protocol::conformance::{
    CollectPlan, CollectRun, DescribeRun, DriverError, FieldUnderTest,
};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::Manifest;
use fieldnotes_test_support::TempDir;

/// The fixture Field's pinned absolute path.
///
/// Cargo resolves this only inside the fixture package's own tests, which is
/// why the executable cases live here rather than in the protocol crate.
pub const FIXTURE_EXECUTABLE: &str = env!("CARGO_BIN_EXE_fieldnotes-field-fixture");

/// The scenario selector the fixture Field reads.
pub const SCENARIO_VARIABLE: &str = "FIELDNOTES_FIXTURE_SCENARIO";

/// The variable the deliberate-leak scenarios read, for the canary negative
/// control.
pub const LEAK_VARIABLE: &str = "FIELDNOTES_FIXTURE_LEAK_VALUE";

/// A run identifier for a describe run.
pub const DESCRIBE_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000001";

/// A run identifier for a collect run.
pub const COLLECT_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000002";

/// A second run identifier, for the resumed half of a crash case.
pub const RESUMED_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000003";

/// The local Field's configured ID.
pub const LOCAL_FIELD: &str = "local_work";

/// The mail Field's configured ID.
pub const MAIL_FIELD: &str = "outlook_mail_work";

/// The local Field's snapshot scope.
pub const LOCAL_SCOPE: &str = "local-root:reference-library-v1";

/// One conformance case: a pinned executable, a scenario, and a staging
/// directory core owns.
pub struct Case {
    field: FieldUnderTest,
    staging: TempDir,
}

impl Case {
    /// Builds a case for one scenario.
    pub fn new(scenario: &str) -> Self {
        let staging = match TempDir::new(&format!("protocol-{scenario}")) {
            Ok(staging) => staging,
            Err(error) => panic!("a staging directory is required: {error}"),
        };
        Case {
            field: FieldUnderTest::new(FIXTURE_EXECUTABLE)
                .with_env(SCENARIO_VARIABLE, scenario)
                // Bounded so a misbehaving scenario cannot hold the suite.
                .with_idle(Duration::from_secs(10))
                .with_wait(Duration::from_secs(15)),
            staging,
        }
    }

    /// Lowers the effective bounds for this run.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.field = self.field.clone().with_limits(limits);
        self
    }

    /// Shortens the idle and wait bounds, for a scenario that stops making
    /// progress on purpose.
    #[must_use]
    pub fn with_timeouts(mut self, idle: Duration, wait: Duration) -> Self {
        self.field = self.field.clone().with_idle(idle).with_wait(wait);
        self
    }

    /// Registers a secret core holds, for the canary scan and core's redaction
    /// pass.
    #[must_use]
    pub fn with_secret(mut self, secret: &str) -> Self {
        self.field = self.field.clone().with_secret(secret);
        self
    }

    /// Adds an environment entry for the child.
    #[must_use]
    pub fn with_env(mut self, name: &str, value: &str) -> Self {
        self.field = self.field.clone().with_env(name, value);
        self
    }

    /// The staging directory core created and will name in the request.
    pub fn staging_dir(&self) -> PathBuf {
        self.staging.path().to_path_buf()
    }

    /// Runs a describe run.
    pub fn describe(&self, field_id: &str) -> Result<DescribeRun, DriverError> {
        self.field.describe(DESCRIBE_RUN, Some(field_id))
    }

    /// Runs a describe run and requires a manifest.
    pub fn manifest(&self, field_id: &str) -> Manifest {
        match self.describe(field_id) {
            Ok(run) => match run.manifest {
                Some(manifest) => manifest,
                None => panic!(
                    "the describe run produced no manifest: {:?} {:?}",
                    run.rejection, run.detail
                ),
            },
            Err(error) => panic!("the describe run could not start: {error}"),
        }
    }

    /// Runs a collect run.
    pub fn collect(&self, manifest: &Manifest, plan: &CollectPlan) -> CollectRun {
        match self.field.collect(manifest, plan) {
            Ok(run) => run,
            Err(error) => panic!("the collect run could not start: {error}"),
        }
    }

    /// The common case: describe, then one incremental collect run.
    pub fn incremental(&self, field_id: &str) -> CollectRun {
        let manifest = self.manifest(field_id);
        let plan = self.plan(field_id);
        self.collect(&manifest, &plan)
    }

    /// An incremental plan against this case's staging directory.
    pub fn plan(&self, field_id: &str) -> CollectPlan {
        match CollectPlan::incremental(COLLECT_RUN, field_id, self.staging_dir()) {
            Ok(plan) => plan,
            Err(error) => panic!("the plan is invalid: {error}"),
        }
    }

    /// A snapshot plan for one declared scope.
    pub fn snapshot_plan(&self, field_id: &str, scope: &str) -> CollectPlan {
        match CollectPlan::snapshot(COLLECT_RUN, field_id, self.staging_dir(), scope) {
            Ok(plan) => plan,
            Err(error) => panic!("the plan is invalid: {error}"),
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

/// Lowered limits for a bound-enforcement case.
#[must_use]
pub fn limits_with_frame_ceiling(bytes: u64) -> Limits {
    Limits::ceilings().with_max_frame_bytes(bytes)
}
