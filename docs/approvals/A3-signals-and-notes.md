# A3 approval: signals, notes, and type-specific rendering

**Status:** Ready for review. **Not approved.** No choice below has been
accepted, no box in the approval checklist is checked, and nothing in this
document may be implemented until the owner approves it explicitly.  
**Scope:** The information architecture of the notebook's readable layer:
the three-tier vocabulary (signal, note, enrichment), what a signal is and
how it serializes, what a note is and where it comes from, type-specific
render templates as contract, the filename grammar and directory layout for
readable records, property ordering for human reading, whether source change
history is retained, and what happens to the entity, relationship,
extraction, observation, proposal, package, and conflict records that
currently sit alongside notes

## Decision requested

A1 froze a public notebook contract in which one record kind — the Note —
is simultaneously the machine record of what a source said, the human-readable
artifact a person opens, and the evidence every derived record cites. A
notebook collected from live Microsoft 365 data (328 records: 248 contacts,
50 mail, 30 calendar events) demonstrates that one record cannot be all
three. The defects are recorded with evidence below.

A3 proposes splitting that single record kind in two: a **signal**, which is
the collected machine record, and a **note**, which is a readable artifact
rendered from a signal by a type-specific template. It proposes a filename
grammar and directory layout for the readable layer, a property ordering for
human reading, and a decision on whether source change history is retained.

This is a format change. It invalidates the A1 fixture corpus, the
`0.1.0` golden fixtures behind release gate R0, and the notebook this
repository's owner has already collected. The recommendation is to make it
now: the product explicitly treats deleting a refetchable notebook as a
supported lifecycle action ([product](../product.md), the roadmap's third
invariant), so migration is a re-sync rather than a migrator, and 328
records in one notebook is the cheapest this change will ever be.

**A3 does not change A2.** See "What A3 must not do" below.

The detailed background lives in:

- [A1 notebook contract](A1-notebook-contract.md)
- [A2 Field protocol](A2-field-protocol.md)
- [Product](../product.md)
- [Notebook format](../notebook-format.md)
- [Property and record-type registry](../property-registry.md)
- [Identity and deterministic graph](../identity-and-graph.md)
- [Optional built-in enhancement](../enhancement.md)
- [Roadmap](../roadmap.md), especially its invariants and gates R0, R2, R8,
  and R9
- [Current-state ADR](../decisions/0001-current-state-and-state-classes.md)
- [ID and hash ADR](../decisions/0004-record-ids-and-hash-domains.md)
- [A1 implementation findings](A1-implementation-findings.md),
  [A2 implementation findings](A2-implementation-findings.md), and
  [A1 graph implementation findings](A1-graph-implementation-findings.md)

No candidate fixture corpus is attached to this document. A1 and A2 were each
reviewed against attached bytes, and A3 should be too; the fixture set A3
would need is specified in "Fixture evidence required for implementation"
below, and building it before the model is chosen would mean building it
twice. If the owner prefers to review bytes rather than prose, the correct
sequence is to settle the four questions in "Questions the reviewer must
settle" first and then produce the corpus against the settled model.

## What A3 must not do

A3 changes the storage and rendering layer only. **The A2 process boundary
and all three Microsoft Fields are unchanged**, and this is not an
aspiration — it follows from what A2 already says.

