//! The deterministic derivation engine.
//!
//! [`derive_graph`] takes the current set of parsed records and returns one
//! [`DerivedGraph`]. It reads no clock of its own, opens no file, enumerates no
//! directory, and never lets an unordered collection reach a result: every
//! intermediate map is a [`BTreeMap`] or [`BTreeSet`], every output vector is
//! sorted by a key that is a function of the evidence, and projection IDs are
//! minted in that same sorted order from the injected generator. Two runs over
//! the same records in any input order therefore produce the same graph, and the
//! same bytes when the injected clock and ID generator are the same.

use std::collections::{BTreeMap, BTreeSet};

use fieldnotes_domain::{
    ArtifactId, Clock, Datetime, DatetimeError, FieldStemRegistry, IdError, RandomSource, RecordId,
    RecordIdGenerator, RecordKind, Scalar, Value,
};
use fieldnotes_format::{
    ParsedRecord, ValidationError, canonical_record_string, record_fingerprint,
    semantic_record_string,
};

use crate::candidate::{CandidateReason, MergeCandidate};
use crate::emit::{ProjectedRecord, entity_record, relationship_record};
use crate::entity::{Entity, EntityKind};
use crate::evidence::{
    CO_PARTICIPANT_RULE, CONTACT_RECORD_RULE, CompetingEvidence, Explanation, ID_REUSE_RULE,
    IdentityJoin, Origin, ResolvedIdentity, RuleId, anchor_rule, exact_rule,
};
use crate::facts::{ArtifactReference, SourceKeyCollapse, Thread, ThreadKey, ThreadKeyKind};
use crate::gap::{ConflictKind, Gap, GapKind, ReportedConflict};
use crate::identity::{
    ARTIFACT_NAMESPACE, IdentityKey, NamespaceRegistry, PARTICIPANT_RULE, normalize_channel_value,
    normalized_display_name, parse_anchor,
};
use crate::relationship::{Relationship, RelationshipKind};

/// Errors the derivation can return.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GraphError {
    /// A projection ID could not be minted.
    Id(IdError),
    /// The generation instant could not be rendered with an explicit offset.
    Datetime(DatetimeError),
    /// A record could not be canonicalized, fingerprinted, or emitted.
    Record(ValidationError),
    /// A count exceeded what a canonical finite JSON number may carry.
    CountOutOfRange {
        /// The property that would have carried it.
        key: String,
    },
}

impl core::fmt::Display for GraphError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            GraphError::Id(error) => write!(f, "projection ID generation failed: {error}"),
            GraphError::Datetime(error) => {
                write!(f, "generation instant is not serializable: {error}")
            }
            GraphError::Record(error) => write!(f, "record handling failed: {error}"),
            GraphError::CountOutOfRange { key } => {
                write!(f, "{key} exceeds the canonical number range")
            }
        }
    }
}

impl std::error::Error for GraphError {}

impl From<IdError> for GraphError {
    fn from(error: IdError) -> Self {
        GraphError::Id(error)
    }
}

impl From<DatetimeError> for GraphError {
    fn from(error: DatetimeError) -> Self {
        GraphError::Datetime(error)
    }
}

impl From<ValidationError> for GraphError {
    fn from(error: ValidationError) -> Self {
        GraphError::Record(error)
    }
}

/// A source of projection record IDs.
///
/// The engine takes this rather than a concrete generator so a caller can inject
/// a fixed clock and byte source and replay the same IDs.
pub trait ProjectionIds {
    /// Mints the next ID of `kind`.
    fn next_id(&mut self, kind: RecordKind) -> Result<RecordId, IdError>;
}

impl<C: Clock, R: RandomSource> ProjectionIds for RecordIdGenerator<C, R> {
    fn next_id(&mut self, kind: RecordKind) -> Result<RecordId, IdError> {
        self.generate(kind)
    }
}

/// Deterministic inputs the derivation needs beyond the records themselves.
#[derive(Debug, Clone)]
pub struct GraphConfig {
    /// The declared identity namespaces and their normalization rules.
    pub namespaces: NamespaceRegistry,
    /// The numeric UTC offset, in minutes east of UTC, used to render
    /// `generated_at`. The composition root supplies the client-local offset at
    /// the generation instant; zero renders as `+00:00`.
    pub generated_at_offset_minutes: i16,
    /// The largest evidence list a projected record may carry. `None` emits the
    /// complete list. When a list is bounded, the record also carries the full
    /// `evidence_count`, so a reader can tell the list is representative.
    pub evidence_limit: Option<usize>,
    /// The registered Field set used to map a Field ID onto its channel stem.
    pub field_stems: FieldStemRegistry,
}

impl Default for GraphConfig {
    fn default() -> Self {
        GraphConfig {
            namespaces: NamespaceRegistry::v1(),
            generated_at_offset_minutes: 0,
            evidence_limit: None,
            field_stems: FieldStemRegistry::v1().clone(),
        }
    }
}

/// The complete derived projection over one set of current records.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedGraph {
    generated_at: Datetime,
    entities: Vec<Entity>,
    relationships: Vec<Relationship>,
    candidates: Vec<MergeCandidate>,
    threads: Vec<Thread>,
    artifacts: Vec<ArtifactReference>,
    collapses: Vec<SourceKeyCollapse>,
    conflicts: Vec<ReportedConflict>,
    gaps: Vec<Gap>,
}

impl DerivedGraph {
    /// The instant the projection was generated, with an explicit offset.
    #[must_use]
    pub fn generated_at(&self) -> &Datetime {
        &self.generated_at
    }

    /// The derived entities, ordered by primary identity anchor.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The derived relationships, ordered by their canonical entity pair.
    #[must_use]
    pub fn relationships(&self) -> &[Relationship] {
        &self.relationships
    }

    /// Weak-evidence matches that were deliberately not applied.
    #[must_use]
    pub fn candidates(&self) -> &[MergeCandidate] {
        &self.candidates
    }

    /// The derived threads, ordered by scoped thread key.
    #[must_use]
    pub fn threads(&self) -> &[Thread] {
        &self.threads
    }

    /// Every referenced original artifact, ordered by artifact ID.
    #[must_use]
    pub fn artifacts(&self) -> &[ArtifactReference] {
        &self.artifacts
    }

    /// The artifacts more than one current Note references.
    pub fn duplicate_artifacts(&self) -> impl Iterator<Item = &ArtifactReference> {
        self.artifacts
            .iter()
            .filter(|reference| reference.is_duplicated())
    }

    /// Input copies that collapsed onto one exact portable source key.
    #[must_use]
    pub fn source_key_collapses(&self) -> &[SourceKeyCollapse] {
        &self.collapses
    }

