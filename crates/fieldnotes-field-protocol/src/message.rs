//! Rust data-transfer objects for every message that crosses the Field process
//! boundary in the proposed protocol v1.
//!
//! # How faithfully these mirror the candidate schemas
//!
//! Each struct closes with `deny_unknown_fields`, which is the Rust spelling of
//! the schemas' `additionalProperties: false`: an unknown member is a failed
//! run, never a warning. Required members are plain fields; optional members
//! are `Option`. No member is required-but-nullable: `Manifest::property_prefix`
//! used to be exactly that, wrapped in a `Nullable` newtype, which meant an
//! omitted key and an explicit `null` were indistinguishable failure modes at
//! best and, at worst, an omitted key silently meant "contributes no prefix"
//! while `null` meant something else entirely. `property_prefix` is now
//! absent-or-present like every other optional member: absent means the Field
//! contributes no prefix, and `null` is simply not a value the schema admits.
//!
//! Conditional rules that JSON Schema expresses with `if`/`then`/`not` cannot
//! be expressed by a Rust field list, so each affected type carries a
//! [`Validate::validate`] that enforces them, and the frame decoders
//! ([`FieldEvent::decode`], [`CoreFrame::decode`], [`CredentialFrame::decode`])
//! always run it. Decoding a frame therefore means the same thing as validating
//! it against the schema its `type` selects.
//!
//! One rule is deliberately enforced at validation rather than by a guarded
//! newtype: the artifact handle grammar. Transcript 11 requires a traversal,
//! absolute-path, or reserved-device-name handle to be rejected as
//! `artifact.invalid_handle` **before any filesystem call**, not as
//! `protocol.schema_invalid`, so [`ArtifactRef::validate`] applies the grammar
//! and returns that code.

use serde::{Deserialize, Deserializer, Serialize};

use crate::artifact::ArtifactHandle;
use crate::codes::{DiagnosticCode, RejectionCode};
use crate::declared::{Cardinality, ListSemantics, ScalarType};
use crate::grammar::{
    AttachmentRef, CancelTag, CheckpointTag, CollectRequestTag, ConstFalse, ConstTrue,
    CredentialRequestTag, CredentialResponseTag, Cursor, DescribeRequestTag, DiagnosticTag,
    DriverName, DriverVersion, FieldIdToken, FieldStemToken, GrantId, IdentityNamespace,
    ManifestTag, MarkdownTag, MediaType, MediaTypeMatcher, MediumText, MessageText, NoteTypeToken,
    ObjectKind, OffsetDatetime, ProfileRef, PropertyNameToken, PropertyPrefix, ProtocolV1,
    RecordTag, RuleName, RunId, Sha256Hex, ShortText, SnapshotScope, SourceIdentity, SourceScope,
    SourceVersion, TombstoneTag,
};
use crate::limits::{Deadline, Limits, MAX_ARTIFACT_MEDIA_TYPES};
use crate::value::{ConfigMap, DiagnosticDetail, RecordProperties};
use crate::version::{MAX_PROTOCOL_REVISION, MAX_PROTOCOL_VERSION, ProtocolRevision};

/// A frame that did not satisfy the contract, with the code core rejects it
/// with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    /// The rejection code core reports.
    pub code: RejectionCode,
    /// Why the frame was refused, in reviewable terms.
    pub message: String,
}

impl SchemaError {
    /// A `protocol.schema_invalid` refusal.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        SchemaError {
            code: RejectionCode::ProtocolSchemaInvalid,
            message: message.into(),
        }
    }

    /// A refusal with an explicit code, for the rules whose code the corpus
    /// pins to something other than `protocol.schema_invalid`.
    #[must_use]
    pub fn with_code(code: RejectionCode, message: impl Into<String>) -> Self {
        SchemaError {
            code,
            message: message.into(),
        }
    }
}

impl core::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SchemaError {}

/// The conditional rules a Rust field list cannot express.
pub trait Validate {
    /// Checks the rules the schema states with `if`, `then`, and `not`.
    fn validate(&self) -> Result<(), SchemaError>;
}

/// A non-empty, unique, bounded list of major protocol versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionList(Vec<u16>);

impl VersionList {
    /// The most versions the schema admits in one list.
    pub const MAX_LEN: usize = 16;

    /// Builds a list, refusing an empty, over-long, duplicated, or out-of-range
    /// set.
    pub fn new(versions: impl IntoIterator<Item = u16>) -> Result<Self, SchemaError> {
        let list = VersionList(versions.into_iter().collect());
        list.validate()?;
        Ok(list)
    }

    /// The versions, in declared order.
    #[must_use]
    pub fn as_slice(&self) -> &[u16] {
        &self.0
    }
}

impl Validate for VersionList {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.0.is_empty() {
            return Err(SchemaError::invalid(
                "a version list names at least one major version",
            ));
        }
        if self.0.len() > Self::MAX_LEN {
            return Err(SchemaError::invalid(format!(
                "a version list names at most {} versions",
                Self::MAX_LEN
            )));
        }
        for (index, version) in self.0.iter().enumerate() {
            if *version == 0 || *version > MAX_PROTOCOL_VERSION {
                return Err(SchemaError::invalid(format!(
                    "protocol version {version} is outside the admitted range"
                )));
            }
            if self.0[..index].contains(version) {
                return Err(SchemaError::invalid(format!(
                    "protocol version {version} is listed twice"
                )));
            }
        }
        Ok(())
    }
}

fn bounded<T>(items: &[T], max: usize, what: &str) -> Result<(), SchemaError> {
    if items.len() > max {
        return Err(SchemaError::invalid(format!(
            "{what}: {} entries exceeds the {max} the schema admits",
            items.len()
        )));
    }
    Ok(())
}

/// Checks a scope-token list: at most 64 members, each 1 to 255 bytes.
fn bounded_scopes(scopes: &[String], what: &str) -> Result<(), SchemaError> {
    bounded(scopes, 64, what)?;
    for scope in scopes {
        if scope.is_empty() || scope.len() > 255 {
            return Err(SchemaError::invalid(format!(
                "{what}: a scope token is 1 to 255 bytes"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Core to Field: describe request
// ---------------------------------------------------------------------------

/// The single frame core writes to a Field's standard input during a describe
/// run.
///
/// A describe run carries no credential grant, no cursor, and no staging
/// directory: negotiation happens before any secret is delivered. Unlike
/// [`CollectRequest`], `limits` is optional here: a describe run emits at most
/// one manifest and reads no records, artifacts, or diagnostics, so it has
/// almost nothing to bound, and requiring the full ceiling table would make
/// every Field parse thirteen numbers it cannot act on. When present, core
/// still enforces it as the frame-size ceiling for this run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribeRequest {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: DescribeRequestTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// Every major version core supports.
    pub supported_protocol_versions: VersionList,
    /// The highest additive revision core understands.
    pub max_protocol_revision: ProtocolRevision,
    /// The configured Field ID, when core has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_id: Option<FieldIdToken>,
    /// The effective bounds for this run, when core states them. A describe
    /// run has almost nothing to bound, so this member is optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    /// The run's wall-clock, idle, and grace bounds.
    pub deadline: Deadline,
}

impl Validate for DescribeRequest {
    fn validate(&self) -> Result<(), SchemaError> {
        self.supported_protocol_versions.validate()?;
        if self.max_protocol_revision > MAX_PROTOCOL_REVISION {
            return Err(SchemaError::invalid(
                "max_protocol_revision is outside the admitted range",
            ));
        }
        if let Some(limits) = &self.limits {
            limits
                .validate()
                .map_err(|error| SchemaError::invalid(error.to_string()))?;
        }
        self.deadline
            .validate()
            .map_err(|error| SchemaError::invalid(error.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Field to core: the describe manifest
// ---------------------------------------------------------------------------

/// One connector-prefixed property the Field may emit.
///
/// This is the mechanism ruling 4 assigned to A2: a prefixed property has no
/// A1 registry entry, so its type and list semantics are declared here and
/// spelling-based inference is retired for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredProperty {
    /// The full prefixed property name.
    pub name: PropertyNameToken,
    /// The A1 scalar type of the value.
    pub value_type: ScalarType,
    /// Whether the property is a scalar or a list.
    pub cardinality: Cardinality,
    /// Required for a list, forbidden for a scalar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_semantics: Option<ListSemantics>,
    /// A short human explanation for review.
    pub description: ShortText,
}

impl Validate for DeclaredProperty {
    fn validate(&self) -> Result<(), SchemaError> {
        match (self.cardinality, self.list_semantics) {
            (Cardinality::List, None) => Err(SchemaError::invalid(format!(
                "declared list property {} must state its list semantics: A1 needs to know \
                 whether to sort the list before a connector emits it",
                self.name
            ))),
            (Cardinality::Scalar, Some(_)) => Err(SchemaError::invalid(format!(
                "declared scalar property {} must not state list semantics",
                self.name
            ))),
            _ => Ok(()),
        }
    }
}

/// One source-object slice this release actually supports.
///
/// A capability list documents what a release supports. It is never a claim
/// that the Field covers everything its vendor offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySlice {
    /// The connector-local object-kind token.
    pub object_kind: ObjectKind,
    /// The primary Note type this slice maps to.
    pub note_type: NoteTypeToken,
    /// Whether the slice emits artifacts.
    pub emits_artifacts: bool,
    /// Whether the slice emits identity anchors.
    pub emits_identity_anchors: bool,
    /// A short human explanation for review.
    pub description: ShortText,
}

/// How core may compare two source-version values for this Field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceVersionOrdering {
    /// Divergent content with no other ordering evidence is a visible conflict.
    Unsupported,
    /// Compare numerically, ascending.
    NumericAscending,
    /// Compare lexicographically, ascending.
    LexicographicAscending,
    /// Compare as RFC 3339 instants, ascending.
    Rfc3339InstantAscending,
}

/// How the Field derives the portable exact-source key.
///
/// The three constants are what make the key reviewable before a Field ships:
/// a scope that depended on the user's local Field label would differ between
/// two instances collecting the same mailbox, and exact cross-instance
/// deduplication would silently stop working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceKeyDeclaration {
    /// The named scope-derivation rule.
    pub scope_rule: RuleName,
    /// The rule's version.
    pub scope_rule_version: u16,
    /// A non-secret illustrative shape of the scope, for review.
    pub scope_shape: ShortText,
    /// Must be false.
    pub scope_depends_on_field_label: ConstFalse,
    /// A non-secret illustrative shape of the identity, for review.
    pub identity_shape: ShortText,
    /// Must be true.
    pub identity_includes_object_kind: ConstTrue,
    /// How core may compare two source versions.
    pub source_version_ordering: SourceVersionOrdering,
    /// Must be true.
    pub stable_across_instances: ConstTrue,
}

/// A matching scope class for an identity anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityScopeClass {
    /// Globally exact within its source namespace.
    SourceGlobal,
    /// Exact only within a named upstream authority.
    AuthorityScoped,
    /// Exact only within a named namespace.
    NamespaceScoped,
    /// A normalized channel identity, such as a mail address.
    NormalizedChannel,
    /// Descriptive only; never exact.
    WeakDescriptive,
}

/// An identity-anchor namespace the Field may emit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityAnchorDeclaration {
    /// The anchor namespace.
    pub namespace: IdentityNamespace,
    /// The scope class anchors in this namespace carry.
    pub scope_class: IdentityScopeClass,
    /// The normalization rule, required for a normalized channel identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_rule: Option<RuleName>,
    /// The normalization rule's version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_version: Option<u16>,
    /// Must be false: an anchor never identifies an upstream object.
    pub substitutes_for_source_key: ConstFalse,
    /// A short human explanation for review.
    pub description: ShortText,
}

impl Validate for IdentityAnchorDeclaration {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.scope_class == IdentityScopeClass::NormalizedChannel
            && (self.normalization_rule.is_none() || self.normalization_version.is_none())
        {
            return Err(SchemaError::invalid(format!(
                "identity anchor namespace {} is a normalized channel identity, so it must \
                 declare its normalization rule and version",
                self.namespace
            )));
        }
        Ok(())
    }
}

/// The kind of authentication a Field performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// No credential at all.
    None,
    /// A long-lived API token.
    ApiToken,
    /// HTTP basic credentials.
    Basic,
    /// OAuth device-code flow.
    OauthDeviceCode,
    /// OAuth authorization-code flow.
    OauthAuthorizationCode,
    /// OAuth client-credentials flow.
    OauthClientCredentials,
}

/// Which side renews expiring credential material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOwner {
    /// Core renews and the Field asks again on the channel.
    Core,
    /// The Field renews with material it holds.
    Field,
    /// There is nothing to renew.
    NotApplicable,
}

