//! The manifest's declared-property mechanism and the ruling 4 enforcement
//! rules.
//!
//! A1 registered property *prefixes* but no registry entry exists for an
//! individual prefixed property, so IG1's interim rule inferred a prefixed
//! property's type from its canonical spelling. That inference is round-trip
//! stable, but it is inference, and it cannot express list semantics at all.
//!
//! Each Field's describe manifest therefore declares every connector-prefixed
//! property it may emit, with its A1 scalar type, its cardinality, and — for a
//! list — whether the list is set-like or order-bearing. Core then enforces, on
//! every record:
//!
//! 1. a prefixed property the declaring manifest does not list is **rejected**;
//! 2. a declared property whose emitted JSON shape contradicts its declared
//!    type or cardinality is **rejected**;
//! 3. a prefixed property belonging to another Field's registered stem is
//!    **rejected**, which is A1 section 4's prefix-to-producer binding;
//! 4. an unprefixed name outside A1's closed shared registry is **rejected**;
//! 5. spelling-based inference is retired for declared properties: the type
//!    comes from the declaration;
//! 6. an unprefixed name that the registry types for a **derived record**
//!    only — [`fieldnotes_domain::property::registry::DERIVED_RECORD_ONLY`],
//!    such as `confidence` or `generated_at` — is **rejected**. A Field collects a
//!    Note, never a derived record, so that subset of the registry is not
//!    Note-applicable and is not a vocabulary a record may use.
//!
//! A manifest may not declare unprefixed properties. Those belong to A1's
//! closed shared registry and take their type from it, which is why
//! [`DeclaredPropertyIndex`] consults [`PropertyRegistry`] rather than carrying
//! a second copy of that vocabulary.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use fieldnotes_domain::property::registry::{
    ListSemantics as RegistryListSemantics, PropertyRegistry, PropertyType,
};
use fieldnotes_domain::{Date, Datetime, FieldStemRegistry, ScalarKind};

use crate::codes::RejectionCode;
use crate::message::{DeclaredProperty, Manifest};
use crate::value::PropertyValue;

/// One of A1's five scalar types, as a manifest declares it.
///
/// The wire spellings are taken from [`ScalarKind::as_str`] rather than written
/// out again here, so the protocol cannot drift from A1's own names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalarType(ScalarKind);

impl ScalarType {
    /// Every A1 scalar type, in the order the schema's `enum` lists them.
    pub const ALL: [ScalarType; 5] = [
        ScalarType(ScalarKind::Text),
        ScalarType(ScalarKind::Number),
        ScalarType(ScalarKind::Bool),
        ScalarType(ScalarKind::Date),
        ScalarType(ScalarKind::Datetime),
    ];

    /// The A1 scalar kind this declaration names.
    #[must_use]
    pub fn kind(self) -> ScalarKind {
        self.0
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.0.as_str()
    }

    /// Parses a wire spelling. The vocabulary is closed, so anything else is
    /// `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        ScalarType::ALL.into_iter().find(|ty| ty.as_str() == text)
    }
}

impl From<ScalarKind> for ScalarType {
    fn from(kind: ScalarKind) -> Self {
        ScalarType(kind)
    }
}

impl fmt::Display for ScalarType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ScalarType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ScalarType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ScalarType::parse(&text).ok_or_else(|| {
            de::Error::invalid_value(
                Unexpected::Str(&text),
                &"one of the A1 scalar types: text, number, boolean, date, datetime",
            )
        })
    }
}

/// Whether a declared property is a scalar or a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    /// One scalar value.
    Scalar,
    /// A homogeneous one-dimensional list.
    List,
}

/// Whether a declared list's order carries meaning.
///
/// These are A1's two list classes. The bridge to
/// [`fieldnotes_domain::property::registry::ListSemantics`] is
/// [`ListSemantics::registry`], so the canonical serializer and the manifest
/// cannot disagree about which lists get sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListSemantics {
    /// Sorted and deduplicated by normalized value.
    Set,
    /// Source or role order is preserved.
    Ordered,
}

impl ListSemantics {
    /// The A1 registry spelling of these semantics.
    #[must_use]
    pub fn registry(self) -> RegistryListSemantics {
        match self {
            ListSemantics::Set => RegistryListSemantics::Set,
            ListSemantics::Ordered => RegistryListSemantics::Ordered,
        }
    }

    /// The manifest spelling of an A1 registry entry's semantics.
    #[must_use]
    pub fn from_registry(semantics: RegistryListSemantics) -> Self {
        match semantics {
            RegistryListSemantics::Set => ListSemantics::Set,
            RegistryListSemantics::Ordered => ListSemantics::Ordered,
        }
    }
}

