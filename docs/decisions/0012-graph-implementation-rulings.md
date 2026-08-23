# ADR 0012: Graph implementation rulings from IG4 findings

- **Status:** Accepted on 2026-08-23
- **Date:** 2026-08-23

## Context

IG4 implemented the `fieldnotes-graph` identity and relationship derivation
library against the approved [A1](../approvals/A1-notebook-contract.md)
corpus and [Identity and deterministic graph](../identity-and-graph.md).
Implementation surfaced eight findings, recorded without deciding, in
[A1 graph implementation findings](../approvals/A1-graph-implementation-findings.md),
because changing an approved shared contract, or settling the product's
identity-resolution scope, requires a coordinator ruling rather than an
implementation guess.

The coordinator has now ruled on three matters spanning four of the eight
findings. This ADR records those rulings, their rationale, the alternatives
rejected, and their consequences. Findings 5 through 8 remain open and are not
addressed here.

## Decision

### 1. The unprovable `title` is fixed by adding evidence, not by removing the claim (finding 1, and finding 4's regeneration)

Finding 1 observed that the frozen fixture `ent_...0002_person.md` (Bob)
carried `title: Bob Rossi` and that the relationship fixture's heading named
him too, but no Note in the corpus supplied that name — reproducing it would
require exactly the display-name inference
[identity-and-graph.md](../identity-and-graph.md#non-goals-and-prohibited-deterministic-claims)
forbids. Two candidate resolutions were on the table: drop the unprovable
`title`, or add a contact record that legitimately supplies it.

**Ruling: add the contact record.** The corpus is reference material a
connector and graph author copies from, so it should demonstrate the full,
correct provenance chain end to end — a name appears in a contact record, the
entity derives it from that contact record's own `title`, and the evidence
cites the contact record — rather than merely avoiding the problem by
projecting no name at all. A corpus that only ever shows the "no name
available" case would under-teach the common, actually-supported case.

`tests/fixtures/notebooks/proposed-v1/notes/` gains
`20260822T082000Z_outlook_contacts_work_contact_note_01a02891-5bd0-7000-8000-000000000010.md`,
a `contact` Note produced by the same `outlook_contacts_work` Field as
Alice's contact record, carrying the same registered properties (`identities`,
`outlook_contacts_company_name`, `outlook_contacts_contact_kind`,
`outlook_contacts_job_title`, `source_identity`, `source_scope`,
`source_url`, `source_version`, `title`) and a single `email:bob@example.net`
identity anchor — Bob has never carried a phone anchor anywhere else in the
corpus, so the fixture does not invent one just to mirror Alice's two-anchor
shape. No registered property, grammar, or vocabulary changed: every property
on the new Note was already registered before this ADR.

**A related gap in the same corpus.** The damaged mail Note
(`..._mail_note_01a028c1-..0000a.md`) carries `from: bob@example.net` but no
`identities` property at all, unlike every other mail Note in the corpus
(compare `..._0005` and `..._000f`, which list every anchor their `from`/
`to`/`cc` roles carry). `identities` is now added: `email:bob@example.net`
and `email:sam@example.net`, the two anchors the Note's `from`/`to` roles
already carry, matching the convention the rest of the corpus already
follows. `identities` is a registered set-like list, so canonical emission
sorted the two members (`bob` before `sam`) and deduplicated trivially (there
was nothing to deduplicate). `content_hash` covers the normalized body only,
never frontmatter, so this frontmatter-only addition does not move it: the
embedded value (`fn-content-v1-sha256:c223190050809860598c6479e532cb022e82aa0b2f0a2a58be50a8b1b3e72174`)
is unchanged, and `fieldnotes-format`'s own
`embedded_content_hashes_recompute_from_bodies` conformance test — which
recomputes every embedded hash from its Note's body on every run — confirms
this by recomputing the identical value against the edited file.

