//! Parsing and validation of public notebook record files.
//!
//! A public record file is UTF-8 without BOM, begins with one `---`-delimited
//! frontmatter document, and contains exactly one blank line between closing
//! frontmatter and Markdown body. Parsers may accept CRLF input and
//! non-canonical key order; the canonical emitter rewrites both.

use std::collections::BTreeSet;

use fieldnotes_domain::datetime::DatetimeError;
use fieldnotes_domain::property::is_valid_property_name;
use fieldnotes_domain::{
    ArtifactId, Date, Datetime, FieldId, FieldStemRegistry, NoteType, RecordId, RecordKind, Scalar,
    ScalarKind, Value,
};

use crate::error::ValidationError;
use crate::jcs;
use crate::normalize::{self, normalize_body_str};
use crate::registry::{PropertyRegistry, PropertyType};
use crate::yaml::{self, RawScalar, RawValue, ScalarStyle};

/// A parsed public record: typed frontmatter plus a normalized Markdown body.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRecord {
    id: RecordId,
    entries: Vec<(String, Value)>,
    body: String,
}

impl ParsedRecord {
    /// Assembles a record from already-typed values.
    ///
    /// Crate-internal on purpose: [`crate::build::RecordBuilder`] is the only
    /// supported construction path, because it re-parses and validates the
    /// emitted canonical bytes before any caller can persist them.
    pub(crate) fn from_typed(id: RecordId, entries: Vec<(String, Value)>, body: String) -> Self {
        ParsedRecord { id, entries, body }
    }

    /// The record's kind-prefixed logical ID.
    #[must_use]
    pub fn id(&self) -> &RecordId {
        &self.id
    }

    /// The record kind derived from the ID prefix.
    #[must_use]
    pub fn kind(&self) -> RecordKind {
        self.id.kind()
    }

    /// The typed frontmatter entries in input order.
    #[must_use]
    pub fn entries(&self) -> &[(String, Value)] {
        &self.entries
    }

    /// Looks up one property value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// The normalized Markdown body, ending with exactly one LF.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    fn get_text(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::Scalar(Scalar::Text(text))) => Some(text),
            _ => None,
        }
    }

    fn get_datetime(&self, key: &str) -> Option<&Datetime> {
        match self.get(key) {
            Some(Value::Scalar(Scalar::Datetime(value))) => Some(value),
            _ => None,
        }
    }

    /// The Note `occurred_at` instant, when present and typed.
    #[must_use]
    pub fn occurred_at(&self) -> Option<&Datetime> {
        self.get_datetime("occurred_at")
    }
}

/// Splits a record file into frontmatter text and raw body text.
///
/// Accepts CRLF/CR line endings (normalized to LF before scanning) and strips
/// one leading BOM at ingestion.
fn split_file(bytes: &[u8]) -> Result<(Vec<String>, String), ValidationError> {
    let text = core::str::from_utf8(bytes).map_err(|_| ValidationError::InvalidUtf8)?;
    let text = normalize::to_lf(normalize::strip_bom(text));
    let rest = text
        .strip_prefix("---\n")
        .ok_or(ValidationError::MissingOpeningDelimiter)?;
    let mut frontmatter_lines = Vec::new();
    let mut remainder = rest;
    loop {
        let Some((line, rest)) = remainder.split_once('\n') else {
            return Err(ValidationError::MissingClosingDelimiter);
        };
        if line == "---" {
            remainder = rest;
            break;
        }
        frontmatter_lines.push(line.to_owned());
        remainder = rest;
    }
    // Exactly one blank separator line, then the body. The separator is a file
    // separator rather than the first body byte, so a body may not itself begin
    // with a blank line: that would both break the canonical grammar and give
    // two records with the same evidence different semantic fingerprints.
    let body = remainder
        .strip_prefix('\n')
        .ok_or(ValidationError::MissingBodySeparator)?;
    if body.starts_with('\n') {
        return Err(ValidationError::ExtraBodySeparator);
    }
    Ok((frontmatter_lines, body.to_owned()))
}