/// The approved shape of one property: its scalar type, its cardinality, and a
/// list's semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyShape {
    /// The scalar type.
    pub value_type: ScalarType,
    /// Whether the value is a scalar or a list.
    pub cardinality: Cardinality,
    /// A list's order semantics.
    pub list_semantics: Option<ListSemantics>,
}

impl PropertyShape {
    /// The shape an A1 registry entry describes.
    #[must_use]
    pub fn from_registry(entry: PropertyType) -> Self {
        match entry {
            PropertyType::Scalar(kind) => PropertyShape {
                value_type: ScalarType(kind),
                cardinality: Cardinality::Scalar,
                list_semantics: None,
            },
            PropertyType::List(kind, semantics) => PropertyShape {
                value_type: ScalarType(kind),
                cardinality: Cardinality::List,
                list_semantics: Some(ListSemantics::from_registry(semantics)),
            },
        }
    }

    /// The shape a manifest entry declares.
    #[must_use]
    pub fn from_declaration(declared: &DeclaredProperty) -> Self {
        PropertyShape {
            value_type: declared.value_type,
            cardinality: declared.cardinality,
            list_semantics: declared.list_semantics,
        }
    }

    /// Whether `value`'s JSON shape agrees with this approved shape.
    ///
    /// A temporal type additionally requires the string to be a well-formed A1
    /// value; a string that merely looks like a date is not one. A malformed
    /// `date` and a malformed `datetime` are reported with their own codes:
    /// [`RejectionCode::RecordInvalidDate`] and
    /// [`RejectionCode::RecordInvalidDatetime`] respectively, so a reviewer
    /// reading logs or metrics can tell which grammar failed without opening
    /// the record. The envelope's own instants (`occurred_at`, `observed_at`)
    /// are guarded at decode by [`crate::grammar::OffsetDatetime`], so this
    /// code path only ever fires for a declared or registered temporal
    /// *property*.
    pub fn accepts(&self, value: &PropertyValue) -> Result<(), RejectionCode> {
        let list_expected = self.cardinality == Cardinality::List;
        if list_expected != value.is_list() {
            return Err(RejectionCode::RecordPropertyTypeMismatch);
        }
        match (self.value_type.kind(), value) {
            (ScalarKind::Text, PropertyValue::Text(_) | PropertyValue::TextList(_))
            | (ScalarKind::Number, PropertyValue::Number(_) | PropertyValue::NumberList(_))
            | (ScalarKind::Bool, PropertyValue::Boolean(_) | PropertyValue::BooleanList(_)) => {
                Ok(())
            }
            (ScalarKind::Date, PropertyValue::Text(text)) => {
                temporal(Date::parse(text).is_ok(), RejectionCode::RecordInvalidDate)
            }
            (ScalarKind::Date, PropertyValue::TextList(members)) => temporal(
                members.iter().all(|text| Date::parse(text).is_ok()),
                RejectionCode::RecordInvalidDate,
            ),
            (ScalarKind::Datetime, PropertyValue::Text(text)) => temporal(
                Datetime::parse(text).is_ok(),
                RejectionCode::RecordInvalidDatetime,
            ),
            (ScalarKind::Datetime, PropertyValue::TextList(members)) => temporal(
                members.iter().all(|text| Datetime::parse(text).is_ok()),
                RejectionCode::RecordInvalidDatetime,
            ),
            _ => Err(RejectionCode::RecordPropertyTypeMismatch),
        }
    }
}

fn temporal(well_formed: bool, code: RejectionCode) -> Result<(), RejectionCode> {
    if well_formed { Ok(()) } else { Err(code) }
}

/// Why one property candidate was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyRejection {
    /// The rejection code core reports.
    pub code: RejectionCode,
    /// The property name that was refused.
    pub name: String,
    /// Why, in reviewable terms.
    pub detail: String,
}

impl fmt::Display for PropertyRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.code, self.name, self.detail)
    }
}

impl std::error::Error for PropertyRejection {}

/// The declaring manifest's view of what property names a record may carry.
///
/// Built once per run from the manifest, the A1 registered stem set, and the A1
/// shared property registry.
#[derive(Debug)]
pub struct DeclaredPropertyIndex<'a> {
    field_stem: &'a str,
    declared: BTreeMap<&'a str, PropertyShape>,
    stems: &'a FieldStemRegistry,
    registry: &'static PropertyRegistry,
}