/// The Field's authentication declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthDeclaration {
    /// The authentication kind.
    pub kind: AuthKind,
    /// Whether a configured credential profile is required.
    pub credential_profile_required: bool,
    /// Whether the protected credential channel is required.
    pub protected_channel_required: bool,
    /// Least-privilege read-only scope tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    /// Which side renews expiring material.
    pub refresh_owner: RefreshOwner,
    /// Must be false: collection is read-only.
    pub writes_to_source: ConstFalse,
}

impl Validate for AuthDeclaration {
    fn validate(&self) -> Result<(), SchemaError> {
        if let Some(scopes) = &self.scopes {
            bounded_scopes(scopes, "auth.scopes")?;
        }
        if self.kind == AuthKind::None
            && (self.credential_profile_required
                || self.protected_channel_required
                || self.refresh_owner != RefreshOwner::NotApplicable)
        {
            return Err(SchemaError::invalid(
                "a Field declaring auth kind 'none' must require no profile, no protected \
                 channel, and no refresh owner: core delivers no grant to it",
            ));
        }
        Ok(())
    }
}

/// Whether explicit tombstone records carry deletion authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TombstoneAuthority {
    /// A delete record removes the current Note.
    Authoritative,
    /// A delete record is rejected.
    Unsupported,
}

/// Whether a completed snapshot carries deletion authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAuthority {
    /// A complete snapshot may remove Notes inside its declared scope.
    Authoritative,
    /// The Field can only ever claim a partial snapshot.
    PartialOnly,
    /// Snapshot reconciliation is not supported at all.
    Unsupported,
}

/// The Field's declared deletion authority.
///
/// Core rejects any deletion signal a Field has not declared authority for. A
/// Field that could acquire the power to delete Notes by emitting a frame would
/// have an unreviewable privilege.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionDeclaration {
    /// Authority for explicit tombstone records.
    pub tombstones: TombstoneAuthority,
    /// Authority for removal by proven absence.
    pub snapshot: SnapshotAuthority,
    /// A short human explanation for review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<ShortText>,
}

/// A collection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionMode {
    /// Collect what changed since the cursor.
    Incremental,
    /// Reconcile a declared scope completely.
    Snapshot,
}

/// Whether a Field can refetch material it already reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefetchSupport {
    /// Refetch is available.
    Supported,
    /// Refetch is available within a stated bound.
    Bounded,
    /// Refetch is not available.
    Unsupported,
}

/// The Field's declared collection behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDeclaration {
    /// Whether incremental collection is supported.
    pub incremental: bool,
    /// The Field's cursor encoding version.
    pub cursor_format_version: u16,
    /// The modes the Field supports.
    pub supported_modes: Vec<CollectionMode>,
    /// Whether the Field honours a bounded window.
    pub window_supported: bool,
    /// Whether the Field can refetch.
    pub refetch: RefetchSupport,
    /// A short human explanation of the refetch bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refetch_note: Option<ShortText>,
    /// The declared deletion authority.
    pub deletion: DeletionDeclaration,
}

impl Validate for CollectionDeclaration {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.supported_modes.is_empty() || self.supported_modes.len() > 2 {
            return Err(SchemaError::invalid(
                "supported_modes names one or two collection modes",
            ));
        }
        if self.supported_modes.len() == 2 && self.supported_modes[0] == self.supported_modes[1] {
            return Err(SchemaError::invalid(
                "supported_modes names each mode at most once",
            ));
        }
        if self.cursor_format_version == 0 {
            return Err(SchemaError::invalid("cursor_format_version starts at 1"));
        }
        if self.deletion.snapshot == SnapshotAuthority::Authoritative
            && !self.supported_modes.contains(&CollectionMode::Snapshot)
        {
            return Err(SchemaError::invalid(
                "snapshot deletion authority requires the Field to actually support snapshot mode",
            ));
        }
        Ok(())
    }
}

/// A known permission, history, or coverage limit a user must be told about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limitation {
    /// The closed-vocabulary code that classifies the limit.
    pub code: DiagnosticCode,
    /// The human explanation.
    pub message: MediumText,
}

/// Deserializes `Manifest::property_prefix` when the key is present.
///
/// `#[serde(default)]` on the field handles a missing key; this function
/// handles a present one, and it deliberately does **not** go through
/// `Option<PropertyPrefix>`'s own `Deserialize` impl, which would read an
/// explicit `null` as `None` and make it indistinguishable from omission.
fn deserialize_present_property_prefix<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PropertyPrefix>, D::Error> {
    PropertyPrefix::deserialize(deserializer).map(Some)
}

/// The single manifest frame a Field emits in response to a describe request.
///
/// The manifest is the Field's complete self-declaration and the only thing
/// core consults about a Field's powers. Capability, deletion authority, and
/// snapshot authority must be declared here **before** they can be exercised;
/// declaration first, enforcement on every frame, is the only ordering that
/// makes "absence is not deletion" enforceable rather than aspirational.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: ManifestTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The single version the Field selected from what core offered.
    pub protocol_version: ProtocolV1,
    /// The Field's own additive revision.
    pub protocol_revision: ProtocolRevision,
    /// Every version the Field supports.
    pub supported_protocol_versions: VersionList,
    /// The connector driver name.
    pub driver: DriverName,
    /// The connector driver version.
    pub driver_version: DriverVersion,
    /// The registered A1 Field stem.
    pub field_stem: FieldStemToken,
    /// The registered property prefix. Absent for a Field contributing none;
    /// never `null`, which the schema does not admit.
    ///
    /// `#[serde(default)]` supplies `None` only when the *key* is missing.
    /// When the key is present, `deserialize_present_property_prefix` runs
    /// [`PropertyPrefix`]'s own deserializer, which fails on `null` exactly
    /// as it fails on any other non-string value: plain `Option<T>` would
    /// instead read an explicit `null` as `None`, indistinguishable from
    /// omission, which is precisely the ambiguity this member exists to
    /// avoid.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_property_prefix"
    )]
    pub property_prefix: Option<PropertyPrefix>,
    /// The exhaustive prefixed-property declaration.
    pub declared_properties: Vec<DeclaredProperty>,
    /// The source-object slices this release supports.
    pub capabilities: Vec<CapabilitySlice>,
    /// How the Field derives the portable exact-source key.
    pub source_key: SourceKeyDeclaration,
    /// The identity-anchor namespaces the Field may emit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_anchors: Option<Vec<IdentityAnchorDeclaration>>,
    /// The Field's authentication declaration.
    pub auth: AuthDeclaration,
    /// The Field's collection behavior.
    pub collection: CollectionDeclaration,
    /// Known limits a user must be told about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limitations: Option<Vec<Limitation>>,
}

