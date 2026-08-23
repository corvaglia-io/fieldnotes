//! Executable conformance cases for the Outlook Contacts Field, driven
//! against the real compiled binary as a child process through the reusable
//! protocol conformance kit.

mod support;

use fieldnotes_field_protocol::codes::{ExitCode, RunOutcome};
use fieldnotes_field_protocol::message::{
    AuthKind, CollectionMode, SnapshotAuthority, TombstoneAuthority,
};

use support::Case;

#[test]
fn describe_reports_a_complete_self_declaration() {
    let case = Case::new("describe");
    let manifest = case.manifest();

    assert_eq!(manifest.field_stem.as_str(), "outlook_contacts");
    assert_eq!(
        manifest
            .property_prefix
            .as_ref()
            .map(|prefix| prefix.as_str()),
        Some("outlook_contacts_")
    );
    assert_eq!(manifest.declared_properties.len(), 3);
    assert_eq!(manifest.capabilities.len(), 1);
    assert_eq!(manifest.capabilities[0].object_kind.as_str(), "contact");
    assert_eq!(manifest.capabilities[0].note_type.as_str(), "contact");
    assert!(manifest.capabilities[0].emits_identity_anchors);
    assert_eq!(manifest.auth.kind, AuthKind::OauthAuthorizationCode);
    assert_eq!(
        manifest.auth.scopes.as_deref(),
        Some(["Contacts.Read".to_owned()].as_slice())
    );
    assert!(
        manifest
            .collection
            .supported_modes
            .contains(&CollectionMode::Incremental)
    );
    assert_eq!(
        manifest.collection.deletion.tombstones,
        TombstoneAuthority::Authoritative,
        "Graph's own delta feed reports removals authoritatively"
    );
    assert_eq!(
        manifest.collection.deletion.snapshot,
        SnapshotAuthority::Unsupported
    );
    let anchors = manifest
        .identity_anchors
        .as_ref()
        .unwrap_or_else(|| panic!("identity anchors must be declared"));
    assert!(
        anchors
            .iter()
            .any(|anchor| anchor.namespace.as_str() == "email")
    );
    assert!(
        anchors
            .iter()
            .any(|anchor| anchor.namespace.as_str() == "phone")
    );
}

#[test]
fn a_collect_run_with_no_credential_grant_is_refused_actionably() {
    let case = Case::new("no-credential");
    let manifest = case.manifest();
    let plan = case.plan_with_tenant_but_no_credential(support::COLLECT_RUN);

    let run = case.collect(&manifest, &plan);

    assert_eq!(
        run.report.outcome,
        RunOutcome::Failed,
        "a Field that requires a protected channel and receives no grant must not silently \
         proceed"
    );
    assert_eq!(run.report.records_accepted, 0);
    assert!(
        run.last_cursor().is_none(),
        "no checkpoint may be committed when the run never obtained credential material"
    );
    assert_eq!(
        run.exit.exit_code(),
        Some(ExitCode::Authentication),
        "the exit code must actionably say re-authentication is needed: {:?}",
        run.exit
    );
}
