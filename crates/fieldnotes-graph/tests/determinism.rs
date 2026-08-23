//! Release-gate R2 properties: the same current evidence always reproduces the
//! same semantic graph, whatever order it arrives in and whether or not the
//! previous projections were deleted first.

mod support;

use fieldnotes_domain::{RecordIdGenerator, RecordKind};
use fieldnotes_format::{ParsedRecord, parse_record};
use fieldnotes_graph::{GraphConfig, derive_graph};
use fieldnotes_test_support::{CountingRandom, FixedClock};
use support::{GENERATED_MILLIS, TestResult, config, corpus_notes, derive, projected_bytes};

/// Two derivations over the same records produce byte-identical records and an
/// equal graph.
#[test]
fn repeated_derivations_are_byte_identical() -> TestResult {
    let notes = corpus_notes()?;
    let first = derive(&notes, &config())?;
    let second = derive(&notes, &config())?;
    assert_eq!(projected_bytes(&first)?, projected_bytes(&second)?);
    assert_eq!(first, second, "the whole graph, not only the emitted bytes");
    assert!(!projected_bytes(&first)?.is_empty());
    Ok(())
}

/// Input order never reaches the result: reversing, rotating, and duplicating
/// the input all produce the same bytes.
#[test]
fn input_order_never_changes_the_result() -> TestResult {
    let notes = corpus_notes()?;
    let expected = projected_bytes(&derive(&notes, &config())?)?;

    let mut reversed = notes.clone();
    reversed.reverse();
    assert_eq!(projected_bytes(&derive(&reversed, &config())?)?, expected);

    for rotation in 1..notes.len() {
        let mut rotated = notes.clone();
        rotated.rotate_left(rotation);
        assert_eq!(
            projected_bytes(&derive(&rotated, &config())?)?,
            expected,
            "rotation by {rotation} changed the projection"
        );
    }

    // A deterministic shuffle: every third record first, then the rest.
    let mut shuffled: Vec<ParsedRecord> = notes
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 3 == 0)
        .map(|(_, record)| record.clone())
        .collect();
    shuffled.extend(
        notes
            .iter()
            .enumerate()
            .filter(|(index, _)| index % 3 != 0)
            .map(|(_, record)| record.clone()),
    );
    assert_eq!(shuffled.len(), notes.len());
    assert_eq!(projected_bytes(&derive(&shuffled, &config())?)?, expected);

    // Exact duplicates of every Note collapse rather than doubling evidence.
    let mut doubled = notes.clone();
    doubled.extend(notes.iter().cloned());
    assert_eq!(projected_bytes(&derive(&doubled, &config())?)?, expected);
    Ok(())
}

/// Deleting the derived records and rebuilding reproduces the same semantic
/// graph; feeding the previous records back keeps the projection IDs too.
#[test]
fn deleting_derived_records_reproduces_the_same_semantic_graph() -> TestResult {
    let notes = corpus_notes()?;
    let first = derive(&notes, &config())?;

    // Rebuild with the previous projections present.
    let mut with_priors = notes.clone();
    for projected in first.projected_records()? {
        with_priors.push(parse_record(projected.record.bytes())?);
    }
    let rebuilt = derive(&with_priors, &config())?;
    assert_eq!(
        projected_bytes(&rebuilt)?,
        projected_bytes(&first)?,
        "a rebuild that can see its own prior projections reuses their IDs and changes nothing"
    );

    // Rebuild after deleting them, from a generator seeded differently so any
    // dependence on the previous IDs would show.
    let clock = FixedClock(GENERATED_MILLIS);
    let mut ids = RecordIdGenerator::new(FixedClock(GENERATED_MILLIS), CountingRandom::new(200));
    let fresh = derive_graph(&notes, &config(), &clock, &mut ids)?;
    assert_ne!(
        fresh.entities().first().map(|entity| entity.id),
        first.entities().first().map(|entity| entity.id),
        "projection IDs are locators, so a rebuild after deletion may mint new ones"
    );
    assert_eq!(
        semantic_shape(&fresh),
        semantic_shape(&first),
        "the semantic graph is identical: same anchors, names, evidence, and edges"
    );
    Ok(())
}