impl Manifest {
    /// Whether `object_kind` is a capability slice this manifest declares.
    #[must_use]
    pub fn declares_object_kind(&self, object_kind: &str) -> bool {
        self.capabilities
            .iter()
            .any(|slice| slice.object_kind.as_str() == object_kind)
    }

    /// The `note_type` the manifest declares for `object_kind`, when that
    /// capability slice is declared at all.
    ///
    /// A record whose own `note_type` differs from this is rejected: without
    /// that check, a slice's declared `note_type` would be decoration rather
    /// than a bound a Field must actually honour.
    #[must_use]
    pub fn declared_note_type_for(&self, object_kind: &str) -> Option<&NoteTypeToken> {
        self.capabilities
            .iter()
            .find(|slice| slice.object_kind.as_str() == object_kind)
            .map(|slice| &slice.note_type)
    }

    /// Whether this Field's manifest declares enough refetch capability to
    /// honor a `collect_request.recollect_targets` request at all.
    ///
    /// No new manifest member was added for recollection: "can this Field
    /// refetch material it already reported" is exactly the question
    /// [`CollectionDeclaration::refetch`] already answers, per ADR 0007.
    #[must_use]
    pub fn supports_recollect(&self) -> bool {
        self.collection.refetch != RefetchSupport::Unsupported
    }
}

impl Validate for Manifest {
    fn validate(&self) -> Result<(), SchemaError> {
        self.supported_protocol_versions.validate()?;
        if self.protocol_revision > MAX_PROTOCOL_REVISION {
            return Err(SchemaError::invalid(
                "protocol_revision is outside the admitted range",
            ));
        }
        bounded(&self.declared_properties, 512, "declared_properties")?;
        for declared in &self.declared_properties {
            declared.validate()?;
        }
        if self.capabilities.is_empty() {
            return Err(SchemaError::invalid(
                "a manifest declares at least one capability slice: a Field that supports \
                 nothing has nothing to negotiate",
            ));
        }
        bounded(&self.capabilities, 64, "capabilities")?;
        if let Some(anchors) = &self.identity_anchors {
            bounded(anchors, 64, "identity_anchors")?;
            for anchor in anchors {
                anchor.validate()?;
            }
        }
        if let Some(limitations) = &self.limitations {
            bounded(limitations, 64, "limitations")?;
        }
        if self.source_key.scope_rule_version == 0 {
            return Err(SchemaError::invalid("scope_rule_version starts at 1"));
        }
        self.auth.validate()?;
        self.collection.validate()
    }
}

// ---------------------------------------------------------------------------
// Core to Field: collection request and cancellation
// ---------------------------------------------------------------------------

/// A bounded collection window.
///
/// A windowed run is never a complete snapshot and can never authorize deletion
/// by absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// The start of the window.
    pub from: OffsetDatetime,
    /// The end of the window.
    pub to: OffsetDatetime,
}

/// The mechanism a protected credential channel uses.
///
/// The channel is named in the request rather than in the environment, and it
/// is separate from standard input, output, and error. Both admitted kinds
/// are path-based. ADR 0013 removed the two descriptor-passing kinds this
/// enum used to admit, `inherited_fd` and `duplicated_handle`: turning either
/// into an I/O object needs `unsafe`, and this workspace sets
/// `unsafe_code = "forbid"` workspace-wide with no per-crate override, so
/// those kinds named a mechanism this project cannot build. They are not
/// merely refused at run time -- this type no longer has variants for
/// them, so a frame naming either is unrepresentable rather than rejected.
/// Do not re-add them without first resolving that contradiction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// A Unix domain socket path.
    UnixSocketPath,
    /// A Windows named pipe.
    WindowsNamedPipe,
}

/// How the Field reaches core to obtain credential material.
///
/// Modelled as one flat object with a `kind` discriminator, exactly as the
/// schema does, rather than as a Rust enum: an internally tagged enum would
/// silently accept members the schema forbids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelDescriptor {
    /// The channel mechanism.
    pub kind: ChannelKind,
    /// The socket or pipe path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Validate for ChannelDescriptor {
    fn validate(&self) -> Result<(), SchemaError> {
        // Both admitted kinds are path-based (ADR 0013), so there is no
        // per-kind shape to branch on any more: every `ChannelKind` this type
        // can represent names its endpoint the same way.
        match &self.path {
            Some(path) if !path.is_empty() && path.len() <= 4096 => Ok(()),
            _ => Err(SchemaError::invalid(
                "a socket or pipe channel names a path of 1 to 4096 bytes",
            )),
        }
    }
}

/// A credential reference, never a value.
///
/// The grant authorizes nothing outside this run's channel and is not source
/// credential material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialGrant {
    /// The non-secret name of the configured credential profile.
    pub profile_ref: ProfileRef,
    /// The single-use per-run channel authorization token.
    pub grant_id: GrantId,
    /// How to reach core for material.
    pub channel: ChannelDescriptor,
    /// The instant after which core refuses the grant.
    pub expires_at: OffsetDatetime,
    /// The granted scopes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

impl Validate for CredentialGrant {
    fn validate(&self) -> Result<(), SchemaError> {
        if let Some(scopes) = &self.scopes {
            bounded_scopes(scopes, "credential.scopes")?;
        }
        self.channel.validate()
    }
}

/// One previously-collected source object, named by its portable exact-source
/// key alone, that core asks a Field to explicitly recollect.
///
/// Deliberately minimal: recollection is a maintenance operation over objects
/// the Field already reported, so nothing beyond the key that identifies them
/// is needed. The Field re-evaluates every attachment of the named object
/// against the *currently* effective retention policy and re-emits an
/// ordinary upsert record, which core installs in place under the object's
/// existing Note ID (A1 section 7's current-state update rule).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecollectTarget {
    /// The upstream authority or account scope.
    pub scope: SourceScope,
    /// The object identity within that scope.
    pub identity: SourceIdentity,
}

/// The single frame core writes to a Field's standard input during a collect
/// run.
///
/// Two omissions are deliberate. It carries no credential material, and it
/// carries no notebook instance ID: producer provenance is core's, the Field
/// has no use for it, and not sending it means a Field cannot embed it in a
/// cursor, a diagnostic, or an upstream request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectRequest {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: CollectRequestTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The negotiated major version.
    pub protocol_version: ProtocolV1,
    /// The negotiated revision, the minimum of the two declared revisions.
    pub protocol_revision: ProtocolRevision,
    /// The configured Field ID.
    pub field_id: FieldIdToken,
    /// The collection mode.
    pub mode: CollectionMode,
    /// The last committed cursor, when one is replayable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// The format version the replayed cursor was stored at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_format_version: Option<u16>,
    /// An optional bounded window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Window>,
    /// The scope a snapshot run claims to cover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_scope: Option<SnapshotScope>,
    /// Non-secret connector configuration.
    pub config: ConfigMap,
    /// A credential reference, when the manifest declared a requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialGrant>,
    /// The absolute, core-created, per-run artifact staging directory.
    pub artifact_staging_dir: String,
    /// The effective bounds for this run.
    pub limits: Limits,
    /// The run's wall-clock, idle, and grace bounds.
    pub deadline: Deadline,
    /// The effective media-type retention include set, in addition to the
    /// size threshold `limits.max_artifact_bytes` already states. Required
    /// for the same reason `limits` is: a well-behaved Field self-polices
    /// against a bound it is told rather than discovering it by being
    /// rejected. Never derived from a source filename, and orthogonal to
    /// A1's frozen media-type-to-extension registry, which governs naming a
    /// retained original rather than whether it is retained at all.
    pub artifact_media_types: Vec<MediaTypeMatcher>,
    /// A bounded list of previously-collected source objects, named by their
    /// portable exact-source key alone, that core asks the Field to
    /// explicitly recollect: refetch current metadata and re-evaluate every
    /// known attachment against the currently effective retention policy,
    /// regardless of the last committed cursor. Present only for an explicit
    /// maintenance re-collection run (ADR 0007), never for an ordinary
    /// incremental or snapshot sync. When present, `cursor` and `window` are
    /// absent: recollection is scoped exactly to the named targets, not to a
    /// cursor-bounded incremental range, and it is never combined with
    /// `snapshot` mode, which already reconciles everything in its scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recollect_targets: Option<Vec<RecollectTarget>>,
}

