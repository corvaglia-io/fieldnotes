//! Flat property candidates and the maps that carry them.
//!
//! A property value is one A1-compatible scalar or one homogeneous
//! one-dimensional list of scalars. The six variants of [`PropertyValue`] are
//! exactly the schema's `oneOf` branches, so a mixed list is a decode failure
//! rather than something to inspect later.
//!
//! Types here carry no A1 vocabulary: they say what JSON shape arrived, and
//! [`crate::declared`] decides whether that shape is the declared or registered
//! one.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::grammar::{PropertyNameToken, is_lower_snake};

/// The largest text scalar the schema admits, in bytes.
pub const MAX_TEXT_SCALAR_BYTES: usize = 65_536;

/// The largest list the schema admits.
pub const MAX_LIST_MEMBERS: usize = 1024;

/// The largest property map the record schema admits.
pub const MAX_RECORD_PROPERTIES: usize = 256;

/// The largest config map the collection-request schema admits.
pub const MAX_CONFIG_PROPERTIES: usize = 64;

/// The largest diagnostic detail map the schema admits.
pub const MAX_DETAIL_PROPERTIES: usize = 32;

/// The largest diagnostic detail string the schema admits, in bytes.
pub const MAX_DETAIL_TEXT_BYTES: usize = 1024;

/// Property names core owns or hoists, which a record may never carry.
///
/// The record schema excludes these **by name grammar**, so a Field that tries
/// to assign a record ID, producer provenance, a capture time, a content hash,
/// a source key duplicate, or a rebuildable projection list fails to decode at
/// all rather than being overruled somewhere later. They are listed in the
/// order the schema's `propertyNames` pattern lists them.
pub const CORE_OWNED_PROPERTY_NAMES: [&str; 21] = [
    "id",
    "instance_id",
    "field_id",
    "type",
    "occurred_at",
    "captured_at",
    "collected_by",
    "content_hash",
    "source_scope",
    "source_identity",
    "source_version",
    "source_url",
    "source_parent_id",
    "artifacts",
    "attachments",
    "identities",
    "entities",
    "related",
    "damaged",
    "truncated",
    "lost_characters",
];

/// Whether `name` is a property name core owns or hoists.
#[must_use]
pub fn is_core_owned_property(name: &str) -> bool {
    CORE_OWNED_PROPERTY_NAMES.contains(&name)
}

/// One property candidate: a scalar, or a homogeneous list of one scalar type.
///
/// `date` and `datetime` values arrive as JSON strings; which of the five A1
/// scalar types a string carries comes from the declaring manifest entry or the
/// A1 shared registry, never from the string's spelling.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// A JSON string.
    Text(String),
    /// A finite JSON number, kept in its wire spelling so a record round-trips
    /// byte for byte. Core, not the protocol, decides the canonical spelling.
    Number(serde_json::Number),
    /// A JSON boolean.
    Boolean(bool),
    /// A non-empty homogeneous list of JSON strings.
    TextList(Vec<String>),
    /// A non-empty homogeneous list of finite JSON numbers.
    NumberList(Vec<serde_json::Number>),
    /// A non-empty homogeneous list of JSON booleans.
    BooleanList(Vec<bool>),
}

impl PropertyValue {
    /// Whether this value arrived as a list.
    #[must_use]
    pub fn is_list(&self) -> bool {
        matches!(
            self,
            PropertyValue::TextList(_)
                | PropertyValue::NumberList(_)
                | PropertyValue::BooleanList(_)
        )
    }

    /// The number of members in a list, or 1 for a scalar.
    #[must_use]
    pub fn member_count(&self) -> usize {
        match self {
            PropertyValue::Text(_) | PropertyValue::Number(_) | PropertyValue::Boolean(_) => 1,
            PropertyValue::TextList(members) => members.len(),
            PropertyValue::NumberList(members) => members.len(),
            PropertyValue::BooleanList(members) => members.len(),
        }
    }

    /// The largest single member's encoded size in bytes, which is what the
    /// per-value bound applies to.
    #[must_use]
    pub fn max_member_bytes(&self) -> usize {
        match self {
            PropertyValue::Text(text) => text.len(),
            PropertyValue::Number(number) => number.to_string().len(),
            PropertyValue::Boolean(_) => 5,
            PropertyValue::TextList(members) => members.iter().map(String::len).max().unwrap_or(0),
            PropertyValue::NumberList(members) => members
                .iter()
                .map(|number| number.to_string().len())
                .max()
                .unwrap_or(0),
            PropertyValue::BooleanList(_) => 5,
        }
    }