impl<'a> DeclaredPropertyIndex<'a> {
    /// Builds the index from a manifest.
    ///
    /// Refuses a manifest that declares a property outside its own registered
    /// prefix, or that declares an unprefixed name: those belong to A1's closed
    /// shared registry and take their type from it.
    pub fn new(
        manifest: &'a Manifest,
        stems: &'a FieldStemRegistry,
    ) -> Result<Self, PropertyRejection> {
        let field_stem = manifest.field_stem.as_str();
        let mut declared = BTreeMap::new();
        for entry in &manifest.declared_properties {
            let name = entry.name.as_str();
            match manifest.property_prefix.as_ref() {
                Some(prefix) if name.starts_with(prefix.as_str()) => {}
                Some(prefix) => {
                    return Err(PropertyRejection {
                        code: RejectionCode::RecordForeignPrefix,
                        name: name.to_owned(),
                        detail: format!(
                            "a declared property must begin with this manifest's own prefix {prefix}"
                        ),
                    });
                }
                None => {
                    return Err(PropertyRejection {
                        code: RejectionCode::RecordUndeclaredProperty,
                        name: name.to_owned(),
                        detail: "a manifest with no registered property prefix declares no \
                                 prefixed properties"
                            .to_owned(),
                    });
                }
            }
            if declared
                .insert(name, PropertyShape::from_declaration(entry))
                .is_some()
            {
                return Err(PropertyRejection {
                    code: RejectionCode::RecordPropertyTypeMismatch,
                    name: name.to_owned(),
                    detail: "declared twice in one manifest, so its approved type is ambiguous"
                        .to_owned(),
                });
            }
        }
        Ok(DeclaredPropertyIndex {
            field_stem,
            declared,
            stems,
            registry: PropertyRegistry::v1(),
        })
    }

    /// The approved shape of a declared prefixed property.
    #[must_use]
    pub fn declared_shape(&self, name: &str) -> Option<PropertyShape> {
        self.declared.get(name).copied()
    }

    /// Checks one property candidate against the declaration and the registry.
    pub fn check(&self, name: &str, value: &PropertyValue) -> Result<(), PropertyRejection> {
        if let Some(stem) = self.stems.property_prefix_for(name) {
            if stem != self.field_stem {
                return Err(PropertyRejection {
                    code: RejectionCode::RecordForeignPrefix,
                    name: name.to_owned(),
                    detail: format!(
                        "the '{stem}_' prefix belongs to another Field's registered stem; a \
                         '{}' Field emitting it is a connector-boundary violation regardless of \
                         whether that Field declares it",
                        self.field_stem
                    ),
                });
            }
            let Some(shape) = self.declared.get(name) else {
                return Err(PropertyRejection {
                    code: RejectionCode::RecordUndeclaredProperty,
                    name: name.to_owned(),
                    detail: "carries this Field's own registered prefix but is absent from the \
                             manifest's declared_properties, so core has no approved type or \
                             list semantics for it and will not infer one"
                        .to_owned(),
                });
            };
            return shape.accepts(value).map_err(|code| PropertyRejection {
                code,
                name: name.to_owned(),
                detail: format!(
                    "declared as {} {} but arrived as a {}",
                    shape.value_type,
                    match shape.cardinality {
                        Cardinality::Scalar => "scalar",
                        Cardinality::List => "list",
                    },
                    value.shape()
                ),
            });
        }

        let Some(entry) = self.registry.lookup(name) else {
            return Err(PropertyRejection {
                code: RejectionCode::RecordUnknownProperty,
                name: name.to_owned(),
                detail: "unprefixed property names are closed by the A1 shared registry and a \
                         Field cannot invent one: use an approved shared property or the Field's \
                         own declared prefixed property"
                    .to_owned(),
            });
        };
        if !fieldnotes_domain::property::registry::is_note_applicable(name) {
            return Err(PropertyRejection {
                code: RejectionCode::RecordUnknownProperty,
                name: name.to_owned(),
                detail: "this name is registered, but only for a derived record generated from \
                         other Notes; a Field collects a Note and maps a source object onto A1 \
                         vocabulary, so it has no evidence span, confidence score, or binding \
                         status to report and may not emit this name"
                    .to_owned(),
            });
        }
        let shape = PropertyShape::from_registry(entry);
        shape.accepts(value).map_err(|code| PropertyRejection {
            code,
            name: name.to_owned(),
            detail: format!(
                "the A1 shared registry types this property as {} {} but it arrived as a {}",
                shape.value_type,
                match shape.cardinality {
                    Cardinality::Scalar => "scalar",
                    Cardinality::List => "list",
                },
                value.shape()
            ),
        })
    }
}