impl Validate for CollectRequest {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.protocol_revision > MAX_PROTOCOL_REVISION {
            return Err(SchemaError::invalid(
                "protocol_revision is outside the admitted range",
            ));
        }
        if self.cursor.is_some() && self.cursor_format_version.is_none() {
            return Err(SchemaError::invalid(
                "a replayed cursor must state the format version it was produced at, or core \
                 would hand a Field a token it may misread",
            ));
        }
        match (self.mode, self.snapshot_scope.as_ref()) {
            (CollectionMode::Snapshot, None) => {
                return Err(SchemaError::invalid(
                    "snapshot mode must name the scope it claims to cover",
                ));
            }
            (CollectionMode::Incremental, Some(_)) => {
                return Err(SchemaError::invalid(
                    "snapshot_scope belongs to snapshot mode only",
                ));
            }
            _ => {}
        }
        if self.artifact_staging_dir.is_empty() || self.artifact_staging_dir.len() > 4096 {
            return Err(SchemaError::invalid(
                "artifact_staging_dir is 1 to 4096 bytes",
            ));
        }
        if self.artifact_media_types.is_empty() {
            return Err(SchemaError::invalid(
                "artifact_media_types names at least one included media type or wildcard",
            ));
        }
        if self.artifact_media_types.len() > MAX_ARTIFACT_MEDIA_TYPES {
            return Err(SchemaError::invalid(format!(
                "artifact_media_types names at most {MAX_ARTIFACT_MEDIA_TYPES} entries"
            )));
        }
        if let Some(credential) = &self.credential {
            credential.validate()?;
        }
        if let Some(cursor) = &self.cursor
            && cursor.as_str().len() > usize::try_from(self.limits.max_cursor_bytes).unwrap_or(4096)
        {
            return Err(SchemaError::invalid(
                "the replayed cursor exceeds the run's cursor bound",
            ));
        }
        if let Some(targets) = &self.recollect_targets {
            if targets.is_empty() {
                return Err(SchemaError::invalid(
                    "recollect_targets, when present, names at least one target",
                ));
            }
            if targets.len() > MAX_ARTIFACT_MEDIA_TYPES {
                return Err(SchemaError::invalid(format!(
                    "recollect_targets names at most {MAX_ARTIFACT_MEDIA_TYPES} entries in one \
                     run; a larger backlog is split across multiple runs"
                )));
            }
            if self.cursor.is_some() || self.window.is_some() {
                return Err(SchemaError::invalid(
                    "recollect_targets is scoped exactly to its named targets, not to a \
                     cursor-bounded or windowed range, so cursor and window must be absent",
                ));
            }
            if self.mode == CollectionMode::Snapshot {
                return Err(SchemaError::invalid(
                    "recollect_targets is never combined with snapshot mode, which already \
                     reconciles everything inside its declared scope",
                ));
            }
        }
        self.limits
            .validate()
            .map_err(|error| SchemaError::invalid(error.to_string()))?;
        self.deadline
            .validate()
            .map_err(|error| SchemaError::invalid(error.to_string()))
    }
}

/// Why core cancelled the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    /// The user interrupted the sync.
    UserRequested,
    /// The run hit its deadline.
    Deadline,
    /// Core is shutting down.
    Shutdown,
    /// A declared bound was exceeded.
    LimitExceeded,
    /// Core itself failed.
    CoreError,
}

/// Core's cooperative cancellation request.
///
/// The Field stops starting new work, may emit one final checkpoint for
/// material it already emitted, and exits with the cancellation code. A
/// cancelled run is never complete, so it can never authorize a removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cancel {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: CancelTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// Why the run is being cancelled.
    pub reason: CancelReason,
    /// Seconds the Field has to exit before core terminates it.
    pub grace_seconds: u32,
}

impl Validate for Cancel {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.grace_seconds == 0 || self.grace_seconds > Deadline::MAX_CANCEL_GRACE_SECONDS {
            return Err(SchemaError::invalid(
                "a cancellation grace period is between 1 and 120 seconds",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Field to core: record events
// ---------------------------------------------------------------------------

/// The portable exact-source key plus optional non-identity source metadata.
///
/// `(scope, identity)` is the only thing that collapses independently collected
/// copies of one upstream object, and the only thing core reconciles Notes by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    /// The upstream authority or account scope.
    pub scope: SourceScope,
    /// The object identity within that scope.
    pub identity: SourceIdentity,
    /// An opaque source-supplied version token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<SourceVersion>,
    /// A source URL, as display evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The identity of a parent source object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_identity: Option<SourceIdentity>,
}

impl SourceRef {
    /// The pair core reconciles by.
    #[must_use]
    pub fn key(&self) -> (&str, &str) {
        (self.scope.as_str(), self.identity.as_str())
    }
}

impl Validate for SourceRef {
    fn validate(&self) -> Result<(), SchemaError> {
        if let Some(url) = &self.url
            && url.len() > 4096
        {
            return Err(SchemaError::invalid("source.url is at most 4096 bytes"));
        }
        Ok(())
    }
}

/// Deterministically normalized source evidence for the Note body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    /// The body format, which v1 fixes at Markdown.
    pub format: MarkdownTag,
    /// The body text. Core applies the A1 body normalization and computes the
    /// content hash; a Field never supplies a content hash.
    pub text: String,
}

/// Whether an artifact's bytes were transferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// The Field wrote the exact original bytes into the staging directory.
    Staged,
    /// The Field knows the artifact by digest and transferred no bytes.
    DigestOnly,
    /// The Field saw this artifact at the source and declined to retain it,
    /// per the retention threshold [`Limits::max_artifact_bytes`] states in
    /// the collection request. Core stores no bytes and computes no digest
    /// for it: the record is still accepted, and the Note it becomes is still
    /// created, without those bytes. "Stays at source" is a policy decision,
    /// never a failure.
    NotRetained,
}

/// An artifact's role in its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    /// Maps into the A1 role-ordered attachments list.
    Attachment,
    /// The source object's own bytes.
    Original,
    /// Bytes embedded in the source object's content.
    Embedded,
    /// Any other role.
    Other,
}

/// One original-byte reference carried by a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    /// Whether the bytes were staged or referenced by digest.
    pub kind: ArtifactKind,
    /// The staging name, for a staged reference.
    ///
    /// Held as an unvalidated string so that a hostile spelling is refused with
    /// [`RejectionCode::ArtifactInvalidHandle`] by [`ArtifactRef::validate`],
    /// which is the code transcript 11 pins, rather than as a generic schema
    /// failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// The Field's declared digest over the exact original bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Sha256Hex>,
    /// The declared byte length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
    /// The declared media type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<MediaType>,
    /// The artifact's role.
    pub role: ArtifactRole,
    /// Display metadata only. Never a path component and never the stored
    /// extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_filename: Option<String>,
    /// A stable connector-namespaced upstream attachment reference. Required
    /// exactly for [`ArtifactKind::NotRetained`] and forbidden otherwise: a
    /// staged or digest-only reference already has an A1 artifact ID, but a
    /// declined artifact has no bytes and no digest, so this is the only
    /// stable identity it carries. Core projects it onto the shared
    /// `skipped_attachments` Note property; A2 does not otherwise interpret
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_ref: Option<AttachmentRef>,
}

impl ArtifactRef {
    /// The parsed handle, for a staged reference.
    pub fn parsed_handle(&self) -> Result<ArtifactHandle, SchemaError> {
        let Some(handle) = &self.handle else {
            return Err(SchemaError::invalid(
                "a staged artifact reference names the handle it was staged under",
            ));
        };
        ArtifactHandle::parse(handle).map_err(|error| {
            SchemaError::with_code(RejectionCode::ArtifactInvalidHandle, error.to_string())
        })
    }
}

impl Validate for ArtifactRef {
    fn validate(&self) -> Result<(), SchemaError> {
        if let Some(filename) = &self.source_filename
            && (filename.is_empty() || filename.len() > 255)
        {
            return Err(SchemaError::invalid(
                "source_filename is 1 to 255 bytes of display evidence",
            ));
        }
        match self.kind {
            ArtifactKind::Staged => {
                // The grammar check comes first, so a traversal handle is
                // refused before anything touches a filesystem.
                let _handle = self.parsed_handle()?;
                if self.byte_length.is_none() {
                    return Err(SchemaError::invalid(
                        "a staged artifact reference declares its byte length so core can bound \
                         the read",
                    ));
                }
                if let Some(length) = self.byte_length
                    && length > 536_870_912
                {
                    return Err(SchemaError::invalid(
                        "byte_length exceeds the frozen single-artifact transfer ceiling",
                    ));
                }
                if self.attachment_ref.is_some() {
                    return Err(SchemaError::invalid(
                        "a staged reference already has an A1 artifact ID from core's own \
                         digest, so it carries no attachment_ref",
                    ));
                }
                Ok(())
            }
            ArtifactKind::DigestOnly => {
                if self.sha256.is_none() {
                    return Err(SchemaError::invalid(
                        "a digest-only reference is nothing but its digest, so the digest is \
                         required",
                    ));
                }
                if self.handle.is_some() || self.byte_length.is_some() {
                    return Err(SchemaError::invalid(
                        "a digest-only reference transferred no bytes, so it carries no handle \
                         and no byte length",
                    ));
                }
                if self.attachment_ref.is_some() {
                    return Err(SchemaError::invalid(
                        "a digest-only reference already has an A1 artifact ID from its digest, \
                         so it carries no attachment_ref",
                    ));
                }
                Ok(())
            }
            ArtifactKind::NotRetained => {
                if self.handle.is_some() || self.sha256.is_some() {
                    return Err(SchemaError::invalid(
                        "a not_retained reference transfers no bytes and computes no digest, so \
                         it carries no handle and no declared digest",
                    ));
                }
                if self.attachment_ref.is_none() {
                    return Err(SchemaError::invalid(
                        "a not_retained reference carries no bytes and no digest, so \
                         attachment_ref is the only stable identity it has; core needs it to \
                         record the declined attachment in skipped_attachments",
                    ));
                }
                // byte_length is deliberately unbounded here: it is the
                // Field's advisory account of how large the declined bytes
                // are, and being over the transfer ceiling is the whole
                // reason the Field declined to stage them.
                Ok(())
            }
        }
    }
}

