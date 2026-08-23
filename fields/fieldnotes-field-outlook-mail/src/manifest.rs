//! Building this Field's `describe` manifest.
//!
//! Every value here is a fixed constant of this release: the capability slice,
//! the declared connector-prefixed properties, the identity-anchor
//! declaration, the source-key derivation, and the declared deletion
//! authority. They are stated once, in one place, so [`crate::describe`] and
//! [`crate::collect`] can never emit something the manifest did not promise.
//!
//! # What this Field declares about deletion
//!
//! `deletion.tombstones: authoritative` and `deletion.snapshot: unsupported`.
//! A Graph delta collection reports a removed item explicitly, as
//! `@removed`, so a removal is a fact the source stated rather than an absence
//! this Field inferred -- which is exactly what an authoritative tombstone is.
//! A full folder enumeration would be a second, far more expensive deletion
//! path carrying no authority the delta feed does not already carry, so
//! `snapshot` is not among the supported modes and absence never removes a
//! Note here.

use fieldnotes_field_protocol::codes::DiagnosticCode;
use fieldnotes_field_protocol::declared::{Cardinality, ListSemantics, ScalarType};
use fieldnotes_field_protocol::grammar::{
    DriverName, DriverVersion, FieldStemToken, IdentityNamespace, ManifestTag, MediumText,
    NoteTypeToken, ObjectKind, PropertyNameToken, PropertyPrefix, ProtocolV1, RuleName, RunId,
    ShortText,
};
use fieldnotes_field_protocol::message::{
    AuthDeclaration, AuthKind, CapabilitySlice, CollectionDeclaration, CollectionMode,
    DeclaredProperty, DeletionDeclaration, IdentityAnchorDeclaration, IdentityScopeClass,
    Limitation, Manifest, RefetchSupport, RefreshOwner, SnapshotAuthority, SourceKeyDeclaration,
    SourceVersionOrdering, TombstoneAuthority, VersionList,
};
use fieldnotes_field_protocol::version::{PROTOCOL_REVISION, PROTOCOL_VERSION};

/// The normalization rule this Field's mail-address anchors declare, and its
/// version. Both are named here so the value in a record and the declaration
/// in the manifest can never disagree.
pub(crate) const ADDRESS_RULE: &str = "mail_address_lowercase";

/// The version of [`ADDRESS_RULE`].
pub(crate) const ADDRESS_RULE_VERSION: u16 = 1;

/// The identity-anchor namespace mail addresses are emitted under, matching
/// the `email:` entries the frozen fixtures' `identities` lists show.
pub(crate) const ADDRESS_NAMESPACE: &str = "email";

/// Parses a literal known at compile time to be valid, panicking only if that
/// invariant is ever violated by an edit to the literal itself.
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

fn identity_namespace(text: &str) -> IdentityNamespace {
    IdentityNamespace::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be an identity namespace: {error}"))
}

fn short(text: &str) -> ShortText {
    ShortText::parse(text).unwrap_or_else(|error| panic!("{text:?} must fit ShortText: {error}"))
}

fn medium(text: &str) -> MediumText {
    MediumText::parse(text).unwrap_or_else(|error| panic!("{text:?} must fit MediumText: {error}"))
}

fn rule(text: &str) -> RuleName {
    RuleName::parse(text).unwrap_or_else(|error| panic!("{text:?} must be a rule name: {error}"))
}

