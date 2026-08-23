//! Derivation over the frozen A1 notebook corpus.
//!
//! The approved fixture bytes are the strongest available evidence: they are the
//! contract every Field and reader shares, so a graph derived from them exercises
//! the real vocabulary rather than a convenient invention.

mod support;

use std::collections::BTreeSet;

use fieldnotes_domain::property::registry::PropertyRegistry;
use fieldnotes_domain::{RecordId, RecordKind};
use fieldnotes_format::{parse_record, validate_record};
use fieldnotes_graph::{EntityKind, GapKind, Origin, RelationshipKind};
use support::{
    TestResult, config, corpus_notes, corpus_notes_with_suffixes, corpus_root, derive, load_records,
};

/// The frozen corpus contains three people, joined only through anchors the
/// approved rules permit, each with the channels, evidence, and time range its
/// Notes support.
#[test]
fn the_approved_corpus_derives_three_explainable_people() -> TestResult {
    let graph = derive(&corpus_notes()?, &config())?;

    let anchors: Vec<Vec<String>> = graph
        .entities()
        .iter()
        .map(|entity| {
            entity
                .identities
                .iter()
                .map(fieldnotes_graph::IdentityKey::anchor_text)
                .collect()
        })
        .collect();
    assert_eq!(
        anchors,
        vec![
            vec![
                "email:alice@example.com".to_owned(),
                "phone:+41441234567".to_owned()
            ],
            vec!["email:bob@example.net".to_owned()],
            vec!["email:sam@example.net".to_owned()],
        ],
        "entities are ordered by primary anchor, and only the contact record's anchors join"
    );
    assert!(
        graph
            .entities()
            .iter()
            .all(|entity| entity.kind == EntityKind::Person)
    );

    let alice = graph
        .entities()
        .first()
        .ok_or("the corpus must derive at least one entity")?;
    assert_eq!(alice.title.as_deref(), Some("Alice Müller"));
    assert_eq!(
        alice.channels,
        vec![
            "jira".to_owned(),
            "outlook_calendar".to_owned(),
            "outlook_contacts".to_owned(),
            "outlook_mail".to_owned(),
            "teams".to_owned()
        ],
        "channels are registered Field stems, not configured Field labels"
    );
    assert_eq!(alice.interaction_count, 8);
    assert_eq!(alice.evidence.len(), 8);
    assert_eq!(
        alice.first_seen.map(|value| value.to_string()),
        Some("2026-08-22T10:00:00+02:00".to_owned())
    );
    assert_eq!(
        alice.last_seen.map(|value| value.to_string()),
        Some("2026-08-23T01:30:00+03:00".to_owned()),
        "last seen keeps the source's own explicit offset for the latest instant"
    );

    // Every relationship is a person_person pair the source stated together.
    assert_eq!(graph.relationships().len(), 3);
    assert!(
        graph
            .relationships()
            .iter()
            .all(|edge| edge.kind == RelationshipKind::PersonPerson
                && edge.explanation.origin == Origin::Explicit)
    );
    Ok(())
}

