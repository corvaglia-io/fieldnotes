//! Typed record construction: the only supported path from typed values to
//! canonical bytes.
//!
//! Writers assemble a [`RecordBuilder`] from domain values and call
//! [`RecordBuilder::build`]. That emits the canonical byte form, parses those
//! exact bytes back with [`parse_record`], validates the result with
//! [`validate_record`] — the same validator the conformance suite runs — and
//! checks that the round trip preserved every typed value. Only then does a
//! [`CanonicalRecord`] exist. No caller can hand-format frontmatter, and no
//! bytes reach a durable writer without passing the public validator first.

use fieldnotes_domain::{Date, Datetime, FieldId, NoteType, RecordId, Scalar, Value};

use crate::emit::canonical_record_string;
use crate::error::ValidationError;
use crate::filename::expected_note_filename;
use crate::normalize::normalize_body_str;
use crate::record::{ParsedRecord, parse_record, validate_record};
use crate::registry::{ListSemantics, PropertyRegistry, PropertyType};

/// A record assembled from typed values, in insertion order.
///
/// Insertion order is irrelevant to the output: the canonical emitter orders
/// the structural properties first and then every remaining property in
/// ascending ASCII byte order.
#[derive(Debug, Clone, Default)]
pub struct RecordBuilder {
    entries: Vec<(String, Value)>,
    body: String,
}

impl RecordBuilder {
    /// Starts a record carrying only its logical ID.
    #[must_use]
    pub fn new(id: &RecordId) -> Self {
        let mut builder = RecordBuilder {
            entries: Vec::new(),
            body: String::new(),
        };
        builder.set_text("id", id.to_string());
        builder
    }

    /// Starts a Note carrying the five required structural properties.
    #[must_use]
    pub fn note(
        id: &RecordId,
        instance_id: &RecordId,
        field_id: &FieldId,
        note_type: NoteType,
        occurred_at: Datetime,
    ) -> Self {
        let mut builder = RecordBuilder::new(id);
        builder.set_text("instance_id", instance_id.to_string());
        builder.set_text("field_id", field_id.as_str().to_owned());
        builder.set_text("type", note_type.as_str().to_owned());
        builder.set_datetime("occurred_at", occurred_at);
        builder
    }

    /// Sets one property, replacing any previous value for that name.
    ///
    /// Replacement rather than appending is why a built record can never carry
    /// a duplicate key.
    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        match self.entries.iter_mut().find(|(name, _)| name == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key.to_owned(), value)),
        }
        self
    }

    /// Sets a text property.
    pub fn set_text(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.set(key, Value::Scalar(Scalar::Text(value.into())))
    }

    /// Sets a datetime property.
    pub fn set_datetime(&mut self, key: &str, value: Datetime) -> &mut Self {
        self.set(key, Value::Scalar(Scalar::Datetime(value)))
    }

    /// Sets a date property.
    pub fn set_date(&mut self, key: &str, value: Date) -> &mut Self {
        self.set(key, Value::Scalar(Scalar::Date(value)))
    }

    /// Sets a number property.
    pub fn set_number(&mut self, key: &str, value: f64) -> &mut Self {
        self.set(key, Value::Scalar(Scalar::Number(value)))
    }

    /// Sets a boolean property.
    pub fn set_bool(&mut self, key: &str, value: bool) -> &mut Self {
        self.set(key, Value::Scalar(Scalar::Bool(value)))
    }

    /// Sets a text list property. An empty list clears the property, because
    /// the canonical form omits empty lists rather than serializing them.
    pub fn set_text_list<I, S>(&mut self, key: &str, values: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let items: Vec<Scalar> = values
            .into_iter()
            .map(|value| Scalar::Text(value.into()))
            .collect();
        if items.is_empty() {
            self.entries.retain(|(name, _)| name != key);
            return self;
        }
        self.set(key, Value::List(items))
    }

    /// Sets the Markdown body. Normalization (line endings, exactly one final
    /// LF) happens at build time.
    pub fn set_body(&mut self, body: impl Into<String>) -> &mut Self {
        self.body = body.into();
        self
    }

    /// Emits, re-parses, and validates the record.
    ///
    /// The returned bytes are the exact canonical form and have already passed
    /// the public parser and validator.
    pub fn build(&self) -> Result<CanonicalRecord, ValidationError> {
        let body = normalize_body_str(&self.body);
        if body == "\n" {
            return Err(ValidationError::EmptyBody);
        }
        let id_text = match self.entries.iter().find(|(key, _)| key == "id") {
            Some((_, Value::Scalar(Scalar::Text(text)))) => text.clone(),
            _ => {
                return Err(ValidationError::MissingRequired {
                    key: "id".to_owned(),
                });
            }
        };
        let id = RecordId::parse(&id_text).map_err(|_| ValidationError::InvalidId {
            key: "id".to_owned(),
        })?;

        let provisional = ParsedRecord::from_typed(id, self.entries.clone(), body.clone());
        let text = canonical_record_string(&provisional)?;

        // The emitted bytes are re-read through the public parser and the
        // conformance validator, so a writer cannot persist anything the
        // reader would reject.
        let record = parse_record(text.as_bytes())?;
        validate_record(&record)?;

        if record.body() != body || record.entries().len() != self.entries.len() {
            return Err(ValidationError::RoundTripMismatch {
                key: "id".to_owned(),
            });
        }
        for (key, value) in &self.entries {
            let parsed = record
                .get(key)
                .ok_or_else(|| ValidationError::RoundTripMismatch { key: key.clone() })?;
            if !values_agree(key, value, parsed) {
                return Err(ValidationError::RoundTripMismatch { key: key.clone() });
            }
        }

        Ok(CanonicalRecord { record, text })
    }
}

