//! The derivation rules, and — more importantly — the joins the resolver
//! refuses to make.
//!
//! Every Note here is built through the canonical [`fieldnotes_format`] builder,
//! so no test hand-formats notebook bytes and every input is a record the public
//! validator accepts.

mod support;

use fieldnotes_domain::{NoteType, RecordId, RecordIdGenerator, RecordKind};
use fieldnotes_format::{ParsedRecord, RecordBuilder};
use fieldnotes_graph::{
    CandidateReason, ConflictKind, EntityKind, GapKind, IdentityKey, ThreadKeyKind,
};
use support::{TestResult, config, derive, note};

const SCOPE: &str = "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001";

fn note_id(suffix: u16) -> String {
    format!("note_01a02900-0000-7000-8000-{suffix:012}")
}

/// The anchors of every entity, as flat anchor text.
fn anchors(graph: &fieldnotes_graph::DerivedGraph) -> Vec<Vec<String>> {
    graph
        .entities()
        .iter()
        .map(|entity| {
            entity
                .identities
                .iter()
                .map(IdentityKey::anchor_text)
                .collect()
        })
        .collect()
}

/// Two people whose only shared evidence is an identical display name stay two
/// people, and the coincidence is reported as a candidate.
#[test]
fn a_name_only_coincidence_is_a_candidate_never_a_merge() -> TestResult {
    let records = vec![
        note(
            &note_id(101),
            "outlook_contacts_work",
            NoteType::Contact,
            "2026-08-22T09:00:00+02:00",
            "# Sam Taylor\n",
            &[("identities", "email:sam@example.com")],
            &[
                ("title", "Sam Taylor"),
                ("source_scope", SCOPE),
                ("source_identity", "contact/1"),
            ],
        )?,
        note(
            &note_id(102),
            "outlook_contacts_work",
            NoteType::Contact,
            "2026-08-22T09:05:00+02:00",
            "# Sam Taylor\n",
            &[("identities", "email:samuel@example.org")],
            &[
                ("title", "Sam Taylor"),
                ("source_scope", SCOPE),
                ("source_identity", "contact/2"),
            ],
        )?,
    ];
    let graph = derive(&records, &config())?;

    assert_eq!(
        anchors(&graph),
        vec![
            vec!["email:sam@example.com".to_owned()],
            vec!["email:samuel@example.org".to_owned()]
        ],
        "an identical display name must not merge two mail identities"
    );
    assert!(
        graph
            .entities()
            .iter()
            .all(|entity| entity.title.as_deref() == Some("Sam Taylor")
                && entity.explanation.joins.is_empty())
    );

    let candidate = graph
        .candidates()
        .first()
        .ok_or("the coincidence must be reported as a candidate")?;
    assert_eq!(candidate.reason, CandidateReason::DisplayNameEquality);
    assert_eq!(candidate.value, "sam taylor");
    assert_eq!(candidate.entities.len(), 2);
    assert_eq!(candidate.identities.len(), 2);
    assert!(candidate.detail.contains("weak descriptive evidence"));
    // A candidate is never materialized as a record.
    assert_eq!(graph.projected_records()?.len(), 2);
    Ok(())
}