/// An anchor's role in its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AnchorRole {
    /// The record's subject.
    Subject,
    /// The sender.
    Sender,
    /// A recipient.
    Recipient,
    /// The organizer.
    Organizer,
    /// An attendee.
    Attendee,
    /// A participant.
    Participant,
    /// Someone mentioned.
    Mentioned,
    /// An assignee.
    Assignee,
    /// A reporter.
    Reporter,
    /// An owner.
    Owner,
    /// Any other role.
    Other,
}

/// A source-declared person, account, organization, or artifact anchor.
///
/// An anchor may relate graph entities. It never identifies an upstream object,
/// is never used for Note reconciliation, and never makes two source objects
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityAnchor {
    /// The anchor namespace.
    pub namespace: IdentityNamespace,
    /// The normalized anchor value.
    pub value: String,
    /// The matching scope class.
    pub scope_class: IdentityScopeClass,
    /// The scope the anchor is exact within.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SourceScope>,
    /// The normalization rule for a normalized channel identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_rule: Option<RuleName>,
    /// The normalization rule's version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_version: Option<u16>,
    /// The anchor's role in this record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<AnchorRole>,
}

impl Validate for IdentityAnchor {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.value.is_empty() || self.value.len() > 1024 {
            return Err(SchemaError::invalid("an anchor value is 1 to 1024 bytes"));
        }
        match self.scope_class {
            IdentityScopeClass::AuthorityScoped | IdentityScopeClass::NamespaceScoped
                if self.scope.is_none() =>
            {
                Err(SchemaError::invalid(format!(
                    "an {} anchor must name the scope it is exact within",
                    match self.scope_class {
                        IdentityScopeClass::AuthorityScoped => "authority-scoped",
                        _ => "namespace-scoped",
                    }
                )))
            }
            IdentityScopeClass::NormalizedChannel
                if self.normalization_rule.is_none() || self.normalization_version.is_none() =>
            {
                Err(SchemaError::invalid(
                    "a normalized channel identity must name its normalization rule and version",
                ))
            }
            _ => Ok(()),
        }
    }
}

/// Known content loss for one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Integrity {
    /// Whether the collected content was damaged.
    pub damaged: bool,
    /// Whether the collected content was truncated.
    pub truncated: bool,
    /// The measured number of lost characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost_characters: Option<u64>,
}

/// Whether a record asserts current state or an authoritative deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// Current mapped state for the source object.
    Upsert,
    /// An explicit authoritative deletion.
    Delete,
}

/// One collected source object as a normalized source envelope: post-mapping
/// and pre-serialization.
///
/// The Field has already done the work only it can do — mapping vendor
/// structures onto Fieldnotes vocabulary — and none of the work only core may
/// do. A record is never a rendered Note, never carries a notebook path or
/// filename, and never carries a path core would treat as a destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordEvent {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: RecordTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The per-run monotonic sequence number.
    pub seq: u64,
    /// Whether this asserts current state or a deletion.
    pub change: Change,
    /// The portable exact-source key.
    pub source: SourceRef,
    /// The declared capability slice this record belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_kind: Option<ObjectKind>,
    /// The primary Note type candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_type: Option<NoteTypeToken>,
    /// The event instant, with an explicit offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<OffsetDatetime>,
    /// Flat property candidates keyed by A1 property names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<RecordProperties>,
    /// Normalized source evidence for the Note body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
    /// Original-byte references, in role order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<ArtifactRef>>,
    /// Structured identity anchors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_anchors: Option<Vec<IdentityAnchor>>,
    /// Known content loss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<Integrity>,
    /// Deletion authority. Present only on a delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<TombstoneTag>,
    /// When the Field observed the deletion. Present only on a delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<OffsetDatetime>,
}

impl Validate for RecordEvent {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.seq == 0 || self.seq > 1_000_000 {
            return Err(SchemaError::invalid(
                "a sequence number starts at 1 and stops at 1000000",
            ));
        }
        self.source.validate()?;
        match self.change {
            Change::Upsert => {
                if self.authority.is_some() || self.observed_at.is_some() {
                    return Err(SchemaError::invalid(
                        "an upsert carries mapped current state and no deletion authority",
                    ));
                }
                if self.object_kind.is_none()
                    || self.note_type.is_none()
                    || self.occurred_at.is_none()
                    || self.body.is_none()
                {
                    return Err(SchemaError::invalid(
                        "an upsert carries its object kind, Note type, event instant, and body",
                    ));
                }
            }
            Change::Delete => {
                if self.object_kind.is_none()
                    || self.authority.is_none()
                    || self.observed_at.is_none()
                {
                    return Err(SchemaError::invalid(
                        "a delete carries its object kind, its declared authority, and when the \
                         deletion was observed",
                    ));
                }
                if self.note_type.is_some()
                    || self.occurred_at.is_some()
                    || self.properties.is_some()
                    || self.body.is_some()
                    || self.artifacts.is_some()
                    || self.identity_anchors.is_some()
                    || self.integrity.is_some()
                {
                    return Err(SchemaError::invalid(
                        "a delete carries no content at all, so a deletion can never be confused \
                         with an empty or partial collection result",
                    ));
                }
            }
        }
        if let Some(body) = &self.body
            && body.text.len() > 1_048_576
        {
            return Err(SchemaError::invalid(
                "body.text exceeds the frozen 1 MiB body ceiling",
            ));
        }
        if let Some(artifacts) = &self.artifacts {
            bounded(artifacts, 64, "artifacts")?;
            for artifact in artifacts {
                artifact.validate()?;
            }
        }
        if let Some(anchors) = &self.identity_anchors {
            bounded(anchors, 256, "identity_anchors")?;
            for anchor in anchors {
                anchor.validate()?;
            }
        }
        if let Some(integrity) = &self.integrity
            && integrity.lost_characters.unwrap_or(0) > 1_099_511_627_776
        {
            return Err(SchemaError::invalid(
                "lost_characters stops at 1099511627776",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Field to core: checkpoint events
// ---------------------------------------------------------------------------

/// Whether a snapshot run enumerated its whole declared scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotState {
    /// The Field's explicit claim that it enumerated the whole declared scope.
    Complete,
    /// The Field did not finish.
    Partial,
}

/// A snapshot-mode completeness claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotClaim {
    /// The scope this claim covers, which must equal the request's scope.
    pub scope: SnapshotScope,
    /// Whether the enumeration completed.
    pub state: SnapshotState,
    /// How many objects the Field enumerated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects_enumerated: Option<u64>,
}

/// The window portion a checkpoint accounts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointWindow {
    /// The start of the covered window.
    pub from: OffsetDatetime,
    /// The end of the covered window.
    pub through: OffsetDatetime,
}

/// A Field's offer of a resume point.
///
/// The Field proposes; core commits. Core commits the cursor only after every
/// record at or below `covers_record_seq_through` has reached durable current
/// state and the store's durability barrier has returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointEvent {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: CheckpointTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The per-run monotonic sequence number.
    pub seq: u64,
    /// The opaque resume token.
    pub cursor: Cursor,
    /// The cursor's format version.
    pub cursor_format_version: u16,
    /// The last record sequence number this cursor accounts for.
    ///
    /// Zero means the cursor advances without any record in between, which a
    /// Field may emit when it has proven that a page contained nothing new.
    pub covers_record_seq_through: u64,
    /// How many records this checkpoint accounts for since the previous one.
    pub records_covered: u64,
    /// The snapshot completeness claim, in a snapshot-mode run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotClaim>,
    /// The window portion this checkpoint accounts for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<CheckpointWindow>,
    /// Whether this is the last checkpoint of the run.
    #[serde(rename = "final")]
    pub is_final: bool,
}