/// Every derived entity and relationship names the Notes and normalized anchors
/// that produced it, and every cited Note is a Note that was actually supplied.
#[test]
fn every_projection_explains_itself_with_cited_notes() -> TestResult {
    let notes = corpus_notes()?;
    let supplied: BTreeSet<RecordId> = notes.iter().map(|note| *note.id()).collect();
    let graph = derive(&notes, &config())?;

    for entity in graph.entities() {
        let explanation = graph
            .explain(&entity.id)
            .ok_or("a derived entity must be explainable")?;
        assert_eq!(explanation.subject, entity.id);
        assert!(!explanation.claim.is_empty());
        assert!(
            fieldnotes_graph::emit::is_deterministic_origin(explanation.origin),
            "a deterministic projection may only be explicit or matched"
        );
        assert!(!explanation.identities.is_empty(), "no anchor cited");
        assert!(!explanation.evidence.is_empty(), "no Note cited");
        assert_eq!(explanation.evidence_count, entity.interaction_count);
        assert!(explanation.first_seen.is_some() && explanation.last_seen.is_some());
        for note in &explanation.evidence {
            assert!(supplied.contains(note), "{note} was never supplied");
        }
        for identity in &explanation.identities {
            assert!(!identity.evidence.is_empty(), "an anchor cites no Note");
            for note in &identity.evidence {
                assert!(supplied.contains(note));
            }
        }
        // The join that produced a multi-anchor entity is retained by rule and
        // by the Note that stated it.
        if entity.identities.len() > 1 {
            assert!(!explanation.joins.is_empty());
            for join in &explanation.joins {
                assert_eq!(join.rule.rule, "contact-record-anchors-v1");
                assert!(supplied.contains(&join.evidence));
            }
        }
    }

    for edge in graph.relationships() {
        let explanation = graph
            .explain(&edge.id)
            .ok_or("a derived relationship must be explainable")?;
        assert_eq!(explanation.evidence, edge.evidence);
        assert_eq!(explanation.evidence_count, edge.interaction_count);
        assert!(explanation.identities.len() >= 2);
        let rendered = explanation.to_string();
        assert!(rendered.contains("origin: explicit"));
        assert!(rendered.contains("rule: fieldnotes-relationship-builder-v1/co-participant-v1"));
        for note in &edge.evidence {
            assert!(rendered.contains(&note.to_string()), "{note} not explained");
        }
    }
    Ok(())
}

/// Alice's projection over exactly the Notes the frozen entity fixture cites
/// reproduces that fixture's frontmatter, property for property.
#[test]
fn person_projection_reproduces_the_frozen_entity_fixture_frontmatter() -> TestResult {
    // The four Notes `entities/ent_…0001_person.md` cites: mail, calendar event,
    // contact, and Teams message.
    let notes = corpus_notes_with_suffixes(&[
        "000000000005",
        "000000000006",
        "000000000007",
        "000000000008",
    ])?;
    let graph = derive(&notes, &config())?;
    let projected = graph.projected_records()?;

    let fixture = load_records(&corpus_root().join("entities"))?;
    let alice_fixture = fixture
        .iter()
        .find(|record| carries_anchor(record, "email:alice@example.com"))
        .ok_or("the corpus must contain Alice's person fixture")?;
    let derived_alice = projected
        .iter()
        .find(|record| carries_anchor(record.record.record(), "email:alice@example.com"))
        .ok_or("the derivation must project Alice")?;

    // Every fixture property except the projection ID is reproduced exactly,
    // including value spelling, and nothing extra is added.
    let expected: Vec<(String, String)> = property_pairs(alice_fixture);
    let actual: Vec<(String, String)> = property_pairs(derived_alice.record.record());
    assert_eq!(
        actual
            .iter()
            .filter(|(key, _)| key != "id")
            .collect::<Vec<_>>(),
        expected
            .iter()
            .filter(|(key, _)| key != "id")
            .collect::<Vec<_>>(),
        "derived person frontmatter must match the frozen fixture apart from the projection ID"
    );
    assert!(derived_alice.relative_path.ends_with("_person.md"));

    // Bob's fixture is identical except for a display name the corpus Notes do
    // not supply: no Note states a name for `bob@example.net`, and inventing one
    // is exactly the inference the product forbids.
    let bob_fixture = fixture
        .iter()
        .find(|record| carries_anchor(record, "email:bob@example.net"))
        .ok_or("the corpus must contain Bob's person fixture")?;
    let derived_bob = projected
        .iter()
        .find(|record| carries_anchor(record.record.record(), "email:bob@example.net"))
        .ok_or("the derivation must project Bob")?;
    assert!(
        bob_fixture.get("title").is_some() && derived_bob.record.record().get("title").is_none(),
        "no current Note states a name for bob@example.net, so none is projected"
    );
    let expected_bob: Vec<(String, String)> = property_pairs(bob_fixture)
        .into_iter()
        .filter(|(key, _)| key != "id" && key != "title")
        .collect();
    let actual_bob: Vec<(String, String)> = property_pairs(derived_bob.record.record())
        .into_iter()
        .filter(|(key, _)| key != "id")
        .collect();
    assert_eq!(actual_bob, expected_bob);
    Ok(())
}