/// Anchors printed on one source contact record are one person; anchors listed on
/// a message are several different people.
#[test]
fn contact_record_anchors_join_but_a_messages_identities_do_not() -> TestResult {
    let mail = note(
        &note_id(111),
        "outlook_mail_work",
        NoteType::Mail,
        "2026-08-22T10:00:00+02:00",
        "# Migration\n",
        &[
            ("identities", "email:a@example.com|email:b@example.com"),
            ("to", "b@example.com"),
        ],
        &[
            ("from", "a@example.com"),
            ("source_scope", SCOPE),
            ("source_identity", "mail-message/1"),
        ],
    )?;
    let separate = derive(std::slice::from_ref(&mail), &config())?;
    assert_eq!(
        anchors(&separate),
        vec![
            vec!["email:a@example.com".to_owned()],
            vec!["email:b@example.com".to_owned()]
        ],
        "two anchors on one message are two people, not one person with two addresses"
    );
    assert_eq!(separate.relationships().len(), 1);

    let contact = note(
        &note_id(112),
        "outlook_contacts_work",
        NoteType::Contact,
        "2026-08-22T10:05:00+02:00",
        "# A\n",
        &[("identities", "email:a@example.com|phone:+41 79 111 22 33")],
        &[
            ("title", "Ada Example"),
            ("source_scope", SCOPE),
            ("source_identity", "contact/1"),
        ],
    )?;
    let joined = derive(&[mail, contact], &config())?;
    assert_eq!(
        anchors(&joined),
        vec![
            vec![
                "email:a@example.com".to_owned(),
                "phone:+41791112233".to_owned()
            ],
            vec!["email:b@example.com".to_owned()]
        ],
        "the contact record states both anchors belong to one subject"
    );
    let ada = joined
        .entities()
        .first()
        .ok_or("the joined entity must exist")?;
    let join = ada
        .explanation
        .joins
        .first()
        .ok_or("the join must be retained as evidence")?;
    assert_eq!(join.rule.rule, "contact-record-anchors-v1");
    assert_eq!(join.evidence.to_string(), note_id(112));
    assert_eq!(ada.title.as_deref(), Some("Ada Example"));
    // A contact record describes one subject, so it records no interaction.
    assert_eq!(joined.relationships().len(), 1);
    Ok(())
}

/// The same artifact ID in two Notes is one artifact carried twice; the Notes
/// stay separate evidence.
#[test]
fn duplicate_artifacts_are_detected_by_artifact_id() -> TestResult {
    let shared = format!("artifact_sha256_{}", "4".repeat(64));
    let other = format!("artifact_sha256_{}", "5".repeat(64));
    let records = vec![
        note(
            &note_id(121),
            "outlook_mail_work",
            NoteType::Mail,
            "2026-08-22T10:00:00+02:00",
            "# One\n",
            &[
                ("artifacts", &shared),
                ("attachments", &shared),
                ("identities", "email:a@example.com"),
            ],
            &[("source_scope", SCOPE), ("source_identity", "mail/1")],
        )?,
        note(
            &note_id(122),
            "teams_work",
            NoteType::Message,
            "2026-08-22T11:00:00+02:00",
            "# Two\n",
            &[
                ("artifacts", &shared),
                ("identities", "email:b@example.com"),
            ],
            &[("source_scope", SCOPE), ("source_identity", "chat/1")],
        )?,
        note(
            &note_id(123),
            "self",
            NoteType::File,
            "2026-08-22T12:00:00+02:00",
            "# Three\n",
            &[("artifacts", &other)],
            &[],
        )?,
    ];
    let graph = derive(&records, &config())?;

    assert_eq!(graph.artifacts().len(), 2);
    let duplicates: Vec<&fieldnotes_graph::ArtifactReference> =
        graph.duplicate_artifacts().collect();
    let duplicate = duplicates
        .first()
        .ok_or("the shared artifact must be reported as duplicated")?;
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicate.artifact.to_string(), shared);
    assert_eq!(
        duplicate
            .notes
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>(),
        vec![note_id(121), note_id(122)]
    );
    assert_eq!(
        duplicate
            .attachment_notes
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>(),
        vec![note_id(121)],
        "only one Note received it as an attachment"
    );
    assert_eq!(
        duplicate.first_seen.map(|value| value.to_string()),
        Some("2026-08-22T10:00:00+02:00".to_owned())
    );
    // Duplicate bytes never merge the Notes or the people in them.
    assert_eq!(graph.entities().len(), 2);
    Ok(())
}