/// The subset of a manifest core snapshots per configured Field.
///
/// If a later manifest changes a declared property's type or cardinality, or
/// changes the cursor format version, core refuses to sync that Field until an
/// explicit migration rather than retyping notebook data in place. This is A1's
/// rule that a property name never changes meaning or scalar/list type within
/// v0.1, made enforceable at the boundary where the change would arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestSnapshot {
    /// The declared cursor format version.
    pub cursor_format_version: u16,
    /// Each declared property's approved shape.
    pub declared: BTreeMap<String, PropertyShape>,
}

/// Why core refuses to sync a Field whose manifest changed incompatibly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationRequired {
    /// The rejection code core reports.
    pub code: RejectionCode,
    /// Why, in reviewable terms.
    pub detail: String,
}

impl fmt::Display for MigrationRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for MigrationRequired {}

impl ManifestSnapshot {
    /// Snapshots a manifest.
    #[must_use]
    pub fn of(manifest: &Manifest) -> Self {
        ManifestSnapshot {
            cursor_format_version: manifest.collection.cursor_format_version,
            declared: manifest
                .declared_properties
                .iter()
                .map(|entry| {
                    (
                        entry.name.as_str().to_owned(),
                        PropertyShape::from_declaration(entry),
                    )
                })
                .collect(),
        }
    }

