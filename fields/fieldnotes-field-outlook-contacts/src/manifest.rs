//! Building this Field's `describe` manifest.
//!
//! Every value here is a fixed constant of this release: capability slices,
//! declared properties, and the source-key derivation are approved with this
//! Field's own release gate rather than by A2 itself, but they are declared
//! once, in one place, so [`crate::describe`] and [`crate::collect`] can
//! never emit something the manifest did not promise.

use fieldnotes_field_protocol::codes::DiagnosticCode;
use fieldnotes_field_protocol::declared::{Cardinality, ScalarType};
use fieldnotes_field_protocol::grammar::{
    DriverName, DriverVersion, FieldStemToken, IdentityNamespace, ManifestTag, NoteTypeToken,
    ObjectKind, PropertyNameToken, PropertyPrefix, ProtocolV1, RuleName, RunId, ShortText,
};
use fieldnotes_field_protocol::message::{
    AuthDeclaration, AuthKind, CapabilitySlice, CollectionDeclaration, CollectionMode,
    DeclaredProperty, DeletionDeclaration, IdentityAnchorDeclaration, IdentityScopeClass,
    Limitation, Manifest, RefetchSupport, RefreshOwner, SnapshotAuthority, SourceKeyDeclaration,
    SourceVersionOrdering, TombstoneAuthority, VersionList,
};
use fieldnotes_field_protocol::version::{PROTOCOL_REVISION, PROTOCOL_VERSION};

fn scalar_type(text: &str) -> ScalarType {
    ScalarType::parse(text).unwrap_or_else(|| panic!("{text:?} must be an A1 scalar type"))
}

fn field_stem(text: &str) -> FieldStemToken {
    FieldStemToken::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be a field stem: {error}"))
}

fn property_prefix(text: &str) -> PropertyPrefix {
    PropertyPrefix::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be a property prefix: {error}"))
}

fn driver_name(text: &str) -> DriverName {
    DriverName::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be a driver name: {error}"))
}

fn driver_version(text: &str) -> DriverVersion {
    DriverVersion::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be a driver version: {error}"))
}

fn object_kind(text: &str) -> ObjectKind {
    ObjectKind::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be an object kind: {error}"))
}

fn note_type(text: &str) -> NoteTypeToken {
    NoteTypeToken::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be a Note type: {error}"))
}

fn property_name(text: &str) -> PropertyNameToken {
    PropertyNameToken::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be a property name: {error}"))
}

fn short(text: &str) -> ShortText {
    ShortText::parse(text).unwrap_or_else(|error| panic!("{text:?} must fit ShortText: {error}"))
}

fn medium(text: &str) -> fieldnotes_field_protocol::grammar::MediumText {
    fieldnotes_field_protocol::grammar::MediumText::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must fit MediumText: {error}"))
}

fn rule(text: &str) -> RuleName {
    RuleName::parse(text).unwrap_or_else(|error| panic!("{text:?} must be a rule name: {error}"))
}

fn namespace(text: &str) -> IdentityNamespace {
    IdentityNamespace::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be an identity namespace: {error}"))
}

/// The connector-prefixed properties this Field may emit.
///
/// Names and types match the frozen `outlook_contacts_work` contact fixture
/// (`tests/fixtures/notebooks/proposed-v1/notes/`), so a notebook this Field
/// produces reads consistently with that corpus.
fn declared_properties() -> Vec<DeclaredProperty> {
    vec![
        DeclaredProperty {
            name: property_name(crate::constants::PROPERTY_COMPANY_NAME),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short("The contact's stated employer name."),
        },
        DeclaredProperty {
            name: property_name(crate::constants::PROPERTY_JOB_TITLE),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short("The contact's stated job title."),
        },
        DeclaredProperty {
            name: property_name(crate::constants::PROPERTY_CONTACT_KIND),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "'person' or 'organization', from this Field's own source-observable \
                 heuristic (a contact with a company name and no personal name is an \
                 organization); see crate::record. Not a shared A1 property: the graph \
                 layer does not read it, since a registered shared distinction between a \
                 person and an organization contact does not yet exist (see this crate's \
                 final report).",
            ),
        },
    ]
}