/// The Alice-Bob edge over the same four Notes reproduces the frozen
/// relationship fixture's frontmatter.
#[test]
fn relationship_projection_reproduces_the_frozen_relationship_fixture() -> TestResult {
    let notes = corpus_notes_with_suffixes(&[
        "000000000005",
        "000000000006",
        "000000000007",
        "000000000008",
    ])?;
    let graph = derive(&notes, &config())?;
    let fixture_records = load_records(&corpus_root().join("relationships"))?;
    let fixture = fixture_records
        .first()
        .ok_or("the corpus must contain the relationship fixture")?;

    // The fixture edge is the one carrying exactly the mail and calendar
    // channels and two supporting Notes.
    let edge = graph
        .relationships()
        .iter()
        .find(|edge| edge.channels == ["outlook_calendar", "outlook_mail"])
        .ok_or("the mail-and-calendar edge must exist")?;
    assert_eq!(edge.interaction_count, 2);
    let from = graph
        .entity(&edge.from_entity_id)
        .ok_or("the edge's from-entity must exist")?;
    let to = graph
        .entity(&edge.to_entity_id)
        .ok_or("the edge's to-entity must exist")?;
    assert_eq!(
        from.primary_identity().map(|key| key.anchor_text()),
        Some("email:alice@example.com".to_owned()),
        "the canonical orientation is by primary anchor, so Alice is the from side"
    );
    assert_eq!(
        to.primary_identity().map(|key| key.anchor_text()),
        Some("email:bob@example.net".to_owned())
    );

    let projected = fieldnotes_graph::relationship_record(edge, from, to, *graph.generated_at())?;
    // The fixture's own `generated_at` is two minutes after the entity
    // fixtures': the corpus was written as if entities and relationships were
    // generated in separate passes. One derivation generates both at the single
    // injected instant, so that property is compared separately.
    let ignored = ["id", "from_entity_id", "to_entity_id", "generated_at"];
    let expected: Vec<(String, String)> = property_pairs(fixture)
        .into_iter()
        .filter(|(key, _)| !ignored.contains(&key.as_str()))
        .collect();
    let actual: Vec<(String, String)> = property_pairs(projected.record.record())
        .into_iter()
        .filter(|(key, _)| !ignored.contains(&key.as_str()))
        .collect();
    assert_eq!(
        graph.generated_at().to_string(),
        "2026-08-22T12:10:00+02:00",
        "generated_at comes from the injected clock and carries an explicit offset"
    );
    assert_eq!(
        actual, expected,
        "derived relationship frontmatter must match the frozen fixture apart from projection IDs"
    );
    Ok(())
}

/// Every projected record parses and validates through the format crate's own
/// public parser and validator, uses only registered property names, and lands
/// at the A1 derived filename grammar.
#[test]
fn projected_records_validate_and_invent_no_property_name() -> TestResult {
    let graph = derive(&corpus_notes()?, &config())?;
    let registry = PropertyRegistry::v1();
    let projected = graph.projected_records()?;
    assert!(!projected.is_empty());

    for record in &projected {
        let reparsed = parse_record(record.record.bytes())?;
        validate_record(&reparsed)?;
        assert_eq!(reparsed, *record.record.record(), "canonical round trip");

        for (key, _) in reparsed.entries() {
            assert!(
                registry.lookup(key).is_some(),
                "{key} is not a registered property name"
            );
        }

        let kind = reparsed.id().kind();
        let (directory, expected_kind) = match kind {
            RecordKind::Entity => ("entities", RecordKind::Entity),
            RecordKind::Relationship => ("relationships", RecordKind::Relationship),
            other => panic!("unexpected projected record kind {other:?}"),
        };
        assert_eq!(kind, expected_kind);
        let expected_prefix = format!("{directory}/{}_", reparsed.id());
        assert!(
            record.relative_path.starts_with(&expected_prefix)
                && record.relative_path.ends_with(".md"),
            "{} does not follow the derived filename grammar",
            record.relative_path
        );
        // Datetimes in a generated record always carry an explicit offset; the
        // canonical emitter would have rejected anything else.
        assert!(
            record
                .record
                .text()
                .contains("generated_at: 2026-08-22T12:10:00+02:00")
        );
    }
    Ok(())
}

