//! Ruling 4 enforcement, negotiation, and the manifest-change migration, all
//! asserted against the fixture Field running as a real child process.
//!
//! Section 4 of the A2 package is required content, not optional: it closes the
//! gap ruling 4 assigned to A2. Every rule it states is one case here.

mod support;

use fieldnotes_field_protocol::codes::{RejectionCode, RunOutcome};
use fieldnotes_field_protocol::conformance::{manifest_prefix, manifest_snapshot};
use fieldnotes_field_protocol::declared::{Cardinality, ListSemantics, ScalarType};
use fieldnotes_field_protocol::message::{
    AuthKind, CollectionMode, RefreshOwner, SnapshotAuthority, SourceVersionOrdering,
    TombstoneAuthority,
};
use fieldnotes_field_protocol::session::ExitObservation;

use support::{Case, LOCAL_FIELD, MAIL_FIELD};

#[test]
fn a_describe_run_negotiates_and_returns_a_complete_self_declaration() {
    let case = Case::new("describe-local");
    let run = match case.describe(LOCAL_FIELD) {
        Ok(run) => run,
        Err(error) => panic!("the describe run could not start: {error}"),
    };

    assert_eq!(run.exit, ExitObservation::Exited(0));
    let negotiation = run
        .negotiation
        .unwrap_or_else(|| panic!("negotiation must settle: {:?}", run.detail));
    assert_eq!(negotiation.version, 1);
    assert_eq!(
        negotiation.revision, 0,
        "the negotiated revision is the minimum of the two declared revisions"
    );
    assert!(!negotiation.admits_revision(1));

    let manifest = run
        .manifest
        .unwrap_or_else(|| panic!("a manifest is required"));
    assert_eq!(manifest.field_stem.as_str(), "local");
    assert_eq!(manifest_prefix(&manifest), Some("local_"));
    assert_eq!(manifest.declared_properties.len(), 5);
    assert_eq!(manifest.capabilities.len(), 2);

    // The three constants whose only honest value is one value.
    assert_eq!(manifest.auth.writes_to_source, Default::default());
    assert_eq!(
        manifest.source_key.scope_depends_on_field_label,
        Default::default()
    );
    assert_eq!(
        manifest.source_key.stable_across_instances,
        Default::default()
    );
    assert_eq!(
        manifest.source_key.source_version_ordering,
        SourceVersionOrdering::Unsupported,
        "a connector may declare an ordering only if it can prove one"
    );

    // The local Field is add-and-update plus authoritative snapshots.
    assert_eq!(
        manifest.collection.deletion.tombstones,
        TombstoneAuthority::Unsupported
    );
    assert_eq!(
        manifest.collection.deletion.snapshot,
        SnapshotAuthority::Authoritative
    );
    assert!(
        manifest
            .collection
            .supported_modes
            .contains(&CollectionMode::Snapshot),
        "snapshot deletion authority requires the Field to actually support snapshot mode"
    );
    assert_eq!(manifest.auth.kind, AuthKind::None);
    assert_eq!(manifest.auth.refresh_owner, RefreshOwner::NotApplicable);
    assert!(!manifest.auth.protected_channel_required);
}

#[test]
fn a_declared_list_property_carries_its_set_versus_ordered_semantics() {
    let case = Case::new("describe-local");
    let manifest = case.manifest(LOCAL_FIELD);

    let tags = manifest
        .declared_properties
        .iter()
        .find(|declared| declared.name.as_str() == "local_tags")
        .unwrap_or_else(|| panic!("local_tags is declared"));
    assert_eq!(tags.cardinality, Cardinality::List);
    assert_eq!(
        tags.list_semantics,
        Some(ListSemantics::Set),
        "the canonical serializer has to know whether to sort a connector's list"
    );

    // And a scalar declaration carries no list semantics at all.
    let flag = manifest
        .declared_properties
        .iter()
        .find(|declared| declared.name.as_str() == "local_document_flag")
        .unwrap_or_else(|| panic!("local_document_flag is declared"));
    assert_eq!(flag.cardinality, Cardinality::Scalar);
    assert_eq!(flag.list_semantics, None);
    assert_eq!(
        flag.value_type,
        ScalarType::from(fieldnotes_domain::ScalarKind::Text),
        "spelling-based inference is retired: the declaration says text, so 'true' stays text"
    );
}

#[test]
fn a_declared_type_taken_from_the_manifest_beats_spelling_based_inference() {
    // `local_document_flag` arrives as the string "true" and `local_document_date`
    // as "2026-08-20". Under the retired inference rule the first would have
    // become a boolean; under the declaration it is text, and the run completes.
    let case = Case::new("incremental");
    let run = case.incremental(LOCAL_FIELD);
    assert_eq!(run.report.outcome, RunOutcome::Complete);
    assert!(run.rejection.is_none());
}