    /// A stable label for the JSON shape that arrived, for diagnostics.
    #[must_use]
    pub fn shape(&self) -> &'static str {
        match self {
            PropertyValue::Text(_) => "text scalar",
            PropertyValue::Number(_) => "number scalar",
            PropertyValue::Boolean(_) => "boolean scalar",
            PropertyValue::TextList(_) => "text list",
            PropertyValue::NumberList(_) => "number list",
            PropertyValue::BooleanList(_) => "boolean list",
        }
    }
}

impl Serialize for PropertyValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PropertyValue::Text(text) => serializer.serialize_str(text),
            PropertyValue::Number(number) => number.serialize(serializer),
            PropertyValue::Boolean(flag) => serializer.serialize_bool(*flag),
            PropertyValue::TextList(members) => members.serialize(serializer),
            PropertyValue::NumberList(members) => members.serialize(serializer),
            PropertyValue::BooleanList(members) => members.serialize(serializer),
        }
    }
}

/// Checks that a JSON number is finite, and keeps its wire spelling.
///
/// JSON has no non-finite literal, so this is a guard against a constructed
/// value rather than a parsed one. Whether an integer is exactly representable
/// in binary64 is A1's rule, checked by core after this guard passes; the
/// protocol keeps the number as the Field sent it.
fn finite<E: de::Error>(number: serde_json::Number) -> Result<serde_json::Number, E> {
    match number.as_f64() {
        Some(value) if value.is_finite() => Ok(number),
        Some(_) | None => Err(de::Error::custom(format!(
            "property number {number} is not a finite binary64 value"
        ))),
    }
}

fn text<E: de::Error>(value: String) -> Result<String, E> {
    if value.len() > MAX_TEXT_SCALAR_BYTES {
        return Err(de::Error::custom(format!(
            "property text of {} bytes exceeds the {MAX_TEXT_SCALAR_BYTES}-byte scalar bound",
            value.len()
        )));
    }
    Ok(value)
}

impl<'de> Deserialize<'de> for PropertyValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(value) => Ok(PropertyValue::Text(text(value)?)),
            serde_json::Value::Number(number) => Ok(PropertyValue::Number(finite(number)?)),
            serde_json::Value::Bool(flag) => Ok(PropertyValue::Boolean(flag)),
            serde_json::Value::Array(members) => list_from(members),
            serde_json::Value::Null => Err(de::Error::custom(
                "null is never a property value; an absent value is omitted",
            )),
            serde_json::Value::Object(_) => Err(de::Error::custom(
                "a property value is a scalar or a flat list, never a nested object",
            )),
        }
    }
}

fn list_from<E: de::Error>(members: Vec<serde_json::Value>) -> Result<PropertyValue, E> {
    if members.is_empty() {
        return Err(de::Error::custom(
            "an empty list is never emitted; core omits an absent value",
        ));
    }
    if members.len() > MAX_LIST_MEMBERS {
        return Err(de::Error::custom(format!(
            "a list of {} members exceeds the {MAX_LIST_MEMBERS}-member bound",
            members.len()
        )));
    }
    let mixed = || {
        de::Error::custom(
            "a list is homogeneous in one scalar type; a mixed list is not an A1 value",
        )
    };
    match members.first() {
        Some(serde_json::Value::String(_)) => {
            let mut collected = Vec::with_capacity(members.len());
            for member in members {
                match member {
                    serde_json::Value::String(value) => collected.push(text(value)?),
                    _ => return Err(mixed()),
                }
            }
            Ok(PropertyValue::TextList(collected))
        }
        Some(serde_json::Value::Number(_)) => {
            let mut collected = Vec::with_capacity(members.len());
            for member in members {
                match member {
                    serde_json::Value::Number(number) => collected.push(finite(number)?),
                    _ => return Err(mixed()),
                }
            }
            Ok(PropertyValue::NumberList(collected))
        }
        Some(serde_json::Value::Bool(_)) => {
            let mut collected = Vec::with_capacity(members.len());
            for member in members {
                match member {
                    serde_json::Value::Bool(flag) => collected.push(flag),
                    _ => return Err(mixed()),
                }
            }
            Ok(PropertyValue::BooleanList(collected))
        }
        _ => Err(de::Error::custom(
            "a list member is a text, number, or boolean scalar",
        )),
    }
}

