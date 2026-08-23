# Identity and deterministic graph

**Status:** Proposed v0.1 contract  
**Scope:** Identity anchors, portable source-object deduplication, entity resolution, relationships, conflicts, and explainability

## Product boundary

The graph is a disposable current-state projection over Notes, retained artifacts, non-secret configuration, and any optional derived evidence that is currently present. Notes remain the source evidence. Entity and relationship files are useful public Markdown records while present, but they are not a contact master, a source-of-truth graph, or an append-only record of prior conclusions.

Deleting graph caches, `entities/`, and `relationships/` must not remove source evidence. Rebuilding them from the same current inputs and rule versions must produce the same semantic graph. A source update may change or remove projected facts; v0.1 does not retain superseded entity or relationship versions as a history ledger.

Human identity aliases, explicit merge/split decisions, and other non-reconstructable intent are not disposable projections. They belong in durable private state under `.fieldnotes/`, remain separate from credentials, and are reapplied on rebuild.

## Four different identity questions

Fieldnotes keeps these concepts separate:

1. **Note identity:** a globally unique `note_...` ID identifies one Fieldnotes Note.
2. **Producer provenance:** `(instance_id, field_id)` identifies the Fieldnotes instance and configured Field that collected it.
3. **Portable source-object identity:** `(source_scope, source_identity)` identifies one upstream object across Fieldnotes instances.
4. **Real-world identity anchors:** namespaced values such as email addresses, source user IDs, phone numbers, logins, or domains help determine whether appearances refer to the same person, organization, or artifact.

A content hash answers none of those identity questions by itself. It proves equality only in its declared hash domain. The same attachment or message text may occur in several different contexts.

## Identity namespace and scope

An identity anchor is conceptually a pair of namespace and normalized value:

```text
email:alice@example.com
phone:+41441234567
domain:example.com
github-user-id:123456
github-login:alice-example
twenty-person-id:83472
```

The serialized form is flat text, but its namespace is mandatory. Fieldnotes never joins two source-local IDs merely because their unqualified string values match.

Every identity namespace has a declared matching scope. Relevant scope classes include:

- **source-global:** an upstream system documents the value as unique across that system;
- **authority or tenant scoped:** the value is exact only within a declared tenant, site, server, account, or other authority;
- **namespace scoped:** a handle or account name is meaningful only within a named service or configured namespace;
- **normalized channel identity:** an email address or phone number is matched according to a versioned deterministic normalization rule;
- **weak descriptive value:** a display name, organization label, role text, or other unverified human description.

Connectors declare the namespace, scope, normalization rule, and evidence origin of the anchors they emit. Scope is connector metadata or durable configuration, not an unqualified convention hidden in graph code. A source value that cannot be scoped safely remains unresolved rather than being treated as exact.

