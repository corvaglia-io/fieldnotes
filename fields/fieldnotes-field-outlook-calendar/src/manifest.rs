//! Building this Field's `describe` manifest.
//!
//! Every value here is a fixed constant of this release: capability slices,
//! declared properties, identity anchors, and the source-key derivation are
//! approved with this Field's own release gate rather than by A2 itself, but
//! they are declared once, in one place, so [`crate::describe`] and
//! [`crate::collect`]/[`crate::record`] can never emit something the
//! manifest did not promise.

use fieldnotes_field_protocol::codes::DiagnosticCode;
use fieldnotes_field_protocol::declared::{Cardinality, ScalarType};
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

fn identity_namespace(text: &str) -> IdentityNamespace {
    IdentityNamespace::parse(text)
        .unwrap_or_else(|error| panic!("{text:?} must be an identity namespace: {error}"))
}

/// The connector-prefixed properties this Field may emit.
///
/// Names, types, and cardinality match what the approved A1/A2 review corpus
/// already shows for `outlook_calendar_work` Notes
/// (`tests/fixtures/notebooks/proposed-v1/notes/`): `outlook_calendar_event_kind`
/// and `outlook_calendar_response_status` are exactly the frozen fixture's
/// own two prefixed properties. `outlook_calendar_all_day` and
/// `outlook_calendar_is_cancelled` are declared in addition, for the same
/// consistency this release's own record-building depends on: an all-day
/// event's interval collapses to UTC midnight boundaries exactly like a
/// timed one, so without this flag a reader could not tell the two apart
/// from the interval alone, and a cancelled-but-not-deleted event (Graph
/// still reports it; only Graph delta's `@removed` is an actual deletion) is
/// otherwise indistinguishable from an ordinary upsert.
fn declared_properties() -> Vec<DeclaredProperty> {
    vec![
        DeclaredProperty {
            name: property_name("outlook_calendar_event_kind"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Graph's own event.type, snake_cased: single_instance, occurrence, or \
                 exception. This Field collects exclusively through calendarView, which \
                 expands a recurring series into its instances, so series_master is declared \
                 for forward compatibility but never emitted by this release.",
            ),
        },
        DeclaredProperty {
            name: property_name("outlook_calendar_response_status"),
            value_type: scalar_type("text"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "The signed-in mailbox's own RSVP for this event, retained verbatim from \
                 Graph's responseStatus.response (for example accepted, tentativelyAccepted, \
                 declined, or notResponded).",
            ),
        },
        DeclaredProperty {
            name: property_name("outlook_calendar_all_day"),
            value_type: scalar_type("boolean"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Graph's isAllDay flag. An all-day event's started_at/ended_at are still \
                 mapped as a genuine UTC-midnight-bounded interval, never collapsed to a date; \
                 this flag is what lets a reader distinguish that from an ordinary timed event.",
            ),
        },
        DeclaredProperty {
            name: property_name("outlook_calendar_is_cancelled"),
            value_type: scalar_type("boolean"),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
            description: short(
                "Graph's isCancelled flag. A cancelled event is still present in the calendar \
                 and is reported as an ordinary upsert; only Graph delta's own @removed marker \
                 is ever mapped to this Field's authoritative tombstone.",
            ),
        },
    ]
}

fn capabilities() -> Vec<CapabilitySlice> {
    vec![CapabilitySlice {
        object_kind: object_kind(crate::constants::OBJECT_KIND_EVENT),
        note_type: note_type(crate::constants::NOTE_TYPE_EVENT),
        emits_artifacts: false,
        emits_identity_anchors: true,
        description: short(
            "A calendar event -- a single instance, or one expanded occurrence or exception of \
             a recurring series -- collected from the signed-in mailbox's default calendar. \
             Event attachments are not collected by this release.",
        ),
    }]
}

fn source_key() -> SourceKeyDeclaration {
    SourceKeyDeclaration {
        scope_rule: rule("graph_tenant_id"),
        scope_rule_version: 1,
        scope_shape: short("microsoft-graph:tenant/<tenant-guid>"),
        scope_depends_on_field_label: Default::default(),
        identity_shape: short("calendar-event/<immutable-graph-event-id>"),
        identity_includes_object_kind: Default::default(),
        source_version_ordering: SourceVersionOrdering::Unsupported,
        stable_across_instances: Default::default(),
    }
}

/// Identity anchors this Field may emit: normalized mail addresses observed
/// as an event's organizer or attendee. Reuses the same namespace,
/// normalization rule, and version `outlook_mail`'s manifest declares for the
/// same underlying fact (a mail address), so an address anchored from a
/// calendar event and one anchored from a mail message are the exact same
/// anchor rather than two that merely look alike.
fn identity_anchors() -> Vec<IdentityAnchorDeclaration> {
    vec![IdentityAnchorDeclaration {
        namespace: identity_namespace("email"),
        scope_class: IdentityScopeClass::NormalizedChannel,
        normalization_rule: Some(rule("email_v1")),
        normalization_version: Some(1),
        substitutes_for_source_key: Default::default(),
        description: short(
            "Normalized mail addresses observed as an event's organizer or attendee.",
        ),
    }]
}

fn auth() -> AuthDeclaration {
    AuthDeclaration {
        kind: AuthKind::OauthAuthorizationCode,
        credential_profile_required: true,
        protected_channel_required: true,
        scopes: Some(vec![crate::constants::GRAPH_SCOPE.to_owned()]),
        refresh_owner: RefreshOwner::Core,
        writes_to_source: Default::default(),
    }
}

fn collection() -> CollectionDeclaration {
    CollectionDeclaration {
        incremental: true,
        cursor_format_version: crate::constants::CURSOR_FORMAT_VERSION,
        // Snapshot mode is not declared: a windowed calendarView collection
        // can never prove it enumerated the whole calendar, and A2 section 10
        // treats a bounded window as independently disqualifying a
        // completeness claim. This Field relies exclusively on Graph delta's
        // own authoritative @removed markers for deletion.
        supported_modes: vec![CollectionMode::Incremental],
        window_supported: true,
        // Recollection of a previously-reported event is not implemented in
        // this release. A fresh (cursor-less) run already re-establishes the
        // full requested window unconditionally, which covers the same
        // practical recovery need a lost or invalid cursor has.
        refetch: RefetchSupport::Unsupported,
        refetch_note: Some(short(
            "Explicit re-collection of a previously-reported event is not implemented in this \
             release; starting a fresh (cursor-less) run re-establishes the full requested \
             window unconditionally and covers the same recovery need.",
        )),
        deletion: DeletionDeclaration {
            tombstones: TombstoneAuthority::Authoritative,
            snapshot: SnapshotAuthority::Unsupported,
            note: Some(short(
                "Graph delta's @removed marker is an authoritative deletion signal for the \
                 event it names; no complete-calendar snapshot is ever claimed, so absence \
                 alone never authorizes removal.",
            )),
        },
    }
}

fn limitations() -> Vec<Limitation> {
    vec![Limitation {
        code: DiagnosticCode::CapabilityUnsupportedObject,
        message: medium(
            "Event attachments are not collected by this release: only the event's own mapped \
             fields and participants are reported, never its attachment bytes.",
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
        identity_anchors: Some(identity_anchors()),
        auth: auth(),
        collection: collection(),
        limitations: Some(limitations()),
    }
}