**Finding 4's regeneration is the same ruling, made concrete.** Finding 4
separately observed that the entity and relationship fixtures reflected the
pre-IG1 four-Note corpus rather than the corpus as it now stands, and that
adding Bob's contact record without also updating those fixtures would make
the problem worse: the fixtures would then claim evidence that omits the very
contact record this ruling adds. `entities/ent_...0001_person.md` (Alice),
`entities/ent_...0002_person.md` (Bob), and
`relationships/rel_...0001_person_person.md` (their edge) were regenerated
directly from `fieldnotes-graph::derive_graph` over the full current corpus,
seeded with the three existing fixture files as prior projections so the
library reused their exact IDs rather than minting new ones — the same
reuse-by-anchor mechanism `crates/fieldnotes-graph/src/derive.rs`'s
`prior_reuse` already implements for an ordinary rebuild that does not delete
`entities/`/`relationships/` first. All three projection IDs (`ent_
...0001`, `ent_...0002`, `rel_...0001`) came back unchanged; regeneration
would have been stopped and reported instead of shipped had any of them
moved. Every other frontmatter property and the Markdown body came directly
from the library's own `entity_record`/`relationship_record` emitters — no
byte was hand-typed — so the corpus is now provably derivable rather than
hand-maintained, which is exactly what a reference corpus should be.

Two consequences of using the library's real output rather than hand-authored
prose:

- Alice's entity now cites all eight Notes that carry her anchor (the
  original four plus the Jira ticket, the skipped-attachment mail, the
  Teams call, and the Teams meeting IG1 added later), and Bob's now cites all
  four that carry his (the original mail and calendar event, his new contact
  record, and the damaged mail Note). This is the direct fix for finding 4.
- Both entities' explanation `claim`, `origin`, and `rule` changed from what
  the hand-authored fixture previously said to what the resolver's actual
  rules produce for this evidence: Alice's is now `matched` /
  `contact-record-anchors-v1` (joining two anchors from her contact record,
  as before), and Bob's is now `matched` / `email-exact-v1` (his anchor
  recurs across four current Notes, which already qualifies as a match
  rather than a single unconfirmed occurrence) rather than the `explicit` /
  `email-anchor-v1` the hand-authored version had asserted. The previous
  wording was never something the library actually produced for this
  evidence; the corrected wording is.
- Both fixtures' `generated_at` is now the same instant
  (`2026-08-22T12:10:00+02:00`), because one `derive_graph` call stamps
  every projected record it returns from one injected clock read. The
  frozen relationship fixture previously carried `generated_at` two minutes
  later than the entity fixtures', which finding 5 separately flagged as an
  open question about whether a rebuild may advance the clock between
  derivation stages. That question is not ruled on here — this ADR did not
  choose single-instant stamping as a policy — but the regenerated fixtures
  now demonstrate what the shipped library actually does, which finding 5's
  entry is updated to note.

**Alternative rejected: drop the unprovable `title` instead.** This was the
minimal fix and remains a legitimate fallback in general, but it teaches a
reader only the "no evidence, no name" case and never the "a contact record
supplies a name" case — the more common and more instructive one, and the
one the corpus already demonstrates for Alice. Doing that instead of adding
the missing evidence would have left the corpus asymmetric for no reason.

### 2. Findings 2 and 3 are resolved by product scope, not deferred pending a registry (findings 2 and 3)