    /// Contradictions preserved rather than resolved.
    #[must_use]
    pub fn conflicts(&self) -> &[ReportedConflict] {
        &self.conflicts
    }

    /// What the graph could not resolve, and why.
    #[must_use]
    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    /// The entity with this projection ID.
    #[must_use]
    pub fn entity(&self, id: &RecordId) -> Option<&Entity> {
        self.entities.iter().find(|entity| &entity.id == id)
    }

    /// The single current entity resting on `key`, if any.
    ///
    /// An anchor belongs to at most one entity by construction, which is what
    /// lets a proposal rebind its `entity_id` only when its `subject_identity`
    /// resolves unambiguously.
    #[must_use]
    pub fn entity_for_identity(&self, key: &IdentityKey) -> Option<&Entity> {
        self.entities.iter().find(|entity| entity.has_identity(key))
    }

    /// The relationship with this projection ID.
    #[must_use]
    pub fn relationship(&self, id: &RecordId) -> Option<&Relationship> {
        self.relationships
            .iter()
            .find(|relationship| &relationship.id == id)
    }

    /// The explanation for any derived entity or relationship.
    #[must_use]
    pub fn explain(&self, id: &RecordId) -> Option<&Explanation> {
        self.entity(id)
            .map(|entity| &entity.explanation)
            .or_else(|| self.relationship(id).map(|edge| &edge.explanation))
    }

    /// Every projection v0.1 materializes as a public notebook record, with its
    /// notebook-relative path, ordered by path.
    ///
    /// The caller owns all I/O: this returns validated canonical bytes and never
    /// touches a filesystem.
    pub fn projected_records(&self) -> Result<Vec<ProjectedRecord>, GraphError> {
        let mut records = Vec::new();
        for entity in &self.entities {
            if !entity.kind.is_materialized() {
                continue;
            }
            records.push(entity_record(entity, self.generated_at)?);
        }
        for relationship in &self.relationships {
            let (Some(from), Some(to)) = (
                self.entity(&relationship.from_entity_id),
                self.entity(&relationship.to_entity_id),
            ) else {
                continue;
            };
            records.push(relationship_record(
                relationship,
                from,
                to,
                self.generated_at,
            )?);
        }
        records.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(records)
    }
}

/// Which property supplied an anchor occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AnchorSource {
    Identities,
    From,
    To,
    Cc,
    Bcc,
    Organizer,
    Participants,
    Artifacts,
}

impl AnchorSource {
    fn property(self) -> &'static str {
        match self {
            AnchorSource::Identities => "identities",
            AnchorSource::From => "from",
            AnchorSource::To => "to",
            AnchorSource::Cc => "cc",
            AnchorSource::Bcc => "bcc",
            AnchorSource::Organizer => "organizer",
            AnchorSource::Participants => "participants",
            AnchorSource::Artifacts => "artifacts",
        }
    }
}

/// The role properties in the fixed order they are read, with whether the
/// registered type is a list.
const ROLE_PROPERTIES: [(AnchorSource, bool); 6] = [
    (AnchorSource::From, false),
    (AnchorSource::Organizer, false),
    (AnchorSource::To, true),
    (AnchorSource::Cc, true),
    (AnchorSource::Bcc, true),
    (AnchorSource::Participants, true),
];

/// Everything the engine needs from one surviving Note.
#[derive(Debug, Clone)]
struct NoteFacts {
    id: RecordId,
    channel: String,
    occurred_at: Datetime,
    /// Anchors and the property that supplied each, deduplicated and ordered.
    anchors: BTreeSet<(IdentityKey, AnchorSource)>,
    artifacts: BTreeSet<ArtifactId>,
    attachments: BTreeSet<ArtifactId>,
    thread_keys: BTreeSet<ThreadKey>,
    /// True when the Note is a source contact record, whose anchors describe one
    /// subject rather than several participants.
    co_identity: bool,
    /// The display name a source contact record supplies for its subject.
    display_name: Option<String>,
}

impl NoteFacts {
    /// The anchors that describe people or organizations, excluding artifacts.
    fn participant_keys(&self) -> BTreeSet<IdentityKey> {
        self.anchors
            .iter()
            .filter(|(key, _)| key.namespace() != ARTIFACT_NAMESPACE)
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Whether any anchor of this Note belongs to `component`.
    fn touches(&self, component: &BTreeSet<IdentityKey>) -> bool {
        self.anchors.iter().any(|(key, _)| component.contains(key))
    }
}

fn text<'a>(record: &'a ParsedRecord, key: &str) -> Option<&'a str> {
    match record.get(key) {
        Some(Value::Scalar(Scalar::Text(value))) => Some(value.as_str()),
        _ => None,
    }
}

fn text_list<'a>(record: &'a ParsedRecord, key: &str) -> Vec<&'a str> {
    match record.get(key) {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                Scalar::Text(value) => Some(value.as_str()),
                _ => None,
            })
            .collect(),
        Some(Value::Scalar(Scalar::Text(value))) => vec![value.as_str()],
        _ => Vec::new(),
    }
}

/// The earlier of two instants, breaking an exact tie by ascending Note ID so
/// the choice never depends on input order.
fn earlier(
    current: Option<(Datetime, RecordId)>,
    candidate: (Datetime, RecordId),
) -> Option<(Datetime, RecordId)> {
    match current {
        Some(existing) => {
            let take = match candidate.0.cmp_instant(&existing.0) {
                core::cmp::Ordering::Less => true,
                core::cmp::Ordering::Equal => candidate.1 < existing.1,
                core::cmp::Ordering::Greater => false,
            };
            Some(if take { candidate } else { existing })
        }
        None => Some(candidate),
    }
}

/// The later of two instants, breaking an exact tie by ascending Note ID.
fn later(
    current: Option<(Datetime, RecordId)>,
    candidate: (Datetime, RecordId),
) -> Option<(Datetime, RecordId)> {
    match current {
        Some(existing) => {
            let take = match candidate.0.cmp_instant(&existing.0) {
                core::cmp::Ordering::Greater => true,
                core::cmp::Ordering::Equal => candidate.1 < existing.1,
                core::cmp::Ordering::Less => false,
            };
            Some(if take { candidate } else { existing })
        }
        None => Some(candidate),
    }
}

/// Notes, channels, and a time range accumulated over a set of Notes.
#[derive(Debug, Default, Clone)]
struct Span {
    notes: BTreeSet<RecordId>,
    channels: BTreeSet<String>,
    first: Option<(Datetime, RecordId)>,
    last: Option<(Datetime, RecordId)>,
}

impl Span {
    fn add(&mut self, note: &NoteFacts) {
        self.notes.insert(note.id);
        self.channels.insert(note.channel.clone());
        self.first = earlier(self.first, (note.occurred_at, note.id));
        self.last = later(self.last, (note.occurred_at, note.id));
    }