impl Validate for CheckpointEvent {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.seq == 0 || self.seq > 1_000_000 {
            return Err(SchemaError::invalid(
                "a sequence number starts at 1 and stops at 1000000",
            ));
        }
        if self.covers_record_seq_through > 1_000_000 || self.records_covered > 1_000_000 {
            return Err(SchemaError::invalid(
                "checkpoint coverage counters stop at 1000000",
            ));
        }
        if self.cursor_format_version == 0 {
            return Err(SchemaError::invalid("cursor_format_version starts at 1"));
        }
        if let Some(claim) = &self.snapshot
            && claim.objects_enumerated.unwrap_or(0) > 1_000_000_000
        {
            return Err(SchemaError::invalid(
                "objects_enumerated stops at 1000000000",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Field to core: diagnostic events
// ---------------------------------------------------------------------------

/// A diagnostic's severity.
///
/// Severity is load-bearing: `error` means the run cannot be reported complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational.
    Info,
    /// A warning that does not disqualify completeness.
    Warning,
    /// The run cannot be reported complete.
    Error,
}

impl Severity {
    /// The wire spelling of this severity.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl core::fmt::Display for Severity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Bounded structured health, permission, rate-limit, truncation,
/// skipped-content, refetch, or damage information.
///
/// A diagnostic about a source object never implies its deletion. A record is
/// the only thing that can change notebook state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticEvent {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: DiagnosticTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The per-run monotonic sequence number.
    pub seq: u64,
    /// The severity.
    pub severity: Severity,
    /// The closed-vocabulary code.
    pub code: DiagnosticCode,
    /// Already-redacted human-readable text.
    pub message: MessageText,
    /// The portable source key this diagnostic is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    /// The object kind this diagnostic is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_kind: Option<ObjectKind>,
    /// A source-supplied backoff hint. Advisory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u32>,
    /// Bounded, already-redacted structured detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<DiagnosticDetail>,
    /// Names of the members or upstream fields the Field removed before
    /// emitting, so a reviewer can see that redaction happened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted: Option<Vec<String>>,
}

impl Validate for DiagnosticEvent {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.seq == 0 || self.seq > 1_000_000 {
            return Err(SchemaError::invalid(
                "a sequence number starts at 1 and stops at 1000000",
            ));
        }
        if let Some(retry) = self.retry_after_seconds
            && retry > 86_400
        {
            return Err(SchemaError::invalid(
                "retry_after_seconds is at most one day",
            ));
        }
        if let Some(source) = &self.source {
            source.validate()?;
        }
        if let Some(redacted) = &self.redacted {
            if redacted.is_empty() {
                return Err(SchemaError::invalid(
                    "the redacted list is present only when something was removed",
                ));
            }
            bounded(redacted, 64, "redacted")?;
            for (index, name) in redacted.iter().enumerate() {
                if name.is_empty() || name.len() > 127 {
                    return Err(SchemaError::invalid("a redacted name is 1 to 127 bytes"));
                }
                if redacted[..index].contains(name) {
                    return Err(SchemaError::invalid(format!(
                        "the redacted list names {name:?} twice"
                    )));
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The protected credential channel
// ---------------------------------------------------------------------------

/// What the Field needs from core on the protected channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPurpose {
    /// The normal case: an access token.
    AccessToken,
    /// A long-lived API token.
    ApiToken,
    /// HTTP basic credentials.
    BasicCredentials,
    /// Ask core to obtain fresh material, when core owns refresh.
    Renew,
}

/// The Field's request for credential material, on the protected channel only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialRequest {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: CredentialRequestTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The grant this request is made under.
    pub grant_id: GrantId,
    /// What the Field needs.
    pub purpose: CredentialPurpose,
    /// The scopes the Field needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

impl Validate for CredentialRequest {
    fn validate(&self) -> Result<(), SchemaError> {
        if let Some(scopes) = &self.scopes {
            bounded_scopes(scopes, "credential_request.scopes")?;
        }
        Ok(())
    }
}

/// The kind of credential material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialKind {
    /// An HTTP bearer token.
    BearerToken,
    /// A long-lived API token.
    ApiToken,
    /// HTTP basic credentials.
    Basic,
}

/// Credential material.
///
/// This is the **only** member of the entire protocol that carries a secret,
/// and it exists on no other channel. Nowhere else: not in process arguments,
/// not in the inherited environment, not in `config`, not in records, not in
/// checkpoints, not in cursors, not in diagnostics, not on standard error, not
/// in notebook material.
///
/// The [`core::fmt::Debug`] implementation is deliberately redacting, so a
/// stray `{:?}` in a log line cannot leak the value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialMaterial {
    /// The material kind.
    pub kind: MaterialKind,
    /// The secret value.
    pub value: String,
    /// The username, required for basic credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// When the material expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<OffsetDatetime>,
    /// The granted scopes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

impl core::fmt::Debug for CredentialMaterial {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CredentialMaterial")
            .field("kind", &self.kind)
            .field("value", &crate::redact::REDACTION_MARKER)
            .field("username", &self.username.as_ref().map(|_| "[redacted]"))
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Validate for CredentialMaterial {
    fn validate(&self) -> Result<(), SchemaError> {
        if self.value.is_empty() || self.value.len() > 65_536 {
            return Err(SchemaError::invalid(
                "credential material is 1 to 65536 bytes",
            ));
        }
        if self.kind == MaterialKind::Basic && self.username.is_none() {
            return Err(SchemaError::invalid(
                "basic credentials carry their username",
            ));
        }
        if let Some(username) = &self.username
            && (username.is_empty() || username.len() > 512)
        {
            return Err(SchemaError::invalid("a username is 1 to 512 bytes"));
        }
        if let Some(scopes) = &self.scopes {
            bounded_scopes(scopes, "credential_response.material.scopes")?;
        }
        Ok(())
    }
}

/// Core's answer to a credential request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialOutcome {
    /// Material is granted.
    Granted,
    /// The request was denied.
    Denied,
    /// The grant expired.
    Expired,
    /// Core does not know this grant.
    UnknownGrant,
    /// Core cannot obtain material right now.
    Unavailable,
}

/// Core's response on the protected channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialResponse {
    /// The major protocol version of this frame.
    pub v: ProtocolV1,
    /// The frame discriminator.
    #[serde(rename = "type")]
    pub frame_type: CredentialResponseTag,
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The grant this response answers.
    pub grant_id: GrantId,
    /// The outcome.
    pub outcome: CredentialOutcome,
    /// The material, present only when the outcome is granted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<CredentialMaterial>,
    /// An actionable, secret-free message, present when the outcome is not
    /// granted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<MediumText>,
}

impl Validate for CredentialResponse {
    fn validate(&self) -> Result<(), SchemaError> {
        match self.outcome {
            CredentialOutcome::Granted => {
                let Some(material) = &self.material else {
                    return Err(SchemaError::invalid(
                        "a granted credential response carries material",
                    ));
                };
                if self.message.is_some() {
                    return Err(SchemaError::invalid(
                        "a granted credential response carries no message",
                    ));
                }
                material.validate()
            }
            _ => {
                if self.material.is_some() {
                    return Err(SchemaError::invalid(
                        "a non-granted credential response carries no material",
                    ));
                }
                if self.message.is_none() {
                    return Err(SchemaError::invalid(
                        "a non-granted credential response says what to do about it",
                    ));
                }
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Frame unions
// ---------------------------------------------------------------------------

/// Reads the `v` and `type` members of a frame without decoding the rest.
///
/// The order matters. A peer answering in a version this build does not
/// implement is reported as a version mismatch, because the actionable fact is
/// the version, not a schema-internal detail. Only then is the discriminator
/// read, and only then is the frame decoded against the schema it selects.
fn frame_type(value: &serde_json::Value) -> Result<String, SchemaError> {
    let object = value.as_object().ok_or_else(|| {
        SchemaError::with_code(
            RejectionCode::ProtocolNotJson,
            "a frame is one JSON object per line",
        )
    })?;
    match object.get("v").and_then(serde_json::Value::as_u64) {
        Some(1) => {}
        Some(other) => {
            return Err(SchemaError::with_code(
                RejectionCode::ProtocolVersionUnsupported,
                format!("frame declares protocol major version {other}; this build implements 1"),
            ));
        }
        None => {
            return Err(SchemaError::invalid(
                "every frame carries the protocol major version in 'v'",
            ));
        }
    }
    object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            SchemaError::with_code(
                RejectionCode::ProtocolUnknownEvent,
                "every frame carries a string 'type' discriminator",
            )
        })
}

fn decode<T: for<'de> Deserialize<'de> + Validate>(
    value: serde_json::Value,
) -> Result<T, SchemaError> {
    let decoded: T = serde_json::from_value(value)
        .map_err(|error| SchemaError::invalid(format!("frame does not validate: {error}")))?;
    decoded.validate()?;
    Ok(decoded)
}

/// Any single frame a Field may write to standard output.
///
/// A frame that matches no branch is a protocol violation and fails the run.
/// There is no permissive or forward-compatible branch, because an unknown
/// event from an untrusted child process is exactly the case that must fail
/// closed.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldEvent {
    /// The describe manifest.
    Manifest(Box<Manifest>),
    /// One collected source object.
    Record(Box<RecordEvent>),
    /// An offered resume point.
    Checkpoint(Box<CheckpointEvent>),
    /// Bounded structured health information.
    Diagnostic(Box<DiagnosticEvent>),
}

impl FieldEvent {
    /// Decodes and validates one Field-to-core frame.
    pub fn decode(value: serde_json::Value) -> Result<Self, SchemaError> {
        let tag = frame_type(&value)?;
        match tag.as_str() {
            ManifestTag::WIRE => Ok(FieldEvent::Manifest(Box::new(decode(value)?))),
            RecordTag::WIRE => Ok(FieldEvent::Record(Box::new(decode(value)?))),
            CheckpointTag::WIRE => Ok(FieldEvent::Checkpoint(Box::new(decode(value)?))),
            DiagnosticTag::WIRE => Ok(FieldEvent::Diagnostic(Box::new(decode(value)?))),
            other => Err(SchemaError::with_code(
                RejectionCode::ProtocolUnknownEvent,
                format!(
                    "'{other}' is not a v1 Field event; a Field cannot introduce an event by \
                     emitting one"
                ),
            )),
        }
    }

    /// The run identifier the frame declares.
    #[must_use]
    pub fn run_id(&self) -> &RunId {
        match self {
            FieldEvent::Manifest(frame) => &frame.run_id,
            FieldEvent::Record(frame) => &frame.run_id,
            FieldEvent::Checkpoint(frame) => &frame.run_id,
            FieldEvent::Diagnostic(frame) => &frame.run_id,
        }
    }

    /// The sequence number the frame declares, for the three collect-run
    /// events. A manifest carries none: a describe run has exactly one answer.
    #[must_use]
    pub fn seq(&self) -> Option<u64> {
        match self {
            FieldEvent::Manifest(_) => None,
            FieldEvent::Record(frame) => Some(frame.seq),
            FieldEvent::Checkpoint(frame) => Some(frame.seq),
            FieldEvent::Diagnostic(frame) => Some(frame.seq),
        }
    }

    /// Re-encodes the frame.
    pub fn to_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            FieldEvent::Manifest(frame) => serde_json::to_value(frame),
            FieldEvent::Record(frame) => serde_json::to_value(frame),
            FieldEvent::Checkpoint(frame) => serde_json::to_value(frame),
            FieldEvent::Diagnostic(frame) => serde_json::to_value(frame),
        }
    }
}

/// Any single frame core may write to a Field's standard input.
///
/// A Field must fail closed on a frame it cannot validate rather than guessing.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreFrame {
    /// The describe request.
    Describe(Box<DescribeRequest>),
    /// The collection request.
    Collect(Box<CollectRequest>),
    /// The cooperative cancellation frame.
    Cancel(Box<Cancel>),
}

impl CoreFrame {
    /// Decodes and validates one core-to-Field frame.
    pub fn decode(value: serde_json::Value) -> Result<Self, SchemaError> {
        let tag = frame_type(&value)?;
        match tag.as_str() {
            DescribeRequestTag::WIRE => Ok(CoreFrame::Describe(Box::new(decode(value)?))),
            CollectRequestTag::WIRE => Ok(CoreFrame::Collect(Box::new(decode(value)?))),
            CancelTag::WIRE => Ok(CoreFrame::Cancel(Box::new(decode(value)?))),
            other => Err(SchemaError::with_code(
                RejectionCode::ProtocolUnknownEvent,
                format!("'{other}' is not a v1 core frame"),
            )),
        }
    }

    /// Re-encodes the frame.
    pub fn to_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            CoreFrame::Describe(frame) => serde_json::to_value(frame),
            CoreFrame::Collect(frame) => serde_json::to_value(frame),
            CoreFrame::Cancel(frame) => serde_json::to_value(frame),
        }
    }
}