/// A thread ID is source-local: two Notes join into one thread only inside one
/// portable source scope, and an unscoped thread ID joins nothing.
#[test]
fn threads_group_only_inside_one_source_scope() -> TestResult {
    let other_scope = "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000002";
    let records = vec![
        note(
            &note_id(131),
            "outlook_mail_work",
            NoteType::Mail,
            "2026-08-22T10:00:00+02:00",
            "# One\n",
            &[("identities", "email:a@example.com")],
            &[
                ("source_scope", SCOPE),
                ("source_identity", "mail/1"),
                ("thread_id", "outlook-thread/abc"),
            ],
        )?,
        note(
            &note_id(132),
            "outlook_mail_work",
            NoteType::Mail,
            "2026-08-22T10:30:00+02:00",
            "# Two\n",
            &[("identities", "email:a@example.com|email:b@example.com")],
            &[
                ("source_scope", SCOPE),
                ("source_identity", "mail/2"),
                ("thread_id", "outlook-thread/abc"),
            ],
        )?,
        note(
            &note_id(133),
            "outlook_mail_other",
            NoteType::Mail,
            "2026-08-22T10:45:00+02:00",
            "# Three\n",
            &[("identities", "email:c@example.com")],
            &[
                ("source_scope", other_scope),
                ("source_identity", "mail/1"),
                ("thread_id", "outlook-thread/abc"),
            ],
        )?,
        note(
            &note_id(134),
            "self",
            NoteType::Text,
            "2026-08-22T11:00:00+02:00",
            "# Local\n",
            &[("identities", "email:a@example.com")],
            &[("thread_id", "outlook-thread/abc")],
        )?,
    ];
    let graph = derive(&records, &config())?;

    assert_eq!(
        graph.threads().len(),
        2,
        "the same thread value in two tenants is two threads: {:#?}",
        graph.threads()
    );
    let first = graph
        .threads()
        .first()
        .ok_or("the scoped thread must exist")?;
    assert_eq!(first.key.kind, ThreadKeyKind::Thread);
    assert_eq!(first.key.scope, SCOPE);
    assert_eq!(first.note_count(), 2);
    assert_eq!(
        first
            .participants
            .iter()
            .map(IdentityKey::anchor_text)
            .collect::<Vec<String>>(),
        vec![
            "email:a@example.com".to_owned(),
            "email:b@example.com".to_owned()
        ]
    );
    assert_eq!(first.entities.len(), 2);
    assert_eq!(
        first.first_seen.map(|value| value.to_string()),
        Some("2026-08-22T10:00:00+02:00".to_owned())
    );

    let gap = graph
        .gaps()
        .iter()
        .find(|gap| gap.kind == GapKind::UnscopableThreadKey)
        .ok_or("an unscoped thread key must be reported")?;
    assert_eq!(gap.value.as_deref(), Some("outlook-thread/abc"));
    assert_eq!(gap.property.as_deref(), Some("thread_id"));
    Ok(())
}

/// First-seen and last-seen compare instants, not offset spellings, and the
/// interaction count is the number of distinct supporting Notes.
#[test]
fn first_and_last_seen_compare_instants_across_offsets() -> TestResult {
    let records = vec![
        note(
            &note_id(141),
            "teams_work",
            NoteType::Message,
            "2026-08-22T23:00:00+03:00",
            "# One\n",
            &[("identities", "email:a@example.com")],
            &[("source_scope", SCOPE), ("source_identity", "chat/1")],
        )?,
        note(
            &note_id(142),
            "teams_work",
            NoteType::Message,
            "2026-08-22T21:30:00+00:00",
            "# Two\n",
            &[("identities", "email:a@example.com")],
            &[("source_scope", SCOPE), ("source_identity", "chat/2")],
        )?,
        note(
            &note_id(143),
            "teams_work",
            NoteType::Message,
            "2026-08-22T14:00:00-05:00",
            "# Three\n",
            &[("identities", "email:a@example.com")],
            &[("source_scope", SCOPE), ("source_identity", "chat/3")],
        )?,
    ];
    let graph = derive(&records, &config())?;
    let entity = graph
        .entities()
        .first()
        .ok_or("the recurring anchor must derive one entity")?;

    assert_eq!(entity.interaction_count, 3);
    assert_eq!(entity.evidence.len(), 3);
    assert_eq!(
        entity.first_seen.map(|value| value.to_string()),
        Some("2026-08-22T14:00:00-05:00".to_owned()),
        "19:00Z is the earliest instant, though its spelling sorts last"
    );
    assert_eq!(
        entity.last_seen.map(|value| value.to_string()),
        Some("2026-08-22T21:30:00+00:00".to_owned()),
        "21:30Z is the latest instant, though 23:00+03:00 reads later"
    );
    assert_eq!(entity.explanation.rule.rule, "email-exact-v1");
    Ok(())
}