/// A record's flat property candidates, keyed by A1 property names.
///
/// Decoding refuses a name outside the A1 property-name grammar, a name core
/// owns or hoists, a duplicate key, and more entries than the schema admits.
/// It does **not** decide whether an accepted name is legal: that is
/// [`crate::declared::DeclaredPropertyIndex`]'s job, because it needs the
/// declaring manifest and the A1 shared registry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordProperties(BTreeMap<String, PropertyValue>);

impl RecordProperties {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        RecordProperties(BTreeMap::new())
    }

    /// Inserts a candidate, rejecting a name core owns or one outside the
    /// grammar.
    pub fn insert(&mut self, name: &str, value: PropertyValue) -> Result<(), &'static str> {
        if !is_lower_snake(name) {
            return Err("property name violates the A1 property-name grammar");
        }
        if is_core_owned_property(name) {
            return Err("property name is core-owned and structurally excluded from a record");
        }
        if self.0.len() >= MAX_RECORD_PROPERTIES && !self.0.contains_key(name) {
            return Err("record carries more property candidates than the schema admits");
        }
        self.0.insert(name.to_owned(), value);
        Ok(())
    }

    /// Iterates the candidates in ascending key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }

    /// The number of candidates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Looks up one candidate.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&PropertyValue> {
        self.0.get(name)
    }
}

impl Serialize for RecordProperties {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RecordProperties {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor;

        impl<'de> Visitor<'de> for MapVisitor {
            type Value = RecordProperties;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a flat map of A1 property candidates")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut collected = BTreeMap::new();
                while let Some(name) = access.next_key::<String>()? {
                    let value = access.next_value::<PropertyValue>()?;
                    if !is_lower_snake(&name) {
                        return Err(de::Error::custom(format!(
                            "property name {name:?} violates the A1 property-name grammar"
                        )));
                    }
                    if is_core_owned_property(&name) {
                        return Err(de::Error::custom(format!(
                            "property name {name:?} is core-owned: record IDs, producer \
                             provenance, capture time, hashes, the source key, and rebuildable \
                             projections are structurally excluded from a record"
                        )));
                    }
                    if collected.len() >= MAX_RECORD_PROPERTIES {
                        return Err(de::Error::custom(format!(
                            "more than {MAX_RECORD_PROPERTIES} property candidates in one record"
                        )));
                    }
                    if collected.insert(name.clone(), value).is_some() {
                        return Err(de::Error::custom(format!(
                            "duplicate property key {name:?}"
                        )));
                    }
                }
                Ok(RecordProperties(collected))
            }
        }

        deserializer.deserialize_map(MapVisitor)
    }
}

/// Non-secret connector configuration: flat scalars and homogeneous scalar
/// lists only.
///
/// `config` is non-secret by construction, because core never puts credential
/// material there. A Field must not treat any value here as a secret.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigMap(BTreeMap<PropertyNameToken, PropertyValue>);

impl ConfigMap {
    /// An empty configuration.
    #[must_use]
    pub fn new() -> Self {
        ConfigMap(BTreeMap::new())
    }

    /// Inserts one entry.
    pub fn insert(
        &mut self,
        name: PropertyNameToken,
        value: PropertyValue,
    ) -> Option<PropertyValue> {
        self.0.insert(name, value)
    }

    /// Looks up one entry.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&PropertyValue> {
        self.0
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value)
    }

    /// Iterates the entries in ascending key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the configuration is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for ConfigMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConfigMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = BTreeMap::<PropertyNameToken, PropertyValue>::deserialize(deserializer)?;
        if raw.len() > MAX_CONFIG_PROPERTIES {
            return Err(de::Error::custom(format!(
                "more than {MAX_CONFIG_PROPERTIES} configuration entries"
            )));
        }
        Ok(ConfigMap(raw))
    }
}

/// One member of a diagnostic's structured detail.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum DetailValue {
    /// A bounded, already-redacted string.
    Text(String),
    /// A finite number, in its wire spelling.
    Number(serde_json::Number),
    /// A boolean.
    Boolean(bool),
}