    fn first_seen(&self) -> Option<Datetime> {
        self.first.map(|(when, _)| when)
    }

    fn last_seen(&self) -> Option<Datetime> {
        self.last.map(|(when, _)| when)
    }
}

/// Applies the configured evidence bound to an ascending Note-ID set.
fn bounded(notes: &BTreeSet<RecordId>, limit: Option<usize>) -> Vec<RecordId> {
    match limit {
        Some(limit) => notes.iter().copied().take(limit).collect(),
        None => notes.iter().copied().collect(),
    }
}

/// A disjoint-set forest over the sorted anchor list.
struct Components {
    parent: Vec<usize>,
}

impl Components {
    fn new(len: usize) -> Self {
        Components {
            parent: (0..len).collect(),
        }
    }

    fn parent_of(&self, index: usize) -> usize {
        self.parent.get(index).copied().unwrap_or(index)
    }

    fn find(&mut self, mut index: usize) -> usize {
        while self.parent_of(index) != index {
            let grandparent = self.parent_of(self.parent_of(index));
            if let Some(slot) = self.parent.get_mut(index) {
                *slot = grandparent;
            }
            index = grandparent;
        }
        index
    }

    fn union(&mut self, left: usize, right: usize) {
        let (left, right) = (self.find(left), self.find(right));
        if left == right {
            return;
        }
        // The higher index always attaches under the lower one, so a component's
        // representative is its lowest member and the result does not depend on
        // the order the unions were applied in.
        let (low, high) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        if let Some(slot) = self.parent.get_mut(high) {
            *slot = low;
        }
    }
}

/// A prior entity projection found in the input, used to keep projection IDs
/// stable across a rebuild that did not delete `entities/`.
#[derive(Debug, Clone)]
struct PriorEntity {
    id: RecordId,
    kind: Option<EntityKind>,
    keys: BTreeSet<IdentityKey>,
}

/// A prior relationship projection found in the input, used to keep a rebuild
/// that changed nothing from rewriting every relationship file under a new ID.
#[derive(Debug, Clone)]
struct PriorRelationship {
    id: RecordId,
    kind: String,
    from: Option<RecordId>,
    to: Option<RecordId>,
}

/// The evidence accumulated for one entity pair.
#[derive(Debug, Clone)]
struct PairEvidence {
    from: RecordId,
    to: RecordId,
    span: Span,
    keys: BTreeSet<IdentityKey>,
}

/// Derives the complete graph from the current records.
///
/// `records` may contain Notes, prior projections, and optional derived records
/// in any order; only Notes contribute evidence. `clock` supplies the
/// `generated_at` instant and `ids` mints projection IDs — both injected, so a
/// test can replay byte-identical output.
///
/// The stages run in a fixed order: reconcile the input, resolve identities,
/// build entities, build relationships, then derive the reproducible facts and
/// the candidates weak evidence only suggests.
pub fn derive_graph(
    records: &[ParsedRecord],
    config: &GraphConfig,
    clock: &dyn Clock,
    ids: &mut dyn ProjectionIds,
) -> Result<DerivedGraph, GraphError> {
    let generated_at = Datetime::from_unix_millis(
        i64::try_from(clock.unix_millis()).map_err(|_| DatetimeError::OutOfRange)?,
        config.generated_at_offset_minutes,
    )?;

    let input = reconcile_input(records, config)?;
    let resolution = resolve_identities(&input.notes, config);
    let stage = build_entities(&input, &resolution, config, ids)?;
    let relationships = build_relationships(&stage, &input, &resolution, config, ids)?;
    let threads = build_threads(&input.notes, &stage.entity_of_key);
    let artifacts = build_artifacts(&input.notes);

    let mut gaps = input.gaps;
    gaps.extend(resolution.gaps);
    gaps.extend(stage.gaps);
    gaps.sort();
    gaps.dedup();
    let mut conflicts = input.conflicts;
    conflicts.extend(stage.conflicts);
    conflicts.sort();
    conflicts.dedup();
    let mut collapses = input.collapses;
    collapses.sort();

    let mut candidates = name_candidates(&stage.entities);
    candidates.extend(unresolved_value_candidates(
        &input.notes,
        &stage.entities,
        &gaps,
    ));
    candidates.sort();
    candidates.dedup();

    Ok(DerivedGraph {
        generated_at,
        entities: stage.entities,
        relationships,
        candidates,
        threads,
        artifacts,
        collapses,
        conflicts,
        gaps,
    })
}

/// The reconciled derivation input: the Notes that are current evidence, the
/// prior projections that were present, and what could not be reconciled.
#[derive(Debug, Default)]
struct ReconciledInput {
    notes: BTreeMap<RecordId, NoteFacts>,
    priors: Vec<PriorEntity>,
    prior_edges: Vec<PriorRelationship>,
    collapses: Vec<SourceKeyCollapse>,
    conflicts: Vec<ReportedConflict>,
    gaps: Vec<Gap>,
}

/// Partitions the input, reconciles exact duplicates, and extracts per-Note
/// facts.
///
/// Reconciliation is exact only: identical Note IDs with identical semantic
/// payloads are one Note, and one portable source key with identical payloads is
/// one upstream object. Divergence is a preserved conflict rather than a choice,
/// and an equal `content_hash` collapses nothing, because it answers no identity
/// question.
fn reconcile_input(
    records: &[ParsedRecord],
    config: &GraphConfig,
) -> Result<ReconciledInput, GraphError> {
    let mut input = ReconciledInput::default();

    // Only Notes are evidence; a prior projection contributes nothing but its ID.
    let mut note_inputs: Vec<&ParsedRecord> = Vec::new();
    for record in records {
        match record.kind() {
            RecordKind::Note => note_inputs.push(record),
            RecordKind::Entity => input.priors.push(prior_entity(record, config)),
            RecordKind::Relationship => input.prior_edges.push(prior_relationship(record)),
            kind => input.gaps.push(Gap {
                kind: GapKind::NonNoteInput,
                value: None,
                property: None,
                records: vec![*record.id()],
                detail: format!(
                    "a {} record contributed no evidence: v0.1 derivation reads Notes and rebuilds \
                     every other projection from them",
                    kind_label(kind)
                ),
            }),
        }
    }
    input.priors.sort_by_key(|prior| prior.id);
    input.prior_edges.sort_by_key(|prior| prior.id);

    let survivors = reconcile_note_ids(note_inputs, &mut input)?;
    let current = reconcile_source_keys(survivors, &mut input);

    for record in current {
        let Some(facts) = note_facts(record, config, &mut input.gaps) else {
            continue;
        };
        if facts.participant_keys().is_empty() && facts.artifacts.is_empty() {
            input.gaps.push(Gap {
                kind: GapKind::NoIdentityAnchors,
                value: None,
                property: None,
                records: vec![facts.id],
                detail:
                    "the Note supplied no identity anchor the declared namespaces cover, so it \
                     supports no entity"
                        .to_owned(),
            });
        }
        input.notes.insert(facts.id, facts);
    }
    Ok(input)
}