/// Identical Note IDs deduplicate exactly; an identical `content_hash` on two
/// different Notes never removes either.
#[test]
fn identical_note_ids_deduplicate_and_content_hash_equality_does_not() -> TestResult {
    let one = note(
        &note_id(151),
        "self",
        NoteType::Text,
        "2026-08-22T09:00:00+02:00",
        "# Same body\n",
        &[("identities", "email:a@example.com")],
        &[],
    )?;
    let duplicate_id = derive(&[one.clone(), one.clone()], &config())?;
    let entity = duplicate_id
        .entities()
        .first()
        .ok_or("one entity must be derived")?;
    assert_eq!(
        entity.interaction_count, 1,
        "the same Note supplied twice is one Note"
    );
    assert!(duplicate_id.conflicts().is_empty());

    // A second Note with a different ID but the same body — and therefore the
    // same content hash — is different evidence, not a duplicate.
    let two = note(
        &note_id(152),
        "self",
        NoteType::Text,
        "2026-08-22T09:00:00+02:00",
        "# Same body\n",
        &[("identities", "email:a@example.com")],
        &[],
    )?;
    let same_content = derive(&[one, two], &config())?;
    let entity = same_content
        .entities()
        .first()
        .ok_or("one entity must be derived")?;
    assert_eq!(
        entity.interaction_count, 2,
        "content equality is not identity, so both Notes remain evidence"
    );
    assert!(same_content.conflicts().is_empty());
    Ok(())
}

/// Two independently collected copies of one upstream object collapse on their
/// portable source key and union their producers.
#[test]
fn a_portable_source_key_collapse_unions_producers() -> TestResult {
    let left = note(
        &note_id(161),
        "outlook_mail_work",
        NoteType::Mail,
        "2026-08-22T10:00:00+02:00",
        "# Migration\n",
        &[
            ("identities", "email:a@example.com"),
            (
                "collected_by",
                "fn_01a02837-2de0-7a2b-8c41-f2481851192a/outlook_mail_work",
            ),
        ],
        &[("source_scope", SCOPE), ("source_identity", "mail/1")],
    )?;
    let right = note(
        &note_id(162),
        "outlook_mail_work",
        NoteType::Mail,
        "2026-08-22T10:00:00+02:00",
        "# Migration\n",
        &[
            ("identities", "email:a@example.com"),
            (
                "collected_by",
                "fn_01a02838-2de0-7a2b-8c41-f2481851192a/outlook_mail_home",
            ),
        ],
        &[("source_scope", SCOPE), ("source_identity", "mail/1")],
    )?;
    let graph = derive(&[left, right], &config())?;

    let collapse = graph
        .source_key_collapses()
        .first()
        .ok_or("the exact portable-source-key match must collapse")?;
    assert_eq!(collapse.source_scope, SCOPE);
    assert_eq!(collapse.source_identity, "mail/1");
    assert_eq!(collapse.survivor.to_string(), note_id(161));
    assert_eq!(collapse.notes.len(), 2);
    assert!(
        collapse
            .producers
            .contains(&"fn_01a02837-2de0-7a2b-8c41-f2481851192a/outlook_mail_work".to_owned())
            && collapse
                .producers
                .contains(&"fn_01a02838-2de0-7a2b-8c41-f2481851192a/outlook_mail_home".to_owned()),
        "every known producer is retained: {:?}",
        collapse.producers
    );
    assert!(collapse.fingerprint.starts_with("fn-record-v1-sha256:"));
    let entity = graph
        .entities()
        .first()
        .ok_or("one entity must be derived")?;
    assert_eq!(
        entity.interaction_count, 1,
        "one upstream object is one piece of evidence, not two"
    );
    assert!(graph.conflicts().is_empty());
    Ok(())
}