#[test]
fn every_ruling_four_rejection_case_is_enforced_with_its_own_code() {
    let expectations = [
        // A prefixed property the declaring manifest does not list.
        (
            "property-undeclared",
            RejectionCode::RecordUndeclaredProperty,
        ),
        // A prefixed property belonging to another Field's registered stem.
        (
            "property-foreign-prefix",
            RejectionCode::RecordForeignPrefix,
        ),
        // An unprefixed name outside A1's closed shared registry.
        ("property-unknown", RejectionCode::RecordUnknownProperty),
        // A declared property emitted with the wrong scalar type or cardinality.
        (
            "property-type-mismatch",
            RejectionCode::RecordPropertyTypeMismatch,
        ),
        // A core-owned property a Field must never supply: structurally
        // impossible, so it fails at the schema rather than being overruled.
        ("property-core-owned", RejectionCode::ProtocolSchemaInvalid),
    ];
    for (scenario, expected) in expectations {
        let run = Case::new(scenario).incremental(LOCAL_FIELD);
        assert_eq!(
            run.rejection_code(),
            Some(expected),
            "{scenario} must be rejected as {expected}"
        );
        assert_eq!(run.report.outcome, RunOutcome::Failed, "{scenario}");
    }
}

#[test]
fn a_note_type_outside_the_a1_registry_and_an_undeclared_capability_are_rejected() {
    let run = Case::new("note-type-invalid").incremental(LOCAL_FIELD);
    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::RecordInvalidNoteType),
        "the protocol does not restate A1's eleven types; core validates against the registry"
    );

    let run = Case::new("capability-undeclared").incremental(LOCAL_FIELD);
    assert_eq!(
        run.rejection_code(),
        Some(RejectionCode::ManifestUndeclaredCapability),
        "capability must be declared before it can be exercised"
    );
}

#[test]
fn a_changed_declared_property_type_is_a_migration_not_a_manifest_edit() {
    let stored = manifest_snapshot(&Case::new("describe-local").manifest(LOCAL_FIELD));
    let arriving = manifest_snapshot(&Case::new("describe-retyped-property").manifest(LOCAL_FIELD));

    match stored.check_against(&arriving) {
        Err(migration) => assert_eq!(
            migration.code,
            RejectionCode::ManifestPropertyTypeChanged,
            "core says so instead of retyping notebook data in place"
        ),
        Ok(()) => panic!("a retyped declared property must block the next sync"),
    }

    // The same manifest twice is not a migration.
    let same = manifest_snapshot(&Case::new("describe-local").manifest(LOCAL_FIELD));
    assert!(stored.check_against(&same).is_ok());
}

#[test]
fn the_mail_manifest_declares_anchors_that_never_substitute_for_the_source_key() {
    let case = Case::new("describe-mail");
    let manifest = case.manifest(MAIL_FIELD);

    let anchors = manifest
        .identity_anchors
        .unwrap_or_else(|| panic!("the mail Field declares identity anchors"));
    assert_eq!(anchors.len(), 2);
    for anchor in &anchors {
        assert_eq!(
            anchor.substitutes_for_source_key,
            Default::default(),
            "an anchor may relate graph entities; it never identifies an upstream object"
        );
    }
    // A normalized channel identity must name its normalization rule and
    // version, and an authority-scoped one need not.
    let email = anchors
        .iter()
        .find(|anchor| anchor.namespace.as_str() == "email")
        .unwrap_or_else(|| panic!("the email namespace is declared"));
    assert_eq!(
        email.normalization_rule.as_ref().map(|rule| rule.as_str()),
        Some("email_v1")
    );
    assert_eq!(email.normalization_version, Some(1));

    // The mail Field's authentication requires the protected channel, and the
    // request still carries no material.
    assert_eq!(manifest.auth.kind, AuthKind::OauthAuthorizationCode);
    assert!(manifest.auth.protected_channel_required);
    assert_eq!(manifest.auth.refresh_owner, RefreshOwner::Core);
    assert_eq!(
        manifest.collection.deletion.snapshot,
        SnapshotAuthority::Unsupported,
        "no complete-mailbox snapshot is claimed, so absence alone never deletes"
    );
}

#[test]
fn a_mail_record_carrying_shared_and_declared_properties_is_accepted() {
    // Exercises the whole declared-property path against a second Field: shared
    // registry names, a declared boolean, and a declared set-like list.
    let case = Case::new("redacted-diagnostic");
    let manifest = case.manifest(MAIL_FIELD);
    let run = case.collect(&manifest, &case.plan(MAIL_FIELD));
    assert_eq!(run.report.records_accepted, 1);
    assert!(
        run.rejection.is_none(),
        "the mail record must be accepted: {:?}",
        run.rejection
    );
}