/// Collapses identical Note IDs and reports divergent ones, returning each
/// surviving Note with its semantic fingerprint.
fn reconcile_note_ids<'a>(
    note_inputs: Vec<&'a ParsedRecord>,
    input: &mut ReconciledInput,
) -> Result<Vec<(String, &'a ParsedRecord)>, GraphError> {
    let mut by_note_id: BTreeMap<RecordId, Vec<(String, String, &ParsedRecord)>> = BTreeMap::new();
    for record in note_inputs {
        let fingerprint = record_fingerprint(&semantic_record_string(record)?);
        let canonical = canonical_record_string(record)?;
        by_note_id
            .entry(*record.id())
            .or_default()
            .push((fingerprint, canonical, record));
    }
    let mut survivors: Vec<(String, &ParsedRecord)> = Vec::new();
    for (note_id, mut copies) in by_note_id {
        copies.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        let fingerprints: BTreeSet<&str> = copies
            .iter()
            .map(|(fingerprint, _, _)| fingerprint.as_str())
            .collect();
        if fingerprints.len() > 1 {
            input.conflicts.push(ReportedConflict {
                kind: ConflictKind::SameNoteIdDivergence,
                notes: vec![note_id],
                entities: Vec::new(),
                fingerprints: fingerprints
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect(),
                values: Vec::new(),
                detail: format!(
                    "{} copies of {note_id} carry divergent semantic content",
                    copies.len()
                ),
            });
            input.gaps.push(Gap {
                kind: GapKind::ExcludedByConflict,
                value: None,
                property: None,
                records: vec![note_id],
                detail: "the Note is excluded from derivation while its same-ID divergence is \
                         unresolved, because using either copy would silently declare it current"
                    .to_owned(),
            });
            continue;
        }
        if let Some((fingerprint, _, record)) = copies.into_iter().next() {
            survivors.push((fingerprint, record));
        }
    }
    Ok(survivors)
}

/// Collapses independently collected copies of one upstream object and reports
/// divergent current state under one portable source key.
fn reconcile_source_keys<'a>(
    survivors: Vec<(String, &'a ParsedRecord)>,
    input: &mut ReconciledInput,
) -> Vec<&'a ParsedRecord> {
    let mut by_source_key: BTreeMap<(String, String), Vec<(String, &ParsedRecord)>> =
        BTreeMap::new();
    let mut current: Vec<&ParsedRecord> = Vec::new();
    for (fingerprint, record) in survivors {
        match (
            text(record, "source_scope"),
            text(record, "source_identity"),
        ) {
            (Some(scope), Some(identity)) => by_source_key
                .entry((scope.to_owned(), identity.to_owned()))
                .or_default()
                .push((fingerprint, record)),
            _ => current.push(record),
        }
    }
    for ((scope, identity), group) in by_source_key {
        let fingerprints: BTreeSet<&str> = group
            .iter()
            .map(|(fingerprint, _)| fingerprint.as_str())
            .collect();
        let note_ids: BTreeSet<RecordId> = group.iter().map(|(_, record)| *record.id()).collect();
        if fingerprints.len() > 1 {
            input.conflicts.push(ReportedConflict {
                kind: ConflictKind::SourceKeyDivergence,
                notes: note_ids.iter().copied().collect(),
                entities: Vec::new(),
                fingerprints: fingerprints
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect(),
                values: vec![scope.clone(), identity.clone()],
                detail: format!(
                    "portable source key ({scope}, {identity}) arrived with divergent current \
                     state and no reliable ordering, so no copy is current"
                ),
            });
            for note in &note_ids {
                input.gaps.push(Gap {
                    kind: GapKind::ExcludedByConflict,
                    value: Some(format!("{scope}/{identity}")),
                    property: Some("source_identity".to_owned()),
                    records: vec![*note],
                    detail: "the Note is excluded while its portable-source-key divergence is \
                             unresolved"
                        .to_owned(),
                });
            }
            continue;
        }
        // The survivor is the lowest Note ID in the group; the rest are exact
        // copies of the same current state.
        let Some(survivor) = note_ids.iter().copied().next() else {
            continue;
        };
        if note_ids.len() > 1 {
            let mut producers: BTreeSet<String> = BTreeSet::new();
            for (_, record) in &group {
                for producer in text_list(record, "collected_by") {
                    producers.insert(producer.to_owned());
                }
                if let (Some(instance), Some(field)) =
                    (text(record, "instance_id"), text(record, "field_id"))
                {
                    producers.insert(format!("{instance}/{field}"));
                }
            }
            input.collapses.push(SourceKeyCollapse {
                source_scope: scope.clone(),
                source_identity: identity.clone(),
                survivor,
                notes: note_ids.iter().copied().collect(),
                producers: producers.into_iter().collect(),
                fingerprint: group
                    .first()
                    .map(|(fingerprint, _)| fingerprint.clone())
                    .unwrap_or_default(),
            });
        }
        if let Some(record) = group
            .iter()
            .find(|(_, record)| *record.id() == survivor)
            .map(|(_, record)| *record)
        {
            current.push(record);
        }
    }
    current
}

/// The result of identity resolution: which anchors belong together, why, and
/// which Notes supplied each anchor.
#[derive(Debug, Default)]
struct Resolution {
    /// Components keyed by their lowest anchor, which is the entity's primary
    /// key. `BTreeMap` order is therefore the entity output order.
    components: BTreeMap<IdentityKey, BTreeSet<IdentityKey>>,
    joins: Vec<IdentityJoin>,
    key_notes: BTreeMap<IdentityKey, BTreeSet<RecordId>>,
    gaps: Vec<Gap>,
}