fn capabilities() -> Vec<CapabilitySlice> {
    vec![CapabilitySlice {
        object_kind: object_kind(crate::constants::OBJECT_KIND_CONTACT),
        note_type: note_type(crate::constants::OBJECT_KIND_CONTACT),
        emits_artifacts: true,
        emits_identity_anchors: true,
        description: short(
            "A Microsoft Graph contact from the target mailbox's default contacts folder.",
        ),
    }]
}

fn source_key() -> SourceKeyDeclaration {
    SourceKeyDeclaration {
        scope_rule: rule("graph_contacts_tenant"),
        scope_rule_version: 1,
        scope_shape: short("microsoft-graph:tenant/<tenant-id>"),
        scope_depends_on_field_label: Default::default(),
        identity_shape: short("contact/<graph-contact-id>"),
        identity_includes_object_kind: Default::default(),
        // Neither a Graph change key nor a content hash gives a reliable
        // ordering between two independently observed copies of one
        // contact, so divergence with no other evidence becomes a visible
        // conflict at the store rather than a guess (A2 section 7,
        // "Questions the reviewer may want to settle" item 2).
        source_version_ordering: SourceVersionOrdering::Unsupported,
        stable_across_instances: Default::default(),
    }
}

/// The identity-anchor namespaces this Field may emit. See [`crate::identity`]
/// for the normalization this Field actually performs, and the crate's
/// module documentation for why anchors are evidence, never identity
/// resolution.
fn identity_anchors() -> Vec<IdentityAnchorDeclaration> {
    vec![
        IdentityAnchorDeclaration {
            namespace: namespace(crate::constants::ANCHOR_NAMESPACE_EMAIL),
            scope_class: IdentityScopeClass::NormalizedChannel,
            normalization_rule: Some(rule(crate::identity::EMAIL_NORMALIZATION_RULE)),
            normalization_version: Some(crate::identity::EMAIL_NORMALIZATION_VERSION),
            substitutes_for_source_key: Default::default(),
            description: short("A stated email address, lowercased and trimmed."),
        },
        IdentityAnchorDeclaration {
            namespace: namespace(crate::constants::ANCHOR_NAMESPACE_PHONE),
            scope_class: IdentityScopeClass::NormalizedChannel,
            normalization_rule: Some(rule(crate::identity::PHONE_NORMALIZATION_RULE)),
            normalization_version: Some(crate::identity::PHONE_NORMALIZATION_VERSION),
            substitutes_for_source_key: Default::default(),
            description: short(
                "A stated business, home, or mobile phone number, reduced to a leading \
                 '+' and digits only.",
            ),
        },
    ]
}

fn auth() -> AuthDeclaration {
    AuthDeclaration {
        kind: AuthKind::OauthAuthorizationCode,
        credential_profile_required: true,
        protected_channel_required: true,
        scopes: Some(vec![crate::constants::GRAPH_SCOPE.to_owned()]),
        // Core owns refresh: this Field asks again on the channel rather
        // than holding long-lived material (A2 section 12).
        refresh_owner: RefreshOwner::Core,
        writes_to_source: Default::default(),
    }
}

fn collection() -> CollectionDeclaration {
    CollectionDeclaration {
        incremental: true,
        cursor_format_version: crate::constants::CURSOR_FORMAT_VERSION,
        // Graph's delta feed is the only collection shape this Field
        // implements. Its own removal markers already give this Field
        // authoritative tombstone declaration, so a separate authoritative
        // `snapshot` walk is not needed for deletion and is not declared.
        supported_modes: vec![CollectionMode::Incremental],
        window_supported: false,
        refetch: RefetchSupport::Unsupported,
        refetch_note: Some(short(
            "Recollection of a previously-reported photo is not implemented in this \
             release; a lost or invalid cursor already recovers fully, because an absent \
             cursor starts an unbounded initial delta collection that re-examines every \
             contact currently in scope.",
        )),
        deletion: DeletionDeclaration {
            // Graph's delta feed reports a contact's removal explicitly and
            // authoritatively (an `@removed` entry), independent of mode or
            // scope, unlike the local Field's directory walk, whose
            // authority instead comes from a *complete snapshot's* proven
            // absence.
            tombstones: TombstoneAuthority::Authoritative,
            snapshot: SnapshotAuthority::Unsupported,
            note: Some(short(
                "Deletion authority comes from Graph's own delta removal marker, not from \
                 a snapshot walk.",
            )),
        },
    }
}