/// Any single frame on the protected credential channel.
#[derive(Debug, Clone, PartialEq)]
pub enum CredentialFrame {
    /// The Field's request for material.
    Request(Box<CredentialRequest>),
    /// Core's response.
    Response(Box<CredentialResponse>),
}

impl CredentialFrame {
    /// Decodes and validates one protected-channel frame.
    pub fn decode(value: serde_json::Value) -> Result<Self, SchemaError> {
        let tag = frame_type(&value)?;
        match tag.as_str() {
            CredentialRequestTag::WIRE => Ok(CredentialFrame::Request(Box::new(decode(value)?))),
            CredentialResponseTag::WIRE => Ok(CredentialFrame::Response(Box::new(decode(value)?))),
            other => Err(SchemaError::with_code(
                RejectionCode::ProtocolUnknownEvent,
                format!("'{other}' is not a v1 credential-channel frame"),
            )),
        }
    }

    /// Re-encodes the frame.
    pub fn to_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            CredentialFrame::Request(frame) => serde_json::to_value(frame),
            CredentialFrame::Response(frame) => serde_json::to_value(frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn json(text: &str) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(text)
    }

    #[test]
    fn an_unknown_event_type_is_not_a_schema_failure() -> Result<(), serde_json::Error> {
        let value = json(
            r#"{"v":1,"type":"note","run_id":"1a4c9f2e-0000-4000-8000-00000000000b","seq":3}"#,
        )?;
        match FieldEvent::decode(value) {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolUnknownEvent),
            Ok(_) => panic!("an unknown event must fail closed"),
        }
        Ok(())
    }

    #[test]
    fn a_future_major_version_is_a_version_failure() -> Result<(), serde_json::Error> {
        let value = json(r#"{"v":2,"type":"manifest"}"#)?;
        match FieldEvent::decode(value) {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolVersionUnsupported),
            Ok(_) => panic!("a future major version must fail closed"),
        }
        Ok(())
    }

    #[test]
    fn a_non_object_frame_is_not_json() -> Result<(), serde_json::Error> {
        match FieldEvent::decode(json("[1,2,3]")?) {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolNotJson),
            Ok(_) => panic!("a frame is one JSON object per line"),
        }
        Ok(())
    }

    #[test]
    fn a_delete_carrying_content_is_refused() -> Result<(), serde_json::Error> {
        let value = json(
            r#"{"v":1,"type":"record","run_id":"1a4c9f2e-0000-4000-8000-000000000006","seq":1,
                "change":"delete","source":{"scope":"s","identity":"i"},
                "object_kind":"mail-message","authority":"tombstone",
                "observed_at":"2026-08-22T12:40:11+02:00",
                "body":{"format":"markdown","text":"leaked"}}"#,
        )?;
        match FieldEvent::decode(value) {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolSchemaInvalid),
            Ok(_) => panic!("content is structurally forbidden on a delete"),
        }
        Ok(())
    }

    #[test]
    fn a_traversal_handle_is_an_artifact_rejection_before_any_filesystem_call()
    -> Result<(), serde_json::Error> {
        let value = json(
            r#"{"v":1,"type":"record","run_id":"1a4c9f2e-0000-4000-8000-00000000000e","seq":1,
                "change":"upsert","source":{"scope":"s","identity":"i"},"object_kind":"file",
                "note_type":"file","occurred_at":"2026-08-22T09:45:00+02:00",
                "body":{"format":"markdown","text":"x"},
                "artifacts":[{"kind":"staged","handle":"../../../../etc/passwd",
                "byte_length":4096,"role":"original"}]}"#,
        )?;
        match FieldEvent::decode(value) {
            Err(error) => assert_eq!(error.code, RejectionCode::ArtifactInvalidHandle),
            Ok(_) => panic!("a traversal handle must be refused"),
        }
        Ok(())
    }

    #[test]
    fn a_manifest_missing_its_required_members_is_refused() -> Result<(), serde_json::Error> {
        let value =
            json(r#"{"v":1,"type":"manifest","run_id":"1a4c9f2e-0000-4000-8000-000000000001"}"#)?;
        assert!(FieldEvent::decode(value).is_err());
        Ok(())
    }

    fn manifest_json_without_property_prefix() -> &'static str {
        r#"{"v":1,"type":"manifest","run_id":"1a4c9f2e-0000-4000-8000-000000000001",
            "protocol_version":1,"protocol_revision":0,"supported_protocol_versions":[1],
            "driver":"local-reference","driver_version":"0.1.1","field_stem":"local",
            "declared_properties":[],
            "capabilities":[{"object_kind":"file","note_type":"file","emits_artifacts":true,
                "emits_identity_anchors":false,"description":"d"}],
            "source_key":{"scope_rule":"local_root_id","scope_rule_version":1,
                "scope_shape":"s","scope_depends_on_field_label":false,
                "identity_shape":"i","identity_includes_object_kind":true,
                "source_version_ordering":"unsupported","stable_across_instances":true},
            "auth":{"kind":"none","credential_profile_required":false,
                "protected_channel_required":false,"refresh_owner":"not_applicable",
                "writes_to_source":false},
            "collection":{"incremental":true,"cursor_format_version":1,
                "supported_modes":["incremental"],"window_supported":false,
                "refetch":"unsupported",
                "deletion":{"tombstones":"unsupported","snapshot":"unsupported"}}}"#
    }

    #[test]
    fn a_manifest_omitting_property_prefix_means_it_contributes_none()
    -> Result<(), serde_json::Error> {
        // Absent-or-present, never required-but-nullable: an omitted
        // `property_prefix` is read as "contributes no prefix" rather than
        // being indistinguishable from an explicit null that silently
        // disabled all prefix checking.
        let value = json(manifest_json_without_property_prefix())?;
        match FieldEvent::decode(value) {
            Ok(FieldEvent::Manifest(manifest)) => assert!(manifest.property_prefix.is_none()),
            other => {
                panic!("an omitted property_prefix must decode as contributing none: {other:?}")
            }
        }
        Ok(())
    }

    #[test]
    fn a_manifest_declaring_property_prefix_null_is_refused() -> Result<(), serde_json::Error> {
        // The schema does not admit `null` at all now: it is absent or a
        // string, never explicitly null.
        let text = manifest_json_without_property_prefix().replace(
            "\"declared_properties\":[]",
            "\"property_prefix\":null,\"declared_properties\":[]",
        );
        let value = json(&text)?;
        assert!(FieldEvent::decode(value).is_err());
        Ok(())
    }

    #[test]
    fn a_describe_request_need_not_state_the_full_limits_object()
    -> Result<(), Box<dyn std::error::Error>> {
        // A describe run emits at most one manifest and reads no records,
        // artifacts, or diagnostics, so it has almost nothing to bound.
        let value = json(
            r#"{"v":1,"type":"describe_request","run_id":"1a4c9f2e-0000-4000-8000-000000000001",
                "supported_protocol_versions":[1],"max_protocol_revision":0,
                "deadline":{"not_after":"2026-08-22T19:30:00+02:00","idle_seconds":120,
                "cancel_grace_seconds":10}}"#,
        )?;
        let decoded: DescribeRequest = serde_json::from_value(value)?;
        assert!(decoded.limits.is_none());
        decoded.validate()?;
        Ok(())
    }

    #[test]
    fn a_declared_list_without_semantics_is_refused() -> Result<(), serde_json::Error> {
        let declared: DeclaredProperty = serde_json::from_value(json(
            r#"{"name":"local_tags","value_type":"text","cardinality":"list","description":"d"}"#,
        )?)?;
        assert!(declared.validate().is_err());
        let scalar: DeclaredProperty = serde_json::from_value(json(
            r#"{"name":"local_media_type","value_type":"text","cardinality":"scalar",
                "list_semantics":"set","description":"d"}"#,
        )?)?;
        assert!(scalar.validate().is_err());
        Ok(())
    }

    #[test]
    fn credential_material_never_appears_in_debug_output() {
        let material = CredentialMaterial {
            kind: MaterialKind::BearerToken,
            value: "FIXTURE-NOT-A-REAL-TOKEN-canary-9f14c0a3".to_owned(),
            username: None,
            expires_at: None,
            scopes: None,
        };
        let rendered = format!("{material:?}");
        assert!(!rendered.contains("canary"), "leaked: {rendered}");
        assert!(rendered.contains("[redacted]"));
    }

    #[test]
    fn a_granted_response_without_material_is_refused() -> Result<(), serde_json::Error> {
        let value = json(
            r#"{"v":1,"type":"credential_response","run_id":"1a4c9f2e-0000-4000-8000-000000000008",
                "grant_id":"9f14c0a3b7e25d68","outcome":"granted"}"#,
        )?;
        assert!(CredentialFrame::decode(value).is_err());
        Ok(())
    }

    fn not_retained_artifact_ref(attachment_ref: Option<&str>) -> serde_json::Value {
        let mut value = json!({
            "kind": "not_retained",
            "byte_length": 1_073_741_824_u64,
            "media_type": "video/mp4",
            "role": "attachment",
            "source_filename": "team-standup-recording.mp4"
        });
        if let Some(reference) = attachment_ref
            && let Some(object) = value.as_object_mut()
        {
            object.insert("attachment_ref".to_owned(), json!(reference));
        }
        value
    }

    #[test]
    fn a_not_retained_reference_requires_an_attachment_ref() -> Result<(), serde_json::Error> {
        let without: ArtifactRef = serde_json::from_value(not_retained_artifact_ref(None))?;
        assert_eq!(
            without.validate(),
            Err(SchemaError::invalid(
                "a not_retained reference carries no bytes and no digest, so attachment_ref is \
                 the only stable identity it has; core needs it to record the declined \
                 attachment in skipped_attachments"
            ))
        );

        let with: ArtifactRef = serde_json::from_value(not_retained_artifact_ref(Some(
            "mail-attachment/AAMkAGI2TQABAAACattach02",
        )))?;
        assert!(with.validate().is_ok());
        Ok(())
    }

    #[test]
    fn a_staged_or_digest_only_reference_forbids_attachment_ref() -> Result<(), serde_json::Error> {
        let staged: ArtifactRef = serde_json::from_value(json!({
            "kind": "staged",
            "handle": "a0001",
            "byte_length": 40,
            "role": "original",
            "attachment_ref": "mail-attachment/should-not-be-here"
        }))?;
        assert!(staged.validate().is_err());

        let digest_only: ArtifactRef = serde_json::from_value(json!({
            "kind": "digest_only",
            "sha256": "449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17",
            "role": "attachment",
            "attachment_ref": "mail-attachment/should-not-be-here"
        }))?;
        assert!(digest_only.validate().is_err());
        Ok(())
    }

    fn base_collect_request_json() -> serde_json::Value {
        json!({
            "v": 1, "type": "collect_request", "run_id": "1a4c9f2e-0000-4000-8000-00000000000f",
            "protocol_version": 1, "protocol_revision": 0, "field_id": "local_work",
            "mode": "incremental", "config": {},
            "artifact_staging_dir": "/tmp/staging",
            "artifact_media_types": ["application/pdf", "image/*"],
            "limits": serde_json::to_value(Limits::ceilings())
                .unwrap_or_else(|error| panic!("limits must encode: {error}")),
            "deadline": {
                "not_after": "2099-01-01T00:00:00+00:00",
                "idle_seconds": 120,
                "cancel_grace_seconds": 10
            }
        })
    }

    #[test]
    fn collect_request_requires_a_non_empty_artifact_media_types_list()
    -> Result<(), serde_json::Error> {
        let mut without = base_collect_request_json();
        if let Some(object) = without.as_object_mut() {
            object.insert("artifact_media_types".to_owned(), json!([]));
        }
        let request: CollectRequest = serde_json::from_value(without)?;
        assert!(request.validate().is_err());

        let request: CollectRequest = serde_json::from_value(base_collect_request_json())?;
        assert!(request.validate().is_ok());
        Ok(())
    }

    #[test]
    fn recollect_targets_forbids_a_cursor_a_window_and_snapshot_mode()
    -> Result<(), serde_json::Error> {
        let targets =
            json!([{"scope": "microsoft-graph:tenant/x", "identity": "mail-message/AAA"}]);

        let mut with_cursor = base_collect_request_json();
        if let Some(object) = with_cursor.as_object_mut() {
            object.insert("cursor".to_owned(), json!("walk:v1:seq=2"));
            object.insert("cursor_format_version".to_owned(), json!(1));
            object.insert("recollect_targets".to_owned(), targets.clone());
        }
        let request: CollectRequest = serde_json::from_value(with_cursor)?;
        assert!(
            request.validate().is_err(),
            "recollect_targets must not combine with a cursor"
        );

        let mut with_snapshot = base_collect_request_json();
        if let Some(object) = with_snapshot.as_object_mut() {
            object.insert("mode".to_owned(), json!("snapshot"));
            object.insert("snapshot_scope".to_owned(), json!("everything"));
            object.insert("recollect_targets".to_owned(), targets.clone());
        }
        let request: CollectRequest = serde_json::from_value(with_snapshot)?;
        assert!(
            request.validate().is_err(),
            "recollect_targets must not combine with snapshot mode"
        );

        let mut alone = base_collect_request_json();
        if let Some(object) = alone.as_object_mut() {
            object.insert("recollect_targets".to_owned(), targets);
        }
        let request: CollectRequest = serde_json::from_value(alone)?;
        assert!(
            request.validate().is_ok(),
            "recollect_targets alone, with no cursor or window and in incremental mode, is legal"
        );
        Ok(())
    }

    #[test]
    fn manifest_supports_recollect_mirrors_the_refetch_declaration() -> Result<(), serde_json::Error>
    {
        let supported: Manifest = serde_json::from_value(json!({
            "v": 1, "type": "manifest", "run_id": "1a4c9f2e-0000-4000-8000-000000000001",
            "protocol_version": 1, "protocol_revision": 0, "supported_protocol_versions": [1],
            "driver": "local-reference", "driver_version": "0.1.1", "field_stem": "local",
            "declared_properties": [],
            "capabilities": [{"object_kind": "file", "note_type": "file", "emits_artifacts": true,
                "emits_identity_anchors": false, "description": "d"}],
            "source_key": {
                "scope_rule": "local_root_id", "scope_rule_version": 1,
                "scope_shape": "s", "scope_depends_on_field_label": false,
                "identity_shape": "i", "identity_includes_object_kind": true,
                "source_version_ordering": "unsupported", "stable_across_instances": true
            },
            "auth": {
                "kind": "none", "credential_profile_required": false,
                "protected_channel_required": false, "refresh_owner": "not_applicable",
                "writes_to_source": false
            },
            "collection": {
                "incremental": true, "cursor_format_version": 1,
                "supported_modes": ["incremental"], "window_supported": false,
                "refetch": "supported",
                "deletion": { "tombstones": "unsupported", "snapshot": "unsupported" }
            }
        }))?;
        assert!(supported.supports_recollect());

        let mut json_value = serde_json::to_value(&supported)
            .unwrap_or_else(|error| panic!("manifest must encode: {error}"));
        if let Some(collection) = json_value
            .get_mut("collection")
            .and_then(serde_json::Value::as_object_mut)
        {
            collection.insert("refetch".to_owned(), json!("unsupported"));
        }
        let unsupported: Manifest = serde_json::from_value(json_value)?;
        assert!(!unsupported.supports_recollect());
        Ok(())
    }
}