/// Groups person and organization anchors into entities-to-be.
///
/// The only join rule v0.1 has is the one a source states outright: the anchors
/// printed on one contact record describe one subject. Everything else — equal
/// display names, coincident activity, similar subjects — is refused here and
/// surfaces as a candidate instead.
fn resolve_identities(notes: &BTreeMap<RecordId, NoteFacts>, config: &GraphConfig) -> Resolution {
    let mut resolution = Resolution::default();
    for facts in notes.values() {
        for (key, _) in &facts.anchors {
            resolution
                .key_notes
                .entry(key.clone())
                .or_default()
                .insert(facts.id);
        }
    }

    let keys: Vec<IdentityKey> = notes
        .values()
        .flat_map(NoteFacts::participant_keys)
        .collect::<BTreeSet<IdentityKey>>()
        .into_iter()
        .collect();
    let index_of: BTreeMap<&IdentityKey, usize> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect();
    let mut components = Components::new(keys.len());
    for facts in notes.values() {
        if !facts.co_identity {
            continue;
        }
        // A source contact record states that these anchors belong to one
        // subject. That is explicit source evidence of co-identity — unlike a
        // mail Note, whose `identities` list names several different people.
        let mut by_kind: BTreeMap<EntityKind, Vec<IdentityKey>> = BTreeMap::new();
        for key in facts.participant_keys() {
            if let Some(policy) = config.namespaces.policy(key.namespace()) {
                by_kind.entry(policy.entity_kind).or_default().push(key);
            }
        }
        if by_kind.len() > 1 {
            resolution.gaps.push(Gap {
                kind: GapKind::MixedClassContactRecord,
                value: None,
                property: Some("identities".to_owned()),
                records: vec![facts.id],
                detail: "the contact record carries anchors of more than one entity class, so the \
                         co-identity rule joined only anchors within each class"
                    .to_owned(),
            });
        }
        for group in by_kind.values() {
            for pair in group.windows(2) {
                let (Some(left), Some(right)) = (pair.first(), pair.last()) else {
                    continue;
                };
                let (Some(left_index), Some(right_index)) =
                    (index_of.get(left), index_of.get(right))
                else {
                    continue;
                };
                components.union(*left_index, *right_index);
                resolution.joins.push(IdentityJoin {
                    rule: RuleId::entity(CONTACT_RECORD_RULE),
                    left: left.clone(),
                    right: right.clone(),
                    evidence: facts.id,
                });
            }
        }
    }
    resolution.joins.sort();
    resolution.joins.dedup();

    for (index, key) in keys.iter().enumerate() {
        let root = components.find(index);
        let Some(root_key) = keys.get(root) else {
            continue;
        };
        resolution
            .components
            .entry(root_key.clone())
            .or_default()
            .insert(key.clone());
    }
    resolution
}

/// The entities and the anchor-to-entity index the later stages need.
#[derive(Debug, Default)]
struct EntityStage {
    entities: Vec<Entity>,
    entity_of_key: BTreeMap<IdentityKey, RecordId>,
    conflicts: Vec<ReportedConflict>,
    gaps: Vec<Gap>,
}

/// Builds one entity per resolved component, in primary-anchor order.
fn build_entities(
    input: &ReconciledInput,
    resolution: &Resolution,
    config: &GraphConfig,
    ids: &mut dyn ProjectionIds,
) -> Result<EntityStage, GraphError> {
    let mut stage = EntityStage::default();

    // How many current entities each prior projection maps onto: a prior that
    // maps onto several has been split, and reusing its ID would be arbitrary.
    let mut prior_matches: BTreeMap<RecordId, usize> = BTreeMap::new();
    for component in resolution.components.values() {
        for prior in &input.priors {
            if prior.keys.iter().any(|key| component.contains(key)) {
                *prior_matches.entry(prior.id).or_default() += 1;
            }
        }
    }

    for (primary, component) in &resolution.components {
        let Some(policy) = config.namespaces.policy(primary.namespace()) else {
            continue;
        };
        let kind = policy.entity_kind;
        let mut span = Span::default();
        for facts in input.notes.values() {
            if facts.touches(component) {
                span.add(facts);
            }
        }
        let (title, competing) = component_name(&input.notes, component);

        let identities: Vec<IdentityKey> = component.iter().cloned().collect();
        let resolved: Vec<ResolvedIdentity> = identities
            .iter()
            .filter_map(|key| {
                let policy = config.namespaces.policy(key.namespace())?;
                Some(ResolvedIdentity {
                    key: key.clone(),
                    scope_class: policy.scope_class,
                    strength: policy.strength,
                    normalization: policy.normalization,
                    evidence: resolution
                        .key_notes
                        .get(key)
                        .map(|notes| notes.iter().copied().collect())
                        .unwrap_or_default(),
                })
            })
            .collect();
        let joins: Vec<IdentityJoin> = resolution
            .joins
            .iter()
            .filter(|join| component.contains(&join.left) && component.contains(&join.right))
            .cloned()
            .collect();
        let occurrences = resolution.key_notes.get(primary).map_or(0, BTreeSet::len);
        let (origin, rule, claim) = if identities.len() > 1 {
            (
                Origin::Matched,
                RuleId::entity(CONTACT_RECORD_RULE),
                format!(
                    "Current deterministic {kind} projection keyed by {primary}, joining {} \
                     normalized identity anchors",
                    identities.len()
                ),
            )
        } else if occurrences > 1 {
            (
                Origin::Matched,
                RuleId::entity(exact_rule(primary.namespace())),
                format!(
                    "Current deterministic {kind} projection for {primary}, matched by the \
                     recurrence of that {} anchor across {occurrences} current Notes",
                    policy.strength
                ),
            )
        } else {
            (
                Origin::Explicit,
                RuleId::entity(anchor_rule(primary.namespace())),
                format!(
                    "Current deterministic {kind} projection for {primary}, resting on that one \
                     {} anchor exactly as the source supplied it",
                    policy.strength
                ),
            )
        };

        let id = match prior_reuse(&input.priors, &prior_matches, component, kind) {
            PriorReuse::Reuse(id) => id,
            PriorReuse::Ambiguous(prior_ids) => {
                stage.conflicts.push(ReportedConflict {
                    kind: ConflictKind::AmbiguousProjectionRebind,
                    notes: span.notes.iter().copied().collect(),
                    entities: prior_ids,
                    fingerprints: Vec::new(),
                    values: identities.iter().map(IdentityKey::anchor_text).collect(),
                    detail: format!(
                        "prior entity projections do not map one-to-one onto the entity keyed by \
                         {primary}, so no prior projection ID is reused ({ID_REUSE_RULE})"
                    ),
                });
                ids.next_id(RecordKind::Entity)?
            }
            PriorReuse::None => ids.next_id(RecordKind::Entity)?,
        };

        if kind.is_materialized() && !identities.iter().any(IdentityKey::is_publishable) {
            stage.gaps.push(Gap {
                kind: GapKind::UnpublishableIdentities,
                value: Some(primary.to_string()),
                property: Some("identities".to_owned()),
                records: vec![id],
                detail: "every anchor of this entity is scope-qualified, and A1 froze no public \
                         flat spelling that carries a scope, so the record carries no identities \
                         list"
                    .to_owned(),
            });
        }
        for key in &identities {
            stage.entity_of_key.insert(key.clone(), id);
        }
        for contradiction in &competing {
            stage.conflicts.push(ReportedConflict {
                kind: ConflictKind::ContradictoryName,
                notes: contradiction.evidence.clone(),
                entities: vec![id],
                fingerprints: Vec::new(),
                values: Vec::new(),
                detail: format!("{} (no name is projected)", contradiction.claim),
            });
        }
        stage.entities.push(Entity {
            id,
            kind,
            identities,
            title,
            channels: span.channels.iter().cloned().collect(),
            evidence: bounded(&span.notes, config.evidence_limit),
            interaction_count: span.notes.len(),
            first_seen: span.first_seen(),
            last_seen: span.last_seen(),
            explanation: Explanation {
                subject: id,
                claim,
                origin,
                rule,
                identities: resolved,
                joins,
                evidence: bounded(&span.notes, config.evidence_limit),
                evidence_count: span.notes.len(),
                first_seen: span.first_seen(),
                last_seen: span.last_seen(),
                competing,
            },
        });
    }
    Ok(stage)
}