/// Whether a re-parsed value still means what the builder was given.
///
/// Scalars must compare equal exactly. A registered set-like list is
/// deduplicated and sorted by the canonical emitter, so it is compared as a
/// set; every other list must keep its exact sequence.
fn values_agree(key: &str, requested: &Value, parsed: &Value) -> bool {
    match (requested, parsed) {
        (Value::Scalar(left), Value::Scalar(right)) => left == right,
        (Value::List(left), Value::List(right)) => {
            if matches!(
                PropertyRegistry::v1().lookup(key),
                Some(PropertyType::List(_, ListSemantics::Set))
            ) {
                let mut left: Vec<&Scalar> = left.iter().collect();
                let mut right: Vec<&Scalar> = right.iter().collect();
                left.sort_unstable_by_key(|scalar| sort_key(scalar));
                left.dedup();
                right.sort_unstable_by_key(|scalar| sort_key(scalar));
                right.dedup();
                left == right
            } else {
                left == right
            }
        }
        _ => false,
    }
}

/// A stable comparison key for set-like list members.
fn sort_key(scalar: &Scalar) -> String {
    match scalar {
        Scalar::Text(text) => text.clone(),
        Scalar::Number(value) => format!("{value}"),
        Scalar::Bool(value) => value.to_string(),
        Scalar::Date(value) => value.to_string(),
        Scalar::Datetime(value) => value.to_string(),
    }
}

/// A validated record together with its exact canonical bytes.
///
/// Constructing one is the proof obligation a durable writer relies on: the
/// bytes are canonical, they parse, and they validate.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalRecord {
    record: ParsedRecord,
    text: String,
}

impl CanonicalRecord {
    /// The parsed, validated record.
    #[must_use]
    pub fn record(&self) -> &ParsedRecord {
        &self.record
    }

    /// The record's logical ID.
    #[must_use]
    pub fn id(&self) -> &RecordId {
        self.record.id()
    }

