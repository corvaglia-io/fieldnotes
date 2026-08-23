//! Turning derived projections into validated canonical notebook records.
//!
//! Nothing here formats notebook bytes by hand. Every record goes through
//! [`RecordBuilder`], which emits the canonical form, re-parses it with the
//! public parser, and runs the public validator before a
//! [`fieldnotes_format::CanonicalRecord`] exists — so a projection that would
//! not survive the conformance suite cannot be produced at all.
//!
//! Only property names the A1 registry already carries appear here: `id`,
//! `type`, `channels`, `evidence`, `evidence_count`, `first_seen`,
//! `from_entity_id`, `generated_at`, `generator_version`, `identities`,
//! `last_seen`, `title`, and `to_entity_id`. No name is invented, because a new
//! shared name needs registry review with fixtures.

use fieldnotes_domain::{Datetime, RecordId};
use fieldnotes_format::{CanonicalRecord, RecordBuilder};

use crate::derive::GraphError;
use crate::entity::Entity;
use crate::evidence::Origin;
use crate::identity::IdentityKey;
use crate::relationship::Relationship;

/// One projected record with the notebook-relative path it belongs at.
///
/// The path is derived from the record's own ID and type, per the A1 derived
/// filename grammar `entities/<ent-id>_<type>.md` and
/// `relationships/<rel-id>_<type>.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedRecord {
    /// The notebook-relative path.
    pub relative_path: String,
    /// The validated canonical record.
    pub record: CanonicalRecord,
}

/// Renders a value that reaches a Markdown body safely.
///
/// Control characters and newlines collapse to single spaces so untrusted source
/// text cannot break the canonical body grammar or forge frontmatter. The result
/// is deterministic for a given input.
fn body_safe(text: &str) -> String {
    let replaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    replaced.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// The column a generated body paragraph wraps at.
const WRAP_COLUMNS: usize = 76;

/// Greedy deterministic word wrap.
///
/// A word longer than the wrap column stays on its own line rather than being
/// split, so no identifier is ever broken.
fn wrap(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if line.chars().count() + 1 + word.chars().count() <= WRAP_COLUMNS {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(core::mem::take(&mut line));
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.join("\n")
}

/// The heading a projection is presented under: its projected name when current
/// evidence supplies exactly one, and otherwise its primary anchor.
fn heading(entity: &Entity) -> String {
    let named = entity.title.as_deref().map(body_safe).unwrap_or_default();
    if !named.is_empty() {
        return named;
    }
    entity
        .primary_identity()
        .map(IdentityKey::anchor_text)
        .unwrap_or_else(|| entity.id.to_string())
}

fn count(value: usize, key: &str) -> Result<f64, GraphError> {
    u32::try_from(value)
        .map(f64::from)
        .map_err(|_| GraphError::CountOutOfRange {
            key: key.to_owned(),
        })
}

fn note_ids(notes: &[RecordId]) -> Vec<String> {
    notes.iter().map(RecordId::to_string).collect()
}

/// Builds the `entities/<ent-id>_<type>.md` record for one entity.
pub fn entity_record(
    entity: &Entity,
    generated_at: Datetime,
) -> Result<ProjectedRecord, GraphError> {
    let mut builder = RecordBuilder::new(&entity.id);
    builder.set_text("type", entity.kind.as_str());
    builder.set_datetime("generated_at", generated_at);
    builder.set_text(
        "generator_version",
        entity.explanation.rule.generator.clone(),
    );
    builder.set_text_list("channels", entity.channels.clone());
    builder.set_text_list("evidence", note_ids(&entity.evidence));
    builder.set_text_list(
        "identities",
        entity
            .publishable_identities()
            .map(IdentityKey::anchor_text)
            .collect::<Vec<String>>(),
    );
    if entity.evidence.len() < entity.interaction_count {
        // The public list is a bounded representative sample, so the record has
        // to say how much evidence there really is.
        builder.set_number(
            "evidence_count",
            count(entity.interaction_count, "evidence_count")?,
        );
    }
    if let Some(first_seen) = entity.first_seen {
        builder.set_datetime("first_seen", first_seen);
    }
    if let Some(last_seen) = entity.last_seen {
        builder.set_datetime("last_seen", last_seen);
    }
    if let Some(title) = entity.title.as_deref() {
        builder.set_text("title", title);
    }

    let mut body = format!("# {}\n\n", heading(entity));
    body.push_str(&wrap(&format!("{}.", body_safe(&entity.explanation.claim))));
    body.push_str(&format!(
        "\n\nOrigin: `{}`  \nRule: `{}`\n",
        entity.explanation.origin, entity.explanation.rule
    ));
    if !entity.explanation.competing.is_empty() {
        body.push_str("\nCompeting current evidence, preserved and unresolved:\n");
        for competing in &entity.explanation.competing {
            body.push_str(&format!("\n- {}\n", body_safe(&competing.claim)));
            for note in &competing.evidence {
                body.push_str(&format!("  - `{note}`\n"));
            }
        }
    }
    builder.set_body(body);

    let record = builder.build()?;
    Ok(ProjectedRecord {
        relative_path: format!("entities/{}_{}.md", entity.id, entity.kind),
        record,
    })
}

/// Builds the `relationships/<rel-id>_<type>.md` record for one relationship.
///
/// `from` and `to` are the entities the relationship's canonical orientation
/// names; they supply the readable heading only.
pub fn relationship_record(
    relationship: &Relationship,
    from: &Entity,
    to: &Entity,
    generated_at: Datetime,
) -> Result<ProjectedRecord, GraphError> {
    let mut builder = RecordBuilder::new(&relationship.id);
    builder.set_text("type", relationship.kind.as_str());
    builder.set_datetime("generated_at", generated_at);
    builder.set_text(
        "generator_version",
        relationship.explanation.rule.generator.clone(),
    );
    builder.set_text("from_entity_id", relationship.from_entity_id.to_string());
    builder.set_text("to_entity_id", relationship.to_entity_id.to_string());
    builder.set_text_list("channels", relationship.channels.clone());
    builder.set_text_list("evidence", note_ids(&relationship.evidence));
    builder.set_number(
        "evidence_count",
        count(relationship.interaction_count, "evidence_count")?,
    );
    if let Some(first_seen) = relationship.first_seen {
        builder.set_datetime("first_seen", first_seen);
    }
    if let Some(last_seen) = relationship.last_seen {
        builder.set_datetime("last_seen", last_seen);
    }

    let mut body = format!("# {} ↔ {}\n\n", heading(from), heading(to));
    body.push_str(&wrap(&format!(
        "{}.",
        body_safe(&relationship.explanation.claim)
    )));
    body.push_str(&format!(
        "\n\nOrigin: `{}`  \nRule: `{}`\n\n",
        relationship.explanation.origin, relationship.explanation.rule
    ));
    body.push_str(&wrap(
        "This relationship reports observed interaction and makes no claim about strength, trust, \
         importance, or sentiment.",
    ));
    body.push('\n');
    builder.set_body(body);

    let record = builder.build()?;
    Ok(ProjectedRecord {
        relative_path: format!("relationships/{}_{}.md", relationship.id, relationship.kind),
        record,
    })
}

/// Whether an origin may appear in a deterministic v0.1 projection.
///
/// `extracted` and `observed` claims arrive with the optional enhancement gate;
/// a deterministic record carrying one would be a defect.
#[must_use]
pub fn is_deterministic_origin(origin: Origin) -> bool {
    matches!(origin, Origin::Explicit | Origin::Matched)
}