/// The name current evidence supplies for one component, and any contradiction.
///
/// A source contact record is the only current evidence v0.1 reads as a person's
/// own name: a `title` on a mail, event, or file Note names the object, not a
/// person. Contradictory names project no name at all, because choosing one
/// would be last-writer-wins.
fn component_name(
    notes: &BTreeMap<RecordId, NoteFacts>,
    component: &BTreeSet<IdentityKey>,
) -> (Option<String>, Vec<CompetingEvidence>) {
    let mut names: BTreeMap<String, (BTreeSet<String>, BTreeSet<RecordId>)> = BTreeMap::new();
    for facts in notes.values() {
        let (true, Some(raw)) = (facts.co_identity, facts.display_name.as_deref()) else {
            continue;
        };
        if !facts.touches(component) {
            continue;
        }
        if let Some(normalized) = normalized_display_name(raw) {
            let entry = names.entry(normalized).or_default();
            entry.0.insert(raw.to_owned());
            entry.1.insert(facts.id);
        }
    }
    match names.len() {
        1 => (
            names
                .values()
                .next()
                .and_then(|(spellings, _)| spellings.iter().next().cloned()),
            Vec::new(),
        ),
        0 => (None, Vec::new()),
        _ => {
            let mut cited: BTreeSet<RecordId> = BTreeSet::new();
            let mut values: Vec<String> = Vec::new();
            for (spellings, notes_for_name) in names.values() {
                cited.extend(notes_for_name.iter().copied());
                values.extend(spellings.iter().cloned());
            }
            (
                None,
                vec![CompetingEvidence {
                    claim: format!(
                        "current contact evidence supplies contradictory names: {}",
                        values.join(", ")
                    ),
                    evidence: cited.into_iter().collect(),
                }],
            )
        }
    }
}

