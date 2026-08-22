//! Note filename computation and whole-name validation.
//!
//! The canonical Note filename is
//! `<YYYYMMDDTHHMMSSZ>_<field-id>_<type>_<note-id>.md`, rendering the
//! `occurred_at` instant in UTC at whole-second precision. Frontmatter is
//! authoritative: validators compute the expected name from validated
//! frontmatter and compare the whole filename, never splitting it on
//! underscores.

use fieldnotes_domain::{RecordKind, Scalar, Value};

use crate::error::ValidationError;
use crate::record::ParsedRecord;

/// Computes the canonical filename for a validated Note record.
pub fn expected_note_filename(record: &ParsedRecord) -> Result<String, ValidationError> {
    if record.kind() != RecordKind::Note {
        return Err(ValidationError::WrongIdKind {
            key: "id".to_owned(),
        });
    }
    let occurred_at = record
        .occurred_at()
        .ok_or_else(|| ValidationError::MissingRequired {
            key: "occurred_at".to_owned(),
        })?;
    let timestamp = occurred_at
        .filename_utc()
        .map_err(|_| ValidationError::InvalidDatetime {
            key: "occurred_at".to_owned(),
        })?;
    let field_id = get_text(record, "field_id")?;
    let note_type = get_text(record, "type")?;
    Ok(format!(
        "{timestamp}_{field_id}_{note_type}_{}.md",
        record.id()
    ))
}

fn get_text<'a>(record: &'a ParsedRecord, key: &str) -> Result<&'a str, ValidationError> {
    match record.get(key) {
        Some(Value::Scalar(Scalar::Text(text))) => Ok(text.as_str()),
        _ => Err(ValidationError::MissingRequired {
            key: key.to_owned(),
        }),
    }
}

/// Compares the actual filename with the computed canonical name.
pub fn validate_note_filename(record: &ParsedRecord, actual: &str) -> Result<(), ValidationError> {
    let expected = expected_note_filename(record)?;
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::FilenameMismatch {
            expected,
            actual: actual.to_owned(),
        })
    }
}