/// The identity, name, evidence, and edge content of a graph, with no
/// projection IDs in it.
fn semantic_shape(graph: &fieldnotes_graph::DerivedGraph) -> String {
    let mut out = String::new();
    for entity in graph.entities() {
        out.push_str(&format!(
            "entity {:?} {:?} {:?} channels={:?} evidence={:?} first={:?} last={:?} count={}\n",
            entity.kind,
            entity
                .identities
                .iter()
                .map(fieldnotes_graph::IdentityKey::anchor_text)
                .collect::<Vec<String>>(),
            entity.title,
            entity.channels,
            entity
                .evidence
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>(),
            entity.first_seen.map(|value| value.to_string()),
            entity.last_seen.map(|value| value.to_string()),
            entity.interaction_count,
        ));
    }
    for edge in graph.relationships() {
        let from = graph
            .entity(&edge.from_entity_id)
            .and_then(|entity| entity.primary_identity())
            .map(fieldnotes_graph::IdentityKey::anchor_text);
        let to = graph
            .entity(&edge.to_entity_id)
            .and_then(|entity| entity.primary_identity())
            .map(fieldnotes_graph::IdentityKey::anchor_text);
        out.push_str(&format!(
            "edge {from:?} {to:?} evidence={:?} count={}\n",
            edge.evidence
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>(),
            edge.interaction_count
        ));
    }
    for gap in graph.gaps() {
        out.push_str(&format!("gap {gap}\n"));
    }
    for candidate in graph.candidates() {
        out.push_str(&format!("candidate {candidate}\n"));
    }
    out
}

/// A bounded evidence list stays deterministic and states the full count, so a
/// reader can tell the list is representative.
#[test]
fn a_bounded_evidence_list_reports_the_full_count() -> TestResult {
    let notes = corpus_notes()?;
    let bounded = GraphConfig {
        evidence_limit: Some(2),
        ..config()
    };
    let graph = derive(&notes, &bounded)?;
    let unbounded = derive(&notes, &config())?;

    let alice = graph
        .entities()
        .first()
        .ok_or("the corpus derives at least one entity")?;
    let alice_full = unbounded
        .entities()
        .first()
        .ok_or("the corpus derives at least one entity")?;
    assert_eq!(alice.evidence.len(), 2);
    assert_eq!(alice.interaction_count, alice_full.interaction_count);
    assert_eq!(
        alice.evidence,
        alice_full.evidence[..2].to_vec(),
        "the bounded list is the first Notes in ascending ID order"
    );

    let projected = graph.projected_records()?;
    let record = projected
        .iter()
        .find(|record| record.record.record().id() == &alice.id)
        .ok_or("Alice must be projected")?;
    assert!(
        record.record.text().contains("evidence_count: 8"),
        "a bounded list must publish the true count: {}",
        record.record.text()
    );
    // The complete list needs no count, because the list is the count.
    let full = unbounded.projected_records()?;
    let full_record = full
        .iter()
        .find(|record| record.record.record().id() == &alice_full.id)
        .ok_or("Alice must be projected")?;
    assert!(!full_record.record.text().contains("evidence_count"));
    Ok(())
}

/// The generation instant comes only from the injected clock, and the offset
/// only from configuration.
#[test]
fn the_generation_instant_comes_only_from_the_injected_clock() -> TestResult {
    let notes = corpus_notes()?;
    let clock = FixedClock(GENERATED_MILLIS + 3_600_000);
    let mut ids = RecordIdGenerator::new(FixedClock(GENERATED_MILLIS), CountingRandom::new(1));
    let later = derive_graph(&notes, &config(), &clock, &mut ids)?;
    assert_eq!(
        later.generated_at().to_string(),
        "2026-08-22T13:10:00+02:00"
    );

    let utc_config = GraphConfig {
        generated_at_offset_minutes: 0,
        ..config()
    };
    let utc = derive(&notes, &utc_config)?;
    assert_eq!(utc.generated_at().to_string(), "2026-08-22T10:10:00+00:00");
    assert!(
        utc.projected_records()?.iter().all(|record| record
            .record
            .text()
            .contains("generated_at: 2026-08-22T10:10:00+00:00")),
        "UTC serializes as +00:00, never as a timezone-less value"
    );
    // Only the instant differs; the semantic graph does not.
    assert_eq!(semantic_shape(&later), semantic_shape(&utc));
    Ok(())
}

/// Projection IDs are minted only from the injected generator, in the graph's
/// own sorted order.
#[test]
fn projection_ids_come_from_the_injected_generator_in_sorted_order() -> TestResult {
    let notes = corpus_notes()?;
    let graph = derive(&notes, &config())?;
    let mut expected = RecordIdGenerator::new(FixedClock(GENERATED_MILLIS), CountingRandom::new(1));
    for entity in graph.entities() {
        assert_eq!(entity.id, expected.generate(RecordKind::Entity)?);
    }
    for edge in graph.relationships() {
        assert_eq!(edge.id, expected.generate(RecordKind::Relationship)?);
    }
    Ok(())
}