fn limitations() -> Vec<Limitation> {
    vec![
        Limitation {
            code: DiagnosticCode::CapabilityUnsupportedObject,
            message: medium(
                "Only the target mailbox's default contacts folder is collected in this \
                 release; contacts in another contact folder are out of scope.",
            ),
        },
        Limitation {
            code: DiagnosticCode::ConfigInvalid,
            message: medium(
                "The tenant ID must be supplied via config.tenant_id in this release; it \
                 is not yet derived automatically from the delegated access token.",
            ),
        },
        Limitation {
            code: DiagnosticCode::ConfigInvalid,
            message: medium(
                "config.mailbox is refused in this release: Microsoft Graph exposes no \
                 documented contacts-delta feed for another mailbox without also naming a \
                 specific contact folder, which this Field does not yet accept as \
                 configuration. Only the signed-in user's own contacts are collected.",
            ),
        },
    ]
}

/// Builds this Field's manifest for `run_id`.
#[must_use]
pub(crate) fn build(run_id: RunId) -> Manifest {
    Manifest {
        v: ProtocolV1,
        frame_type: ManifestTag,
        run_id,
        protocol_version: ProtocolV1,
        protocol_revision: PROTOCOL_REVISION,
        supported_protocol_versions: VersionList::new([PROTOCOL_VERSION])
            .unwrap_or_else(|error| panic!("supported_protocol_versions must build: {error}")),
        driver: driver_name(crate::constants::DRIVER_NAME),
        driver_version: driver_version(env!("CARGO_PKG_VERSION")),
        field_stem: field_stem(crate::constants::FIELD_STEM),
        property_prefix: Some(property_prefix(crate::constants::PROPERTY_PREFIX)),
        declared_properties: declared_properties(),
        capabilities: capabilities(),
        source_key: source_key(),
        identity_anchors: Some(identity_anchors()),
        auth: auth(),
        collection: collection(),
        limitations: Some(limitations()),
    }
}

#[cfg(test)]
mod tests {
    use super::build;
    use fieldnotes_field_protocol::grammar::RunId;
    use fieldnotes_field_protocol::message::{SnapshotAuthority, TombstoneAuthority};

    fn run_id() -> RunId {
        RunId::parse("1a4c9f2e-0000-4000-8000-000000000001")
            .unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    #[test]
    fn the_manifest_declares_the_registered_stem_and_prefix() {
        let manifest = build(run_id());
        assert_eq!(manifest.field_stem.as_str(), "outlook_contacts");
        assert_eq!(
            manifest.property_prefix.as_ref().map(|p| p.as_str()),
            Some("outlook_contacts_")
        );
        assert_eq!(manifest.declared_properties.len(), 3);
        assert_eq!(manifest.capabilities.len(), 1);
    }

    #[test]
    fn deletion_authority_is_tombstone_only_matching_graphs_delta_feed() {
        let manifest = build(run_id());
        assert_eq!(
            manifest.collection.deletion.tombstones,
            TombstoneAuthority::Authoritative
        );
        assert_eq!(
            manifest.collection.deletion.snapshot,
            SnapshotAuthority::Unsupported
        );
    }

    #[test]
    fn the_manifest_validates_against_its_own_schema() {
        use fieldnotes_field_protocol::message::Validate;
        build(run_id())
            .validate()
            .unwrap_or_else(|error| panic!("the manifest must validate: {error}"));
    }
}