/// Two current contact records that disagree about a person's name leave the
/// projected name unset and report the contradiction.
#[test]
fn contradictory_names_leave_no_title_and_report_a_conflict() -> TestResult {
    let records = vec![
        note(
            &note_id(171),
            "outlook_contacts_work",
            NoteType::Contact,
            "2026-08-22T09:00:00+02:00",
            "# Sam\n",
            &[("identities", "email:sam@example.com")],
            &[
                ("title", "Sam Taylor"),
                ("source_scope", SCOPE),
                ("source_identity", "contact/1"),
            ],
        )?,
        note(
            &note_id(172),
            "outlook_contacts_work",
            NoteType::Contact,
            "2026-08-22T09:30:00+02:00",
            "# Samuel\n",
            &[("identities", "email:sam@example.com")],
            &[
                ("title", "Samuel Taylor-Smith"),
                ("source_scope", SCOPE),
                ("source_identity", "contact/2"),
            ],
        )?,
    ];
    let graph = derive(&records, &config())?;
    let entity = graph
        .entities()
        .first()
        .ok_or("the shared mail anchor derives one entity")?;
    assert_eq!(entity.identities.len(), 1);
    assert_eq!(
        entity.title, None,
        "choosing one name would be last-writer-wins"
    );
    let competing = entity
        .explanation
        .competing
        .first()
        .ok_or("the contradiction must be part of the explanation")?;
    assert!(competing.claim.contains("Sam Taylor"));
    assert!(competing.claim.contains("Samuel Taylor-Smith"));
    assert_eq!(competing.evidence.len(), 2);

    let conflict = graph
        .conflicts()
        .first()
        .ok_or("the contradiction must be reported")?;
    assert_eq!(conflict.kind, ConflictKind::ContradictoryName);
    assert_eq!(conflict.entities, vec![entity.id]);

    // The public record shows the contradiction rather than hiding it.
    let projected = graph.projected_records()?;
    let record = projected.first().ok_or("the entity must be projected")?;
    assert!(record.record.text().contains("Competing current evidence"));
    assert!(!record.record.text().contains("title:"));
    Ok(())
}

/// A role value that normalizes into no declared namespace is a reported gap,
/// and at most a candidate when it happens to match a known display name.
#[test]
fn an_unresolvable_role_value_is_a_gap_and_at_most_a_candidate() -> TestResult {
    let records = vec![
        note(
            &note_id(181),
            "outlook_contacts_work",
            NoteType::Contact,
            "2026-08-22T09:00:00+02:00",
            "# Sam\n",
            &[("identities", "email:sam@example.com")],
            &[
                ("title", "Sam Taylor"),
                ("source_scope", SCOPE),
                ("source_identity", "contact/1"),
            ],
        )?,
        note(
            &note_id(182),
            "outlook_calendar_work",
            NoteType::Event,
            "2026-08-22T10:00:00+02:00",
            "# Review\n",
            &[("participants", "Sam Taylor|Conference Room 4")],
            &[
                ("organizer", "044 123 45 67"),
                ("source_scope", SCOPE),
                ("source_identity", "event/1"),
            ],
        )?,
    ];
    let graph = derive(&records, &config())?;

    let unresolved: Vec<&fieldnotes_graph::Gap> = graph
        .gaps()
        .iter()
        .filter(|gap| gap.kind == GapKind::UnresolvedRoleValue)
        .collect();
    assert_eq!(unresolved.len(), 3, "{:#?}", graph.gaps());
    assert!(
        unresolved
            .iter()
            .any(|gap| gap.value.as_deref() == Some("044 123 45 67")
                && gap.detail.contains("country context")),
        "a national-format phone number has no country context"
    );
    assert!(
        unresolved
            .iter()
            .any(|gap| gap.value.as_deref() == Some("Conference Room 4"))
    );

    let candidate = graph
        .candidates()
        .iter()
        .find(|candidate| candidate.reason == CandidateReason::UnresolvedValueMatchesName)
        .ok_or("the name match must surface as a candidate")?;
    assert_eq!(candidate.value, "sam taylor");
    assert_eq!(candidate.entities.len(), 1);
    assert!(candidate.detail.contains("never a merge"));

    // The calendar Note supports no entity, so Sam's evidence is the contact
    // record alone: a name match adds no evidence.
    let entity = graph
        .entities()
        .first()
        .ok_or("the contact record derives one entity")?;
    assert_eq!(entity.interaction_count, 1);
    assert_eq!(entity.channels, vec!["outlook_contacts".to_owned()]);
    Ok(())
}

