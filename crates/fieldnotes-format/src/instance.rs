//! The `.fieldnotes/instance.yaml` operational metadata exception.
//!
//! The exact A1 schema and canonical key order are `instance_id`,
//! `created_at`, and the optional display-only `name`. The file uses UTF-8,
//! LF, exactly one final LF, and the A1 scalar rules, but no `---` delimiters
//! or Markdown body. The `fn_` UUIDv7 timestamp is the same creation instant
//! as `created_at`, to millisecond precision.

use fieldnotes_domain::{Datetime, RecordId, RecordKind};

use crate::emit::plain_style_allowed;
use crate::error::ValidationError;
use crate::jcs;
use crate::yaml::{self, RawValue, ScalarStyle};

/// Parsed instance metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceMetadata {
    /// The `fn_`-prefixed instance ID.
    pub instance_id: RecordId,
    /// The explicit-offset creation instant.
    pub created_at: Datetime,
    /// The optional, non-secret, display-only name.
    pub name: Option<String>,
}

fn invalid(reason: &'static str) -> ValidationError {
    ValidationError::InvalidInstanceMetadata { reason }
}

/// Parses and validates `.fieldnotes/instance.yaml` bytes.
pub fn parse_instance_yaml(bytes: &[u8]) -> Result<InstanceMetadata, ValidationError> {
    let text = core::str::from_utf8(bytes).map_err(|_| ValidationError::InvalidUtf8)?;
    let Some(body) = text.strip_suffix('\n') else {
        return Err(invalid("missing final LF"));
    };
    if body.ends_with('\n') {
        return Err(invalid("more than one final LF"));
    }
    let lines: Vec<&str> = body.split('\n').collect();
    let entries = yaml::parse_flat_block(&lines)?;
    if entries.len() < 2 || entries.len() > 3 {
        return Err(invalid(
            "expected exactly instance_id, created_at, and optional name",
        ));
    }
    if entries[0].key != "instance_id" || entries[1].key != "created_at" {
        return Err(invalid(
            "canonical key order is instance_id, created_at, name",
        ));
    }

    let RawValue::Scalar(id_scalar) = &entries[0].value else {
        return Err(invalid("instance_id must be a scalar"));
    };
    if id_scalar.style != ScalarStyle::Plain {
        return Err(invalid("instance_id must be plain text"));
    }
    let instance_id = RecordId::parse(&id_scalar.text).map_err(|_| ValidationError::InvalidId {
        key: "instance_id".to_owned(),
    })?;
    if instance_id.kind() != RecordKind::Instance {
        return Err(ValidationError::WrongIdKind {
            key: "instance_id".to_owned(),
        });
    }

    let RawValue::Scalar(created_scalar) = &entries[1].value else {
        return Err(invalid("created_at must be a scalar"));
    };
    if created_scalar.style != ScalarStyle::Plain {
        return Err(invalid("created_at must be a plain datetime"));
    }
    let created_at = Datetime::parse(&created_scalar.text).map_err(|error| {
        use fieldnotes_domain::datetime::DatetimeError;
        match error {
            DatetimeError::OffsetRequired => ValidationError::OffsetRequired {
                key: "created_at".to_owned(),
            },
            DatetimeError::NegativeZeroOffset => ValidationError::NegativeZeroOffset {
                key: "created_at".to_owned(),
            },
            _ => ValidationError::InvalidDatetime {
                key: "created_at".to_owned(),
            },
        }
    })?;

    // The UUIDv7 timestamp is the same creation instant, to the millisecond.
    let uuid_millis = i64::try_from(instance_id.uuid().unix_millis())
        .map_err(|_| invalid("instance UUIDv7 timestamp out of range"))?;
    if uuid_millis != created_at.unix_millis() {
        return Err(invalid(
            "instance UUIDv7 timestamp disagrees with created_at",
        ));
    }

    let name = match entries.get(2) {
        None => None,
        Some(entry) => {
            if entry.key != "name" {
                return Err(invalid("the only optional third key is name"));
            }
            let RawValue::Scalar(scalar) = &entry.value else {
                return Err(invalid("name must be a text scalar"));
            };
            if scalar.style == ScalarStyle::Plain
                && !yaml::core_schema_resolves_string(&scalar.text)
            {
                return Err(invalid("name must be a text scalar"));
            }
            Some(scalar.text.clone())
        }
    };

    Ok(InstanceMetadata {
        instance_id,
        created_at,
        name,
    })
}

/// Serializes instance metadata in the exact canonical form.
#[must_use]
pub fn instance_yaml_string(metadata: &InstanceMetadata) -> String {
    let mut out = String::new();
    out.push_str("instance_id: ");
    out.push_str(&metadata.instance_id.to_string());
    out.push('\n');
    out.push_str("created_at: ");
    out.push_str(&metadata.created_at.to_string());
    out.push('\n');
    if let Some(name) = &metadata.name {
        out.push_str("name: ");
        if plain_style_allowed(name) {
            out.push_str(name);
        } else {
            out.push_str(&jcs::serialize_string(name));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_canonical_metadata() -> Result<(), ValidationError> {
        let text = "instance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a\ncreated_at: 2026-08-22T08:45:00+02:00\nname: fixture-workstation\n";
        let metadata = parse_instance_yaml(text.as_bytes())?;
        assert_eq!(metadata.name.as_deref(), Some("fixture-workstation"));
        assert_eq!(instance_yaml_string(&metadata), text);
        Ok(())
    }

    #[test]
    fn rejects_schema_violations() {
        let wrong_order = "created_at: 2026-08-22T08:45:00+02:00\ninstance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a\n";
        assert!(parse_instance_yaml(wrong_order.as_bytes()).is_err());
        let extra_key = "instance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a\ncreated_at: 2026-08-22T08:45:00+02:00\nextra: x\n";
        assert!(parse_instance_yaml(extra_key.as_bytes()).is_err());
        let drifted = "instance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a\ncreated_at: 2026-08-22T08:45:01+02:00\n";
        assert!(parse_instance_yaml(drifted.as_bytes()).is_err());
        let note_id = "instance_id: note_01a02837-2de0-7a2b-8c41-f2481851192a\ncreated_at: 2026-08-22T08:45:00+02:00\n";
        assert!(matches!(
            parse_instance_yaml(note_id.as_bytes()),
            Err(ValidationError::WrongIdKind { .. })
        ));
    }
}
