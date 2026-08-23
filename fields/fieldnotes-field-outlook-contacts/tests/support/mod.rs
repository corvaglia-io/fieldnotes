//! Shared harness for the Outlook Contacts Field's executable conformance
//! cases.
//!
//! Every case here starts the real `fieldnotes-field-outlook-contacts`
//! binary as a **real child process** and talks to it over real pipes
//! through the reusable conformance kit
//! (`fieldnotes_field_protocol::conformance`), exercising the same process
//! boundary a live sync run does. The conformance kit does not itself serve
//! the protected credential channel -- the per-platform mechanism is `0.1.3`
//! authentication-gate scope, not A2's -- so every case here either needs no
//! credential at all (`describe`) or deliberately exercises this Field's own
//! actionable refusal when no credential grant arrives at all.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use fieldnotes_field_protocol::conformance::{CollectPlan, CollectRun, FieldUnderTest};
use fieldnotes_field_protocol::grammar::PropertyNameToken;
use fieldnotes_field_protocol::message::Manifest;
use fieldnotes_field_protocol::value::PropertyValue;
use fieldnotes_test_support::TempDir;

/// This Field's pinned absolute path, resolved by Cargo.
pub const FIELD_EXECUTABLE: &str = env!("CARGO_BIN_EXE_fieldnotes-field-outlook-contacts");

/// The configured Field ID every case uses, matching the frozen
/// `outlook_contacts_work` fixture.
pub const FIELD_ID: &str = "outlook_contacts_work";

/// A run identifier for a describe run.
pub const DESCRIBE_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000001";

/// A run identifier for a collect run.
pub const COLLECT_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000002";

fn config_key(name: &str) -> PropertyNameToken {
    PropertyNameToken::parse(name).unwrap_or_else(|error| panic!("{name} must be a key: {error}"))
}

/// One conformance case: a pinned executable and a staging directory core
/// owns.
pub struct Case {
    field: FieldUnderTest,
    staging: TempDir,
}

impl Case {
    /// Builds a case with a fresh, empty staging directory.
    pub fn new(label: &str) -> Self {
        let staging = TempDir::new(&format!("outlook-contacts-staging-{label}"))
            .unwrap_or_else(|error| panic!("a staging directory is required: {error}"));
        Case {
            field: FieldUnderTest::new(FIELD_EXECUTABLE)
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

    /// An incremental plan configured with a valid tenant but, deliberately,
    /// no credential grant -- so the only thing under test is this Field's
    /// own actionable refusal when the manifest requires one and none
    /// arrived.
    pub fn plan_with_tenant_but_no_credential(&self, run_id: &str) -> CollectPlan {
        let mut plan = CollectPlan::incremental(run_id, FIELD_ID, self.staging_dir())
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"));
        plan.config.insert(
            config_key("tenant_id"),
            PropertyValue::Text("8d820000-0000-7000-8000-000000000001".to_owned()),
        );
        plan
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