[A2 section 6](A2-field-protocol.md#6-the-record-envelope-a-normalized-source-envelope)
chose the normalized source envelope precisely so that a Field emits values
and never spells bytes. It states that core owns "the canonical key order;
the canonical scalar spelling; the filename; and every durable write," and
that a record "is never a rendered Note, never carries a notebook path or
filename." A signal is core's durable storage of an A2 `record`; a note is
core's rendering. Neither is visible to a Field.

Concretely, nothing in A3 requires a change to: any A2 frame type, member,
grammar, limit, ordering rule, cursor rule, checkpoint rule, exit code,
rejection code, diagnostic code, or schema file; the `describe` manifest;
declared-property enforcement; artifact staging or the handle grammar; the
protected credential channel; or the sixteen frozen transcripts. A Field's
capability slice still declares a `note_type` and core still enforces it —
that value now selects a render template as well as a record type, which is
a core-side consequence of a value the Field already sends.

One consequence runs the other way and is stated in section 2 rather than
hidden: because A2's `record.properties` admits only scalars and homogeneous
scalar lists
(`common.schema.json#/$defs/propertyValue` is `oneOf[scalarValue,
scalarList]`), a signal cannot contain nested structure a Field did not send,
and a Field cannot send nested structure under protocol v1. Any proposal to
give signals nested vendor structure is therefore an A2 amendment, not an A3
decision, and A3 does not make one.

## The problems this must solve

Each defect below is named with evidence from the live notebook or from A1's
own prose. They are the reason this package exists; a restructure that fixes
fewer than all of them is not worth its cost.

### P1. A readable file whose name is unreadable

A contact's filename is:

```text
20250512T201356Z_outlook_contacts_work_contact_note_<uuid>.md
```

91 characters. The person's name appears nowhere in it. 91 bytes of a
readable file's name carry a timestamp, a Field ID, a record type, a record
kind prefix, and a UUID — every one of which is also present in the
frontmatter, which A1 section 3 already declares authoritative.

### P2. 248 files in one flat directory

`notes/` is flat in v0.1 ([notebook format](../notebook-format.md)). The live
notebook's `notes/` holds 328 files, 248 of them contacts, and every one of
those 248 is named by the pattern in P1. A directory listing is unusable as a
directory listing: nothing distinguishes one entry from the next except a
timestamp and a UUID.

### P3. Machine bookkeeping literally precedes the one field a reader wants

A1 section 5 orders frontmatter as the five structural Note keys, then every
remaining key in ascending ASCII byte order. For a live contact the resulting
order is:

```text
id, instance_id, field_id, type, occurred_at,
captured_at, content_hash, identities,
outlook_contacts_contact_kind, source_identity, source_scope,
source_version, title
```

`title` is key 13 of 13. `captured_at` and `content_hash` — a durability
timestamp and a hash of the body immediately below — sort above it because
`c` precedes `t`. The ordering rule is correct as a serialization rule and
wrong as a reading order, and A1 section 5 says so itself: the
structural-keys-first exception exists because lexicographic order "hides the
five structural Note properties in larger records." The same argument applies
to `title` and was not extended to it.

### P4. The filename separator is ambiguous by construction

A1 section 3 states: "Readers must not naively split the underscore-delimited
filename, because registered Field stems and labels may contain
underscores." That is not a caveat about a hostile input; it is an admission
that the grammar is ambiguous for the corpus that already exists.
`outlook_contacts_work_contact` contains four underscores and decomposes into
a two-part stem (`outlook_contacts`), a label (`work`), and a type
(`contact`) only with the registry in hand.

### P5. Contacts sort by a date that means nothing

Note filenames render `occurred_at` in UTC (A1 section 3). For a contact,
`occurred_at` is the source object's last-modified instant, because a contact
has no event time — a contact is not an event. In the live notebook the 248
contacts' filename timestamps span 2025-05-12 to 2026-07-27, an interval that
describes when a directory-sync last touched each record and nothing a reader
would ever want to sort by. The one ordering the flat directory provides is
therefore meaningless for 76% of its contents.

### P6. Property names are machine-oriented throughout

`source_version`, `content_hash`, `captured_at`, `collected_by`,
`outlook_mail_internet_message_id`, `outlook_calendar_response_status`. These
are correct names for what they are and they are the majority of what a
reader sees. A live contact carries thirteen properties, of which five are
Fieldnotes bookkeeping, four are source identity or versioning, one is a
vendor-prefixed classification, and one — `title` — is something a person
asked for.

### P7. The properties a reader wants are not properties

A live contact's most useful values are not in frontmatter at all. Of 248
contact records, 245 carry an email address and 232 carry a phone number
**as Markdown body prose**, alongside an organization on 135 and a role on
109. There is no `email` property, no `phone` property, and no address
property on any of them.

Two mechanisms carry fragments of that information and neither is adequate.
`identities` carries `email:` and `phone:` anchors, but it is a set-like list
that A1 sorts and deduplicates by normalized text, so it cannot distinguish a
business phone from a mobile, or a primary address from an alias — the role
is erased by the list semantics. The Markdown body carries the human-readable
version, but as text, so it is not queryable, not typed, and not addressable
by `fieldnotes.base` or any frontmatter-aware tool.

This is the flat-frontmatter rule working as designed and producing a bad
outcome for one record type. It is also the defect most likely to be misread
as an argument for nested signals; see section 2 for why that argument does
not survive contact with A2.

## Recommended restructure

### 1. Three tiers, and the vocabulary

Approve three tiers with three names, and use the names consistently
everywhere afterwards:

| Tier | Record | What it is | Lifecycle |
|---|---|---|---|
| Evidence | **signal** | The collected machine record of what a source actually said | Reconciled, replaced, or removed by collection; not rebuildable |
| Readable | **note** | A human-readable artifact rendered from a signal, or authored directly by the user | Derived notes are disposable and rebuildable; authored notes are not |
| Enrichment | Extraction, Observation | Optional, evidence-cited derived records | Disposable |

The word **Note** in every approved document means what **signal** means
here. That is the whole of the rename, and it is the most expensive part of
this proposal to get wrong, because A1, A2, ADR 0001 through ADR 0013, the
property registry, and the roadmap all use "Note" in the old sense several
hundred times. A3 does not propose editing them in place. It proposes that
A3 approval carries a stated equivalence — *approved-document "Note" reads as
"signal" except where a document is amended* — and that each document is
amended when it is next touched for another reason. Sweeping every file at
once would produce a diff nobody can review against fixtures that no longer
exist.

Two consequences of the tiering are load-bearing and must be stated as rules
rather than left as implications.

**Deterministic rendering, by default, with no model.** A note is produced
from a signal by template render: no model, no network, no GPU, no optional
component. Enrichment is applied on top when it exists. This is not a
preference; the roadmap's invariant that "no release before the enhancement
milestone may require a model, model download, GPU, or network access for
notebook use" means that if rendering needed a model, a freshly synced
notebook would contain nothing a person could read.

**A derived note carries no durable state.** Once a note is rebuildable from
its signal, every property on it must be a function of the signal, the
template, and whatever derived records currently exist. Anything else is
durable state in a file the product promises is safe to delete. This rule
decides several things below — most sharply the enrichment timestamps in
section 4 and change tracking in section 8 — and it is the single most useful
test to apply to any later addition to the note model.

#### Alternatives considered

- **Keep one record kind and fix its presentation.** Reorder frontmatter, add
  a slug to the filename, partition the directory, and leave the Note as the
  single canonical record. This is much cheaper and it fixes P1 through P6.
  It does not fix P7, and more importantly it leaves every presentation
  decision permanently coupled to the evidence contract: a better contact
  layout becomes a change to hashed, merged, conflict-bearing canonical bytes,
  so it needs a format version every time. The split is worth its cost
  primarily because it decouples "how this reads" from "what was collected."
- **Three tiers, but render on demand rather than to a file.** Keep only
  signals on disk and have `fieldnotes inspect` render a readable view. This
  is the cheapest possible storage model and it destroys the product promise:
  the notebook must be readable in Obsidian and by ordinary filesystem tools
  with no Fieldnotes executable present ([product](../product.md), gate R9).
  A view that exists only inside the tool is not a notebook.
- **Two tiers, with the readable layer as an Obsidian-only projection**
  (`fieldnotes.base` views over signals). Attractive because it adds no
  record kind, and rejected because it makes exactly one editor the readable
  interface, which contradicts "a person browsing in a text editor" and "an
  AI agent grounding work in source-derived context" as independently
  supported cases.

#### Consequences

- The notebook has two file populations with different durability promises,
  and a user cannot tell them apart by looking at a directory. Section 3's
  `origin` property and section 6's layout exist to fix that, and the
  deletion rule in section 9 exists because getting it wrong destroys
  user-authored material.
- Every derived record's evidence citation moves domain, from a Note ID to a
  signal ID. That is a mechanical change with a large surface; see section 9.
- The roadmap's first invariant — "Notes and retained artifacts are the
  canonical representation" — is **reversed as written and preserved in
  substance**: signals and retained artifacts are canonical, and notes join
  the disposable class. It must be restated, not merely reinterpreted.

### 2. What a signal is, and how it serializes

A signal is what an A1 Note is today. Same required properties (`id`,
`instance_id`, `field_id`, `type`, `occurred_at`), same portable source key
`(source_scope, source_identity)`, same bookkeeping (`captured_at`,
`collected_by`, `content_hash`, `source_version`), same deterministically
normalized Markdown body of source evidence, same artifact references, same
current-state reconciliation, same merge survivor rule, same conflict
behavior.

**The recommendation is that a signal keeps A1's approved byte form
exactly: flat YAML frontmatter in the A1 subset, one blank line, a
normalized Markdown body, UTF-8, LF, one final LF.** It changes its record
kind prefix, its directory, and nothing else.

This is a recommendation against the owner's proposal that signals become
canonical JSON. The reasoning is below, because the JSON question is the one
place in this package where the intuition and the mechanics point in
different directions.

#### Assessing JSON against the flat-YAML subset

The case for JSON is real and worth stating at full strength. JSON has a
canonical form — RFC 8785, the JSON Canonicalization Scheme — where YAML
has none, which is exactly why this repository needed a hand-written parser:
`crates/fieldnotes-format/src/yaml.rs` is a 397-line scanner whose own module
documentation describes it as "a hand-written parser for the byte grammar A1
defines." And the JCS machinery is already here:
`crates/fieldnotes-format/src/jcs.rs` implements RFC 8785 section 3.2.2.3
number spelling and section 3.2.2.2 string serialization, plus a decoder, in
314 lines, because A1 section 5 already borrowed both rules for YAML scalars.
A1 did not avoid canonical JSON; it reimplemented half of it inside a YAML
grammar it then had to specify byte by byte.

Three further reductions are genuine:

1. **The semantic-record encoding's hand-specified deviations collapse.**
   ADR 0006 ruling 2 had to state that `fn-record-v1`'s encoding differs from
   the public emitter in two ways rather than one, the second being
   "unconditional ascending-key order with no structural-keys-first
   exception." Under JCS that is not a deviation to specify — lexicographic
   key order is what JCS *is*. One of the two documented differences
   disappears into the spec.
2. **One fewer grammar to own.** The frontmatter/body split, the `---`
   delimiters, the exactly-one-blank-line rule, the plain-versus-quoted text
   rule, and the block-list form are all A1 inventions that JSON does not
   need.
3. **A1's own alternatives section conceded the point in one direction.**
   Section 5 rejected "nested YAML or JSON objects" because they "violate
   flat Obsidian/generic-tool behavior" — a reason that applies to a file a
   human opens and does not apply to a machine record nobody opens.

The case against is stronger, and it turns on two things.

**First, the YAML subset does not go away, so JSON is an addition rather than
a replacement.** Notes are Markdown with flat YAML frontmatter — that is the
whole point of the readable tier — and so is every derived record:
extractions, observations, entities, relationships, proposals, conflict
bundles, package manifests. If signals become JSON, this repository owns two
canonical serializations, two validators, two rejection-code families, and
two fingerprint domains where it now owns one. `yaml.rs`, `emit.rs`,
`record.rs`, and `normalize.rs` all survive unchanged for the readable tier.
Net complexity goes up, and the count of places a byte can be spelled wrong
goes up with it. JCS also does not carry the rules that actually cost effort
to specify: which keys are admissible, that a datetime must carry an explicit
offset, that a list is homogeneous, that an integer outside the exactly
representable binary64 range is invalid rather than rounded. Every one of
those has to be restated for JSON, and every one is already implemented and
tested for YAML.

**Second — and this is decisive — the one thing JSON can express that flat
YAML cannot is nesting, and a signal cannot be given nested structure under
protocol v1.** P7 is the strongest argument for JSON in this whole package: a
contact's emails, phones, and addresses are typed, role-bearing, repeating
structures that flat frontmatter mangles into a deduplicated anchor list and
body prose. But that structure would have to arrive from a Field, and A2's
`record.properties` admits only `scalarValue` or `scalarList`
(`common.schema.json#/$defs/propertyValue`), with each declared property
typed as one of A1's five scalars at `scalar` or `list` cardinality
(A2 section 4). A Field cannot send a nested value, so core cannot store one.
Adopting JSON without amending A2 buys a re-spelling of exactly the same flat
data.

There is also a cost the owner's framing understates. Today portability is
*provable*: the canonical artifact is the file a human opens, so R9's
criterion — "notebooks remain useful offline and without Fieldnotes
installed" — is demonstrated by opening a notebook in Obsidian. With
JSON signals the claim becomes "any tool can parse the signal, and the
readable layer rebuilds deterministically," which is weaker in a specific
way: a notebook whose `notes/` was deleted, or whose notes were never
rendered, contains its evidence in a form no ordinary Markdown or
frontmatter tool can read, and restoring readability requires the Fieldnotes
executable and the correct template version. Section 1's
deterministic-rendering rule mitigates that; it does not make the claim as
strong as the one being given
up, and this package should not pretend otherwise. If signals keep the
Markdown form, both tiers stay readable and the R9 claim survives intact.

**Recommendation.** Signals keep the A1 flat-YAML-plus-Markdown form.
Canonical JSON signals with nested vendor structure are a coherent and
probably correct future step, and they are an A2-amending package: a nested
record envelope, per-Field declaration of nested property shapes, a nested
value model in the A1 registry, and a new signal serialization, reviewed
together. Splitting that into "JSON now, nesting later" spends the migration
cost twice and delivers the benefit neither time.

#### What this does to `fn-content-v1` and `fn-record-v1`

Under the recommendation, **nothing**. This is the largest single saving in
the package and the reason the recommendation is worth taking.

`fn-content-v1-sha256` hashes the canonical normalized Markdown body bytes
under the `fieldnotes-content-v1\0` domain separator (A1 section 9). A signal
has exactly that body, normalized by exactly those six rules. Every hash
vector in `tests/fixtures/hashes/proposed-v1/` reproduces unchanged.

`fn-record-v1-sha256` hashes the canonical semantic payload with A1 section
8's bookkeeping exclusions, UTC-normalized datetimes, and unconditional
ascending key order. A signal's property set is a Note's property set, so the
encoding, the exclusion list, the survivor rule, and the frozen
`semantic-record-canonical.md` vector are all unchanged.

Two clarifications are needed even so, and both are prose rather than bytes:

- **`fn-content-v1` applies to signals and to authored notes, and not to
  derived notes.** A derived note's body is a deterministic function of its
  signal and its template. Hashing it would produce a value that changes
  whenever a template is revised, compares equal only between notebooks
  running the same template version, and identifies nothing a reader or a
  merge needs. A derived note carries no `content_hash`.
- **Neither domain applies to a note's identity or comparison.** Notes do not
  merge, do not conflict, and are not compared by fingerprint. Reconciliation
  and conflict detection happen entirely in the evidence tier. Section 9
  records the one exception: an authored note, which has no signal, keeps a
  Note-like merge identity.

Had the owner's JSON proposal been adopted, both domains would need new
versions: `fn-content-v1` has no Markdown body to hash on a JSON signal, and
`fn-record-v1` would be redefined as JCS over the signal minus exclusions.
That redefinition is genuinely cleaner than what exists — it is one rule
instead of a rule plus two documented deviations — but it is a new hash
domain, which A1's change policy correctly treats as requiring an explicit
format version and migration proposal, and it invalidates every checked-in
hash vector.

#### Signal identity and location

Approve a new record kind. This requires registry review under A1's change
policy, which "new shared properties, primary types, prefixes, or record
kinds require registry review and fixtures" already covers.

- **Prefix:** `sig_`, joining the table in A1 section 1.
- **Location:** `signals/<field-id>/<signal-filename>.md`, partitioned by
  Field so a single Field's collection is inspectable and so no directory
  holds every record in the notebook.
- **Filename:** A1 section 3's grammar, unchanged, with `sig_` in place of
  `note_`:
  `<YYYYMMDDTHHMMSSZ>_<field-id>_<type>_<signal-id>.md`. The machine-oriented
  filename is correct for a machine record. P1 and P5 stop being defects the
  moment the file is not the one a person opens; they were only ever defects
  because that file was doing both jobs.

The signal ID itself is a real choice, because a note has to cite it and the
citation has to survive a rebuild.

**Recommendation: derive a signal's ID deterministically from its portable
source key** — the lowercase hex of SHA-256 over a domain-separated encoding
of `(source_scope, source_identity)`, truncated to a fixed width, prefixed
`sig_` — for every signal that has such a key. A `self` signal, which has no
portable source key, keeps a UUIDv7.

This reverses A1 section 1's rejection of "deterministic/content-derived IDs
for every record," and the reversal is narrow enough to argue for
explicitly. A1 rejected content-derived IDs because they "couple identity to
mutable serialization, leak equality, and turn ordinary edits into identity
changes." None of those apply here, because the derivation input is not the
content: it is the portable source key, which A1 section 6 already requires
to be stable within its scope and identical across instances, and which A1
section 7 already treats as the thing that identifies the object across
updates. Deriving the ID from it makes explicit what the reconciliation rule
already assumes. A1 made the same kind of exception for artifacts in section
2, for the same kind of reason.

The payoff is that a note's citation of its signal cannot dangle across a
rebuild, a re-sync, or a copy between notebooks, so none of A1 section 12's
rebinding machinery (`binding_status`, a separate stable anchor, refusal to
present a stale projection ID) is needed for the note-to-signal link. The
cost is a third ID family and a truncation width that must be justified as
collision-resistant and frozen as a vector.

#### Alternatives considered

- **UUIDv7 signal IDs, exactly as Notes have today.** No new mechanism, and
  the citation problem returns: a signal removed by authoritative deletion
  and later refetched gets a new ID (A1 section 7), so every note citing it
  dangles, and the note-to-signal link needs the `binding_status` treatment
  A1 section 12 built for proposals. Workable, and it adds a state machine to
  the most common link in the notebook.
- **Cite the portable source key directly instead of a signal ID.** Removes
  the ID question entirely and is the most honest citation available, since
  the pair *is* the identity. Rejected as the primary form because
  `source_identity` values are up to 1024 bytes of arbitrary non-control text
  (A2 `common.schema.json`), which makes them poor property values to read,
  impossible in a filename, and awkward to index. The recommendation is that
  a derived note carry the signal ID as its link and that the signal remain
  the only place the full pair is written.
- **Keep `note_` as the signal prefix and give the readable record a new
  prefix.** Migration becomes a directory move with no ID rewrite. Rejected:
  it permanently inverts the vocabulary this section exists to fix, and the
  ID rewrite is free anyway, because `id` is excluded from `fn-record-v1`
  comparison and absent from `fn-content-v1` input, so re-prefixing changes
  no hash.

#### Consequences

- The A1 serializer, validator, hash implementation, merge rule, and conflict
  machinery are reused verbatim for the evidence tier. The implementation
  cost of A3 is concentrated in the render layer, which is new code rather
  than changed code.
- `signals/` is machine-oriented and stays that way. No promise is made about
  its filenames being meaningful to a person, and P1 and P5 are answered by
  saying so rather than by fixing the names.
- A notebook contains the same evidence twice in different shapes — once in a
  signal, once rendered into a note. Storage roughly doubles for text-bearing
  records. For 328 records this is irrelevant; it is worth stating because it
  scales linearly and because a reader may expect notes to be small.

### 3. The note model: two origins, one signal link, and enrichment timestamps

A note is a readable Markdown record with flat YAML frontmatter in the A1
subset. It has **two origins**, and the model must say which:

- **`derived`** — rendered from exactly one signal by a type-specific
  template. Disposable, rebuildable, and safe to delete.
- **`authored`** — created directly by the user. `fieldnotes note "call Alice
  back"` produces a note, not a signal, and it is the only copy of that
  material. Not rebuildable, and never removed by a rebuild.

Approve a required `origin` property on every note, with the closed
vocabulary `authored derived`. **This is a new shared property name and
requires registry review with fixtures.**

Making origin explicit and required, rather than inferring it from the
presence of a signal link, is deliberate. The inference is available — a note
with no signal link is authored — but the consequence of getting it wrong is
that `rebuild` deletes the only copy of something a person wrote. A property
that must be present and must be one of two values fails closed; an inference
over an absent field fails silently. This is the same argument A2 section 3
makes for declaring capability rather than inferring it from behavior.

A derived note carries:

- `signal_id`: the ID of the signal it renders. **New shared property; needs
  registry review.** Exactly one, in v0.1: a note that aggregates several
  signals (a thread note, a person note) is a coherent later record type and
  is deliberately not proposed here, because its identity, its rebuild
  trigger, and its conflict behavior are all different questions.
- `template_id` and `template_version`: which template produced these bytes.
  **New shared properties; need registry review.**
- `origin: derived`.
- Whatever properties its template's declared property set selects
  (section 5).

A derived note does not carry `content_hash`, `collected_by`,
`source_version`, `captured_at`, or `instance_id`. Those describe collection,
they live on the signal, and their presence on a readable record is P6.

An authored note carries `origin: authored`, no `signal_id`, and the A1
required properties it already carries today.

**Coverage is derived from notes, never flagged on signals.** Which signals
have notes is answered by scanning notes for `signal_id` values and
subtracting. Nothing is written into a signal to record that a note exists.
This is settled and the reason is worth keeping in the document: a
"has a note" flag would let the readable tier mutate the evidence tier, so a
signal's bytes would depend on what had been rendered from it, an idempotent
re-collection could churn it, and its `fn-record-v1` fingerprint would change
for a reason that has nothing to do with the source. The cost is that
coverage is an O(notes) scan rather than a lookup; that scan's result belongs
in `.fieldnotes/cache/`, ADR 0001's disposable class, where losing it costs
one rescan.

#### Enrichment timestamps

The proposal asks that a note may carry timestamps for its last extraction
and observation. Approve them as **projections recomputed at render time,
never stored facts**: `last_extraction_at` is the maximum `generated_at` over
extractions currently citing this note's signal, and `last_observation_at`
the same over observations. Both are omitted when no such record exists.
**Both are new shared property names and need registry review.**

The distinction matters because of section 1's rule that a derived note
carries no durable state. If these were stamped when enrichment ran and left
alone, deleting `extractions/` would leave a note asserting an extraction
time for an extraction that no longer exists — durable state in a file the
product says is disposable, and unrecoverable on rebuild. As projections they
simply disappear when their evidence does, which is the correct behavior and
is also the only behavior a rebuild can reproduce.

This collides with release gate R8, which requires that "deleting
`extractions/` and `observations/` leaves Notes byte-for-byte unchanged."
Under this model deleting those directories changes a derived note's bytes,
because the timestamps and any enrichment sections vanish. **R8 must be
restated, and restating it makes it stronger:** the property that matters is
that enhancement never mutates *evidence*, so the criterion becomes "deleting
`extractions/` and `observations/` leaves every signal and every authored
note byte-for-byte unchanged, and re-rendering restores every derived note."
That is what R8 was protecting; the old wording only expressed it because
evidence and readable artifact were the same file.

#### Alternatives considered

- **One origin, with authored material as a `self`-Field signal that renders
  a note.** Structurally uniform, and it makes `fieldnotes note "call Alice
  back"` produce two files where the user asked for one, with the readable
  one disposable and the machine one canonical. The owner's constraint that
  authored notes stay first-class is a constraint against this, and it is the
  right constraint: for authored material the readable file *is* the
  evidence.
- **Separate directories for authored and derived notes.** Makes the
  durability difference visible in the filesystem, which is a real safety
  gain, and splits the readable layer the restructure exists to create — a
  person browsing would have to look in two places for "my notes." Section 6
  recommends against it and section 9 carries the deletion rule that makes
  the single directory safe.
- **Store enrichment timestamps when enrichment runs.** One less thing to
  recompute, and it puts non-reproducible state in a disposable file. Bluntly
  the same mistake as change-tracking option 2 in section 8, at smaller
  scale.

#### Consequences

- `rebuild` becomes a destructive operation over `notes/` and must be keyed
  on `origin`. Section 9 states the rule.
- A note's frontmatter is not stable across enhancement state changes. A
  reader diffing two notebooks may see notes differ where signals agree; the
  signal is the thing to diff.
- `last_extraction_at` and `last_observation_at` require the render pass to
  read `extractions/` and `observations/`, so rendering depends on more than
  the signal and its template. Rendering remains deterministic — same inputs,
  same bytes — but "rebuild one note" needs the enrichment index, not just
  one signal file.

### 4. Type-specific templates, and what it means for a template to be contract

A note is rendered by a template selected by its signal's primary type. A
template declares:

1. **A property set** — the ordered list of frontmatter properties this
   type's notes carry, each drawn from the A1 shared registry or the
   signal's own Field prefix, with the order being reading order
   (section 5).
2. **A body layout** — the Markdown sections that make sense for the type, in
   order, each with a stated source in the signal.
3. **A slug source** — the ordered list of properties the filename slug is
   drawn from (section 6).
4. **A signals section** — required, last, carrying the traceability: the
   signal's ID, its `(source_scope, source_identity)`, its `source_url` when
   present, and the relative path to the signal file.

Eleven templates, one per primary Note type, is the initial set. A worked
example, for `contact`:

```markdown
---
id: note_01a02b40-0000-7000-8000-000000000001
type: contact
origin: derived
title: "Alice Müller"
organization: Acme AG
role: Head of Operations
signal_id: sig_9c1f8ab2d4e60517
template_id: contact
template_version: 1
---

# Alice Müller

Head of Operations at Acme AG.

## Reaching Alice

- Email: alice@example.com
- Phone: +41 44 123 45 67

## Signals

- `sig_9c1f8ab2d4e60517` — Outlook Contacts (`outlook_contacts_work`),
  collected 2026-08-22, source `contact/AAMkAGI2CONTACT01`
  ([signal](../../signals/outlook_contacts_work/20260822T101500Z_outlook_contacts_work_contact_sig_9c1f8ab2d4e60517.md))
```

Two properties in that example do not exist. `organization` and `role` are
illustrative of what a contact template would want and are **not proposed as
approved names**; both require registry review, and `role` in particular
needs care because `identity-and-graph.md` treats a role string as weak
descriptive evidence and the registry must not let a template's convenience
turn it into something stronger. The related open question from
[A1 graph implementation findings, finding 8](A1-graph-implementation-findings.md)
— whether an unprefixed property should record whether a contact record
describes a person or an organization — becomes more pressing here, because a
contact template has to choose a layout and currently can only do so by
reading a vendor-prefixed property, which core is not allowed to do.

**What it means for a template to be contract.** The recommendation is that a
template is **reviewable but not frozen**: its identifier and version are
approved and recorded on every note it renders, per-type golden fixtures are
required at this gate, and a template revision is a **rebuild**, not a format
version bump and not a migration.

That is only defensible because of a chain of decisions above, and the chain
is the point:

- a derived note's bytes are not hashed (section 2), so no hash breaks;
- a derived note is not compared, merged, or conflict-bearing (section 2), so
  no reconciliation breaks;
- a derived note's filename slug is stored rather than recomputed (section 6),
  so a template revision does not rename 248 files.

Remove any one of those and a template edit becomes a format change to every
note of that type, and the readable layer becomes as expensive to improve as
the evidence layer — which is the coupling section 1 exists to break.

#### Alternatives considered

- **Freeze template bytes as contract, like the canonical emitter.** Gives
  the strongest reproducibility claim: any implementation renders identical
  bytes. Rejected because it prices every presentation improvement at a
  format version, and because the reproducibility it buys is not a property
  anyone needs — two notebooks with identical signals and different template
  versions are not in conflict about anything, since the signals agree.
- **User-editable templates.** The obvious next request, and deliberately not
  proposed. A user-edited template makes the readable layer non-reproducible
  across notebooks, gives `template_version` no meaning, and creates a file
  under `.fieldnotes/` whose loss silently changes output. It is also a
  plausible later feature, and keeping templates release-owned now does not
  foreclose it.
- **One generic template with per-type sections.** Fewer artifacts to
  review, and it reproduces the current defect: a contact rendered by a
  generic template gets a generic layout, which is how `captured_at` ended up
  above `title`.

#### Consequences

- Eleven templates plus their golden fixtures are the bulk of this gate's
  review surface, and every one of the eleven primary types needs a real
  example. A1's fixture list already carried this debt — it noted that IG1
  must add the missing `meeting`, `call`, and `document` body templates — and
  A3 makes it unavoidable.
- A notebook's notes may have been rendered by different template versions at
  different times. `template_version` on each note is what makes that
  visible, and a re-render is what makes it uniform.
- A template that wants a property the registry does not have cannot invent
  it. Every gap found while writing the eleven templates becomes a registry
  review item, which is the correct pressure and will produce a list longer
  than the two names in the example above.

### 5. Property ordering for human reading

Approve a reading order for **readable records only** — notes, and only
notes:

1. `title` when present, then the type's remaining reading-order properties in
   the order its template declares.
2. Then `id`, `type`, `origin`, and, for a derived note, `signal_id`,
   `template_id`, `template_version`.
3. Then every remaining property in ascending ASCII byte order.

Signals, extractions, observations, entities, relationships, proposals,
conflict bundles, and package manifests keep A1 section 5's ordering exactly:
structural keys first, then ascending ASCII. Nothing about the machine tier's
serialization changes.

The rule A1 states — "ordering is serialization, not meaning" — survives.
Template-declared order is still serialization; a parser may accept any
order; the canonical serializer still rewrites to one deterministic form. The
change is which deterministic form, for one record kind, and the
justification is A1 section 5's own: it already broke pure lexicographic
order for the five structural keys because lexicographic order "hides" what a
reader needs. P3 is that argument applied to `title`.

Putting identity and provenance *after* content, rather than first, is the
part most likely to feel wrong. It is deliberate: on a readable record those
properties are the traceability, they are duplicated in the body's signals
section, and a reader who wants them is looking for them rather than reading
past them.

#### Alternatives considered

- **`title` first, then A1's existing rule unchanged.** The minimal fix,
  fully sufficient for P3, and one line of specification. It leaves a
  contact's `organization` and `role` sorted among `id` and `template_id`,
  which reintroduces the same defect one property down. Worth taking if the
  owner wants the smallest possible change to A1 section 5.
- **Registry-declared global order for every property.** A1 already rejected
  this, and the rejection still holds: adding a property changes the ordering
  policy and every generic writer needs the full registry. Template-declared
  order is per-type and local, which is why it does not have that problem.
- **No ordering rule; let the template emit whatever order it likes.**
  Simplest to implement and it gives up canonical bytes for notes, so two
  renders of one signal could differ. Rejected: even a disposable file should
  round-trip.

#### Consequences

- The canonical emitter takes a per-record-kind ordering policy rather than
  one global rule. That is a small change to `emit.rs`, and it is a change to
  the one function whose output every fixture asserts.
- Round-trip validation for notes compares against template-declared order,
  so a template revision that reorders properties changes note bytes. Under
  section 4 that is a rebuild, not a migration.

### 6. Filename grammar and directory layout

#### Layout

```text
notebook/
├── signals/
│   ├── outlook_contacts_work/
│   ├── outlook_mail_work/
│   └── self/
├── notes/
│   ├── contact/
│   ├── mail/
│   └── text/
├── artifacts/
├── extractions/
├── observations/
├── entities/
├── relationships/
├── proposals/
├── conflicts/
└── packages/
```

`signals/` partitions by Field ID. `notes/` partitions by primary type.
Authored and derived notes share `notes/`, distinguished by `origin` rather
than by location, for the reason section 3 gives: a person browsing wants one
place for readable material.

Partitioning `notes/` reverses the approved statement that "`notes/` is flat
in v0.1" and gives up the property that directory listing produces one global
timeline. P2 and P5 are the case for giving it up: the flat directory's
listing is unusable at 328 records, and its one ordering is meaningless for
the 248 that are contacts. A global timeline across types is a query, and it
is one `find` or one `fieldnotes.base` view away; it is not worth a directory
nobody can read.

#### Filename grammar

Use a delimiter that cannot occur inside a segment, so a reader may split the
name — which fixes P4 rather than documenting it:

```text
<readable-part>--<record-id>.md
```

`--` is unambiguous because every segment matches `[a-z0-9_]` or, for the
slug, `[a-z0-9-]` with runs of `-` collapsed to one, and a UUID contains only
single hyphens. Nothing in the grammar can produce a double hyphen except the
delimiter.

The readable part is type-specific:

- **Event-like types** (`mail`, `message`, `event`, `meeting`, `call`,
  `ticket`, `text`, `voice`, `file`, `document`) keep a UTC timestamp, so the
  per-type directory still sorts as a timeline:
  `<YYYYMMDDTHHMMSSZ>--<slug>`
- **Entity-like types** (`contact`) carry no timestamp, because P5:
  `<slug>`

Before and after, for the two live cases:

```text
# contact, today (91 characters, name absent)
notes/20250512T201356Z_outlook_contacts_work_contact_note_<uuid>.md

# contact, proposed
notes/contact/alice-muller--note_01a02b40-0000-7000-8000-000000000001.md

# mail, today
notes/20260822T080000Z_outlook_mail_work_mail_note_01a0287d-acc0-7000-8000-000000000005.md

# mail, proposed
notes/mail/20260822T080000Z--migration-thursday--note_01a0287d-acc0-7000-8000-000000000005.md
```

The name is shorter for a contact and about the same length for mail, and in
both cases the readable part leads, so a narrow file pane or a truncated
column shows the name rather than a timestamp. It is worth being honest that
the UUID still dominates: 36 bytes of any note filename are a record ID, and
they stay because
[notebook format](../notebook-format.md) requires every filename to contain
its record ID and because that requirement is what makes uniqueness free. A
materially shorter name means dropping the ID, which trades a guarantee for
cosmetics.

#### Slug determinism, absence, change, unsafe characters, and uniqueness

**Determinism.** A1 section 3's rule that a validator "computes the expected
filename from validated frontmatter and compares the whole name" is preserved
exactly, because the slug is itself a stored property. Approve `title_slug`
(**new shared property, needs registry review**): a text property, written
once when the note is first rendered, from which the filename is thereafter
computed. The folding rules below therefore govern *initial derivation* only.
This matters more than it looks: if the slug were recomputed on every render,
the ASCII folding table would have to be frozen as fixture bytes with the
same rigour as A1's media-type-to-extension registry, because two
implementations folding `Müller` differently would compute different expected
filenames and each would reject the other's notebook. With a stored slug,
divergent folding produces a cosmetically different name in a newly rendered
note and no validation failure anywhere.

**Derivation.** From the first present, non-empty property in the template's
declared slug-source list (`contact`: `title`; `mail`: `subject`, then
`title`; `event`: `title`, then `subject`): apply Unicode NFKD; drop combining
marks; apply a frozen ASCII folding table for the characters NFKD does not
decompose (`ß`, `ø`, `đ`, and their kin); lowercase, ASCII only; replace
every run of characters outside `[a-z0-9]` with one `-`; trim leading and
trailing `-`; truncate to 48 bytes at a `-` boundary. The folding table needs
registry review; it is small, and leaving it to implementation is how two
notebooks end up with `alice-muller` and `alice-mller`.

**Absence.** The slug segment and its delimiter are both omitted, and the
grammar is unambiguous without it because the ID segment always begins with a
known kind prefix:

```text
notes/contact/note_01a02b40-0000-7000-8000-000000000001.md
notes/mail/20260822T080000Z--note_01a0287d-acc0-7000-8000-000000000005.md
```

The slug is omitted when: no slug-source property is present; the derived
string is empty after filtering, which is the case for a title written
entirely in a script the folding table does not cover; the result matches a
reserved Windows device name (`con`, `prn`, `aux`, `nul`, `com0`–`com9`,
`lpt0`–`lpt9` — the same list A2 section 8 already excludes from artifact
handles); or the result would begin with a character a filesystem treats
specially. Omission is always available, always valid, and never a failure.

**Change.** A stored slug does not change when its source property changes.
This is the recommendation, and it is the one that makes the whole scheme
affordable. A recomputed slug would mean that an upstream display-name edit
renames a file, and a rename Fieldnotes performs behind Obsidian's back
breaks every wikilink and backlink pointing at the old name — Obsidian
updates links for renames *it* performs, not for renames that appear
underneath it. At 248 contacts, an upstream directory-sync touching display
names is a mass link-breaking event. Renaming is therefore an explicit
operation (`rebuild --rename` or equivalent, a `0.1.x` CLI decision, not
frozen here), and the default is that names are stable for the life of the
note. The same rule covers the authored case for free: a person's own note
keeps the name it was given.

**Uniqueness.** Guaranteed unconditionally by the record ID, which is in
every filename. The slug is never load-bearing for identity, so no
disambiguating suffix, counter, or collision-retry logic is needed — the
failure mode those mechanisms exist to prevent cannot occur. Two notes whose
slugs differ only by case are still distinct files on a case-insensitive
filesystem because their IDs differ, and restricting the slug to lowercase
ASCII means no filesystem's Unicode normalization (APFS and HFS+ differ here)
can make two distinct slugs collide.

**Path length.** A note path is roughly `notes/` plus a type segment plus 48
slug bytes plus 2 plus 41 plus 3 — about 110 bytes, well under the 255-byte
per-component limit but consuming a real fraction of Windows' 260-character
`MAX_PATH` once a user's notebook sits a few directories deep. Signals, at
`signals/<field-id>/` plus A1's grammar, are similar. Worth a bound-checking
test on Windows rather than a rule.

#### Alternatives considered

- **Keep A1's `_` separator and add the slug as another `_`-delimited
  segment.** No new grammar, and it makes P4 strictly worse by adding a
  segment whose contents may contain the delimiter's cousin. Rejected.
- **Drop the record ID from note filenames.** The only route to a genuinely
  short name. It makes uniqueness depend on slug plus timestamp, so two
  contacts with the same name collide and need a disambiguator, and the
  disambiguator must itself be a stored property for the filename to stay
  computable — which is a worse version of `title_slug`, doing more work for
  a weaker guarantee.
- **Recompute the slug on every render.** Names always match current content,
  and it costs a frozen folding table on the compatibility path plus mass
  renames on upstream edits. Rejected above.
- **Keep `notes/` flat and rely on the slug alone.** Preserves the global
  timeline and A1's approved statement, and leaves 248 contacts sharing a
  directory with everything else. Legitimate if the owner values the flat
  timeline more than P2; the slug alone does fix the "which one is Alice"
  problem.

#### Consequences

- `notebook format`'s statement that "the filename contains no subject,
  participant, organization, or other mutable human content" is **directly
  reversed** for notes. A note filename now contains human content, and that
  content is on the readable tier where a person needs it. Signals keep the
  old rule.
- A notebook's readable layer is no longer one directory listing. Tools that
  glob `notes/*.md` must glob `notes/*/*.md`.
- One new shared property (`title_slug`) exists purely to make a filename
  stable. That is a real cost and the alternative is mass renames.

### 7. Change tracking over time: the one genuinely open decision

**This section does not make a recommendation the owner can simply accept.
It is the decision the owner must make, and the reason it is separated from
everything above is that the rest of this package is compatible with any of
the three answers.**

The proposal asks that a contact keep its original as a signal and track
changes over time, so a note can show both changes made server-side and
changes detected from other evidence such as mail, with observations landing
in the note when something changes.

That is in direct tension with an approved invariant. The roadmap states that
"Fieldnotes represents the current collected state, not an append-only
history ledger" and "does not retain source revisions merely because it
observed them during earlier syncs." ADR 0001 makes the same commitment: "a
source update can replace the current Note without retaining its old body."
A1 section 7 implements it: "v0.1 keeps no source revision ledger." The
product boundary says Fieldnotes is not "an immutable archive, audit trail,
compliance store, or evidence ledger."

Showing that a contact's job title changed requires retaining a value that
current-state reconciliation overwrites. There is no way around that fact.

#### Option 1: bounded versioned signals

Retain a bounded number of prior revisions per source object, in the evidence
tier, as first-class retained state.

- **What it buys.** The only option where change history is genuinely
  reconstructible. A note can say "job title changed from X to Y between
  these two collections" and cite both revisions. Rebuild reproduces it,
  because the evidence is on disk. Deleting derived files loses nothing.
- **What it costs.** It reverses the invariant, openly. Storage grows with
  revision depth. Merge semantics need extending: A1 section 8's survivor rule
  assumes one current record per portable source key, and two notebooks with
  different retained revision sets for the same object need a union rule,
  an ordering rule, and a conflict rule that do not exist. Pruning needs a
  policy. The product boundary statement needs amending, because a bounded
  revision store is a history ledger with a bound on it.
- **The bound is not a detail.** A bounded ring means the history is provably
  incomplete: past the bound, a change is silently unrecoverable, so a note
  that reports changes reports some of them. That is honest and it is weak,
  and a reviewer should decide whether a partial history is worth an invariant.

#### Option 2: change observations recorded at detection time

When reconciliation replaces a signal, write an observation recording what
changed, then discard the prior value.

- **What it buys.** It looks derived. It costs almost no storage. It needs no
  merge rule, because an observation is just another derived record.
- **Why it should be rejected.** It is durable state wearing a disposable
  costume. The prior value is gone once overwritten, so the observation
  cannot be regenerated from anything — delete `observations/` and that
  change is unrecoverable, which contradicts the property that derived
  material can always be deleted and rebuilt. It also breaks release gate R2
  as written: "deleting all caches and derived graph files reproduces the same
  semantic
  graph." A change observation is not reproducible from current evidence, so
  a notebook that deletes and rebuilds produces a *different* graph, and the
  gate cannot pass without exempting one record class from the rule that the
  gate exists to enforce. ADR 0001 already states the governing principle:
  "human decisions must be stored separately from rebuildable generated
  projections." A change observation is neither a human decision nor
  rebuildable, and it would sit in the directory reserved for rebuildable
  things.
- **The honest version of option 2 is option 1.** If the change record must
  be durable, put it in the evidence tier where non-reproducibility is
  expected, and accept the invariant reversal. Putting it in
  `observations/` does not avoid the reversal; it hides it.

#### Option 3: current state only

Keep the invariant. Drop change tracking. Signals hold current state; notes
render current state.

- **What it buys.** Every approved invariant stands. Every release gate stays
  satisfiable. The merge rule, the conflict rule, the pruning story, and the
  product boundary are unchanged. A3 becomes a presentation and storage
  change with no product-boundary consequence, which is what makes it cheap.
- **What it costs.** The owner does not get "the job title changed."

#### Recommendation

**Option 3 for this gate, with option 1 as the only acceptable route if the
owner wants change tracking, taken as its own approval package.** Option 2
should be rejected outright rather than held as a fallback.

Three reasons.

First, the restructure's value does not depend on change tracking. P1 through
P7 are all fixed by sections 1 through 6 with the invariant untouched.
Bundling an invariant reversal into the same decision means the owner cannot
approve the cheap, uncontroversial part without also approving the expensive,
contested part.

Second, option 1 done properly is a larger package than this one. It needs a
revision-retention policy, a bound with a justification, a merge union rule, a
conflict rule for divergent revision sets, a pruning rule, an amendment to
the roadmap invariant, an amendment to ADR 0001, an amendment to the product
boundary, and fixtures for all of it. Attaching that to A3 would make A3
unreviewable.

Third — and this is the substantive finding — **a large part of what the
proposal asks for does not need history at all, and section 8 explains why.**
The request bundles two different things: "the server changed this value"
(needs retention) and "this contact record and this mail signature disagree"
(needs neither). The second is already in scope. Delivering it under option 3
may satisfy most of the actual want, and the owner is better placed to judge
that after reading section 8 than before.

#### Consequences of the recommendation

- The notebook cannot answer "what changed" in v0.1, and a user who wants
  that answer must keep it elsewhere. The product boundary already says so.
- `source_version` still moves when a source object changes, and
  `content_hash` still differs, so the notebook can detect *that* something
  changed at collection time. It cannot report *what*, and it cannot report it
  after the fact, because both values are overwritten too.
- If the owner chooses option 1, nothing in sections 1 through 6 needs
  revisiting: bounded revisions are additional records in the evidence tier,
  which is exactly where section 1 puts non-rebuildable state.

### 8. What the entity graph can and cannot contribute

The graph is relevant here and is not a substitute for history, and the
difference is worth stating precisely, because it is where most of the
proposal's contact-change requirement actually lives.

**What the graph already does.** Correlating a contact with mail evidence is
not a new capability — it is what
[identity-and-graph.md](../identity-and-graph.md) already specifies and what
`fieldnotes-graph` already implements. A person entity joins identity anchors
across Fields and cites the records that supplied them; the frozen corpus
demonstrates exactly this, with one entity citing eight records across five
channels. Medium and weak evidence produces a **candidate** rather than a
silent union, and every projection carries its origin class, its rule
version, and its cited evidence. So "with the entity graph a note can show
changes detected from other evidence such as mail" is, in its correlation
half, already available: a contact note can render the entity its signal
resolves to, and that entity already points at every mail, calendar, and chat
record naming the same person.

**What the graph cannot do.** It only ever sees current state. It can
therefore report that two *current* sources **disagree** — a contact record
says one role, a mail signature extraction says another — but it cannot
report that a value **changed**, because the prior value was overwritten
before the graph ran. `identity-and-graph.md` is explicit about both halves:
its conflict classes include "contradictory names, roles, affiliations, or
source values across current evidence," and its prohibited-claims list
includes "historical trends from superseded source revisions, because v0.1
keeps no revision ledger."

That distinction is the finding. A contact note *can*, today, under every
approved invariant, carry a section that says: this contact record states one
role; a signature extraction from a mail record two weeks later states
another; here are both, with citations. That is a disagreement between
concurrent evidence, it needs no retention, it is fully rebuildable, and it is
probably a large fraction of what "track changes over time" was reaching for
— because in practice the reason a job title looks stale is usually that a
directory record and a live signature disagree, not that anyone needs an
audit trail.

What the graph cannot give, under any option but option 1, is server-side
revision history: the contact record itself changed, and Fieldnotes saw both
values. Nothing in the graph layer recovers that.

#### Consequences

- Under option 3, a contact template should render an evidence-disagreement
  section rather than a change section, and the vocabulary should say
  "disagrees with" rather than "changed," so a reader is not misled about what
  the notebook knows.
- Populating that section from mail signatures depends on the `0.1.8`
  signature-extraction capability, which is optional. The deterministic floor
  is the entity's cited evidence, which needs no model.
- Entity and relationship evidence citations move from Note IDs to signal IDs
  (section 9), so the graph derives from the evidence tier and never from the
  rendered tier.

### 9. Derived, projected, and reconciliation records

These record kinds currently sit alongside notes and cite Notes as evidence.
Each is affected, and none is affected the same way.

**Entities and relationships.** Unchanged in shape. Their `evidence` lists
change domain from Note IDs to signal IDs, because the graph must derive from
the evidence tier: gate R2 requires that deleting derived files and rebuilding
reproduces the same semantic graph "from the same current evidence," and if
the graph derived from notes it would be a projection over a projection whose
inputs a rebuild might have deleted. `channels` still derives from `field_id`,
which signals carry. Every entity and relationship fixture must be
regenerated. A note may keep an `entities` list, which is a projection link
onto a projection — rebuildable, already excluded from `fn-record-v1`
comparison, and harmless.

**Extractions.** The most seriously affected, and the one place the proposal
creates a problem the owner has probably not seen. An extraction cites one
`source_note_id` and validates its spans against "the deterministic
normalized Markdown body of the cited current Note"
([enhancement](../enhancement.md)). If notes are rendered from templates,
then every span offset is an offset into template output, so **a template
revision silently invalidates every extraction span for that type** —
precisely the "stale body coordinates" case enhancement's validator is
required to reject. Extraction spans must therefore anchor to the **signal's**
normalized body, never to a rendered note. That requires a signal-citing
property in place of `source_note_id` (**registry review**), and it means the
`0.1.8` evidence coordinate system — whose unit, byte-versus-scalar, is
already flagged as an open question in `enhancement.md` — must be defined
over signal bodies. This is a genuine simplification once made: a signal body
is normalized source evidence with a frozen normalization contract, which is
a far better coordinate system than template output, and it is the coordinate
system enhancement was always describing before notes and signals were
separate things.

**Observations.** `supported_by` moves domain the same way, to signals and
extractions. Under section 7's recommendation, observations gain no
change-tracking role; under option 2 they would gain a durable one, which
section 7 rejects.

**Proposals.** Shape unchanged. Evidence citations move to signals. The
`entity_id` / `subject_identity` / `binding_status` machinery and the durable
private review intent under `.fieldnotes/state/proposals/` are untouched, and
A1 section 12's rule that "ordinary graph rebuild must not discard proposal
files or accepted/rejected review intent" now has a sibling rule for notes,
below.

**Packages.** The `pkg_` envelope stays reserved and the schema stays at the
`0.1.7` gate. One thing is worth recording now so the gate does not have to
rediscover it: a portable package must carry **signals** as its evidence and
may carry notes as a readability convenience, because a package containing
only rendered notes contains no evidence, and a package containing only
signals is unreadable without the tool.

**Conflicts.** Mostly a simplification. Conflicts arise from divergent current
state for one portable source key, which is now entirely a signal-tier event,
so `conflicts/<conf-id>/` holds signal candidates and
`candidate_fingerprints` are `fn-record-v1` fingerprints over signals —
unchanged in domain and computation, per section 2. A derived note never
conflicts, because it is rendered rather than collected. **The one exception
is an authored note**, which has no signal, is the only copy of its content,
and merges by note ID exactly as A1 section 8 rule 2 describes: same ID,
divergent content, always a conflict. So a conflict bundle carries two
candidate shapes, and the type registry needs a conflict type for each.

**The deletion rule that makes `notes/` safe.** Rebuild may delete and
regenerate a note **only** when that file parses, validates, and carries
`origin: derived`. A file it cannot prove is derived is left alone and
reported. This is the same shape as A1 section 12's protection for proposal
review state and ADR 0001's requirement that human decisions live separately
from rebuildable projections; the difference is that here the two populations
share a directory, so the check is per-file rather than per-directory and must
fail closed. It is also the single most dangerous line of code in this
proposal: a mis-scoped rebuild deletes the only copy of user-authored
material, and no source can refetch it.

#### Consequences

- Every derived-record fixture in the corpus changes: evidence citations move
  domain, and the entity, relationship, extraction, observation, proposal, and
  conflict fixtures must be regenerated rather than edited.
- `enhancement.md`'s evidence-validation section needs revision before
  `0.1.8`, and its open coordinate-unit question is now a question about
  signal bodies.
- `notes/` acquires a per-file safety check that no other directory needs.

## Compatibility and migration

A3 changes the notebook format. Under A1's change policy this is exactly the
case that "requires an explicit format version/migration proposal," and the
roadmap's scope controls say the same: "if implementation evidence
invalidates an approved contract, return to the relevant approval milestone
with an explicit migration and compatibility proposal." This document is that
proposal.

**What is invalidated.** Every directory name a Note lives in; every Note
filename; the identity of the record kind a derived record cites; the
`0.1.0` golden fixtures behind gate R0; the entity, relationship, extraction,
observation, proposal, and conflict fixtures; `fieldnotes.base` default views;
and the notebook the owner has already collected.

**What is not.** Every hash domain and every hash vector (section 2). The A1
canonical serializer, flat-YAML grammar, scalar spelling, datetime rules,
artifact identity, extension registry, merge survivor rule, and conflict
bundle layout. The whole of A2 and all three Microsoft Fields. Every
credential, cursor, and checkpoint mechanism.

**Migration is a re-sync, not a migrator.** The product treats deleting a
refetchable notebook as a supported lifecycle action, the roadmap states it as
an invariant, and A2's own artifact-retention default rests on it ("a notebook
is disposable working material, not a system of record — deleting and
refetching is a supported lifecycle action"). So the supported path is:
delete the notebook, `init`, `sync`. For the live notebook that is 328
records from three read-only Microsoft Fields, all refetchable.

An in-place migrator is *possible* and is deliberately not recommended.
Because a signal is byte-identical to today's Note except for its `id` prefix
and its path, and because `id` is excluded from `fn-record-v1` comparison and
absent from `fn-content-v1` input, a mechanical rewrite would change no hash.
It would still need to rewrite every cross-reference in every derived record,
and it would need its own test corpus and its own correctness argument — for
a population of one notebook that can be refetched in minutes. Specifying and
verifying the migrator costs more than re-syncing.

**Anything a user authored is the exception**, and it is the only thing a
re-sync cannot recover. Authored notes have no source. Migration guidance must
say plainly that authored material must be copied out first, and the CLI
should refuse to delete a notebook containing `origin: authored` notes without
an explicit acknowledgement.

**This is far cheaper now than later.** One notebook, one user, 328 records,
no published fixture consumers outside this repository, and no shipped
connector whose property vocabulary depends on the readable layer. Every one
of those numbers grows monotonically. The same argument ADR 0010 made about
fixing a crate dependency at `0.1.1` — "the last point at which this was
cheap" — applies here with a much larger multiplier.

**Where approved invariants are touched.** Recorded explicitly, because
several are:

| Approved statement | Where | Effect |
|---|---|---|
| "Notes and retained artifacts are the canonical representation" | roadmap invariants | Reversed as written: signals and artifacts are canonical; notes join the disposable class |
| "`notes/` is flat in v0.1" | notebook format | Reversed: `notes/` partitions by primary type |
| "The filename contains no subject, participant, organization, or other mutable human content" | notebook format | Reversed for notes; retained for signals |
| Structural keys first, then ascending ASCII | A1 section 5 | Replaced for notes only; unchanged everywhere else |
| Note filename grammar | A1 section 3 | Replaced for notes; retained for signals |
| "Note: the primary readable file produced from a Field or user input" | product, core concepts | Redefined; a note is rendered or authored, and a signal is collected |
| `fn-content-v1` applicability | A1 section 9 | Narrowed to signals and authored notes; no domain or vector changes |
| "at most one active Note for an exact portable source key" | A1 section 7 | Reads as "at most one active signal" |
| Rejection of deterministic/content-derived record IDs | A1 section 1 | Partially reversed for signal IDs, derived from the portable source key rather than from content |
| Four state classes, class 1 | ADR 0001 | Restated: evidence is signals, artifacts, and authored notes; derived notes join the disposable projections |
| "deleting `extractions/` and `observations/` leaves Notes byte-for-byte unchanged" | gate R8 | Restated over signals and authored notes; strengthened, not weakened |
| "deleting all caches and derived graph files reproduces the same semantic graph" | gate R2 | Unchanged, and it is the gate that rejects change-tracking option 2 |
| "notebooks remain useful offline and without Fieldnotes installed" | gate R9 | Unchanged under the recommendation; would weaken under JSON signals (section 2) |
| "no release ... may require a model ... for notebook use" | roadmap invariants | Unchanged, and it is why deterministic rendering is non-negotiable |
| "Fieldnotes represents the current collected state, not an append-only history ledger" | roadmap invariants, ADR 0001, A1 section 7 | Untouched under section 7's recommendation; reversed under option 1 |
| Golden fixtures are stable | gate R0 | Invalidated; the corpus must be regenerated |
| The whole of A2 | A2 | **Untouched** |

**New shared property names proposed, none settled.** Every name below
requires registry review with fixtures before use, per A1's change policy and
the roadmap's control that "no release may introduce unapproved shared
property names opportunistically":

`origin`, `signal_id`, `template_id`, `template_version`, `title_slug`,
`last_extraction_at`, `last_observation_at`, a signal-citing replacement for
`source_note_id`, and whatever a contact template turns out to need
(`organization` and `role` are illustrative only). Also requiring registry
review: the `sig_` record-kind prefix, the ASCII folding table for slug
derivation, the signal-ID derivation and truncation width, conflict types for
signal candidates and authored-note candidates, and the still-open
`contact_kind`-equivalent from
[A1 graph implementation findings, finding 8](A1-graph-implementation-findings.md).

## Fixture evidence required for implementation

A3 approval should freeze bytes, the way A1 and A2 did. The corpus must cover
at least:

- one signal per primary Note type, in the A1 byte form, at its new path,
  demonstrating that the evidence tier is byte-compatible with the existing
  corpus apart from `id` and path;
- one rendered note per primary Note type, from its own template, with its
  template's declared property order, its body sections, and its required
  signals section;
- the same signal rendered by two template versions, proving that a template
  revision changes note bytes and no signal byte, no hash, and no filename;
- an authored note, with `origin: authored`, no `signal_id`, and a name that
  does not change when anything is rebuilt;
- a derived note whose signal was removed by authoritative deletion, proving
  rebuild removes the note;
- an authored note in the same directory as derived notes, with a rebuild
  transcript proving the authored file is untouched — and a negative case
  where a malformed file with an unreadable `origin` is left alone and
  reported rather than deleted;
- slug vectors: an ASCII title; a title needing NFKD folding
  (`Alice Müller` → `alice-muller`); a title needing the folding table; a
  title in a script the table does not cover, yielding an omitted slug; an
  absent slug source; a title that folds to a reserved Windows device name; a
  title exceeding the 48-byte bound, truncated at a `-` boundary; two notes
  whose slugs differ only by case; a title whose slug source changes upstream,
  proving the stored slug and the filename do not;
- filename vectors for both grammars, event-like and entity-like, with and
  without a slug, each validated as a whole name computed from frontmatter;
- a `--` delimiter case proving a reader can split the name, over a Field ID
  containing underscores (`outlook_contacts_work`);
- `fn-content-v1` and `fn-record-v1` vectors reproduced unchanged from
  `tests/fixtures/hashes/proposed-v1/` against the renamed signals, which is
  the evidence that section 2's recommendation costs no hash change;
- enrichment-timestamp cases: a note with extractions present, the same note
  after `extractions/` is deleted, and the same note re-rendered;
- regenerated entity, relationship, extraction, observation, proposal, and
  conflict fixtures with signal-domain evidence citations;
- an extraction whose spans are validated against a signal body, plus a
  negative case proving a span validated against rendered note bytes is
  rejected;
- a conflict bundle with signal candidates, and a second with authored-note
  candidates;
- a coverage case: several signals, some with notes, proving coverage is
  computed from notes and that no signal byte records it;
- rejection cases: a note with `origin: derived` and no `signal_id`; a note
  with `origin: authored` and a `signal_id`; a note citing a signal that does
  not exist; a signal carrying a note-only property; a note carrying
  `content_hash`.

Every fixture must state whether it is normative at A3 or illustrative for a
later gate, in the style the A1 and A2 corpus READMEs already use. Hash
vectors must state exact input bytes reproducibly.

## Explicit approval checklist

**Nothing below is approved.** Each box records a choice the owner has not
yet made.

### Model and vocabulary

- [ ] Three tiers — signal, note, enrichment — with "Note" in every
  approved document reading as "signal" until that document is amended.
- [ ] Deterministic rendering with no model, network, or GPU, as a rule rather
  than a default.
- [ ] A derived note carries no durable state, as a rule that governs every
  later addition to the note model.
- [ ] `signals/` and derived `notes/` join ADR 0001's state classes as
  evidence and disposable projection respectively, with authored notes as
  evidence.

### Signals

- [ ] A signal keeps A1's approved flat-YAML-plus-Markdown byte form, and
  canonical JSON signals are deferred to a package that also amends A2 to
  admit nested structure.
- [ ] `sig_` as a new record-kind prefix, and `signals/<field-id>/` with A1
  section 3's filename grammar unchanged.
- [ ] Signal IDs derived deterministically from the portable source key, with
  `self` signals keeping UUIDv7 — reversing A1 section 1's rejection of
  content-derived IDs on the narrow ground that the derivation input is an
  immutable identity rather than mutable content.
- [ ] `fn-content-v1` and `fn-record-v1` unchanged in domain, input, and
  vectors, with `fn-content-v1` narrowed in applicability to signals and
  authored notes.

### Notes

- [ ] A required `origin` property with the closed vocabulary
  `authored derived`.
- [ ] `signal_id` as a single scalar link on a derived note, with
  multi-signal notes deliberately out of scope.
- [ ] Enrichment timestamps as projections recomputed at render time, never
  stored facts, and gate R8 restated over signals and authored notes.
- [ ] Coverage derived by scanning notes, with nothing written into a signal.

### Templates

- [ ] Eleven type-specific templates, each declaring a property set, a body
  layout, a slug source, and a required signals section.
- [ ] A template is reviewable but not frozen: a revision is a rebuild, not a
  format version bump, which holds only because notes are unhashed,
  uncompared, and carry a stored slug.
- [ ] Templates are release-owned, with no user-editable template surface in
  v0.1.

### Filenames, layout, and ordering

- [ ] `notes/<type>/` partitioning, reversing "`notes/` is flat in v0.1" and
  giving up the single global timeline listing.
- [ ] `--` as a delimiter that cannot occur inside a segment, so a reader may
  split the name.
- [ ] A type-specific readable part: a UTC timestamp for event-like types, and
  no timestamp for `contact`.
- [ ] `title_slug` as a stored property, so the filename stays computable from
  frontmatter and names do not churn on upstream edits.
- [ ] Slug omission as always available and never a failure, with the frozen
  folding table, the reserved-device-name exclusion, and the 48-byte bound.
- [ ] Uniqueness carried entirely by the record ID, with no disambiguator.
- [ ] Human content in a note filename, reversing notebook format's rule for
  notes while retaining it for signals.
- [ ] Template-declared reading order for notes only, with A1 section 5
  unchanged for every other record kind.

### Change tracking

- [ ] **Option 3 — current state only** — for this gate, keeping the
  invariant.
- [ ] Option 2 rejected outright, because a change observation is durable
  state in a disposable directory and defeats gate R2.
- [ ] Option 1 named as the only acceptable route to change tracking, taken as
  its own approval package with its own migration.
- [ ] A contact template renders evidence *disagreement* between current
  sources rather than change over time, with vocabulary that says so.

### Other record kinds

- [ ] Entity, relationship, observation, and proposal evidence citations move
  from Note IDs to signal IDs.
- [ ] Extraction spans anchor to signal bodies, never to rendered note bytes,
  with `enhancement.md`'s coordinate system defined over signals.
- [ ] Conflicts are a signal-tier event, with authored notes as the one
  note-tier exception.
- [ ] A handback package carries signals as evidence, with the schema still
  at `0.1.7`.
- [ ] Rebuild deletes a note only when it parses, validates, and proves
  `origin: derived`, failing closed otherwise.

### Migration and policy

- [ ] Migration is a re-sync, with the in-place migrator explicitly declined.
- [ ] Authored material must be copied out before a notebook is deleted, and
  the CLI must refuse without acknowledgement.
- [ ] Every new shared property name, the `sig_` prefix, the folding table,
  the signal-ID derivation, and the new conflict types go through registry
  review with fixtures before use; none is approved by this checklist.
- [ ] The fixture corpus above is required A3 evidence, and the gate is not
  closed until it exists.

## Approval effect

Approval would unblock: a signal store reusing the existing A1 serializer and
hashes; the render layer and eleven templates; the new filename and directory
grammar; registry review for every new name the compatibility section lists;
regeneration of the notebook fixture corpus; and a restatement of the
roadmap invariants, ADR 0001's state classes, and gates R0, R8, and R9.

It would not approve: any change to A2 or to any Field; nested signal
structure or JSON serialization; change tracking in any form; a
user-editable template surface; the `0.1.7` handback schema; the `0.1.8`
enhancement capabilities or their coordinate unit; any new shared property
name, which still requires registry review; or the CLI surface for rebuild
and rename.

## Questions the reviewer must settle

These four are genuinely open. The recommendation above takes a position on
each and the owner may prefer a different one; the rest of the package works
either way.

1. **Change tracking.** Option 1, 2, or 3 (section 7). The recommendation is
   3 for this gate, 1 as a separate package if wanted, and 2 rejected. This
   is the only question that touches a product invariant, and it is the one
   the owner alone can answer.
2. **Signal serialization.** Flat YAML plus Markdown, as recommended, or
   canonical JSON (section 2). Choosing JSON means either accepting a second
   serialization for no capability gain, or bundling an A2 amendment that
   admits nested vendor structure — which this package is required not to
   propose. If the owner wants nested contact data, that is the package to
   ask for, and A3 should probably wait for it.
3. **`notes/` partitioning.** By primary type, as recommended, or flat with
   slugs only. Flat preserves the global-timeline listing and A1's approved
   statement, and leaves 248 contacts in one directory with everything else.
4. **Slug stability.** Stored once, as recommended, or recomputed on every
   render. Recomputing keeps names matching current content and costs a
   frozen folding table on the compatibility path plus mass renames on
   upstream edits; storing costs one new shared property.

Two smaller choices are recorded here rather than buried: whether the reading
order in section 5 should be the full template-declared order or merely
"`title` first, then A1's existing rule"; and whether authored and derived
notes share `notes/` (recommended, with the per-file safety check) or live in
separate directories (safer, less coherent for a reader).