impl<'de> Deserialize<'de> for DetailValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(value) if value.len() <= MAX_DETAIL_TEXT_BYTES => {
                Ok(DetailValue::Text(value))
            }
            serde_json::Value::String(value) => Err(de::Error::custom(format!(
                "diagnostic detail text of {} bytes exceeds the {MAX_DETAIL_TEXT_BYTES}-byte bound",
                value.len()
            ))),
            serde_json::Value::Number(number) => Ok(DetailValue::Number(finite(number)?)),
            serde_json::Value::Bool(flag) => Ok(DetailValue::Boolean(flag)),
            _ => Err(de::Error::custom(
                "diagnostic detail carries bounded scalars only, never a source payload",
            )),
        }
    }
}

/// Bounded, already-redacted structured diagnostic detail.
///
/// Never a source payload, HTTP trace, credential, or protected-channel value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiagnosticDetail(BTreeMap<PropertyNameToken, DetailValue>);

impl DiagnosticDetail {
    /// An empty detail map.
    #[must_use]
    pub fn new() -> Self {
        DiagnosticDetail(BTreeMap::new())
    }

    /// Inserts one member.
    pub fn insert(&mut self, name: PropertyNameToken, value: DetailValue) -> Option<DetailValue> {
        self.0.insert(name, value)
    }

    /// Iterates the members in ascending key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &DetailValue)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }

    /// The number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the detail map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for DiagnosticDetail {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DiagnosticDetail {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = BTreeMap::<PropertyNameToken, DetailValue>::deserialize(deserializer)?;
        if raw.len() > MAX_DETAIL_PROPERTIES {
            return Err(de::Error::custom(format!(
                "more than {MAX_DETAIL_PROPERTIES} diagnostic detail members"
            )));
        }
        Ok(DiagnosticDetail(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<PropertyValue, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn scalars_and_homogeneous_lists_decode() -> Result<(), serde_json::Error> {
        assert_eq!(parse("\"normal\"")?, PropertyValue::Text("normal".into()));
        assert_eq!(
            parse("42")?,
            PropertyValue::Number(serde_json::Number::from(42))
        );
        assert_eq!(parse("true")?, PropertyValue::Boolean(true));
        assert_eq!(
            parse("[\"contracts\",\"legal\"]")?,
            PropertyValue::TextList(vec!["contracts".into(), "legal".into()])
        );
        Ok(())
    }

    #[test]
    fn mixed_empty_and_nested_values_are_refused() {
        assert!(parse("[\"a\",1]").is_err());
        assert!(parse("[]").is_err());
        assert!(parse("null").is_err());
        assert!(parse("{\"nested\":1}").is_err());
        assert!(parse("[[1]]").is_err());
    }

    #[test]
    fn a_core_owned_property_name_fails_to_decode() {
        let json = r#"{"title":"Rollout reference","content_hash":"fn-content-v1-sha256:0000"}"#;
        let error = serde_json::from_str::<RecordProperties>(json)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("core-owned"), "unexpected error: {error}");
    }

    #[test]
    fn a_record_id_a_field_supplied_is_structurally_impossible() {
        assert!(serde_json::from_str::<RecordProperties>(r#"{"id":"note_01"}"#).is_err());
        assert!(serde_json::from_str::<RecordProperties>(r#"{"artifacts":["a"]}"#).is_err());
        assert!(serde_json::from_str::<RecordProperties>(r#"{"identities":["a"]}"#).is_err());
    }

    #[test]
    fn an_ill_formed_property_name_fails_to_decode() {
        assert!(serde_json::from_str::<RecordProperties>(r#"{"Title":"x"}"#).is_err());
        assert!(serde_json::from_str::<RecordProperties>(r#"{"with-hyphen":"x"}"#).is_err());
        assert!(serde_json::from_str::<RecordProperties>(r#"{"":"x"}"#).is_err());
    }

    #[test]
    fn duplicate_keys_are_refused() {
        assert!(serde_json::from_str::<RecordProperties>(r#"{"title":"a","title":"b"}"#).is_err());
    }

    #[test]
    fn an_oversized_text_scalar_is_refused() {
        let long = "a".repeat(MAX_TEXT_SCALAR_BYTES + 1);
        let json = serde_json::to_string(&long).unwrap_or_default();
        assert!(parse(&json).is_err());
    }

    #[test]
    fn member_counts_and_sizes_are_measured() {
        let value = PropertyValue::TextList(vec!["ab".into(), "abcd".into()]);
        assert_eq!(value.member_count(), 2);
        assert_eq!(value.max_member_bytes(), 4);
        assert!(value.is_list());
        assert!(!PropertyValue::Boolean(false).is_list());
    }
}