/// The connector-prefixed properties this Field may emit, with the scalar type
/// and list semantics core enforces on every record (ADR 0006 ruling 4, which
/// A2 section 4 makes enforceable).
///
/// `outlook_mail_importance` and `outlook_mail_internet_message_id` match what
/// the frozen `outlook_mail_work` fixtures already show. Every property here
/// is populated by this release from a field this Field explicitly `$select`s;
/// none is declared speculatively, because adding one later is a Field release
/// change needing no migration, while changing or removing one is a migration.
fn declared_properties() -> Vec<DeclaredProperty> {
    vec![
        DeclaredProperty {
            name: property_name("outlook_mail_categories"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::List,
            list_semantics: Some(ListSemantics::Set),
            description: short(
                "Outlook categories assigned to the message, deduplicated: a set, because \
                 category order carries no meaning upstream.",
            ),
        },
        DeclaredProperty {
            name: property_name("outlook_mail_importance"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Sender-declared importance, retained as the opaque upstream text ('low', \
                 'normal', or 'high') rather than coerced to a number or a flag.",
            ),
        },
        DeclaredProperty {
            name: property_name("outlook_mail_internet_message_id"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "RFC 5322 Message-ID header value, angle brackets included, as the message \
                 itself carries it. Evidence only: never used as source identity.",
            ),
        },
        DeclaredProperty {
            name: property_name("outlook_mail_is_draft"),
            value_type: scalar_type("boolean"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short("Whether the mailbox reports the message as an unsent draft."),
        },
        DeclaredProperty {
            name: property_name("outlook_mail_is_read"),
            value_type: scalar_type("boolean"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Whether the mailbox reports the message as read at collection time.",
            ),
        },
        DeclaredProperty {
            name: property_name("outlook_mail_parent_folder_id"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Opaque Graph identifier of the mail folder the message was in when collected.",
            ),
        },
    ]
}

fn capabilities() -> Vec<CapabilitySlice> {
    vec![CapabilitySlice {
        object_kind: object_kind(crate::constants::OBJECT_KIND_MAIL_MESSAGE),
        note_type: note_type(crate::constants::NOTE_TYPE_MAIL),
        emits_artifacts: true,
        emits_identity_anchors: true,
        description: short(
            "One mail message in the configured mail folder, with its file attachments as \
             original artifacts.",
        ),
    }]
}

fn identity_anchors() -> Vec<IdentityAnchorDeclaration> {
    vec![IdentityAnchorDeclaration {
        namespace: identity_namespace(ADDRESS_NAMESPACE),
        scope_class: IdentityScopeClass::NormalizedChannel,
        normalization_rule: Some(rule(ADDRESS_RULE)),
        normalization_version: Some(ADDRESS_RULE_VERSION),
        substitutes_for_source_key: Default::default(),
        description: short(
            "A mail address the message names, ASCII-lowercased and trimmed. A channel \
             identity for graph entity relation only; never this message's identity.",
        ),
    }]
}

fn source_key() -> SourceKeyDeclaration {
    SourceKeyDeclaration {
        scope_rule: rule("microsoft_graph_tenant"),
        scope_rule_version: 1,
        scope_shape: short("microsoft-graph:tenant/<entra-tenant-guid>"),
        scope_depends_on_field_label: Default::default(),
        identity_shape: short("mail-message/<graph-message-id>"),
        identity_includes_object_kind: Default::default(),
        // Graph's `changeKey` is an opaque token that Graph itself does not
        // document as ordered, so this Field cannot prove an ordering and does
        // not claim one. Divergence with no other evidence is then a visible
        // conflict rather than a silent overwrite (A2 section 7).
        source_version_ordering: SourceVersionOrdering::Unsupported,
        stable_across_instances: Default::default(),
    }
}

fn auth() -> AuthDeclaration {
    AuthDeclaration {
        kind: AuthKind::OauthAuthorizationCode,
        credential_profile_required: true,
        protected_channel_required: true,
        scopes: Some(vec![crate::constants::GRAPH_SCOPE.to_owned()]),
        // Core owns refresh: this Field asks again on the protected channel
        // rather than holding long-lived material, and holds an access token
        // only for the requests of one run.
        refresh_owner: RefreshOwner::Core,
        writes_to_source: Default::default(),
    }
}

fn collection() -> CollectionDeclaration {
    CollectionDeclaration {
        incremental: true,
        cursor_format_version: crate::constants::CURSOR_FORMAT_VERSION,
        // `snapshot` is deliberately absent: see the module documentation.
        supported_modes: vec![CollectionMode::Incremental],
        window_supported: true,
        refetch: RefetchSupport::Supported,
        refetch_note: Some(short(
            "A recollection request refetches each named message by its portable source key and \
             re-evaluates every attachment against the run's current retention policy; it never \
             advances the delta cursor.",
        )),
        deletion: DeletionDeclaration {
            tombstones: TombstoneAuthority::Authoritative,
            snapshot: SnapshotAuthority::Unsupported,
            note: Some(short(
                "A Graph delta collection reports a removed item explicitly, so a removal is a \
                 fact the source stated; absence from any run of this Field is never deletion.",
            )),
        },
    }
}

fn limitations() -> Vec<Limitation> {
    vec![
        Limitation {
            code: DiagnosticCode::ContentUnsupportedFormat,
            message: medium(
                "An HTML mail body is reduced deterministically to plain text (tags removed, \
                 block elements turned into line breaks, character references resolved) rather \
                 than round-tripped as Markdown; the original HTML is not retained.",
            ),
        },
        Limitation {
            code: DiagnosticCode::ContentSkipped,
            message: medium(
                "Only file attachments carry bytes. An item attachment (an embedded message or \
                 event) and a reference attachment (a link to cloud storage) are reported as \
                 not retained with their attachment reference, because neither has original \
                 bytes at the mail endpoint this Field reads. An attachment the listing reports \
                 as a file attachment, but whose bytes Graph then refuses outright, is reported \
                 the same way rather than as an error.",
            ),
        },
        Limitation {
            code: DiagnosticCode::CursorResetRequired,
            message: medium(
                "A Graph delta link longer than the run's cursor bound is dropped rather than \
                 truncated, so the next run starts an unbounded delta collection: over- \
                 collection is safe, and a truncated delta link would resume from an unknown \
                 point.",
            ),
        },
    ]
}

/// Builds this Field's manifest for `run_id`.
///
/// `run_id`, `protocol_version`, `protocol_revision`, and
/// `supported_protocol_versions` are the only members that vary per describe
/// run; every other member is a fixed constant of this release.
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
    use fieldnotes_field_protocol::declared::{Cardinality, ListSemantics};
    use fieldnotes_field_protocol::message::{
        SnapshotAuthority, TombstoneAuthority, Validate as _,
    };

    fn manifest() -> fieldnotes_field_protocol::message::Manifest {
        let run_id = fieldnotes_field_protocol::grammar::RunId::parse(
            "1a4c9f2e-0000-4000-8000-000000000001",
        )
        .unwrap_or_else(|error| panic!("must parse: {error}"));
        build(run_id)
    }

    #[test]
    fn the_manifest_satisfies_its_own_schema() {
        if let Err(error) = manifest().validate() {
            panic!("the manifest must validate: {error:?}");
        }
    }

    #[test]
    fn every_declared_property_carries_this_fields_registered_prefix() {
        for declared in manifest().declared_properties {
            assert!(
                declared.name.as_str().starts_with("outlook_mail_"),
                "{} is not prefixed for this Field",
                declared.name
            );
        }
    }

    #[test]
    fn a_declared_list_states_its_semantics_and_a_scalar_does_not() {
        for declared in manifest().declared_properties {
            match declared.cardinality {
                Cardinality::List => assert_eq!(
                    declared.list_semantics,
                    Some(ListSemantics::Set),
                    "{} is a list and must state its semantics",
                    declared.name
                ),
                Cardinality::Scalar => assert_eq!(
                    declared.list_semantics, None,
                    "{} is a scalar and must not state list semantics",
                    declared.name
                ),
            }
        }
    }

    #[test]
    fn the_two_fixture_properties_are_declared_with_the_fixture_types() {
        let manifest = manifest();
        for name in [
            "outlook_mail_importance",
            "outlook_mail_internet_message_id",
        ] {
            let declared = manifest
                .declared_properties
                .iter()
                .find(|declared| declared.name.as_str() == name)
                .unwrap_or_else(|| panic!("{name} must be declared"));
            assert_eq!(declared.value_type.as_str(), "text");
            assert_eq!(declared.cardinality, Cardinality::Scalar);
        }
    }

    #[test]
    fn deletion_authority_is_tombstones_only() {
        let collection = manifest().collection;
        assert_eq!(
            collection.deletion.tombstones,
            TombstoneAuthority::Authoritative
        );
        assert_eq!(collection.deletion.snapshot, SnapshotAuthority::Unsupported);
        assert!(
            !collection
                .supported_modes
                .contains(&fieldnotes_field_protocol::message::CollectionMode::Snapshot),
            "a Field that cannot reconcile by absence must not offer snapshot mode"
        );
    }

    #[test]
    fn the_declared_scope_is_least_privilege_and_read_only() {
        let auth = manifest().auth;
        assert_eq!(auth.scopes, Some(vec!["Mail.Read".to_owned()]));
        assert!(auth.protected_channel_required);
        assert!(auth.credential_profile_required);
    }

    #[test]
    fn windowing_is_declared_because_it_is_implemented() {
        assert!(manifest().collection.window_supported);
    }
}