    /// Compares a stored snapshot with the manifest that just arrived.
    ///
    /// The declared-property change policy: **adding** a declared property is
    /// a Field release change, not a migration, so it is allowed without one.
    /// **Changing or removing** one requires a migration. A changed type or
    /// cardinality is refused because core will not silently retype notebook
    /// data in place; a removed name is refused for the same reason a changed
    /// one is — the manifest is the only place that type was ever recorded,
    /// so once the declaration disappears core can no longer tell a
    /// previously-collected value of that property from an undeclared one on
    /// the next divergence check, which is exactly the ambiguity ruling 4
    /// exists to prevent. Changing the cursor format is a migration too.
    pub fn check_against(&self, current: &ManifestSnapshot) -> Result<(), MigrationRequired> {
        if self.cursor_format_version != current.cursor_format_version {
            return Err(MigrationRequired {
                code: RejectionCode::ManifestCursorFormatChanged,
                detail: format!(
                    "the stored cursor was written at format version {} but the manifest now \
                     declares {}; core will not hand a Field a token it may misread",
                    self.cursor_format_version, current.cursor_format_version
                ),
            });
        }
        for (name, stored) in &self.declared {
            match current.declared.get(name) {
                Some(now)
                    if stored.value_type != now.value_type
                        || stored.cardinality != now.cardinality
                        || stored.list_semantics != now.list_semantics =>
                {
                    return Err(MigrationRequired {
                        code: RejectionCode::ManifestPropertyTypeChanged,
                        detail: format!(
                            "declared property {name} changed shape between runs; core says so \
                             instead of retyping notebook data in place"
                        ),
                    });
                }
                Some(_) => {}
                None => {
                    return Err(MigrationRequired {
                        code: RejectionCode::ManifestPropertyTypeChanged,
                        detail: format!(
                            "declared property {name} is no longer declared; removing a \
                             declared property is a migration, not a manifest edit, because \
                             its type and list semantics are recorded nowhere else"
                        ),
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_type_spellings_come_from_the_a1_vocabulary() {
        assert_eq!(
            ScalarType::parse("boolean").map(ScalarType::kind),
            Some(ScalarKind::Bool)
        );
        assert_eq!(
            ScalarType::parse("datetime").map(ScalarType::kind),
            Some(ScalarKind::Datetime)
        );
        assert_eq!(ScalarType::parse("bool"), None);
        assert_eq!(ScalarType::from(ScalarKind::Text).as_str(), "text");
    }

    #[test]
    fn list_semantics_bridge_to_the_a1_registry() {
        assert_eq!(ListSemantics::Set.registry(), RegistryListSemantics::Set);
        assert_eq!(
            ListSemantics::from_registry(RegistryListSemantics::Ordered),
            ListSemantics::Ordered
        );
    }

    #[test]
    fn a_declared_shape_accepts_only_the_declared_json_shape() {
        let text_scalar = PropertyShape {
            value_type: ScalarType::from(ScalarKind::Text),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
        };
        assert!(
            text_scalar
                .accepts(&PropertyValue::Text("true".into()))
                .is_ok()
        );
        assert_eq!(
            text_scalar.accepts(&PropertyValue::Boolean(true)),
            Err(RejectionCode::RecordPropertyTypeMismatch)
        );
        assert_eq!(
            text_scalar.accepts(&PropertyValue::TextList(vec!["a".into()])),
            Err(RejectionCode::RecordPropertyTypeMismatch)
        );
    }

    #[test]
    fn a_declared_date_requires_a_well_formed_a1_date() {
        let date = PropertyShape {
            value_type: ScalarType::from(ScalarKind::Date),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
        };
        assert!(
            date.accepts(&PropertyValue::Text("2026-08-20".into()))
                .is_ok()
        );
        assert_eq!(
            date.accepts(&PropertyValue::Text("August 20".into())),
            Err(RejectionCode::RecordInvalidDate),
            "a malformed date-only value has its own code, distinct from an invalid datetime"
        );
    }

    #[test]
    fn a_malformed_date_and_a_malformed_datetime_use_different_codes() {
        let date = PropertyShape {
            value_type: ScalarType::from(ScalarKind::Date),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
        };
        let datetime = PropertyShape {
            value_type: ScalarType::from(ScalarKind::Datetime),
            cardinality: Cardinality::Scalar,
            list_semantics: None,
        };
        assert_eq!(
            date.accepts(&PropertyValue::Text("not-a-date".into())),
            Err(RejectionCode::RecordInvalidDate)
        );
        assert_eq!(
            datetime.accepts(&PropertyValue::Text("not-a-datetime".into())),
            Err(RejectionCode::RecordInvalidDatetime)
        );
    }

    #[test]
    fn the_registry_supplies_shared_property_shapes() {
        let entry = PropertyRegistry::v1().lookup("to");
        match entry {
            Some(entry) => {
                let shape = PropertyShape::from_registry(entry);
                assert_eq!(shape.cardinality, Cardinality::List);
                assert_eq!(shape.list_semantics, Some(ListSemantics::Ordered));
            }
            None => panic!("'to' is an approved A1 shared property"),
        }
    }

    fn stem_registry() -> &'static FieldStemRegistry {
        FieldStemRegistry::v1()
    }

    fn local_manifest_json() -> serde_json::Value {
        serde_json::json!({
            "v": 1, "type": "manifest", "run_id": "1a4c9f2e-0000-4000-8000-000000000001",
            "protocol_version": 1, "protocol_revision": 0, "supported_protocol_versions": [1],
            "driver": "local-reference", "driver_version": "0.1.1", "field_stem": "local",
            "property_prefix": "local_",
            "declared_properties": [],
            "capabilities": [{
                "object_kind": "file", "note_type": "file", "emits_artifacts": true,
                "emits_identity_anchors": false, "description": "d"
            }],
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
                "refetch": "unsupported",
                "deletion": { "tombstones": "unsupported", "snapshot": "unsupported" }
            }
        })
    }

    #[test]
    fn an_unprefixed_derived_record_only_name_is_rejected_as_unknown()
    -> Result<(), Box<dyn std::error::Error>> {
        let manifest: Manifest = serde_json::from_value(local_manifest_json())?;
        let stems = stem_registry();
        let index = DeclaredPropertyIndex::new(&manifest, stems)?;
        match index.check(
            "confidence",
            &PropertyValue::Number(serde_json::Number::from(1)),
        ) {
            Err(rejection) => assert_eq!(rejection.code, RejectionCode::RecordUnknownProperty),
            Ok(()) => panic!("confidence is derived-record-only and must be rejected"),
        }
        // An ordinary shared Note property remains usable.
        assert!(
            index
                .check("title", &PropertyValue::Text("x".into()))
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn adding_a_declared_property_is_not_a_migration_but_removing_one_is()
    -> Result<(), Box<dyn std::error::Error>> {
        let without_tags = local_manifest_json();
        let mut with_tags = local_manifest_json();
        if let Some(properties) = with_tags.get_mut("declared_properties") {
            *properties = serde_json::json!([{
                "name": "local_tags", "value_type": "text", "cardinality": "list",
                "list_semantics": "set", "description": "d"
            }]);
        }
        let stored = ManifestSnapshot::of(&serde_json::from_value::<Manifest>(without_tags)?);
        let current = ManifestSnapshot::of(&serde_json::from_value::<Manifest>(with_tags)?);

        // Adding local_tags between the stored and the current manifest is not
        // a migration: the added property carries no prior notebook data.
        assert!(stored.check_against(&current).is_ok());

        // Removing it (going the other direction) is a migration: its type and
        // list semantics are recorded nowhere else once the declaration is gone.
        match current.check_against(&stored) {
            Err(migration) => {
                assert_eq!(migration.code, RejectionCode::ManifestPropertyTypeChanged)
            }
            Ok(()) => panic!("removing a declared property must require a migration"),
        }
        Ok(())
    }
}