/// An organization entity comes only from an explicit `domain:` anchor, never
/// from a mail address's domain, and a contact record never joins classes.
#[test]
fn organizations_come_only_from_explicit_domain_anchors() -> TestResult {
    let records = vec![
        note(
            &note_id(191),
            "outlook_mail_work",
            NoteType::Mail,
            "2026-08-22T10:00:00+02:00",
            "# Migration\n",
            &[(
                "identities",
                "domain:Example.COM|email:a@example.com|email:b@other.example",
            )],
            &[("source_scope", SCOPE), ("source_identity", "mail/1")],
        )?,
        note(
            &note_id(192),
            "outlook_contacts_work",
            NoteType::Contact,
            "2026-08-22T10:30:00+02:00",
            "# C\n",
            &[("identities", "domain:third.example|email:c@third.example")],
            &[
                ("title", "Cy Example"),
                ("source_scope", SCOPE),
                ("source_identity", "contact/1"),
            ],
        )?,
    ];
    let graph = derive(&records, &config())?;

    assert_eq!(
        anchors(&graph),
        vec![
            vec!["domain:example.com".to_owned()],
            vec!["domain:third.example".to_owned()],
            vec!["email:a@example.com".to_owned()],
            vec!["email:b@other.example".to_owned()],
            vec!["email:c@third.example".to_owned()],
        ],
        "a mail address's domain never becomes an organization anchor by itself"
    );
    assert_eq!(
        graph
            .entities()
            .iter()
            .filter(|entity| entity.kind == EntityKind::Organization)
            .count(),
        2
    );
    // The contact record carried both a person-class and an organization-class
    // anchor, so the co-identity rule refused to join across classes.
    let gap = graph
        .gaps()
        .iter()
        .find(|gap| gap.kind == GapKind::MixedClassContactRecord)
        .ok_or("the cross-class refusal must be reported")?;
    assert_eq!(gap.records.len(), 1);
    assert!(
        graph
            .entities()
            .iter()
            .all(|entity| entity.identities.len() == 1)
    );

    // Only `person_person` edges exist; a person-organization pairing has no
    // approved relationship type.
    assert_eq!(graph.relationships().len(), 1);
    let projected = graph.projected_records()?;
    assert!(
        projected
            .iter()
            .any(|record| record.relative_path.ends_with("_organization.md"))
    );
    assert_eq!(projected.len(), 6, "five entities and one relationship");
    Ok(())
}