fn coerce_scalar(raw: &RawScalar, kind: ScalarKind, key: &str) -> Result<Scalar, ValidationError> {
    match kind {
        ScalarKind::Text => match raw.style {
            ScalarStyle::DoubleQuoted => Ok(Scalar::Text(raw.text.clone())),
            ScalarStyle::Plain => {
                if yaml::core_schema_resolves_string(&raw.text) {
                    Ok(Scalar::Text(raw.text.clone()))
                } else {
                    Err(ValidationError::TypeMismatch {
                        key: key.to_owned(),
                        expected: ScalarKind::Text,
                    })
                }
            }
        },
        ScalarKind::Number => {
            if raw.style != ScalarStyle::Plain {
                return Err(ValidationError::TypeMismatch {
                    key: key.to_owned(),
                    expected: ScalarKind::Number,
                });
            }
            match jcs::parse_number(&raw.text) {
                Ok(Some(value)) => Ok(Scalar::Number(value)),
                Ok(None) => Err(ValidationError::TypeMismatch {
                    key: key.to_owned(),
                    expected: ScalarKind::Number,
                }),
                Err(error) => Err(number_error(error, key)),
            }
        }
        ScalarKind::Bool => match (raw.style, raw.text.as_str()) {
            (ScalarStyle::Plain, "true") => Ok(Scalar::Bool(true)),
            (ScalarStyle::Plain, "false") => Ok(Scalar::Bool(false)),
            _ => Err(ValidationError::TypeMismatch {
                key: key.to_owned(),
                expected: ScalarKind::Bool,
            }),
        },
        ScalarKind::Date => {
            if raw.style != ScalarStyle::Plain {
                return Err(ValidationError::TypeMismatch {
                    key: key.to_owned(),
                    expected: ScalarKind::Date,
                });
            }
            Date::parse(&raw.text)
                .map(Scalar::Date)
                .map_err(|error| datetime_error(error, key))
        }
        ScalarKind::Datetime => {
            if raw.style != ScalarStyle::Plain {
                return Err(ValidationError::TypeMismatch {
                    key: key.to_owned(),
                    expected: ScalarKind::Datetime,
                });
            }
            Datetime::parse(&raw.text)
                .map(Scalar::Datetime)
                .map_err(|error| datetime_error(error, key))
        }
    }
}

fn number_error(error: jcs::NumberError, key: &str) -> ValidationError {
    match error {
        jcs::NumberError::Malformed => ValidationError::InvalidNumber {
            key: key.to_owned(),
        },
        jcs::NumberError::OutOfRange => ValidationError::IntegerOutOfRange {
            key: key.to_owned(),
        },
        jcs::NumberError::NonFinite => ValidationError::NonFiniteNumber {
            key: key.to_owned(),
        },
    }
}

fn datetime_error(error: DatetimeError, key: &str) -> ValidationError {
    match error {
        DatetimeError::OffsetRequired => ValidationError::OffsetRequired {
            key: key.to_owned(),
        },
        DatetimeError::NegativeZeroOffset => ValidationError::NegativeZeroOffset {
            key: key.to_owned(),
        },
        _ => ValidationError::InvalidDatetime {
            key: key.to_owned(),
        },
    }
}

fn looks_like_datetime(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() >= 11
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'T'
}

/// Infers the scalar for a source-prefixed property with no registry entry.
fn infer_scalar(raw: &RawScalar, key: &str) -> Result<Scalar, ValidationError> {
    match raw.style {
        ScalarStyle::DoubleQuoted => Ok(Scalar::Text(raw.text.clone())),
        ScalarStyle::Plain => {
            let token = raw.text.as_str();
            if looks_like_datetime(token) {
                // Plain datetime-shaped tokens are datetimes; a canonical text
                // value of this shape would be double-quoted (it contains `:`).
                return Datetime::parse(token)
                    .map(Scalar::Datetime)
                    .map_err(|error| datetime_error(error, key));
            }
            if Date::parse(token).is_ok() {
                return Date::parse(token)
                    .map(Scalar::Date)
                    .map_err(|error| datetime_error(error, key));
            }
            match token {
                "true" => return Ok(Scalar::Bool(true)),
                "false" => return Ok(Scalar::Bool(false)),
                _ => {}
            }
            if yaml::core_schema_resolves_string(token) {
                return Ok(Scalar::Text(token.to_owned()));
            }
            // Remaining plain non-strings must be canonical JSON numbers.
            match jcs::parse_number(token) {
                Ok(Some(value)) => Ok(Scalar::Number(value)),
                Ok(None) => Err(ValidationError::InvalidNumber {
                    key: key.to_owned(),
                }),
                Err(error) => Err(number_error(error, key)),
            }
        }
    }
}

