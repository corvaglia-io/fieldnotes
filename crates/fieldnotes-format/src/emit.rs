//! Canonical serialization: the exact approved byte form of public records.
//!
//! Property order is the five structural Note properties (or `id`, `type` for
//! non-Notes) followed by every remaining property in ascending ASCII byte
//! order. Scalars use the approved deterministic spelling; set-like lists are
//! deduplicated and sorted; lists use block style with `- ` items indented two
//! spaces.

use fieldnotes_domain::{Scalar, Value};

use crate::error::ValidationError;
use crate::jcs;
use crate::record::ParsedRecord;
use crate::registry::{ListSemantics, PropertyRegistry, PropertyType, SEMANTIC_EXCLUSIONS};
use crate::yaml::core_schema_resolves_string;

/// Whether text may use YAML plain style: it matches
/// `[A-Za-z0-9_./@+-]+(?: [A-Za-z0-9_./@+-]+)*` and resolves as a string under
/// the YAML 1.2 Core Schema.
#[must_use]
pub fn plain_style_allowed(text: &str) -> bool {
    fn allowed(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'@' | b'+' | b'-')
    }
    if text.is_empty() {
        return false;
    }
    let tokens_ok = text
        .split(' ')
        .all(|token| !token.is_empty() && token.bytes().all(allowed));
    tokens_ok && core_schema_resolves_string(text)
}

fn emit_scalar(scalar: &Scalar, key: &str) -> Result<String, ValidationError> {
    match scalar {
        Scalar::Text(text) => {
            if plain_style_allowed(text) {
                Ok(text.clone())
            } else {
                Ok(jcs::serialize_string(text))
            }
        }
        Scalar::Number(value) => {
            jcs::format_number(*value).ok_or_else(|| ValidationError::NonFiniteNumber {
                key: key.to_owned(),
            })
        }
        Scalar::Bool(value) => Ok(if *value { "true" } else { "false" }.to_owned()),
        Scalar::Date(date) => Ok(date.to_string()),
        Scalar::Datetime(datetime) => Ok(datetime.to_string()),
    }
}

fn emit_entry(
    out: &mut String,
    key: &str,
    value: &Value,
    registry: &PropertyRegistry,
) -> Result<(), ValidationError> {
    match value {
        Value::Scalar(scalar) => {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&emit_scalar(scalar, key)?);
            out.push('\n');
        }
        Value::List(items) => {
            // Empty lists are omitted rather than serialized.
            if items.is_empty() {
                return Ok(());
            }
            // Pair each item with its normalized text sort key so set-like
            // ordering follows the value, not its quoted rendering.
            let mut rendered = Vec::with_capacity(items.len());
            for item in items {
                let sort_key = match item {
                    Scalar::Text(text) => text.clone(),
                    other => emit_scalar(other, key)?,
                };
                rendered.push((sort_key, emit_scalar(item, key)?));
            }
            if matches!(
                registry.lookup(key),
                Some(PropertyType::List(_, ListSemantics::Set))
            ) {
                rendered.sort_unstable();
                rendered.dedup();
            }
            let rendered: Vec<String> = rendered.into_iter().map(|(_, text)| text).collect();
            out.push_str(key);
            out.push_str(":\n");
            for item in rendered {
                out.push_str("  - ");
                out.push_str(&item);
                out.push('\n');
            }
        }
    }
    Ok(())
}

/// The record's entries in canonical order: the structural keys it actually
/// carries, then every remaining key in ascending ASCII byte order.
fn ordered_entries<'a>(
    record: &'a ParsedRecord,
    structural: &[&'a str],
) -> Vec<(&'a str, &'a Value)> {
    let mut rest: Vec<(&str, &Value)> = record
        .entries()
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .filter(|(key, _)| !structural.contains(key))
        .collect();
    rest.sort_unstable_by_key(|(key, _)| *key);
    let mut ordered: Vec<(&str, &Value)> = Vec::with_capacity(record.entries().len());
    for key in structural {
        if let Some(value) = record.get(key) {
            ordered.push((*key, value));
        }
    }
    ordered.extend(rest);
    ordered
}