/// When prior projections do not map one-to-one onto current entities, no prior
/// ID is reused and the ambiguity is reported.
#[test]
fn an_ambiguous_projection_rebind_is_reported_and_mints_a_new_id() -> TestResult {
    let contact = note(
        &note_id(201),
        "outlook_contacts_work",
        NoteType::Contact,
        "2026-08-22T09:00:00+02:00",
        "# D\n",
        &[("identities", "email:d@example.com|phone:+41 79 000 11 22")],
        &[
            ("title", "Dee Example"),
            ("source_scope", SCOPE),
            ("source_identity", "contact/1"),
        ],
    )?;
    // Two prior projections, one per anchor, as an earlier rebuild without the
    // contact record would have produced.
    let priors = vec![
        prior_entity(
            "ent_01a028f2-dcc0-7000-8000-000000000301",
            "email:d@example.com",
        )?,
        prior_entity(
            "ent_01a028f2-dcc0-7000-8000-000000000302",
            "phone:+41790001122",
        )?,
    ];
    let mut records = vec![contact];
    records.extend(priors);
    let graph = derive(&records, &config())?;

    let entity = graph
        .entities()
        .first()
        .ok_or("the contact record derives one entity")?;
    assert_eq!(entity.identities.len(), 2);
    let mut expected = RecordIdGenerator::new(
        fieldnotes_test_support::FixedClock(support::GENERATED_MILLIS),
        fieldnotes_test_support::CountingRandom::new(1),
    );
    assert_eq!(
        entity.id,
        expected.generate(RecordKind::Entity)?,
        "a fresh ID is minted rather than one of the two prior IDs being preferred"
    );
    let conflict = graph
        .conflicts()
        .iter()
        .find(|conflict| conflict.kind == ConflictKind::AmbiguousProjectionRebind)
        .ok_or("the ambiguous rebind must be reported")?;
    assert_eq!(conflict.entities.len(), 2);
    assert!(conflict.detail.contains("entity-id-reuse-v1"));
    Ok(())
}

/// Builds a prior entity projection record carrying one anchor.
fn prior_entity(id: &str, anchor: &str) -> Result<ParsedRecord, Box<dyn std::error::Error>> {
    let id = RecordId::parse(id)?;
    let mut builder = RecordBuilder::new(&id);
    builder.set_text("type", "person");
    builder.set_text_list("identities", [anchor]);
    builder.set_text("generator_version", "fieldnotes-entity-resolver-v1");
    builder.set_datetime(
        "generated_at",
        fieldnotes_domain::Datetime::parse("2026-08-22T11:00:00+02:00")?,
    );
    builder.set_body(format!("# {anchor}\n"));
    Ok(builder.build()?.record().clone())
}

/// One portable source key with divergent current state and no reliable
/// ordering stays a visible conflict, and neither copy becomes evidence.
#[test]
fn divergent_state_under_one_source_key_is_a_preserved_conflict() -> TestResult {
    let left = note(
        &note_id(211),
        "outlook_mail_work",
        NoteType::Mail,
        "2026-08-22T10:00:00+02:00",
        "# Window\n",
        &[("identities", "email:a@example.com")],
        &[
            ("source_scope", SCOPE),
            ("source_identity", "mail/1"),
            ("subject", "Migration window at 18:00"),
        ],
    )?;
    let right = note(
        &note_id(212),
        "outlook_mail_work",
        NoteType::Mail,
        "2026-08-22T10:00:00+02:00",
        "# Window\n",
        &[("identities", "email:a@example.com")],
        &[
            ("source_scope", SCOPE),
            ("source_identity", "mail/1"),
            ("subject", "Migration window at 20:00"),
        ],
    )?;
    let graph = derive(&[left, right], &config())?;

    let conflict = graph
        .conflicts()
        .first()
        .ok_or("the divergence must be reported")?;
    assert_eq!(conflict.kind, ConflictKind::SourceKeyDivergence);
    assert_eq!(conflict.notes.len(), 2);
    assert_eq!(conflict.fingerprints.len(), 2);
    assert!(conflict.values.contains(&"mail/1".to_owned()));
    assert!(
        graph.entities().is_empty(),
        "no copy is silently declared current"
    );
    assert_eq!(
        graph
            .gaps()
            .iter()
            .filter(|gap| gap.kind == GapKind::ExcludedByConflict)
            .count(),
        2
    );
    Ok(())
}