The coordinator has settled a broader product question that both findings
were implicitly waiting on: **Fieldnotes does not perform formal identity
mapping.** Collected anchors — email addresses, phone numbers, source IDs,
and the like — are surfaced as *evidence* for downstream observation and
proposal work to reason over. They are not keys a resolver uses to decide
that two appearances are the same real-world person, except through the
already-approved, narrow deterministic rules
[identity-and-graph.md](../identity-and-graph.md#deterministic-auto-merge)
already states (exact scoped source ID, normalized source-authoritative
email, normalized phone with country context, explicit contact-record
co-identity, or a durable user override).

This resolves finding 2 and finding 3 differently from how their own text
framed the ask, and the framing matters enough to record explicitly:

**Finding 2 (no frozen namespace/normalization registry) — resolved by
scope, marked resolved rather than fixed.** Finding 2's stated concern was
that two independent implementations could each read
`identity-and-graph.md`'s prose, choose different normalization details for
the same namespace, and silently produce different entities from the same
source anchor — "a correctness gap with no test able to catch it." That
concern is real only if normalization details decide *merges*: if divergent
case-folding or phone normalization could join two anchors that another
implementation would keep apart (or vice versa), that is a correctness gap,
because it changes what entities exist. Since anchors are evidence rather
than merge keys, and every deterministic join rule now runs the same narrow
approved list of contact-record co-identity plus exact/normalized-anchor
recurrence regardless of the namespace registry's internal normalization
choices, divergent normalization between implementations changes *how an
anchor's text is spelled in an output record*, not *which entities exist*.
That is a quality and interoperability concern — two notebooks describing
the same person with differently-cased email text is worth fixing — but it
is not a correctness gap in the sense finding 2 raised, so no frozen registry
is required for v0.1 compatibility. `NamespaceRegistry::v1`'s existing
namespace table, scope classes, and normalization rules remain exactly as
IG4 shipped them; nothing in this ruling changes a byte of it.

**Finding 3 (no public spelling for a scope-qualified anchor) — resolved by
scope, and no longer critical path.** This is the finding this ADR must
correct most emphatically, because finding 3's own text asserts something
that is now wrong: "Microsoft Fields in 0.1.3 will produce tenant-scoped
anchors as a matter of course, and without an approved public spelling for a
scope-qualified anchor, 0.1.3 cannot write their `identities` property at
all." That was true only if a `0.1.3` Field needed to publish a
scope-qualified value inside the flat `identities` list to make its tenant
provenance recoverable. It does not: **every Note already carries
`source_scope`** (A1 section 6, required on every external Note with a
stable source object), which is exactly the tenant/authority-scope carrier
finding 3 was looking for a new anchor form to provide. A Microsoft Field's
Note already states `source_scope: "microsoft-graph:tenant/<tenant-id>"`
today (see the corpus's own mail, calendar, and contact Notes); an anchor
inside `identities` can stay an unqualified `email:`/`phone:` value exactly
as A1 already allows, and a reader who needs to know which tenant a given
Note's anchors were observed under reads `source_scope` on that Note, not a
third colon-delimited segment invented for the anchor itself. `0.1.3` is not
blocked on this, was never actually blocked on it once `source_scope`
existed, and a future contributor who read finding 3's original text without
this correction would wrongly believe Outlook Mail could not ship.

IG4's `NamespaceRegistry::requires_scope`/`RefusalReason::ScopeRequired`
machinery is unaffected: it still exists for the case a *future* namespace
needs (an anchor whose value is only unique inside a scope that is not
already the Note's own `source_scope` — for instance, two different
namespace-scoped account handles that collide in spelling across two
different named services). That case is real but is not the `0.1.3`
Microsoft Graph case, which `source_scope` already covers.

**What would reopen either finding.** Both are resolved *by scope*, not
closed as never-relevant: any future feature that makes merge decisions from
normalized anchors — for example, an automatic cross-tenant person merge
keyed on a normalized email, or a feature that unions entities from two
notebooks based on anchor text equality without a human decision — would
reopen finding 2, because at that point normalization detail starts to
decide which entities exist rather than only how they are displayed, and a
frozen registry would then be required for correctness. Similarly, a future
namespace whose scope is not already carried by the Note's own
`source_scope` would reopen finding 3's need for a scope-qualified public
anchor spelling. Neither condition is met by anything approved or shipped
today.

**Alternative rejected: freeze a full namespace/normalization registry now,
"to be safe."** Rejected because there is no compatibility promise it would
protect: nothing today decides a merge from normalization detail, so freezing
one now would spend registry-review effort pinning bytes that carry no
correctness weight yet, while likely needing revision once the anchors the
product actually needs before a real merge-affecting feature ships are
known. Registry review remains available whenever a feature that does
decide merges from anchors is actually proposed.

**Alternative rejected: add a scope-qualified anchor form now, "to be safe
for 0.1.3."** Rejected for the same reason finding 3's "critical path" framing
was wrong: `source_scope` already carries what a scope-qualified anchor form
would have carried for the one case that was said to need it. Adding a new
anchor form now would be inventing a shared property/grammar extension A1's
change policy requires registry review for, to solve a problem `source_scope`
already solves.

### 3. A1 section 12 forbids Fieldnotes performing the write, not an awkward proposal format (part of finding coverage from A1, not the numbered graph findings)

Section 12 of [A1](../approvals/A1-notebook-contract.md#12-proposal-and-handback-record-envelopes)
states a proposal must "never contain an executable vendor API payload," with
the stated reason that a machine-executable payload "would encourage
automatic action beyond Fieldnotes' boundary." Read in isolation, that
sentence could be misread as also forbidding any format specific enough to be
mechanically consumed — including a structured, vendor-neutral rendering a
downstream agent or script could parse cleanly.

**Ruling: the line A1 actually draws is that Fieldnotes never performs the
write, not that the sidecar format must be awkward to consume.** A
vendor-neutral, human-and-agent-readable sidecar — for example, a vCard 4.0
(RFC 6350) rendering of a proposed contact update — is permitted alongside a
proposal record or inside a handback package. What remains prohibited,
regardless of format, is anything that performs or triggers a write against
a destination system: an executable vendor API call, a payload shaped to be
replayed directly against a vendor endpoint, or any artifact whose purpose is
to cause a destination mutation without a human or a later, explicitly
approved writeback capability in between.

**This is worth writing down now, not leaving implicit, precisely because a
sidecar makes the write tempting.** A vCard sitting next to a proposal is one
short script away from being piped into a destination system's import
endpoint, and that temptation is exactly why the boundary needs to be
explicit rather than inferred from the surrounding prose: the risk section 12
was written to guard against is not "the format is too easy to parse," it is
"Fieldnotes, or something one step downstream of it, performs a write
Fieldnotes never approved." A vendor-neutral rendering that is easy to
consume does not itself cross that line; a payload that performs or is
purpose-built to trigger the write does, no matter how it is encoded — a
vCard-shaped write trigger would be exactly as prohibited as a raw vendor API
payload.

This ruling **does not approve any vCard rendering.** No vCard generator,
schema mapping, or fixture is added by this ADR. Rendering a proposal or
handback contact as vCard is a later phase's implementation work, and belongs
to core (the component that already owns proposal and package rendering),
not to `fieldnotes-graph`. This ADR records only the boundary the future work
must respect.

**Alternative rejected: read section 12 as forbidding any structured sidecar
format.** Rejected because it would forbid legitimate, useful renderings
(vCard, iCalendar, and similar vendor-neutral standards are exactly the kind
of interchange format a handback package should be able to offer) for a risk
they do not actually create. The risk is who performs the write, not how
parseable the proposal's supporting material is.

**Alternative rejected: leave section 12 as originally worded and rely on
implementers reading the rationale correctly.** Rejected because the
original wording's stated reason ("would encourage automatic action beyond
Fieldnotes' boundary") is exactly the sentence that invites the overbroad
reading a future contributor could take literally. Recording the
clarification in A1's amendment block removes the ambiguity for anyone
reading section 12 without also reading this ADR.

## Consequences

- `tests/fixtures/notebooks/proposed-v1/notes/` gains one Note (Bob's contact
  record); `tests/fixtures/notebooks/proposed-v1/entities/ent_...0001` and
  `..._0002` and `tests/fixtures/notebooks/proposed-v1/relationships/rel_
  ...0001` are regenerated from the current corpus by the graph library
  itself, with all three projection IDs unchanged. The damaged mail Note
  gains an `identities` property; its `content_hash` is unchanged.
  `tests/fixtures/notebooks/proposed-v1/README.md`'s gate-classification
  table and corpus description are updated to describe the addition.
- `fieldnotes-format`'s own conformance suite (`crates/fieldnotes-format/
  tests/conformance_valid.rs`) has its three corpus-size assertions updated
  (28 total record fixtures, 17 Notes checked for filename validity, 19
  embedded content hashes checked) to include the new Note; no assertion's
  meaning changed, only the count.
- `crates/fieldnotes-graph/tests/corpus.rs`'s
  `person_projection_reproduces_the_frozen_entity_fixture_frontmatter` and
  `relationship_projection_reproduces_the_frozen_relationship_fixture` now
  derive over the full current corpus (previously a curated four-Note
  subset) and compare Bob's frontmatter the same way Alice's already was,
  since Bob's fixture now carries a `title` too. Every other assertion's
  meaning is unchanged.
- `docs/approvals/A1-notebook-contract.md`'s approved-amendments block gains
  an entry recording the corpus regeneration and the section 12
  clarification.
- `docs/approvals/A1-graph-implementation-findings.md` records findings 1,
  2, 3, and 4 as ruled, each linking here, and its status line is updated to
  reflect that findings 5 through 8 remain open.
- `docs/identity-and-graph.md` is corrected so it no longer describes the
  namespace/normalization registry as blocking compatibility, and no longer
  implies a scope-qualified anchor form is needed before Microsoft Fields can
  write `identities`.
- `docs/roadmap.md`'s `0.1.2` entry no longer cites the namespace/scoping
  registry as an open compatibility gap, and nothing in `0.1.3`'s entry is
  described as blocked on an anchor spelling that `source_scope` already
  makes unnecessary.
- No property name, record type, grammar, limit, rejection code, or list
  semantics changed anywhere in this ADR. No new shared property was
  introduced. `NamespaceRegistry::v1`'s namespace table is unchanged.
