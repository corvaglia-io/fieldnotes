//! Building this Field's `describe` manifest.
//!
//! Every value here is a fixed constant of this release: capability slices,
//! declared properties, and the source-key derivation are approved with this
//! Field's own release gate rather than by A2 itself, but they are declared
//! once, in one place, so [`crate::describe`] and [`crate::collect`] can
//! never emit something the manifest did not promise.

use fieldnotes_field_protocol::codes::DiagnosticCode;
use fieldnotes_field_protocol::declared::{Cardinality, ListSemantics, ScalarType};
use fieldnotes_field_protocol::grammar::{
    DriverName, DriverVersion, FieldStemToken, ManifestTag, MediumText, NoteTypeToken, ObjectKind,
    PropertyNameToken, PropertyPrefix, ProtocolV1, RuleName, RunId, ShortText,
};
use fieldnotes_field_protocol::message::{
    AuthDeclaration, AuthKind, CapabilitySlice, CollectionDeclaration, CollectionMode,
    DeclaredProperty, DeletionDeclaration, Limitation, Manifest, RefetchSupport, RefreshOwner,
    SnapshotAuthority, SourceKeyDeclaration, SourceVersionOrdering, TombstoneAuthority,
    VersionList,
};
use fieldnotes_field_protocol::version::{PROTOCOL_REVISION, PROTOCOL_VERSION};

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

fn short(text: &str) -> ShortText {
    ShortText::parse(text).unwrap_or_else(|error| panic!("{text:?} must fit ShortText: {error}"))
}

fn medium(text: &str) -> MediumText {
    MediumText::parse(text).unwrap_or_else(|error| panic!("{text:?} must fit MediumText: {error}"))
}

fn rule(text: &str) -> RuleName {
    RuleName::parse(text).unwrap_or_else(|error| panic!("{text:?} must be a rule name: {error}"))
}

/// The connector-prefixed properties this Field may emit.
///
/// Names, types, and list semantics match what the approved A1/A2 review
/// corpus already shows for `local_work` Notes
/// (`tests/fixtures/notebooks/proposed-v1/notes/`), so a notebook this Field
/// produces reads consistently with that corpus. `local_document_date`,
/// `local_document_flag`, and `local_tags` are declared for that
/// consistency and for forward compatibility, but this release does not yet
/// populate them: it has no reliable, filename-independent source for a
/// document's own cover-page date, an opaque source flag, or declared tags
/// without deeper per-format parsing this release does not implement.
fn declared_properties() -> Vec<DeclaredProperty> {
    vec![
        DeclaredProperty {
            name: property_name("local_relative_path"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Path of the collected file relative to the configured root, retained as \
                 display evidence and never used as a path component by core.",
            ),
        },
        DeclaredProperty {
            name: property_name("local_media_type"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Media type detected from file content alone; absent when content sniffing \
                 cannot determine one without relying on the filename.",
            ),
        },
        DeclaredProperty {
            name: property_name("local_document_date"),
            value_type: scalar_type("date"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Document date stated by a text-bearing source document. Not populated by \
                 this release.",
            ),
        },
        DeclaredProperty {
            name: property_name("local_document_flag"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Source-declared document flag retained as opaque text, never coerced to a \
                 boolean. Not populated by this release.",
            ),
        },
        DeclaredProperty {
            name: property_name("local_tags"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::List,
            list_semantics: Some(ListSemantics::Set),
            description: short(
                "Deduplicated tags declared by the configured root. Not populated by this \
                 release.",
            ),
        },
    ]
}

fn capabilities() -> Vec<CapabilitySlice> {
    vec![
        CapabilitySlice {
            object_kind: object_kind(crate::constants::OBJECT_KIND_FILE),
            note_type: note_type(crate::constants::OBJECT_KIND_FILE),
            emits_artifacts: true,
            emits_identity_anchors: false,
            description: short(
                "Generic file collected from the configured root without modification.",
            ),
        },
        CapabilitySlice {
            object_kind: object_kind(crate::constants::OBJECT_KIND_DOCUMENT),
            note_type: note_type(crate::constants::OBJECT_KIND_DOCUMENT),
            emits_artifacts: true,
            emits_identity_anchors: false,
            description: short(
                "Office document or PDF collected from the configured root, whose own \
                 document identity is primary.",
            ),
        },
    ]
}

fn source_key() -> SourceKeyDeclaration {
    SourceKeyDeclaration {
        scope_rule: rule("local_root_id"),
        scope_rule_version: 1,
        scope_shape: short("local-root:<sha256-of-canonical-root-path>"),
        scope_depends_on_field_label: Default::default(),
        identity_shape: short("<object-kind>/<root-relative-posix-path>"),
        identity_includes_object_kind: Default::default(),
        source_version_ordering: SourceVersionOrdering::Unsupported,
        stable_across_instances: Default::default(),
    }
}

fn auth() -> AuthDeclaration {
    AuthDeclaration {
        kind: AuthKind::None,
        credential_profile_required: false,
        protected_channel_required: false,
        scopes: None,
        refresh_owner: RefreshOwner::NotApplicable,
        writes_to_source: Default::default(),
    }
}

fn collection() -> CollectionDeclaration {
    CollectionDeclaration {
        incremental: true,
        cursor_format_version: crate::constants::CURSOR_FORMAT_VERSION,
        supported_modes: vec![CollectionMode::Incremental, CollectionMode::Snapshot],
        window_supported: false,
        // `recollect_targets` handling is not implemented by this release:
        // an explicit maintenance re-collection request is refused by the
        // schema requiring `collection.refetch` to admit it at all, rather
        // than accepted and silently ignored. A full `snapshot` run already
        // covers this Field's practical recovery need -- a lost or invalid
        // cursor, or a retention-policy change -- since it re-examines
        // every file regardless of the cursor.
        refetch: RefetchSupport::Unsupported,
        refetch_note: Some(short(
            "Recollection of previously-reported attachments is not implemented in this \
             release; a snapshot run re-examines every file unconditionally and covers the \
             same recovery need.",
        )),
        deletion: DeletionDeclaration {
            tombstones: TombstoneAuthority::Unsupported,
            snapshot: SnapshotAuthority::Authoritative,
            note: Some(short(
                "A completed walk of the configured root is authoritative for absence inside \
                 that root and nowhere else.",
            )),
        },
    }
}

fn limitations() -> Vec<Limitation> {
    vec![Limitation {
        code: DiagnosticCode::ContentUnsupportedFormat,
        message: medium(
            "Media type detection is content-based only and cannot distinguish the Office \
             and OpenDocument formats, or CSV, from a plain ZIP or arbitrary text (ADR 0008); \
             such files are collected with no declared media type rather than one guessed \
             from the filename.",
        ),
    }]
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
        identity_anchors: Some(Vec::new()),
        auth: auth(),
        collection: collection(),
        limitations: Some(limitations()),
    }
}