fn type_entries(
    raw_entries: Vec<crate::yaml::RawEntry>,
    registry: &PropertyRegistry,
    stems: &FieldStemRegistry,
) -> Result<Vec<(String, Value)>, ValidationError> {
    let mut entries = Vec::with_capacity(raw_entries.len());
    for entry in raw_entries {
        let key = entry.key;
        let registered = registry.lookup(&key);
        if registered.is_none() {
            if !is_valid_property_name(&key) {
                return Err(ValidationError::InvalidPropertyName { key });
            }
            if !stems.has_registered_prefix(&key) {
                return Err(ValidationError::UnknownUnprefixed { key });
            }
        }
        let value = match (registered, entry.value) {
            (Some(PropertyType::Scalar(kind)), RawValue::Scalar(raw)) => {
                Value::Scalar(coerce_scalar(&raw, kind, &key)?)
            }
            (Some(PropertyType::Scalar(_)), RawValue::List(_)) => {
                return Err(ValidationError::ScalarRequired { key });
            }
            (Some(PropertyType::List(..)), RawValue::Scalar(_)) => {
                return Err(ValidationError::ListRequired { key });
            }
            (Some(PropertyType::List(kind, _)), RawValue::List(items)) => {
                let mut scalars = Vec::with_capacity(items.len());
                for item in &items {
                    match coerce_scalar(item, kind, &key) {
                        Ok(scalar) => scalars.push(scalar),
                        Err(ValidationError::TypeMismatch { .. }) => {
                            return Err(ValidationError::MixedList { key });
                        }
                        Err(other) => return Err(other),
                    }
                }
                Value::List(scalars)
            }
            (None, RawValue::Scalar(raw)) => Value::Scalar(infer_scalar(&raw, &key)?),
            (None, RawValue::List(items)) => {
                let mut scalars: Vec<Scalar> = Vec::with_capacity(items.len());
                for item in &items {
                    let scalar = infer_scalar(item, &key)?;
                    if let Some(first) = scalars.first()
                        && first.kind() != scalar.kind()
                    {
                        return Err(ValidationError::MixedList { key });
                    }
                    scalars.push(scalar);
                }
                Value::List(scalars)
            }
        };
        entries.push((key, value));
    }
    Ok(entries)
}

/// Parses one public record file into typed frontmatter and a normalized body.
///
/// Uses the frozen v1 property registry and Field-stem registry.
pub fn parse_record(bytes: &[u8]) -> Result<ParsedRecord, ValidationError> {
    let (frontmatter_lines, body) = split_file(bytes)?;
    let line_refs: Vec<&str> = frontmatter_lines.iter().map(String::as_str).collect();
    let raw_entries = yaml::parse_flat_block(&line_refs)?;
    let stems = FieldStemRegistry::v1();
    let entries = type_entries(raw_entries, PropertyRegistry::v1(), stems)?;
    let id_text = entries
        .iter()
        .find(|(key, _)| key == "id")
        .and_then(|(_, value)| match value {
            Value::Scalar(Scalar::Text(text)) => Some(text.as_str()),
            _ => None,
        })
        .ok_or(ValidationError::MissingRequired {
            key: "id".to_owned(),
        })?;
    let id = RecordId::parse(id_text).map_err(|_| ValidationError::InvalidId {
        key: "id".to_owned(),
    })?;
    Ok(ParsedRecord {
        id,
        entries,
        body: normalize_body_str(&body),
    })
}

fn is_valid_record_type(text: &str) -> bool {
    // Non-Note record types use the lowercase type grammar with the shared
    // 63-byte property-name-style bound; the frozen corpus includes a 34-byte
    // Observation type, so the 32-byte primary-Note bound cannot apply here.
    !text.is_empty()
        && text.len() <= 63
        && text.as_bytes()[0].is_ascii_lowercase()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn require_text<'a>(record: &'a ParsedRecord, key: &str) -> Result<&'a str, ValidationError> {
    record
        .get_text(key)
        .ok_or_else(|| ValidationError::MissingRequired {
            key: key.to_owned(),
        })
}