/// Builds one `person_person` relationship per pair of person entities the
/// source recorded in the same current object.
fn build_relationships(
    stage: &EntityStage,
    input: &ReconciledInput,
    resolution: &Resolution,
    config: &GraphConfig,
    ids: &mut dyn ProjectionIds,
) -> Result<Vec<Relationship>, GraphError> {
    let mut pairs: BTreeMap<(IdentityKey, IdentityKey), PairEvidence> = BTreeMap::new();
    for facts in input.notes.values() {
        if facts.co_identity {
            // A contact record describes one subject; it records no interaction.
            continue;
        }
        let mut sides: BTreeMap<RecordId, BTreeSet<IdentityKey>> = BTreeMap::new();
        for key in facts.participant_keys() {
            let Some(entity_id) = stage.entity_of_key.get(&key) else {
                continue;
            };
            sides.entry(*entity_id).or_default().insert(key);
        }
        let sides: Vec<(RecordId, BTreeSet<IdentityKey>)> = sides.into_iter().collect();
        for (index, (left_id, left_keys)) in sides.iter().enumerate() {
            for (right_id, right_keys) in sides.iter().skip(index + 1) {
                let (Some(left), Some(right)) = (
                    stage.entities.iter().find(|entity| &entity.id == left_id),
                    stage.entities.iter().find(|entity| &entity.id == right_id),
                ) else {
                    continue;
                };
                if left.kind != EntityKind::Person || right.kind != EntityKind::Person {
                    // A1 reserves only `person_person`. Any other pairing needs a
                    // registry-reviewed relationship type before it can exist.
                    continue;
                }
                let (Some(left_primary), Some(right_primary)) =
                    (left.primary_identity(), right.primary_identity())
                else {
                    continue;
                };
                // The canonical orientation is by primary anchor, so the same
                // evidence always yields the same record.
                let (low, high) = if left_primary <= right_primary {
                    (
                        (left_primary.clone(), left.id),
                        (right_primary.clone(), right.id),
                    )
                } else {
                    (
                        (right_primary.clone(), right.id),
                        (left_primary.clone(), left.id),
                    )
                };
                let entry = pairs
                    .entry((low.0, high.0))
                    .or_insert_with(|| PairEvidence {
                        from: low.1,
                        to: high.1,
                        span: Span::default(),
                        keys: BTreeSet::new(),
                    });
                entry.span.add(facts);
                entry.keys.extend(left_keys.iter().cloned());
                entry.keys.extend(right_keys.iter().cloned());
            }
        }
    }

    let mut edge_reuse: BTreeMap<(RecordId, RecordId, String), Vec<RecordId>> = BTreeMap::new();
    for prior in &input.prior_edges {
        if let (Some(from), Some(to)) = (prior.from, prior.to) {
            edge_reuse
                .entry((from, to, prior.kind.clone()))
                .or_default()
                .push(prior.id);
        }
    }

    let mut relationships: Vec<Relationship> = Vec::new();
    for ((low_key, high_key), evidence) in pairs {
        // A prior relationship record between the same two current entities is
        // the same edge, so a rebuild that changed nothing rewrites nothing.
        let reuse_key = (
            evidence.from,
            evidence.to,
            RelationshipKind::PersonPerson.as_str().to_owned(),
        );
        let id = match edge_reuse.get(&reuse_key).map(Vec::as_slice) {
            Some([single]) => *single,
            _ => ids.next_id(RecordKind::Relationship)?,
        };
        let resolved: Vec<ResolvedIdentity> = evidence
            .keys
            .iter()
            .filter_map(|key| {
                let policy = config.namespaces.policy(key.namespace())?;
                Some(ResolvedIdentity {
                    key: key.clone(),
                    scope_class: policy.scope_class,
                    strength: policy.strength,
                    normalization: policy.normalization,
                    evidence: resolution
                        .key_notes
                        .get(key)
                        .map(|notes| {
                            notes
                                .iter()
                                .copied()
                                .filter(|note| evidence.span.notes.contains(note))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect();
        relationships.push(Relationship {
            id,
            kind: RelationshipKind::PersonPerson,
            from_entity_id: evidence.from,
            to_entity_id: evidence.to,
            channels: evidence.span.channels.iter().cloned().collect(),
            evidence: bounded(&evidence.span.notes, config.evidence_limit),
            interaction_count: evidence.span.notes.len(),
            first_seen: evidence.span.first_seen(),
            last_seen: evidence.span.last_seen(),
            explanation: Explanation {
                subject: id,
                claim: format!(
                    "{low_key} and {high_key} appear together in {} cited current Notes",
                    evidence.span.notes.len()
                ),
                origin: Origin::Explicit,
                rule: RuleId::relationship(CO_PARTICIPANT_RULE),
                identities: resolved,
                joins: Vec::new(),
                evidence: bounded(&evidence.span.notes, config.evidence_limit),
                evidence_count: evidence.span.notes.len(),
                first_seen: evidence.span.first_seen(),
                last_seen: evidence.span.last_seen(),
                competing: Vec::new(),
            },
        });
    }
    Ok(relationships)
}

/// Groups Notes into threads by their scoped source thread and conversation
/// keys, and derives each thread's participants.
fn build_threads(
    notes: &BTreeMap<RecordId, NoteFacts>,
    entity_of_key: &BTreeMap<IdentityKey, RecordId>,
) -> Vec<Thread> {
    let mut thread_spans: BTreeMap<ThreadKey, (Span, BTreeSet<IdentityKey>)> = BTreeMap::new();
    for facts in notes.values() {
        for key in &facts.thread_keys {
            let entry = thread_spans.entry(key.clone()).or_default();
            entry.0.add(facts);
            entry.1.extend(facts.participant_keys());
        }
    }
    thread_spans
        .into_iter()
        .map(|(key, (span, participants))| {
            let entities: BTreeSet<RecordId> = participants
                .iter()
                .filter_map(|key| entity_of_key.get(key).copied())
                .collect();
            Thread {
                key,
                notes: span.notes.iter().copied().collect(),
                participants: participants.into_iter().collect(),
                entities: entities.into_iter().collect(),
                first_seen: span.first_seen(),
                last_seen: span.last_seen(),
            }
        })
        .collect()
}

/// Collects every referenced original artifact, which makes exact duplicate
/// references across Notes visible.
fn build_artifacts(notes: &BTreeMap<RecordId, NoteFacts>) -> Vec<ArtifactReference> {
    let mut artifact_spans: BTreeMap<ArtifactId, (Span, BTreeSet<RecordId>)> = BTreeMap::new();
    for facts in notes.values() {
        for artifact in &facts.artifacts {
            let entry = artifact_spans.entry(*artifact).or_default();
            entry.0.add(facts);
            if facts.attachments.contains(artifact) {
                entry.1.insert(facts.id);
            }
        }
    }
    artifact_spans
        .into_iter()
        .map(|(artifact, (span, attachments))| ArtifactReference {
            artifact,
            notes: span.notes.iter().copied().collect(),
            attachment_notes: attachments.into_iter().collect(),
            first_seen: span.first_seen(),
            last_seen: span.last_seen(),
        })
        .collect()
}

fn kind_label(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Instance => "instance",
        RecordKind::Note => "Note",
        RecordKind::Extraction => "Extraction",
        RecordKind::Observation => "Observation",
        RecordKind::Entity => "entity",
        RecordKind::Relationship => "relationship",
        RecordKind::Proposal => "proposal",
        RecordKind::Package => "package",
        RecordKind::Conflict => "conflict",
    }
}

/// Reads a prior entity projection's anchors so its ID can stay stable.
fn prior_entity(record: &ParsedRecord, config: &GraphConfig) -> PriorEntity {
    let kind = match text(record, "type") {
        Some("person") => Some(EntityKind::Person),
        Some("organization") => Some(EntityKind::Organization),
        Some("artifact") => Some(EntityKind::Artifact),
        _ => None,
    };
    let keys = text_list(record, "identities")
        .into_iter()
        .filter_map(|raw| parse_anchor(raw, &config.namespaces, None).ok())
        .collect();
    PriorEntity {
        id: *record.id(),
        kind,
        keys,
    }
}

/// Reads a prior relationship projection's entity pair so its ID can stay
/// stable when the same two entities still record the same connection.
fn prior_relationship(record: &ParsedRecord) -> PriorRelationship {
    let entity_ref = |key: &str| {
        text(record, key)
            .and_then(|value| RecordId::parse(value).ok())
            .filter(|id| id.kind() == RecordKind::Entity)
    };
    PriorRelationship {
        id: *record.id(),
        kind: text(record, "type").unwrap_or_default().to_owned(),
        from: entity_ref("from_entity_id"),
        to: entity_ref("to_entity_id"),
    }
}

/// The outcome of looking for a reusable prior projection ID.
enum PriorReuse {
    /// Exactly one prior projection maps onto this entity.
    Reuse(RecordId),
    /// Prior projections do not map one-to-one, so nothing is reused.
    Ambiguous(Vec<RecordId>),
    /// No prior projection shares an anchor with this entity.
    None,
}

fn prior_reuse(
    priors: &[PriorEntity],
    prior_matches: &BTreeMap<RecordId, usize>,
    component: &BTreeSet<IdentityKey>,
    kind: EntityKind,
) -> PriorReuse {
    let matching: Vec<&PriorEntity> = priors
        .iter()
        .filter(|prior| {
            prior.kind == Some(kind) && prior.keys.iter().any(|key| component.contains(key))
        })
        .collect();
    match matching.as_slice() {
        [] => PriorReuse::None,
        [single] => {
            // A prior projection whose anchors now spread across several entities
            // has been split; reusing its ID for one of them would be arbitrary.
            if prior_matches.get(&single.id).copied().unwrap_or_default() > 1 {
                PriorReuse::Ambiguous(vec![single.id])
            } else {
                PriorReuse::Reuse(single.id)
            }
        }
        several => PriorReuse::Ambiguous(several.iter().map(|prior| prior.id).collect()),
    }
}

/// Extracts anchors, artifacts, thread keys, and the contact-record name from
/// one Note. Returns `None` for a record that is not a usable Note.
fn note_facts(
    record: &ParsedRecord,
    config: &GraphConfig,
    gaps: &mut Vec<Gap>,
) -> Option<NoteFacts> {
    let id = *record.id();
    let Some(occurred_at) = record.occurred_at().copied() else {
        gaps.push(Gap {
            kind: GapKind::MalformedNote,
            value: None,
            property: Some("occurred_at".to_owned()),
            records: vec![id],
            detail: "the Note carries no typed occurred_at, so it can support no first-seen or \
                     last-seen claim"
                .to_owned(),
        });
        return None;
    };
    let field_id = text(record, "field_id").unwrap_or_default();
    let channel = config
        .field_stems
        .property_prefix_for(field_id)
        .unwrap_or(field_id)
        .to_owned();
    let source_scope = text(record, "source_scope");
    let co_identity = text(record, "type") == Some("contact");

    let mut anchors: BTreeSet<(IdentityKey, AnchorSource)> = BTreeSet::new();
    for raw in text_list(record, "identities") {
        match parse_anchor(raw, &config.namespaces, source_scope) {
            Ok(key) => {
                anchors.insert((key, AnchorSource::Identities));
            }
            Err(refusal) => gaps.push(Gap {
                kind: GapKind::UnresolvedIdentityAnchor,
                value: Some(refusal.raw.clone()),
                property: Some(AnchorSource::Identities.property().to_owned()),
                records: vec![id],
                detail: refusal.reason.to_string(),
            }),
        }
    }
    for (source, is_list) in ROLE_PROPERTIES {
        let values: Vec<&str> = if is_list {
            text_list(record, source.property())
        } else {
            text(record, source.property()).into_iter().collect()
        };
        for raw in values {
            match normalize_channel_value(raw, &config.namespaces) {
                Ok(key) => {
                    anchors.insert((key, source));
                }
                Err(refusal) => gaps.push(Gap {
                    kind: GapKind::UnresolvedRoleValue,
                    value: Some(refusal.raw.clone()),
                    property: Some(source.property().to_owned()),
                    records: vec![id],
                    detail: format!("{} ({PARTICIPANT_RULE})", refusal.reason),
                }),
            }
        }
    }

    let mut artifacts: BTreeSet<ArtifactId> = BTreeSet::new();
    let mut attachments: BTreeSet<ArtifactId> = BTreeSet::new();
    for (property, is_attachment) in [("artifacts", false), ("attachments", true)] {
        for raw in text_list(record, property) {
            match ArtifactId::parse(raw) {
                Ok(artifact) => {
                    artifacts.insert(artifact);
                    if is_attachment {
                        attachments.insert(artifact);
                    }
                    if let Some(digest) = raw.strip_prefix(ArtifactId::PREFIX) {
                        anchors.insert((
                            IdentityKey::new(ARTIFACT_NAMESPACE, None, digest.to_ascii_lowercase()),
                            AnchorSource::Artifacts,
                        ));
                    }
                }
                Err(_) => gaps.push(Gap {
                    kind: GapKind::MalformedArtifactReference,
                    value: Some(raw.to_owned()),
                    property: Some(property.to_owned()),
                    records: vec![id],
                    detail: "the value is not a content-addressed artifact ID".to_owned(),
                }),
            }
        }
    }

    let mut thread_keys: BTreeSet<ThreadKey> = BTreeSet::new();
    for (kind, property) in [
        (ThreadKeyKind::Thread, "thread_id"),
        (ThreadKeyKind::Conversation, "conversation_id"),
    ] {
        let Some(value) = text(record, property) else {
            continue;
        };
        match source_scope {
            Some(scope) => {
                thread_keys.insert(ThreadKey {
                    kind,
                    scope: scope.to_owned(),
                    value: value.to_owned(),
                });
            }
            None => gaps.push(Gap {
                kind: GapKind::UnscopableThreadKey,
                value: Some(value.to_owned()),
                property: Some(property.to_owned()),
                records: vec![id],
                detail: "a thread identity is source-local, and this Note declares no \
                         source_scope, so it is never joined with another Note's thread"
                    .to_owned(),
            }),
        }
    }

    let display_name = if co_identity {
        text(record, "title").map(str::to_owned)
    } else {
        None
    };

    Some(NoteFacts {
        id,
        channel,
        occurred_at,
        anchors,
        artifacts,
        attachments,
        thread_keys,
        co_identity,
        display_name,
    })
}

/// Candidates raised because two entities share a normalized display name.
fn name_candidates(entities: &[Entity]) -> Vec<MergeCandidate> {
    let mut by_name: BTreeMap<String, Vec<&Entity>> = BTreeMap::new();
    for entity in entities {
        let Some(normalized) = entity.title.as_deref().and_then(normalized_display_name) else {
            continue;
        };
        by_name.entry(normalized).or_default().push(entity);
    }
    by_name
        .into_iter()
        .filter(|(_, group)| group.len() > 1)
        .map(|(name, group)| {
            let mut entity_ids: Vec<RecordId> = group.iter().map(|entity| entity.id).collect();
            entity_ids.sort();
            let mut identities: Vec<IdentityKey> = group
                .iter()
                .flat_map(|entity| entity.identities.iter().cloned())
                .collect();
            identities.sort();
            identities.dedup();
            let mut evidence: Vec<RecordId> = group
                .iter()
                .flat_map(|entity| entity.evidence.iter().copied())
                .collect();
            evidence.sort();
            evidence.dedup();
            MergeCandidate {
                reason: CandidateReason::DisplayNameEquality,
                value: name,
                entities: entity_ids,
                identities,
                evidence,
                detail: "display-name equality is weak descriptive evidence, so these entities \
                         stay separate until an exact anchor, an approved rule, or a durable user \
                         decision joins them"
                    .to_owned(),
            }
        })
        .collect()
}

/// Candidates raised because an unresolvable role value equals a known name.
fn unresolved_value_candidates(
    notes: &BTreeMap<RecordId, NoteFacts>,
    entities: &[Entity],
    gaps: &[Gap],
) -> Vec<MergeCandidate> {
    let mut by_name: BTreeMap<String, Vec<&Entity>> = BTreeMap::new();
    for entity in entities {
        if let Some(normalized) = entity.title.as_deref().and_then(normalized_display_name) {
            by_name.entry(normalized).or_default().push(entity);
        }
    }
    let mut candidates: BTreeMap<(String, RecordId), MergeCandidate> = BTreeMap::new();
    for gap in gaps {
        if gap.kind != GapKind::UnresolvedRoleValue {
            continue;
        }
        let Some(raw) = gap.value.as_deref() else {
            continue;
        };
        let Some(normalized) = normalized_display_name(raw) else {
            continue;
        };
        let Some(group) = by_name.get(&normalized) else {
            continue;
        };
        for entity in group {
            let evidence: Vec<RecordId> = gap
                .records
                .iter()
                .copied()
                .filter(|note| notes.contains_key(note))
                .collect();
            candidates.insert(
                (normalized.clone(), entity.id),
                MergeCandidate {
                    reason: CandidateReason::UnresolvedValueMatchesName,
                    value: normalized.clone(),
                    entities: vec![entity.id],
                    identities: entity.identities.clone(),
                    evidence,
                    detail: format!(
                        "the role value {raw:?} normalizes into no declared identity namespace and \
                         matches this entity only by display name, which is never a merge"
                    ),
                },
            );
        }
    }
    candidates.into_values().collect()
}