A1 enumerates no frozen registry of namespaces and normalization vectors, and the corpus demonstrates only `email:` and `phone:` anchors with no fixture pinning their normalization vector, scope class, or strength as approved bytes. That gap does not block v0.1 compatibility: Fieldnotes performs no formal identity mapping, and anchors are evidence for downstream observation and proposal work rather than keys a resolver uses to decide merges. Divergent normalization between implementations is therefore a quality and interoperability concern, not a correctness gap — it would only become one if a future feature started making merge decisions from normalized anchor text, which would then need a frozen registry before it could ship. See [ADR 0012](decisions/0012-graph-implementation-rulings.md) and [A1 graph implementation findings, finding 2](approvals/A1-graph-implementation-findings.md#finding-2-the-identity-namespace-registry-a1-was-said-to-freeze-does-not-exist). This document defines the semantics, not an exhaustive public schema.

## Identity strength

Strength determines whether deterministic rules may join identities automatically or must leave a review candidate.

| Evidence | Typical strength | Matching boundary |
|---|---|---|
| documented source object/user/account ID | exact | only inside its declared global, authority, tenant, or account scope |
| verified or source-authoritative email address | strong | after approved email normalization; conflicting source evidence remains visible |
| normalized phone number with sufficient country context | strong | after approved phone normalization |
| configured alias or user-approved identity override | exact for this notebook | durable user intent; retains the rule that caused the join |
| stable service username or login | medium | service/namespace scoped; automatic merge requires an additional approved rule or evidence |
| source-provided organization affiliation | explicit relationship evidence | preserves source scope and time; does not prove all identities with that organization name are one organization |
| display name, role, organization label, subject, or nearby timestamp | weak | candidate generation only; never an automatic union by itself |

“Strong” does not mean infallible or globally unique in all circumstances. The resolver records the exact rule and evidence used and preserves contradictions. Connectors must not upgrade an inferred or free-text value to source-authoritative merely because it resembles a well-formed email address or ID.

## Portable source-object deduplication

External Notes carry both:

```text
portable source-object key = (source_scope, source_identity)
producer provenance key    = (instance_id, field_id)
```

`source_scope` is connector-namespaced, stable across Fieldnotes instances, non-secret, and based on the upstream authority or account rather than the user's local Field label. `source_identity` is stable inside that scope and includes an object-kind namespace when the upstream service can reuse ID values across kinds.

This enables exact, portable reconciliation:

- matching portable source key and matching semantic current content collapses to one current Note;
- all known producer references are retained through the approved flat provenance representation;
- a reliably ordered newer source version may replace an older current representation;
- divergent states with no reliable ordering become a visible merge conflict;
- matching Note ID with divergent semantic content is always a conflict;
- matching `content_hash` without a matching portable source key never collapses contextual Notes.

The same key also drives ordinary sync reconciliation. An update replaces the current Note atomically while preserving its Note ID for as long as that Note remains in the notebook. An authoritative source deletion may remove it. Fieldnotes creates no revision or tombstone Note merely to remember former state. Absence from an incomplete query, page, time window, or failed sync is not authoritative deletion evidence.

Notes from the built-in `self` Field have no portable source-object key. Copy deduplication for them relies on their global Note ID; content similarity is not identity.

Exact source-object deduplication is a Note/storage concern. Entity resolution is a separate graph concern and must not be used to erase source Notes.

## Entities

An entity is Fieldnotes' current evidence-backed projection that several identity anchors refer to one real-world thing. Initial entity types remain deliberately broad:

- `person`;
- `organization`;
- `artifact`.

An entity may project names, observed identities, channels, first/last seen instants, source-provided affiliations, and evidence-backed communication conventions. Entity files use flat frontmatter and readable Markdown bodies. Every datetime is RFC 3339 with an explicit numeric UTC offset; date-only values remain dates. Filenames may use UTC `...Z` timestamps where the notebook contract requires them.

Entity IDs are projection identifiers, not cross-notebook proof of real-world identity. Two rebuilt or independently produced graphs may assign different entity IDs to the same person. Merging uses the underlying anchors and approved resolution rules, not coincidental equality of entity IDs.

### Deterministic auto-merge

The resolver may automatically join identities only when an approved deterministic rule establishes the match, for example:

- the same exact source ID inside the same declared scope;
- the same normalized, source-authoritative email address;
- the same normalized phone number with adequate country context;
- an explicit configured alias or user-approved override.

The rule version, identity values, scope, and supporting Notes or configuration are retained as explainability evidence.

### Candidates, splits, and overrides

Medium or weak evidence produces a candidate rather than a silent union. A matching display name, similar signature, shared organization label, or coincident activity is not enough by itself.

When evidence conflicts, Fieldnotes preserves the competing anchors and the Notes that supplied them. A user may approve a merge, declare an alias, or force a split through durable local intent. Rebuild reapplies that intent and continues to expose later conflicting evidence; an override does not rewrite the source Notes that motivated it.

## Relationships

A relationship is an evidence-backed connection between Notes, identities, entities, or artifacts. Deterministic relationships include:

- sender, recipient, caller, organizer, or attendee links explicitly supplied by a source;
- source-provided reply, thread, conversation, parent, attachment, and reference links;
- exact duplicate-artifact links;
- configured identity or organization aliases;
- first/last seen values, channel presence, interaction counts, and other reproducible aggregates;
- document, ticket, meeting, and cross-Field reference links established by approved deterministic rules.

Relationships are current projections. If the supporting current Notes change or disappear, a rebuild may update or remove the edge. Large evidence sets may use a bounded representative list in public frontmatter only when the full evidence set remains reconstructable from current Notes and rules.

## Evidence origin classes

Every projected claim or edge identifies one of these origins:

- `explicit`: the source directly supplied the relationship or fact;
- `matched`: an approved deterministic rule established it;
- `extracted`: an optional Extraction recovered literal evidence from one Note;
- `observed`: an optional Observation synthesized cited evidence.

An extracted string does not automatically become an existing entity. The deterministic resolver must match it through a known identity, alias, domain, scoped source value, or other approved rule. Otherwise it remains unresolved extracted evidence.

Deleting Extractions and Observations removes `extracted` and `observed` projections on the next rebuild without affecting deterministic Notes, identities, or relationships.

## Explainability contract

Every derived identity join, entity property, or relationship must expose:

- the claim;
- its origin class;
- the Note, Extraction, Observation, or durable configuration references used as evidence;
- the resolver rule or generator identifier and version;
- the identity namespace and scope relevant to the decision;
- enough normalized values, counts, and time range to reproduce the conclusion;
- competing evidence, ambiguity, and unresolved conflicts.

Datetimes in explanations and generated frontmatter use explicit offsets, for example `2026-08-22T11:36:14+02:00`; no timezone-less datetime is valid.

An explanation must remain meaningful without access to a disposable graph database. A cache may accelerate lookup, but it cannot hold the only copy of a join rule, conflict, or cited claim currently exposed by generated public records.

## Conflicts

Fieldnotes distinguishes at least these conflict classes:

- same Note ID with divergent semantic content;
- same portable source-object key with divergent unordered current state;
- one strong identity anchor attached to competing entities;
- contradictory names, roles, affiliations, or source values across current evidence;
- user merge/split intent contradicted by later source evidence;
- derived claims whose cited evidence no longer exists after current-state reconciliation.

Conflicts are preserved and made inspectable. Fieldnotes may select a current or usual projection only through a documented deterministic or Observation rule while retaining the competing evidence. Last-writer-wins, filename order, and highest-confidence-wins are not general conflict-resolution policies.

Conflict material is not a revision ledger: it exists only while reconciliation is unresolved or while contradictory current evidence remains relevant.

## Non-goals and prohibited deterministic claims

The graph is not a CRM, a social score, or a business-judgment engine. It does not derive:

- relationship strength, trust, importance, or strategic value;
- sentiment, customer health, risk, personality, or intent;
- lead, opportunity, account-owner, or lifecycle semantics unless preserving an explicit source-system value under its proper scope;
- priority unless the source explicitly supplied it;
- entity identity from display-name equality alone;
- historical trends from superseded source revisions, because v0.1 keeps no revision ledger.

It may expose objective evidence—counts, channels, first/last seen instants, explicit roles, or cited references—for a human or downstream agent to interpret.

## Verification direction

The deterministic graph and merge milestone is accepted when tests demonstrate:

- namespace and scope qualification prevents cross-tenant/source ID collisions;
- strong approved identities merge reproducibly while weak values produce candidates;
- exact portable source-object merge is order-independent and unions producer provenance;
- matching content without matching source identity preserves both Notes;
- unordered divergent current state and same-ID divergence remain visible conflicts;
- deleting caches and generated graph files reproduces the same semantic result from the same current evidence, config, and rule versions;
- user merge/split intent survives projection rebuild;
- every edge and entity projection has a complete explanation;
- no prohibited judgment appears in deterministic output;
- all generated datetimes contain explicit offsets.

Exact public property names, relationship materialization thresholds, evidence-list bounds, conflict filenames, and entity auto-merge rules beyond the approved initial registry remain A1 or implementation-spike decisions. They must be frozen in fixtures before compatibility is claimed rather than inferred from examples in this document.