/// Notes that carry no resolvable anchor are reported as gaps rather than being
/// silently dropped, and the corpus's `self` and `local` material is exactly
/// that.
#[test]
fn notes_without_resolvable_anchors_are_reported_as_gaps() -> TestResult {
    let graph = derive(&corpus_notes()?, &config())?;
    let unresolved: Vec<&fieldnotes_graph::Gap> = graph
        .gaps()
        .iter()
        .filter(|gap| gap.kind == GapKind::NoIdentityAnchors)
        .collect();
    assert_eq!(
        unresolved.len(),
        3,
        "the self text Note and the two local-file Notes carry no identity anchor: {:#?}",
        graph.gaps()
    );
    for gap in unresolved {
        assert_eq!(gap.records.len(), 1);
        assert!(gap.detail.contains("no identity anchor"));
    }
    // Nothing in the approved corpus is refused for a reason the resolver cannot
    // explain, and no anchor in it is malformed.
    assert!(
        !graph
            .gaps()
            .iter()
            .any(|gap| gap.kind == GapKind::UnresolvedIdentityAnchor
                || gap.kind == GapKind::MalformedArtifactReference)
    );
    Ok(())
}

/// The corpus's same-Note-ID conflict fixtures stay a visible conflict, and
/// neither divergent copy is treated as current evidence.
#[test]
fn divergent_copies_of_one_note_id_become_a_preserved_conflict() -> TestResult {
    let root = corpus_root().join("conflicts").join("same-id");
    let mut records = load_records(&root.join("left").join("notes"))?;
    records.extend(load_records(&root.join("right").join("notes"))?);
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0].id(),
        records[1].id(),
        "the fixtures share one ID"
    );

    let graph = derive(&records, &config())?;
    let conflict = graph
        .conflicts()
        .first()
        .ok_or("the divergence must be reported")?;
    assert_eq!(
        conflict.kind,
        fieldnotes_graph::ConflictKind::SameNoteIdDivergence
    );
    assert_eq!(conflict.fingerprints.len(), 2, "both fingerprints retained");
    assert!(graph.entities().is_empty(), "no copy is declared current");
    assert!(
        graph
            .gaps()
            .iter()
            .any(|gap| gap.kind == GapKind::ExcludedByConflict)
    );
    Ok(())
}

/// Whether a record's `identities` list carries `anchor`.
fn carries_anchor(record: &fieldnotes_format::ParsedRecord, anchor: &str) -> bool {
    matches!(record.get("identities"), Some(fieldnotes_domain::Value::List(items))
        if items.iter().any(|item| matches!(item, fieldnotes_domain::Scalar::Text(text) if text == anchor)))
}

/// The `id` and value spelling of each frontmatter property, in canonical order.
fn property_pairs(record: &fieldnotes_format::ParsedRecord) -> Vec<(String, String)> {
    let text = fieldnotes_format::canonical_record_string(record).unwrap_or_default();
    let mut pairs = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines().skip(1) {
        if line == "---" {
            break;
        }
        if let Some(item) = line.strip_prefix("  - ") {
            if let Some((_, value)) = current.as_mut() {
                value.push('\u{1f}');
                value.push_str(item);
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(": ") {
            if let Some(pair) = current.take() {
                pairs.push(pair);
            }
            current = Some((key.to_owned(), value.to_owned()));
        } else if let Some(key) = line.strip_suffix(':') {
            if let Some(pair) = current.take() {
                pairs.push(pair);
            }
            current = Some((key.to_owned(), String::new()));
        }
    }
    if let Some(pair) = current.take() {
        pairs.push(pair);
    }
    pairs
}