/// An authority-scoped anchor with the same unqualified value in two tenants is
/// two identities, and it is never published as a flat globally exact anchor.
#[test]
fn an_authority_scoped_anchor_never_joins_across_scopes() -> TestResult {
    let namespaces =
        fieldnotes_graph::NamespaceRegistry::with_policies([fieldnotes_graph::NamespacePolicy {
            namespace: "graph-user-id".to_owned(),
            scope_class: fieldnotes_graph::ScopeClass::AuthorityScoped,
            strength: fieldnotes_graph::Strength::Exact,
            normalization: fieldnotes_graph::NormalizationRule::OpaqueTokenV1,
            entity_kind: EntityKind::Person,
        }])?;
    let settings = fieldnotes_graph::GraphConfig {
        namespaces,
        ..config()
    };
    let records = vec![
        note(
            &note_id(221),
            "teams_work",
            NoteType::Message,
            "2026-08-22T10:00:00+02:00",
            "# One\n",
            &[("identities", "graph-user-id:U1")],
            &[("source_scope", SCOPE), ("source_identity", "chat/1")],
        )?,
        note(
            &note_id(222),
            "teams_other",
            NoteType::Message,
            "2026-08-22T10:05:00+02:00",
            "# Two\n",
            &[("identities", "graph-user-id:U1")],
            &[
                (
                    "source_scope",
                    "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000002",
                ),
                ("source_identity", "chat/1"),
            ],
        )?,
    ];
    let graph = derive(&records, &settings)?;

    assert_eq!(
        graph.entities().len(),
        2,
        "the same source-local ID in two authorities is two people"
    );
    for entity in graph.entities() {
        let key = entity
            .primary_identity()
            .ok_or("each entity rests on one anchor")?;
        assert_eq!(
            key.value(),
            "U1",
            "an opaque source ID keeps its exact case"
        );
        assert!(key.scope().is_some());
        assert!(!key.is_publishable());
    }
    assert_eq!(
        graph
            .gaps()
            .iter()
            .filter(|gap| gap.kind == GapKind::UnpublishableIdentities)
            .count(),
        2,
        "A1 froze no public flat spelling that carries a scope"
    );
    for record in graph.projected_records()? {
        assert!(!record.record.text().contains("identities:"));
    }
    Ok(())
}

/// Optional derived input is read but contributes nothing in v0.1.2, and that is
/// reported rather than silently ignored.
#[test]
fn extractions_and_observations_contribute_nothing_and_are_reported() -> TestResult {
    let mut records = vec![note(
        &note_id(231),
        "self",
        NoteType::Voice,
        "2026-08-22T09:30:00+02:00",
        "# Voice\n",
        &[("identities", "email:a@example.com")],
        &[],
    )?];
    records.extend(support::load_records(
        &support::corpus_root().join("extractions"),
    )?);
    records.extend(support::load_records(
        &support::corpus_root().join("observations"),
    )?);
    let graph = derive(&records, &config())?;

    assert_eq!(graph.entities().len(), 1);
    let reported: Vec<&fieldnotes_graph::Gap> = graph
        .gaps()
        .iter()
        .filter(|gap| gap.kind == GapKind::NonNoteInput)
        .collect();
    assert_eq!(reported.len(), 2, "{:#?}", graph.gaps());
    assert!(
        reported.iter().any(|gap| gap.detail.contains("Extraction"))
            && reported
                .iter()
                .any(|gap| gap.detail.contains("Observation"))
    );
    // The Observation's `subject_entity_id` points at a projection ID this
    // rebuild did not mint; nothing is invented to match it.
    assert!(
        graph
            .entities()
            .iter()
            .all(|entity| entity.explanation.origin != fieldnotes_graph::Origin::Observed)
    );
    Ok(())
}
