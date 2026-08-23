# A1 implementation findings from IG4 (identity graph)

**Status:** The coordinator has ruled on findings 1 through 4; the rulings,
rationale, and rejected alternatives are recorded in
[ADR 0012](../decisions/0012-graph-implementation-rulings.md). Findings 5
through 8 remain open, awaiting coordinator rulings.  
**Scope:** Contract defects and gaps surfaced while building
`fieldnotes-graph` against the approved [A1](A1-notebook-contract.md) corpus
and [Identity and deterministic graph](../identity-and-graph.md). This
document itself amends nothing; each finding named the workaround the
implementation shipped so the behavior was reviewable while a ruling was
pending, exactly as [IG1 did for A1](A1-implementation-findings.md) and as the
[A2 implementation findings](A2-implementation-findings.md) already do. Four
findings have since been ruled; the resulting A1 amendments live in
[A1's approved-amendments block](A1-notebook-contract.md#approved-amendments)
and [ADR 0012](../decisions/0012-graph-implementation-rulings.md), not in this
document.

The roadmap's own compatibility policy requires exactly this: "if
implementation evidence invalidates an approved contract, return to the
relevant approval milestone with an explicit migration and compatibility
proposal" ([roadmap](../roadmap.md#release-and-scope-controls)).

## Finding 1: a derived entity fixture asserts a name no evidence supplies

The frozen fixture
`tests/fixtures/notebooks/proposed-v1/entities/ent_01a028f2-dcc0-7000-8000-000000000002_person.md`
carries `title: Bob Rossi`, and the relationship fixture's body heading
(`rel_01a028f4-b180-7000-8000-000000000001_person_person.md`) names him too.
A repository-wide search shows the string `Bob Rossi` occurring only in those
two derived records; no Note in the corpus supplies it. The entity's only
cited evidence is the mail and calendar Notes carrying `email:bob@example.net`
and the first name "Bob" in body text — never a full name. Contrast the
sibling entity: `ent_...0001_person.md`'s `title: "Alice Müller"` is
reproducible from the `outlook_contacts_work` contact Note, which carries
`title: "Alice Müller"` in its own frontmatter. No equivalent contact record
exists for Bob.

Deriving `Bob Rossi` from body text mentioning only "Bob" would be name
inference, which the product forbids
([non-goals](../identity-and-graph.md#non-goals-and-prohibited-deterministic-claims):
"entity identity from display-name equality alone", and more generally the
product take on evidence-backed projection). IG4 therefore cannot reproduce
this fixture's `title` byte-for-byte from any deterministic rule; it takes
`title` only from a contact record's own `title` property, or from a
matched, source-authoritative full name, and leaves the property absent
otherwise.

This matters more than a cosmetic slip because the corpus is the reference
material a connector or graph author copies from. An unprovable `title` in an
approved fixture invites the exact inference the product prohibits everywhere
else.

**Candidate resolutions:**

- **(a) Drop the unprovable `title`.** Minimal: remove `title: Bob Rossi` and
  the body heading's surname from the two frozen fixtures. The corpus then
  demonstrates an entity with no name, which is a legitimate but less
  illustrative outcome.
- **(b) Add a contact Note that legitimately supplies it.** Larger: add an
  `outlook_contacts_work` contact record for `bob@example.net` carrying
  `title: "Bob Rossi"`, matching the shape already used for Alice, and update
  the entity/relationship fixtures' evidence and `title` to cite it.

IG4 recommends (b): it is a small, mechanical addition (one more contact
Note, in the same shape as the existing Alice contact), and unlike (a) it
lets the corpus demonstrate the full provenance chain — contact record to
entity `title` to relationship heading — end to end, which is exactly the
kind of case a reader of the reference corpus needs to see. This is the
coordinator's decision, not IG4's; (a) remains a legitimate fallback if the
corpus is not meant to carry that additional fixture right now.

**Ruled.** The coordinator chose (b): a Bob contact Note was added, and the
entity/relationship fixtures were regenerated from the graph library itself
rather than hand-edited, so the corpus is provably derivable. See
[ADR 0012](../decisions/0012-graph-implementation-rulings.md) ruling 1.

## Finding 2: the identity namespace registry A1 was said to freeze does not exist

[Identity and deterministic graph](../identity-and-graph.md) states: "the
final v0.1 registry of namespaces and normalization vectors is part of the A1
notebook-contract approval." [A1](A1-notebook-contract.md) and the
[property registry](../property-registry.md) enumerate no such registry: the
`identities` property is registered only as `list[text]` of "namespaced
identity anchors present in the Note," and the frozen corpus demonstrates
exactly two namespaces, `email:` and `phone:`, with no fixture pinning their
normalization rule, scope class, or strength as approved bytes.

IG4 implemented a minimal in-crate table (`NamespaceRegistry::v1`) covering
`email`, `phone`, `domain`, and `artifact-sha256`, each declaring a scope
class, a strength, and a versioned normalization rule, plus a
`with_policies` path for a caller to declare additional namespaces. An anchor
whose namespace is not declared is refused (`RefusalReason::UnknownNamespace`)
rather than treated as an exact match.

Those four namespace policies and their normalization vectors are **not**
frozen fixtures. Identity normalization therefore currently has no approved
compatibility guarantee: nothing pins, for example, exactly how `email:`
values are case-folded or how `phone:` values are normalized against country
context. The concrete risk is that two independent implementations (or two
releases of this one) could each read
[identity-and-graph.md](../identity-and-graph.md)'s prose, choose different
normalization details for the same namespace, and silently produce different
entities from the same source anchor — a correctness gap with no test able to
catch it, because no fixture exists to disagree with.

**Needed:** registry review with frozen fixtures — namespace list, scope
class, strength, and exact normalization vector per namespace — before
compatibility can be claimed for identity normalization at all.

**Ruled: resolved by scope, not fixed.** The coordinator settled that
Fieldnotes performs no formal identity mapping: collected anchors are
evidence for downstream observation and proposal work, not merge keys. This
finding's concern — that divergent normalization could silently produce
different entities from the same anchor — matters only if normalization
decides merges. Since it does not, divergent normalization is a quality and
interoperability concern rather than a correctness gap, and no frozen
registry is required for v0.1 compatibility. `NamespaceRegistry::v1` is
unchanged. **What would reopen this finding:** any future feature that makes
merge decisions from normalized anchors (for example, an automatic
cross-tenant merge keyed on normalized email text). See
[ADR 0012](../decisions/0012-graph-implementation-rulings.md) ruling 2.

## Finding 3: no public spelling for a scope-qualified identity anchor blocks `0.1.3`

A1 froze identity anchors as flat `namespace:value` text
(`email:alice@example.com`, `phone:+41441234567`). `identity-and-graph.md`
already documents that some namespaces are "authority or tenant scoped: the
value is exact only within a declared tenant, site, server, account, or other
authority," but no approved serialization exists for such an anchor. Publishing
it in flat `namespace:value` form loses the scope: the same unqualified value
from two different tenants would collide once both landed in the same
notebook.

IG4's `NamespaceRegistry` supports declaring an authority- or
namespace-scoped policy (`ScopeClass::AuthorityScoped`,
`ScopeClass::NamespaceScoped`, `ScopeClass::requires_scope`), but when such an
anchor has no way to carry its scope in the public form, IG4 refuses to emit
it (`RefusalReason::ScopeRequired`) and reports it to the caller rather than
publishing something ambiguous.

This is on the **critical path for `0.1.3`, not a nicety**: the roadmap's
Outlook Mail milestone requires "identity anchors," and Microsoft Graph
identities (a source user ID, an object ID) are meaningful only within a
tenant — exactly the case this finding describes. Microsoft Fields in
`0.1.3` will produce tenant-scoped anchors as a matter of course, and without
an approved public spelling for a scope-qualified anchor, `0.1.3` cannot write
their `identities` property at all.

**Needed:** an approved serialization for a scope-qualified anchor (for
example, a third colon-delimited segment, or a distinct property) before
`0.1.3` mapping work can proceed, with fixtures pinning the exact form.

**Ruled: resolved by scope, and no longer critical path.** This finding's own
"blocked on" claim is now wrong and is corrected here explicitly, because a
future contributor reading only the paragraphs above would otherwise believe
Outlook Mail cannot ship. Every Note already carries `source_scope` — the
tenant/authority-scope carrier this finding was asking a new anchor form to
provide. A Microsoft Field's Note already states
`source_scope: "microsoft-graph:tenant/<tenant-id>"` today; an anchor inside
`identities` stays an unqualified `email:`/`phone:` value exactly as A1
already allows, and a reader who needs the tenant reads `source_scope` on
the Note, not a new anchor segment. `0.1.3` is not blocked on this. IG4's
`ScopeClass::requires_scope`/`RefusalReason::ScopeRequired` machinery is
unaffected and remains available for a future namespace whose scope is not
already carried by the Note's own `source_scope`. **What would reopen this
finding:** a future namespace needing scope that `source_scope` does not
already carry, or any feature that makes merge decisions from a
scope-qualified anchor's spelling. See
[ADR 0012](../decisions/0012-graph-implementation-rulings.md) ruling 2.

## Finding 4: entity fixtures reflect the pre-IG1 four-Note corpus, not the current fourteen

The frozen entity fixtures'
(`ent_...0001_person.md` and `ent_...0002_person.md`) `evidence`, `channels`,
and `last_seen` values are internally consistent with each other but stale
against the corpus as it stands today. The corpus now has fourteen Notes, and
several were added after these entity fixtures were authored:

- `ent_...0001_person.md` (Alice) cites four Notes and four channels
  (`outlook_calendar`, `outlook_contacts`, `outlook_mail`, `teams`) with
  `last_seen: 2026-08-22T11:00:00+02:00`. The corpus also carries a
  `jira_acme` ticket Note with `identities: ["email:alice@example.com", ...]`
  and `occurred_at: 2026-08-22T10:45:00+02:00` — inside the fixture's own
  first/last-seen window, but absent from both `evidence` and `channels` —
  plus later Teams call and meeting Notes naming Alice with `occurred_at`
  values of `2026-08-22T14:00:00+00:00` and `2026-08-23T01:30:00+03:00`, well
  past the fixture's `last_seen`.
- `ent_...0002_person.md` (Bob) cites only the two Notes from the original
  four-Note set; it does not reflect the additional mail Notes added later
  that carry `bob@example.net`.

None of this is wrong as a snapshot of an earlier, smaller corpus, but a
frozen fixture that is inconsistent with the frozen Notes it is supposed to
be a deterministic projection of undermines the "rebuild reproduces the same
graph from current evidence" guarantee the corpus exists to demonstrate.

**Needed:** either regenerate the entity (and relationship) fixtures against
the full fourteen-Note corpus, or document that they are pinned to a named
subset and are not a full projection of the corpus as checked in.

**Ruled.** The coordinator chose regeneration, folded into the same ruling
that added Bob's contact record (finding 1): both entity fixtures and the
relationship fixture were regenerated from `fieldnotes-graph::derive_graph`
over the current corpus, with the prior fixtures supplied as input so their
projection IDs were reused unchanged. Alice's entity now cites all eight
current Notes carrying her anchor; Bob's now cites all four carrying his.
The fixtures remain a curated pair of entities (Alice, Bob) and their edge —
not the complete three-entity, three-relationship projection the full corpus
would produce once Sam's own entity is included — which is the "named
subset" half of this finding's alternative, made explicit rather than left
implicit. See [ADR 0012](../decisions/0012-graph-implementation-rulings.md)
ruling 1.

## Finding 5: the relationship fixture's `generated_at` is two minutes later than the entity fixtures'

Both entity fixtures carry `generated_at: 2026-08-22T12:10:00+02:00`, while
the relationship fixture carries `generated_at: 2026-08-22T12:12:00+02:00`.
That is consistent with two separate generation passes (entities, then
relationships, reading the entities' output) rather than one atomic rebuild
producing every derived record at a single instant. Nothing in A1 or
`identity-and-graph.md` states whether a rebuild is required to stamp every
derived record from a single clock read or is permitted to advance the clock
between derivation stages; the fixture bytes are the only evidence either
way, and they show the two-pass shape.

**Needed:** confirm whether a single `generated_at` per rebuild is required,
or whether per-stage timestamps (as the fixture already shows) are the
approved shape.

**Not ruled, but the regenerated fixtures no longer show the gap this finding
described.** The finding-1/finding-4 regeneration (see
[ADR 0012](../decisions/0012-graph-implementation-rulings.md) ruling 1) used
one `derive_graph` call to produce both the entity and relationship
fixtures, and `derive_graph`'s public API only ever takes one clock read per
call, so every projected record it returns necessarily carries the same
`generated_at`. The regenerated relationship fixture's `generated_at` is now
`2026-08-22T12:10:00+02:00`, identical to the entity fixtures', where the
previous hand-authored fixture carried a value two minutes later. This is
what the shipped library actually does when a caller derives everything in
one pass; it is not a coordinator ruling that per-stage timestamps are
disallowed, since a caller could still choose to call the entity and
relationship builders from two separate clock reads if it wanted to. The
question this finding raises — whether A1 or `identity-and-graph.md` should
require single-instant stamping — remains open.

## Finding 6: `person_person` relationship direction is undefined

A1 reserves a `person_person` relationship type with `from_entity_id` and
`to_entity_id`, but neither A1 nor `identity-and-graph.md` states what
direction means for a symmetric co-participation edge (two people appearing
together on a mail thread and a calendar event, as in the frozen fixture) —
there is no sender/recipient or organizer/attendee asymmetry to anchor a
direction to.

IG4 adopted a documented canonical orientation: `from_entity_id` is the
lower-ordered entity and `to_entity_id` the higher-ordered entity by primary
identity anchor, so that regenerating the same evidence always produces the
same pair ordering regardless of input order. The relationship's body states
explicitly that neither side implies an initiator or a direction, so a reader
of the public file is not misled by the `from`/`to` naming.

**Needed:** confirm this canonical-orientation convention as the approved
rule for symmetric relationship types, or specify a different one.

## Finding 7: no frozen organization entity fixture exists

The [property registry](../property-registry.md#proposed-derived-types)
reserves `person`, `organization`, and `artifact` entity types, and
`identity-and-graph.md` describes organization entities with the same
envelope as person entities. The frozen corpus contains only `person`
entities (`ent_...0001` and `ent_...0002`); no `organization` entity fixture
exists, even though the contact Note already in the corpus carries
`outlook_contacts_company_name: Example AG` — evidence that could back one.

IG4 emits `organization` entities using the same envelope shape as `person`
entities (same required properties, same explainability contract), but this
is unreviewed: no fixture pins an organization entity's exact byte form,
`title` source, or evidence rule.

**Needed:** an approved organization entity fixture, most naturally added
alongside the contact-record addition in Finding 1.

## Finding 8: no registered unprefixed property records person-versus-organization for a contact record

The existing `outlook_contacts_work` contact Note fixture already carries a
vendor-prefixed `outlook_contacts_contact_kind: person` property, but A1
registers no *unprefixed* shared property for "this contact record describes
a person" versus "this contact record describes an organization." Reading a
vendor-prefixed property in core to decide entity classification would put
connector-specific logic in core, which the crate boundary rules disallow.

IG4 therefore classifies a contact-derived entity by anchor class alone
(an anchor scoped to a person-shaped namespace such as `email:` or `phone:`
produces a `person` entity; a `domain:`-only anchor with no person-shaped
anchor produces an `organization` candidate) rather than by reading
`outlook_contacts_contact_kind` or any other connector-prefixed property.
This works for the current fixture but is a narrower rule than the source
data actually supports.

**Needed:** registry review of whether an unprefixed
`contact_kind`-equivalent property (or equivalent typed classification)
should be added to the shared registry so a contact record can state its
subject kind without connector-specific logic in core.

## What the graph deliberately does not derive

Recorded here so a reader treats these as withheld rather than missing:

- **No person-to-organization affiliation edge.** A company-name string
  (such as `outlook_contacts_company_name`) is weak descriptive evidence per
  `identity-and-graph.md`'s strength table, and inferring affiliation from a
  mail domain is inference the product prohibits. No `person_organization`
  relationship type is reserved by the property registry in any case. This
  needs registry review — both the relationship type and the evidence rule
  that could deterministically establish it — before it could be
  materialized.
- **No artifact entity records.** `identity-and-graph.md` and the property
  registry reserve an `artifact` entity type, but no frozen fixture defines
  one. IG4 derives no `artifact` entities until a fixture exists to pin their
  shape.
- **Thread and duplicate-artifact facts are returned to the caller, not
  materialized.** IG4 computes reproducible thread groupings
  (`source_scope`-qualified `thread_id`/`conversation_id`) and duplicate
  content-hash groupings, and exposes them as in-memory facts the caller may
  use for `explain`/`gaps` output, but writes no public record for either,
  because no approved record type defines one. This needs registry review
  before either fact class could be materialized as a public file.

## Non-findings worth recording

- **Deterministic auto-merge rules already frozen (email, phone) are
  sufficient to reproduce the two-entity fixture.** No additional rule was
  needed to pass the existing corpus; the gaps above are about coverage
  beyond it, not about the rules IG4 already has approved evidence for.
- **`identities` list ordering and deduplication** follow A1's set-like-list
  rule (sorted, deduplicated by normalized text value) without modification;
  no finding was needed there.