const NOTE_STRUCTURAL: [&str; 5] = ["id", "instance_id", "field_id", "type", "occurred_at"];
const RECORD_STRUCTURAL: [&str; 2] = ["id", "type"];

/// Serializes a validated record to its exact canonical byte form.
pub fn canonical_record_string(record: &ParsedRecord) -> Result<String, ValidationError> {
    let structural: &[&str] = if record.kind() == fieldnotes_domain::RecordKind::Note {
        &NOTE_STRUCTURAL
    } else {
        &RECORD_STRUCTURAL
    };
    let registry = PropertyRegistry::v1();
    let mut out = String::new();
    out.push_str("---\n");
    for (key, value) in ordered_entries(record, structural) {
        emit_entry(&mut out, key, value, registry)?;
    }
    out.push_str("---\n\n");
    out.push_str(record.body());
    Ok(out)
}

fn to_utc_scalar(scalar: &Scalar) -> Result<Scalar, ValidationError> {
    if let Scalar::Datetime(value) = scalar {
        let utc = value
            .to_utc()
            .map_err(|_| ValidationError::InvalidDatetime { key: String::new() })?;
        Ok(Scalar::Datetime(utc))
    } else {
        Ok(scalar.clone())
    }
}

/// Serializes the `fn-record-v1` canonical semantic encoding of a record:
/// bookkeeping exclusions removed, every retained key in ascending ASCII
/// order, and datetimes rendered as their instant in UTC `+00:00`.
pub fn semantic_record_string(record: &ParsedRecord) -> Result<String, ValidationError> {
    let registry = PropertyRegistry::v1();
    let mut retained: Vec<(&str, &Value)> = record
        .entries()
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .filter(|(key, _)| !SEMANTIC_EXCLUSIONS.contains(key))
        .collect();
    retained.sort_unstable_by_key(|(key, _)| *key);

    let mut out = String::new();
    out.push_str("---\n");
    for (key, value) in retained {
        let converted = match value {
            Value::Scalar(scalar) => Value::Scalar(to_utc_scalar(scalar)?),
            Value::List(items) => Value::List(
                items
                    .iter()
                    .map(to_utc_scalar)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        };
        emit_entry(&mut out, key, &converted, registry)?;
    }
    out.push_str("---\n\n");
    out.push_str(record.body());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_style_rule_matches_the_contract() {
        assert!(plain_style_allowed("alice@example.com"));
        assert!(plain_style_allowed("In Progress"));
        assert!(plain_style_allowed("mail/message/AAMk123"));
        assert!(plain_style_allowed("ACME-42"));
        assert!(!plain_style_allowed("1745317800000"));
        assert!(!plain_style_allowed("true"));
        assert!(!plain_style_allowed("microsoft-graph:tenant/8d82"));
        assert!(!plain_style_allowed("Alice Müller"));
        assert!(!plain_style_allowed("two  spaces"));
        assert!(!plain_style_allowed(" leading"));
        assert!(!plain_style_allowed("trailing "));
        assert!(!plain_style_allowed(""));
        assert!(!plain_style_allowed("<angle@example.com>"));
    }

    #[test]
    fn set_like_lists_are_sorted_and_deduplicated() -> Result<(), ValidationError> {
        let bytes = b"---\nid: ent_01a028f2-dcc0-7000-8000-000000000001\ntype: person\nidentities:\n  - \"email:b@example.com\"\n  - \"email:a@example.com\"\n  - \"email:b@example.com\"\n---\n\nBody.\n";
        let record = crate::record::parse_record(bytes)?;
        let canonical = canonical_record_string(&record)?;
        assert!(
            canonical.contains(
                "identities:\n  - \"email:a@example.com\"\n  - \"email:b@example.com\"\n"
            )
        );
        assert_eq!(canonical.matches("email:b@example.com").count(), 1);
        Ok(())
    }
}