fn validate_artifact_list(record: &ParsedRecord, key: &str) -> Result<(), ValidationError> {
    if let Some(Value::List(items)) = record.get(key) {
        for item in items {
            if let Scalar::Text(text) = item
                && ArtifactId::parse(text).is_err()
            {
                return Err(ValidationError::InvalidId {
                    key: key.to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn validate_note(record: &ParsedRecord) -> Result<(), ValidationError> {
    // The five required structural properties.
    let instance_id = require_text(record, "instance_id")?;
    let instance = RecordId::parse(instance_id).map_err(|_| ValidationError::InvalidId {
        key: "instance_id".to_owned(),
    })?;
    if instance.kind() != RecordKind::Instance {
        return Err(ValidationError::WrongIdKind {
            key: "instance_id".to_owned(),
        });
    }
    let field_id = require_text(record, "field_id")?;
    let stems = FieldStemRegistry::v1();
    FieldId::parse(field_id, stems).map_err(|_| ValidationError::InvalidFieldId {
        value: field_id.to_owned(),
    })?;

    // Connector-prefixed properties may only bind to this Note's own Field:
    // a property carrying a different registered stem's prefix is a
    // connector-boundary violation, not evidence this Note is entitled to
    // carry. Unprefixed shared-registry properties are unaffected. `self`
    // contributes no prefix, so a `self` Note may carry only unprefixed
    // registry properties; that falls out of this general rule rather than
    // needing a special case.
    let own_prefix = stems.property_prefix_for(field_id);
    let registry = PropertyRegistry::v1();
    for (key, _) in record.entries() {
        if registry.lookup(key).is_some() {
            continue;
        }
        if let Some(prefix) = stems.property_prefix_for(key)
            && Some(prefix) != own_prefix
        {
            return Err(ValidationError::ForeignPrefix { key: key.clone() });
        }
    }

    let note_type = require_text(record, "type")?;
    if NoteType::parse(note_type).is_none() {
        return Err(ValidationError::UnknownNoteType {
            value: note_type.to_owned(),
        });
    }
    if record.occurred_at().is_none() {
        return Err(ValidationError::MissingRequired {
            key: "occurred_at".to_owned(),
        });
    }

    // Portable source identity travels as a pair.
    let has_scope = record.get_text("source_scope").is_some();
    let has_identity = record.get_text("source_identity").is_some();
    if has_identity && !has_scope {
        return Err(ValidationError::ScopeRequired);
    }
    if has_scope && !has_identity {
        return Err(ValidationError::IdentityRequired);
    }

    // Artifact references are content-addressed IDs; every attachment also
    // appears in `artifacts`.
    validate_artifact_list(record, "artifacts")?;
    validate_artifact_list(record, "attachments")?;
    if let Some(Value::List(attachments)) = record.get("attachments") {
        let artifact_set: BTreeSet<&str> = match record.get("artifacts") {
            Some(Value::List(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Scalar::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect(),
            _ => BTreeSet::new(),
        };
        for attachment in attachments {
            if let Scalar::Text(text) = attachment
                && !artifact_set.contains(text.as_str())
            {
                return Err(ValidationError::AttachmentNotInArtifacts {
                    value: text.clone(),
                });
            }
        }
    }

    // content_hash carries the fn-content-v1 domain form when present.
    if let Some(hash) = record.get_text("content_hash") {
        let valid = hash
            .strip_prefix("fn-content-v1-sha256:")
            .is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            });
        if !valid {
            return Err(ValidationError::InvalidContentHash);
        }
    }

    // collected_by members are `<instance_id>/<field_id>` producer references.
    if let Some(Value::List(items)) = record.get("collected_by") {
        for item in items {
            let Scalar::Text(text) = item else { continue };
            let valid = text.split_once('/').is_some_and(|(instance, field)| {
                RecordId::parse(instance).is_ok_and(|id| id.kind() == RecordKind::Instance)
                    && FieldId::parse(field, stems).is_ok()
            });
            if !valid {
                return Err(ValidationError::InvalidId {
                    key: "collected_by".to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn validate_proposal(record: &ParsedRecord) -> Result<(), ValidationError> {
    if let Some(status) = record.get_text("status")
        && !matches!(status, "proposed" | "accepted" | "rejected" | "superseded")
    {
        return Err(ValidationError::UnknownProposalStatus {
            value: status.to_owned(),
        });
    }
    if let Some(binding) = record.get_text("binding_status") {
        let has_entity = record.get_text("entity_id").is_some();
        match binding {
            "bound" if has_entity => {}
            "unresolved" | "ambiguous" if !has_entity => {}
            _ => return Err(ValidationError::BindingStatusViolation),
        }
    }
    Ok(())
}

/// Validates a parsed record for its kind.
///
/// Filename agreement is a separate check because it needs the actual
/// filename; see [`crate::filename::validate_note_filename`].
pub fn validate_record(record: &ParsedRecord) -> Result<(), ValidationError> {
    let type_text = require_text(record, "type")?;
    match record.kind() {
        RecordKind::Note => validate_note(record)?,
        RecordKind::Proposal => {
            if !is_valid_record_type(type_text) {
                return Err(ValidationError::InvalidRecordType {
                    value: type_text.to_owned(),
                });
            }
            validate_proposal(record)?;
        }
        _ => {
            if !is_valid_record_type(type_text) {
                return Err(ValidationError::InvalidRecordType {
                    value: type_text.to_owned(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(frontmatter: &str, body: &str) -> Vec<u8> {
        format!("---\n{frontmatter}---\n\n{body}").into_bytes()
    }

    const MINIMAL: &str = "id: note_01a028d5-90c0-7248-a74b-c8bc1085ab19\n\
instance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a\n\
field_id: self\n\
type: text\n\
occurred_at: 2026-08-22T11:36:14+02:00\n";

    #[test]
    fn parses_and_validates_a_minimal_note() -> Result<(), ValidationError> {
        let record = parse_record(&note(MINIMAL, "# Title\n"))?;
        validate_record(&record)?;
        assert_eq!(record.kind(), RecordKind::Note);
        assert_eq!(record.body(), "# Title\n");
        Ok(())
    }

    #[test]
    fn rejects_more_than_one_blank_separator_line() {
        // A second blank line would otherwise become the first body byte,
        // giving identical evidence two different semantic fingerprints.
        let extra = format!("---\n{MINIMAL}---\n\n\n# Title\n").into_bytes();
        assert_eq!(
            parse_record(&extra),
            Err(ValidationError::ExtraBodySeparator)
        );
    }

    #[test]
    fn rejects_multibyte_text_in_a_datetime_property() {
        // Fixed byte offsets in datetime parsing must not slice mid-codepoint.
        let frontmatter = MINIMAL.replace(
            "occurred_at: 2026-08-22T11:36:14+02:00",
            "occurred_at: 2026-08-22T11:36:1é+02:00",
        );
        assert_eq!(
            parse_record(&note(&frontmatter, "# Title\n")),
            Err(ValidationError::InvalidDatetime {
                key: "occurred_at".to_owned()
            })
        );
    }

    #[test]
    fn accepts_crlf_input() -> Result<(), ValidationError> {
        let crlf = note(MINIMAL, "# Title\n")
            .iter()
            .flat_map(|&b| {
                if b == b'\n' {
                    vec![b'\r', b'\n']
                } else {
                    vec![b]
                }
            })
            .collect::<Vec<u8>>();
        let record = parse_record(&crlf)?;
        validate_record(&record)?;
        assert_eq!(record.body(), "# Title\n");
        Ok(())
    }

    #[test]
    fn rejects_source_identity_without_scope() -> Result<(), ValidationError> {
        let frontmatter = format!("{MINIMAL}source_identity: file/x\n");
        let record = parse_record(&note(&frontmatter, "b\n"))?;
        assert_eq!(
            validate_record(&record),
            Err(ValidationError::ScopeRequired)
        );
        Ok(())
    }

    #[test]
    fn rejects_unknown_unprefixed_properties() {
        let frontmatter = format!("{MINIMAL}chat_id: \"19:abc\"\n");
        assert!(matches!(
            parse_record(&note(&frontmatter, "b\n")),
            Err(ValidationError::UnknownUnprefixed { .. })
        ));
    }

    #[test]
    fn accepts_a_note_property_matching_its_own_field_prefix() -> Result<(), ValidationError> {
        let frontmatter = MINIMAL
            .replace("field_id: self", "field_id: outlook_mail_work")
            .replace("type: text", "type: mail");
        let frontmatter = format!("{frontmatter}outlook_mail_importance: normal\n");
        let record = parse_record(&note(&frontmatter, "b\n"))?;
        validate_record(&record)?;
        Ok(())
    }

    #[test]
    fn rejects_a_note_property_using_a_different_fields_prefix() -> Result<(), ValidationError> {
        let frontmatter = MINIMAL
            .replace("field_id: self", "field_id: outlook_mail_work")
            .replace("type: text", "type: mail");
        let frontmatter = format!("{frontmatter}teams_chat_id: \"19:abc\"\n");
        let record = parse_record(&note(&frontmatter, "b\n"))?;
        assert_eq!(
            validate_record(&record),
            Err(ValidationError::ForeignPrefix {
                key: "teams_chat_id".to_owned()
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_a_self_note_carrying_any_connector_prefixed_property() -> Result<(), ValidationError>
    {
        let frontmatter = format!("{MINIMAL}teams_chat_id: \"19:abc\"\n");
        let record = parse_record(&note(&frontmatter, "b\n"))?;
        assert_eq!(
            validate_record(&record),
            Err(ValidationError::ForeignPrefix {
                key: "teams_chat_id".to_owned()
            })
        );
        Ok(())
    }

    #[test]
    fn accepts_any_registered_prefix_on_non_note_records() -> Result<(), ValidationError> {
        let frontmatter = "id: obs_01a028ee-48e0-7000-8000-000000000001\n\
type: organization_affiliation_candidate\n\
teams_chat_id: \"19:abc\"\n";
        let record = parse_record(&note(frontmatter, "# Observation\n"))?;
        validate_record(&record)?;
        assert_eq!(record.kind(), RecordKind::Observation);
        Ok(())
    }
}