    /// The canonical file text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The canonical file bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    /// The canonical Note filename computed from this record's frontmatter.
    pub fn note_filename(&self) -> Result<String, ValidationError> {
        expected_note_filename(&self.record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_domain::FieldStemRegistry;

    fn ids() -> Result<(RecordId, RecordId), ValidationError> {
        let note = RecordId::parse("note_01a02844-f150-7000-8000-000000000001").map_err(|_| {
            ValidationError::InvalidId {
                key: "id".to_owned(),
            }
        })?;
        let instance =
            RecordId::parse("fn_01a02837-2de0-7a2b-8c41-f2481851192a").map_err(|_| {
                ValidationError::InvalidId {
                    key: "id".to_owned(),
                }
            })?;
        Ok((note, instance))
    }

    #[test]
    fn builds_the_frozen_text_note_bytes() -> Result<(), ValidationError> {
        let (note_id, instance_id) = ids()?;
        let field = FieldId::parse("self", FieldStemRegistry::v1()).map_err(|_| {
            ValidationError::InvalidFieldId {
                value: "self".to_owned(),
            }
        })?;
        let occurred_at = Datetime::parse("2026-08-22T09:00:00+02:00").map_err(|_| {
            ValidationError::InvalidId {
                key: "occurred_at".to_owned(),
            }
        })?;
        let captured_at = Datetime::parse("2026-08-22T09:00:02+02:00").map_err(|_| {
            ValidationError::InvalidId {
                key: "captured_at".to_owned(),
            }
        })?;
        let mut builder =
            RecordBuilder::note(&note_id, &instance_id, &field, NoteType::Text, occurred_at);
        // Deliberately out of canonical order: the emitter, not the caller,
        // decides property order.
        builder.set_text("title", "Rollout reminder");
        builder.set_datetime("captured_at", captured_at);
        builder.set_text(
            "content_hash",
            "fn-content-v1-sha256:ca59b2515ae57d5e85195fa59fb57c7982eacd6b87eb1aeb0988f0afbe692129",
        );
        builder.set_body("# Rollout reminder\r\n\r\nAsk Alice whether the rollout can begin on Thursday.\r\n\r\n\r\n");
        let built = builder.build()?;
        assert_eq!(
            built.text(),
            concat!(
                "---\n",
                "id: note_01a02844-f150-7000-8000-000000000001\n",
                "instance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a\n",
                "field_id: self\n",
                "type: text\n",
                "occurred_at: 2026-08-22T09:00:00+02:00\n",
                "captured_at: 2026-08-22T09:00:02+02:00\n",
                "content_hash: \"fn-content-v1-sha256:ca59b2515ae57d5e85195fa59fb57c7982eacd6b87eb1aeb0988f0afbe692129\"\n",
                "title: Rollout reminder\n",
                "---\n",
                "\n",
                "# Rollout reminder\n",
                "\n",
                "Ask Alice whether the rollout can begin on Thursday.\n",
            )
        );
        assert_eq!(
            built.note_filename()?,
            "20260822T070000Z_self_text_note_01a02844-f150-7000-8000-000000000001.md"
        );
        Ok(())
    }

    #[test]
    fn rejects_records_the_public_validator_would_reject() -> Result<(), ValidationError> {
        let (note_id, instance_id) = ids()?;
        let field = FieldId::parse("self", FieldStemRegistry::v1()).map_err(|_| {
            ValidationError::InvalidFieldId {
                value: "self".to_owned(),
            }
        })?;
        let occurred_at = Datetime::parse("2026-08-22T09:00:00+02:00").map_err(|_| {
            ValidationError::InvalidId {
                key: "occurred_at".to_owned(),
            }
        })?;
        // A `self` Note may not carry a connector prefix.
        let mut builder =
            RecordBuilder::note(&note_id, &instance_id, &field, NoteType::Text, occurred_at);
        builder.set_body("Body.\n");
        builder.set_text("teams_chat_id", "19:abc");
        assert_eq!(
            builder.build(),
            Err(ValidationError::ForeignPrefix {
                key: "teams_chat_id".to_owned()
            })
        );
        // An empty body cannot be canonically serialized.
        let mut empty =
            RecordBuilder::note(&note_id, &instance_id, &field, NoteType::Text, occurred_at);
        empty.set_body("\n\n");
        assert_eq!(empty.build(), Err(ValidationError::EmptyBody));
        // A missing ID is caught before anything is emitted.
        let mut anonymous = RecordBuilder::default();
        anonymous.set_body("Body.\n");
        assert_eq!(
            anonymous.build(),
            Err(ValidationError::MissingRequired {
                key: "id".to_owned()
            })
        );
        Ok(())
    }

    #[test]
    fn set_replaces_and_empty_lists_are_dropped() -> Result<(), ValidationError> {
        let (note_id, instance_id) = ids()?;
        let field = FieldId::parse("self", FieldStemRegistry::v1()).map_err(|_| {
            ValidationError::InvalidFieldId {
                value: "self".to_owned(),
            }
        })?;
        let occurred_at = Datetime::parse("2026-08-22T09:00:00+02:00").map_err(|_| {
            ValidationError::InvalidId {
                key: "occurred_at".to_owned(),
            }
        })?;
        let mut builder =
            RecordBuilder::note(&note_id, &instance_id, &field, NoteType::Text, occurred_at);
        builder.set_body("Body.\n");
        builder.set_text("title", "first");
        builder.set_text("title", "second");
        let empty: [String; 0] = [];
        builder.set_text_list("artifacts", empty);
        let built = builder.build()?;
        assert_eq!(built.text().matches("title:").count(), 1);
        assert!(built.text().contains("title: second\n"));
        assert!(!built.text().contains("artifacts"));
        Ok(())
    }
}
