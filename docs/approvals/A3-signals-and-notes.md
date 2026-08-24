# A3 approval: signals, notes, and type-specific rendering

**Status:** Ready for review. **Not approved.** No box in the approval
checklist is checked, and nothing in this document may be implemented until
the owner approves it explicitly. The owner has settled the decisions
recorded in "Settled by the owner" below. Section 13 is the handoff: if
this package is approved, a new session should be able to write an
implementation plan from this document plus A0–A2, without the chat that
produced it.  
**Scope:** The information architecture of the notebook's readable layer;
the collect path; Contacts as vCard working data; in-note extractions and
observations; the local-LLM policy; and the engine-evaluation work the
enhancement path must do before pinning tools or models.

## Decision requested

A1 froze a public notebook contract in which one record kind — the Note —
is simultaneously the machine record of what a source said, the human-readable
artifact a person opens, and the evidence every derived record cites. A
notebook collected from live Microsoft 365 data (328 records: 248 contacts,
50 mail, 30 calendar events) demonstrates that one record cannot be all
three. The defects are recorded with evidence below.

A3 proposes splitting that single record kind in two:

- a **signal** is the collected machine record of what a source actually
  said, reconciled to current state, and the evidence every derived record
  cites;
- a **note** is what a person opens: either rendered from a signal by a
  type-specific template, or authored directly by the user.

**A note must never be the raw dump of what a Field sent.** That is the whole
point of the split, and every other decision in this document is downstream
of it. Where a note ends up on disk, and what its filename looks like, are
consequences and are treated as consequences: they come after the model, not
before it.

This is a format change. It invalidates the A1 fixture corpus, the
`0.1.0` golden fixtures behind release gate R0, and the notebook this
repository's owner has already collected. The recommendation is to make it
now: the product explicitly treats deleting a refetchable notebook as a
supported lifecycle action ([product](../product.md), the roadmap's third
invariant), so migration is a re-sync rather than a migrator, and 328
records in one notebook is the cheapest this change will ever be.

**A3 does not change A2 record, checkpoint, or credential frames.** It adds
an optional `describe.note_sections` member. See "What A3 must not do".

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
reviewed against attached bytes, and A3 should be too. The corpus this gate
needs is now deliberately small — five cases, listed in "Review corpus
required at this gate" — because the model is what is being reviewed, and a
model is reviewable from a handful of files.

## Settled by the owner

These are decided. They are recorded here so the rest of the document can
build on them, and they are not re-offered in the approval checklist.

1. **Change tracking: current state only.** The approved invariant stands.
   Disagreement between *concurrent* current sources may surface on a note;
   "the job title changed" waits. Change observations recorded at detection
   time are **rejected outright**. Versioned signals are a possible later
   package and are not part of A3. Section 8 records all three positions and
   why.
2. **Signal serialization: A1's flat YAML plus Markdown, unchanged.**
   Canonical JSON is declined for this gate. There is no A2 amendment in this
   package and no nested vendor structure anywhere in it. Section 2 records
   the reasoning, because it is worth keeping.
3. **Layout: public `notes/` stays flat; signals are private.** Authored and
   derived notes live together in one flat `notes/`. Type partitioning is
   dropped. Signals do **not** get a second public directory. They live under
   `.fieldnotes/signals/<field-id>/` so they travel with the vault, stay out
   of the reading surface, and can be partitioned by Field without a public
   format change. Each Field may also keep connector working data under
   `.fieldnotes/fields/<field-id>/`. Section 7 states the tree.
4. **The note-to-signal model.** One derived note per signal in v0.1,
   expressed as a `signals` list whose length is exactly one, with the
   invariant that a given signal identifier appears on at most one note.
   Section 3 states it. A later multi-signal note would renegotiate that
   uniqueness invariant, not the property's type.
5. **The persistent signal-to-note index is required, not deferred.** A
   one-shot scan of `notes/` per run is not an acceptable steady-state design
   for a mailbox-scale working set. Section 4 specifies it. `inspect` grows a
   fast mode that trusts the index and says so; a full scan remains available
   and is not the default at mailbox scale.
6. **Collect does not rewrite a note that already exists for that signal.**
   Coverage is "did we already emit a note for this signal?", answered by the
   index and verified against the file. Default: skip. `--force` (or
   equivalent) rewrites the markdown from the current signal and **keeps the
   stored slug**. Fieldnotes is not responsible for duplicates in whatever
   system the user copies notes into. If the user deleted the note, a later
   collect may emit a new one; that is the user's vault, not a de-dupe
   contract with a second brain.
7. **Slug is a core observation, not a Field add-in.** Same observation
   machinery (JSON, generator stamp, engine cascade), prompt owned by
   **core**. Fields do not declare a slug section. First emit still
   writes a deterministic fold into `title_slug` and the filename so the
   note exists with no model. After extraction, the core slug observation
   may replace `title_slug` with a valid proposal and rename **only
   inside that first-emit transaction** (the fold name was never a
   published stable path). Later collect and `--force` keep the stored
   slug. Grammar reject or engine off → fold stands. Keep A1's `_`
   delimiter.
8. **Reading order: `title` first, then A1's existing rule.**
   Template-declared order waits for the template package; moving later is a
   rebuild, not a migration.
9. **Signal IDs are derived from the portable source key, at 96 bits
   (24 hex characters), prefixed `sig_`.** `self` signals keep UUIDv7. This
   reverses A1 section 1's rejection of content-derived IDs on the narrow
   ground that the input is an immutable identity rather than mutable
   content.
10. **Contacts are not notes.** The 248 contact markdown files were product
    drift. The Contacts Field does not render `notes/contact_*.md`. It
    stages vCard 4 (point 17). "Is this a new person?" is a graph question
    over `UID` / `EMAIL` / `TEL` plus identity anchors on mail, calendar,
    and chat signals — the resolver in
    [identity-and-graph.md](../identity-and-graph.md). A public note about a
    person is emitted only when something happens worth writing down (a new
    unmatched identity, an extracted property that is new in this notebook),
    not because a directory record exists.
11. **This gate's review corpus is mail plus authored text**, not contact.
    Contact-as-note fixtures would freeze the drift.
12. **Cut from this gate:** `template_id`, `last_extraction_at`, and
    `last_observation_at`. `origin`, `signals`, and `title_slug` stay.
    `template_version` waits with the template package.
13. **Pipeline: deterministic note, then extraction, then observations.**
    First emit always writes a complete, model-free note (fold slug,
    extraction *slots*, empty observation headings, `## Signals` last).
    Then mechanical masked **extraction** fills those slots from the
    signal. Then **observations** run: core's slug observation first,
    then Field-declared observation sections (and the one-line image
    caption). Fields supply prompts only for *their* observation
    sections via `describe.note_sections`. Core still writes every byte.
    Collect is valid if extraction or observations never run.
14. **Engine cascade: Apple on-device if present, else optional llama,
    else fold.** Collect never fails for want of a model.
    1. **macOS + Apple Intelligence available** → Apple Foundation Models
       (`SystemLanguageModel`, on-device only). No download, no GGUF, runs
       on the Neural Engine. This is the fastest path to slug + observation
       fill on the machine this project is developed on.
    2. **Otherwise, if a llama.cpp (or equivalent) engine is installed** →
       use that. This is the Win/Linux *parity* path and the fallback on a
       Mac where Apple Intelligence is off, ineligible, or not ready. It is
       **not** required to ship the first Mac-useful notes.
    3. **Else** → deterministic fold for the slug, mechanical extractions,
       empty observation headings. This is the Windows/Linux default until
       (2) exists, and it is what CI exercises.

    Detect availability at runtime; never assume. **Private Cloud Compute
    is not the default** (network). Guardrail refusal or a missing model
    is a skip of LLM fill, not a failed collect. Compile and CI stay free
    of model downloads and GPU. The hard "no BYO" prohibition is
    withdrawn; BYO is still not built here. A config flag can force fold.
15. **Extractions are mechanical and masked. Observations are LLM or
    human, including slug.** Extraction may only copy signal properties
    or literal spans. Observations use the engine cascade: core owns
    slug (and picture caption); the Field owns type-specific sections
    (ask, etc.). Empty headings if the engine is off. Human text (no
    generator stamp) is not overwritten by `--force`. `--force` may
    rerun Field observation sections and caption; it does **not** rerun
    slug.
16. **Work queue by default, keep-everything without a second layout.**
    Skip-unless-`--force` is the queue. Keeping everything is "don't
    delete the notes." No extra directory scheme, no revision ledger, no
    processed-set beyond the index.
17. **Contacts working data is a vCard 4 subset** (RFC 6350), one `.vcf`
    per contact under `.fieldnotes/fields/<field-id>/contacts/`. Chosen as
    a standard for repeating, typed EMAIL/TEL, not as a PIM. Core is still
    the only durable writer: the Field stages `text/vcard`, core validates
    the subset and installs. The graph matches on `UID`, `EMAIL`, and
    `TEL`.
18. **Next shipped LLM jobs: core slug observation, and a one-line
    picture caption.** Both are observations after extraction. Document
    OCR is a converter (MarkItDown or Firecrawl AnyDoc), not the model.
    Caption eval: 13.6 E. Field observation-section scoring still waits
    on `describe` prompts.
19. **PII is postponed. Presidio is out.** Presidio needs Python and spaCy
    model assets, which this workspace will not take onto any path that
    collect can hit. Do not evaluate it further. A later optional
    capability may look at non-Python span taggers; it is not this gate
    and it is not required for v0.1 notes.

A3 still does not change A2 *record, checkpoint, or credential* frames. It
does change the Contacts Field's product role, and it adds an optional
`note_sections` declaration to `describe`. That is in scope because the
live notebook that motivated this package was wrong about contacts, and
because Field-provided section prompts cannot live in core without a
place to declare them.

## What A3 must not do

A3 changes the storage and rendering layer, the Contacts Field's product
role (settled point 10), and — additively — the `describe` manifest so a
Field can declare note sections and observation prompts (settled point 13).
**A2 record, checkpoint, diagnostic, credential, and artifact-handle
grammar do not otherwise move.** The Contacts Field still speaks A2. Core
is still the only durable writer. Core stops turning contact records into
public notes.

[A2 section 6](A2-field-protocol.md#6-the-record-envelope-a-normalized-source-envelope)
chose the normalized source envelope precisely so that a Field emits values
and never spells bytes. It states that core owns "the canonical key order;
the canonical scalar spelling; the filename; and every durable write," and
that a record "is never a rendered Note, never carries a notebook path or
filename." A signal is core's durable storage of an A2 `record`; a note is
core's rendering. Neither is visible to a Field.

Concretely, nothing in A3 requires a change to: any A2 frame type, member,
grammar, limit, ordering rule, cursor rule, checkpoint rule, exit code,
rejection code, diagnostic code, or schema file; the `describe` manifest
shape; declared-property enforcement; artifact staging or the handle grammar;
the protected credential channel; or the sixteen frozen transcripts. A
note-producing Field's capability slice still declares a `note_type` and
core still enforces it — that value now selects a render template as well as
a record type. A matching-only Field (Contacts, settled point 10) still
declares its types for the protocol; core does not render them into
`notes/`.

One consequence runs the other way and is stated in section 10 rather than
hidden: because A2's `record.properties` admits only scalars and homogeneous
scalar lists
(`common.schema.json#/$defs/propertyValue` is `oneOf[scalarValue,
scalarList]`), a signal cannot contain nested structure a Field did not send,
and a Field cannot send nested structure under protocol v1. Any proposal to
give signals nested vendor structure is therefore an A2 amendment, not an A3
decision, and A3 does not make one. That is also why P7 is not solved here.

## The problems this must solve

Each defect below is named with evidence from the live notebook or from A1's
own prose. The numbering is stable from the first draft of this package; the
grouping is new, and the grouping is the point. The structural defects are
the reason the split exists. The presentation defects are real, and two of
them this package deliberately does not fix.

Where each one lands:

| Defect | Fixed here? |
|---|---|
| P7 properties a reader wants are not properties | **No.** Needs an A2 amendment; section 10 |
| P3 machine bookkeeping precedes `title` | Yes; section 6 |
| P6 property names are machine-oriented | Yes for notes; the names stay on signals |
| P1 the filename is unreadable | Partly; a slug leads the name, the ID still dominates |
| P5 contacts sort by a meaningless date | **Moot.** Contacts are not notes; settled point 10 |
| P4 the filename separator is ambiguous | **No.** `_` is kept; P4 stays documented |
| P2 one flat directory of near-identical names | **Partly.** Public `notes/` stays flat; signals leave the reading surface. A mailbox of kept mail notes is still a search problem, not a folder problem |

### Structural defects: the record does the wrong job

#### P7. The properties a reader wants are not properties

A live contact's most useful values are not in frontmatter at all. Of 248
contact records, 245 carry an address as **Markdown body prose**, and 232
carry a phone number the same way, alongside an organization on 135 and a
role on 109. There is no `email` property, no `phone` property, and no
address property on any of them.

Two mechanisms carry fragments of that information and neither is adequate.
All 248 carry `identities` anchors with `email:` and `phone:` prefixes, but
`identities` is a set-like list that A1 sorts and deduplicates by normalized
text, so it cannot distinguish a business phone from a mobile, or a primary
address from an alias — the role is erased by the list semantics. The
Markdown body carries the human-readable version, but as text, so it is not
queryable, not typed, and not addressable by `fieldnotes.base` or any
frontmatter-aware tool.

This is the flat-frontmatter rule working as designed and producing a bad
outcome for one record type. **The split does not fix it**, and section 10
says so plainly rather than leaving the reader to infer otherwise.

#### P3. Machine bookkeeping literally precedes the one field a reader wants

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

#### P6. Property names are machine-oriented throughout

`source_version`, `content_hash`, `captured_at`, `collected_by`,
`outlook_mail_internet_message_id`, `outlook_calendar_response_status`. These
are correct names for what they are and they are the majority of what a
reader sees. A live contact carries thirteen properties, of which five are
Fieldnotes bookkeeping, four are source identity or versioning, one is a
vendor-prefixed classification, and one — `title` — is something a person
asked for.

### Presentation defects: naming and layout

#### P1. A readable file whose name is unreadable

A contact's filename is:

```text
20250512T201356Z_outlook_contacts_work_contact_note_<uuid>.md
```

91 characters. The person's name appears nowhere in it. 91 bytes of a
readable file's name carry a timestamp, a Field ID, a record type, a record
kind prefix, and a UUID — every one of which is also present in the
frontmatter, which A1 section 3 already declares authoritative.

#### P2. 248 files in one flat directory

`notes/` is flat in v0.1 ([notebook format](../notebook-format.md)). The live
notebook's `notes/` holds 328 files, 248 of them contacts, and every one of
those 248 is named by the pattern in P1. A directory listing is unusable as a
directory listing: nothing distinguishes one entry from the next except a
timestamp and a UUID.

The owner has decided that `notes/` stays flat. So P2 is answered only by the
readable part of the filename, not by the directory, and at mailbox scale it
gets worse rather than better. Section 7 records that consequence honestly
instead of claiming the package fixes it.

#### P4. The filename separator is ambiguous by construction

A1 section 3 states: "Readers must not naively split the underscore-delimited
filename, because registered Field stems and labels may contain
underscores." That is not a caveat about a hostile input; it is an admission
that the grammar is ambiguous for the corpus that already exists.
`outlook_contacts_work_contact` contains four underscores and decomposes into
a two-part stem (`outlook_contacts`), a label (`work`), and a type
(`contact`) only with the registry in hand.

#### P5. Contacts sort by a date that means nothing

Note filenames render `occurred_at` in UTC (A1 section 3). For a contact,
`occurred_at` is the source object's last-modified instant, because a contact
has no event time — a contact is not an event. In the live notebook the 248
contacts' filename timestamps span 2025-05-12 to 2026-07-27, an interval that
describes when a directory-sync last touched each record and nothing a reader
would ever want to sort by. The one ordering the flat directory provides is
therefore meaningless for 76% of its contents.

## Recommended restructure

### 1. The split, and the vocabulary

Approve three tiers with three names, and use the names consistently
everywhere afterwards:

| Tier | Record | What it is | Lifecycle |
|---|---|---|---|
| Evidence | **signal** | The collected machine record of what a source actually said | Reconciled, replaced, or removed by collection; private; refetchable |
| Readable | **note** | A human-readable artifact: deterministic scaffold, then optional fill | Snapshot until `--force`; user may keep, move, or delete |
| Working data | **vCard** (Contacts), other Field-private files | Matching ammunition, not a reading surface | Current state, private, refetchable |

The vocabulary is the load-bearing part. A1, A2, ADR 0001 through ADR 0013,
the property registry, and the roadmap use "Note" in the old sense several
hundred times, and A3 does **not** propose that those occurrences silently
come to mean "signal." Section 12 carries exact replacement wording for the
four load-bearing statements, and the rest of the sweep is scoped work in the
implementation gate rather than a standing equivalence rule. An unwritten
rename is how a vocabulary change becomes permanently ambiguous.

Two consequences of the tiering are load-bearing and must be stated as rules
rather than left as implications.

**Deterministic scaffold first, small local LLM on by default.** A freshly
emitted note is always readable without a model: slug, title, extraction
sections copied from the signal, empty observation headings, `## Signals`
last. When the pinned local engine is present and not disabled, it fills
observation sections. When it is missing or off, collect still succeeds.
The roadmap line "no release before the enhancement milestone may require a
model" is **restated**: no release may *fail* because a model is absent;
the default product experience after the engine is installed is that a
small local LLM is on. Compile and CI remain free of model downloads and
GPU. Bring-your-own inference is not shipped here and is no longer a
hard product "No."

**A derived note is a snapshot, not a pure function.** `title_slug` is
durable. Human-written observation sections (no generator stamp) are
durable. `--force` regenerates mechanical extraction sections and
LLM-stamped observation sections, and keeps the slug and any human
observation text. That is the work-queue rule, not a projection that is
safe to delete and recreate byte-identical.

#### Alternatives considered

- **Keep one record kind and fix its presentation.** Reorder frontmatter, add
  a slug to the filename, and leave the Note as the single canonical record.
  This is much cheaper and it fixes P1, P3, P5, and P6. It does not fix P7
  either, and more importantly it leaves every presentation decision
  permanently coupled to the evidence contract: a better contact layout
  becomes a change to hashed, merged, conflict-bearing canonical bytes, so it
  needs a format version every time. The split is worth its cost primarily
  because it decouples "how this reads" from "what was collected."
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

- The notebook has two file populations with different durability promises in
  one directory, and a user cannot tell them apart by looking. Section 3's
  `origin` property exists to fix that, and the deletion rule in section 11
  exists because getting it wrong destroys user-authored material.
- Every derived record's evidence citation moves domain, from a Note ID to a
  signal ID. That is a mechanical change with a large surface; see section 11.
- The roadmap's first invariant — "Notes and retained artifacts are the
  canonical representation" — is **reversed as written and preserved in
  substance**: signals, authored notes, and retained artifacts are canonical,
  and derived notes join the disposable class. Section 12 restates it in
  exact words.

### 2. What a signal is, and how it serializes

A signal is what an A1 Note is today. Same required properties (`id`,
`instance_id`, `field_id`, `type`, `occurred_at`), same portable source key
`(source_scope, source_identity)`, same bookkeeping (`captured_at`,
`collected_by`, `content_hash`, `source_version`), same deterministically
normalized Markdown body of source evidence, same artifact references, same
current-state reconciliation, same merge survivor rule, same conflict
behavior.

**Settled: a signal keeps A1's approved byte form exactly** — flat YAML
frontmatter in the A1 subset, one blank line, a normalized Markdown body,
UTF-8, LF, one final LF. It changes its record kind prefix, its directory,
and nothing else. Canonical JSON is declined for this gate, and no nested
vendor structure appears anywhere in this package.

The reasoning is kept because the JSON question is the one place in this
package where the intuition and the mechanics point in different directions,
and a later package will ask it again.

#### Why canonical JSON was declined

The case for JSON is real. JSON has a canonical form — RFC 8785, the JSON
Canonicalization Scheme — where YAML has none, which is exactly why this
repository needed a hand-written parser:
`crates/fieldnotes-format/src/yaml.rs` is a 397-line scanner whose own module
documentation describes it as "a hand-written parser for the byte grammar A1
defines." And the JCS machinery is already here:
`crates/fieldnotes-format/src/jcs.rs` implements RFC 8785 section 3.2.2.3
number spelling and section 3.2.2.2 string serialization, plus a decoder, in
314 lines, because A1 section 5 already borrowed both rules for YAML scalars.
A1 did not avoid canonical JSON; it reimplemented half of it inside a YAML
grammar it then had to specify byte by byte. Under JCS, one of ADR 0006
ruling 2's two documented `fn-record-v1` deviations —
"unconditional ascending-key order with no structural-keys-first exception" —
would stop being a deviation at all, because lexicographic key order is what
JCS *is*.

The case against turns on two things.

**First, the YAML subset does not go away, so JSON is an addition rather than
a replacement.** Notes are Markdown with flat YAML frontmatter — that is the
whole point of the readable tier — and so is every derived record:
extractions, observations, entities, relationships, proposals, conflict
bundles, package manifests. If signals become JSON, this repository owns two
canonical serializations, two validators, two rejection-code families, and
two fingerprint domains where it now owns one. `yaml.rs`, `emit.rs`,
`record.rs`, and `normalize.rs` all survive unchanged for the readable tier.
JCS also does not carry the rules that actually cost effort to specify: which
keys are admissible, that a datetime must carry an explicit offset, that a
list is homogeneous, that an integer outside the exactly representable
binary64 range is invalid rather than rounded. Every one of those has to be
restated for JSON, and every one is already implemented and tested for YAML.

**Second — and this is decisive — the one thing JSON can express that flat
YAML cannot is nesting, and a signal cannot be given nested structure under
protocol v1.** P7 is the strongest argument for JSON in this whole package,
and A2's `record.properties` admits only `scalarValue` or `scalarList`
(`common.schema.json#/$defs/propertyValue`), with each declared property
typed as one of A1's five scalars at `scalar` or `list` cardinality
(A2 section 4). A Field cannot send a nested value, so core cannot store one.
Adopting JSON without amending A2 buys a re-spelling of exactly the same flat
data.

There is also a cost worth naming. Today portability is *provable*: the
canonical artifact is the file a human opens, so R9's criterion —
"notebooks remain useful offline and without Fieldnotes installed" — is
demonstrated by opening a notebook in Obsidian. With JSON signals the claim
becomes "any tool can parse the signal, and the readable layer rebuilds
deterministically," which is weaker in a specific way: a notebook whose
`notes/` was deleted, or whose notes were never rendered, would contain its
evidence in a form no ordinary Markdown or frontmatter tool can read, and
restoring readability would require the Fieldnotes executable and the correct
template version.

That last point survives the decision and deserves care, because the split
weakens it slightly even with Markdown signals. Once the canonical artifact
is not the one a human opens, "portable" means *both* files are readable
rather than *the* file being readable. Under this recommendation both tiers
are Markdown with flat frontmatter, so a signal is legible in Obsidian even
though it was not designed for a reader, and the R9 claim survives largely
intact. It is not identical to the claim A1 could make, and this package
should not pretend it is.

Canonical JSON signals with nested vendor structure remain a coherent and
probably correct future step, and they are an A2-amending package: a nested
record envelope, per-Field declaration of nested property shapes, a nested
value model in the A1 registry, and a new signal serialization, reviewed
together. Splitting that into "JSON now, nesting later" spends the migration
cost twice and delivers the benefit neither time.

#### What this does to `fn-content-v1` and `fn-record-v1`

**Nothing.** This is the largest single saving in the package.

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
  and conflict detection happen entirely in the evidence tier. Section 11
  records the one exception: an authored note, which has no signal, keeps a
  Note-like merge identity.

#### Signal identity and location

Approve a new record kind. This requires registry review under A1's change
policy, which "new shared properties, primary types, prefixes, or record
kinds require registry review and fixtures" already covers.

- **Prefix:** `sig_`, joining the table in A1 section 1.
- **Location:** `.fieldnotes/signals/<field-id>/`, private, partitioned by
  Field. Per-Field subdirectories are available here because this is not a
  public directory; changing that partition later is not a notebook-format
  migration. Settled point 3.
- **Filename:** A1 section 3's grammar, unchanged, with `sig_` in place of
  `note_`:
  `<YYYYMMDDTHHMMSSZ>_<field-id>_<type>_<signal-id>.md`. The machine-oriented
  filename is correct for a machine record. P1 and P5 stop being defects the
  moment the file is not the one a person opens.

**Settled: derive a signal's ID deterministically from its portable source
key, at 96 bits.** The lowercase hex of SHA-256 over a domain-separated
encoding of `(source_scope, source_identity)`, truncated to 24 hex
characters, prefixed `sig_`, for every signal that has such a key. A `self`
signal, which has no portable source key, keeps a UUIDv7. The examples in
this document that still show 16 hex characters are stale and must be
regenerated at 24.

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
present a stale projection ID) is needed for the note-to-signal link. It also
makes the index in section 4 cheaper to rebuild and impossible to
mis-associate: the key of the map is a pure function of the source key.

The cost is a third ID family. 64 bits at 10^5 signals leaves roughly 2^32
of birthday headroom, which is safe and looks thin in five years. 96 bits
costs eight filename bytes and closes the question. That is why 96 was
taken.

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
  a note's `signals` list carries signal IDs and that the signal remains the
  only place the full pair is written.
- **Keep `note_` as the signal prefix and give the readable record a new
  prefix.** Migration becomes a directory move with no ID rewrite. Rejected:
  it permanently inverts the vocabulary this section exists to fix, and the
  ID rewrite is free anyway, because `id` is excluded from `fn-record-v1`
  comparison and absent from `fn-content-v1` input, so re-prefixing changes
  no hash.

#### Consequences

- The A1 serializer, validator, hash implementation, merge rule, and conflict
  machinery are reused verbatim for the evidence tier. The implementation
  cost of A3 is concentrated in the render layer and the index, which are new
  code rather than changed code.
- `signals/` is machine-oriented and stays that way. No promise is made about
  its filenames being meaningful to a person, and P1 and P5 are answered
  there by saying so rather than by fixing the names.
- A notebook contains the same evidence twice in different shapes — once in a
  signal, once rendered into a note. **Storage roughly doubles for
  text-bearing records.** For 328 records this is irrelevant; it is worth
  stating because it scales linearly, and at mailbox scale it is the
  difference between one copy of a mail corpus and two.

### 3. The note model: two origins, a `signals` list, and one invariant

A note is a readable Markdown record with flat YAML frontmatter in the A1
subset. It has **two origins**, and the model must say which:

- **`derived`** — rendered from exactly one signal by a type-specific
  template. Fieldnotes output the user may keep, move, or delete. Collect
  does not rewrite it unless `--force`. Safe to delete from Fieldnotes'
  point of view; resurrection on a later collect is allowed and is not a
  de-dupe contract with any destination.
- **`authored`** — created directly by the user. `fieldnotes note "call Alice
  back"` produces a note, not a signal, and it is the only copy of that
  material. Never removed by collect, `--force`, or rebuild.

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

#### The link is a list, and its length is one

**Settled: the note-to-signal link is a `signals` list.** A derived note
carries `origin: derived` and `signals` with **exactly one** signal
identifier. An authored note carries `origin: authored` and `signals` empty
or absent — and since A1 omits empty lists, "absent" is the canonical
spelling.

The list is a list even though v0.1 never fills it past one. A later
multi-signal note — a thread note, a person note — is a coherent record type
with its own identity, rebuild trigger, and conflict behavior, and it is
deliberately not proposed here. But when it arrives it must not have to
rename the property or change its type, because every consumer written
against a scalar `signal_id` would break at once: `fieldnotes.base` views,
the index in section 4, every query a user wrote, every agent reading the
notebook. A list that is always length one costs two extra bytes of YAML now
and removes a breaking change later.

`signals` is a **new shared property name and requires registry review with
fixtures.** It should be registered as a set-like list, so A1 section 5's
deduplicate-and-sort rule applies and the canonical form of a multi-signal
note is already decided.

**The invariant: a given signal identifier appears on at most one note.**

That is the whole reason the model is affordable. Coverage — "does this signal
have a note?" — is a **map lookup keyed by signal identifier**, not a search
over notes. One key, at most one value. Every question the render path asks is
that question, and it asks it once per collected record.

Two notes listing the same signal is not a merge, not a conflict, and not a
tolerated state: it is an invariant violation. `inspect` reports it, and
`rebuild` repairs it by keeping the note it can prove is the one the index
names and reporting the other rather than silently deleting it — because one
of the two might be authored, and section 11's deletion rule governs.

#### The collect path

For each record a **note-producing** Field emits:

1. **Upsert the signal** by portable source key, current state, exactly as
   today's reconciliation works (A1 section 7), writing it under
   `.fieldnotes/signals/<field-id>/`.
2. **Look up whether a note already lists that signal identifier**, through
   the index in section 4, then re-verify against the file.
3. **If one does**, default is **skip**. `--force` is 4b.
4. **If none does**, run the three-stage first emit, then record the
   mapping in the index:
   1. **Deterministic note** — fold `title_slug`, empty observation
      headings, `## Signals` last. File is valid with no model.
   2. **Extraction** — fill masked sections from the signal (and from
      the converter for office/PDF/HTML). No LLM.
   3. **Observations** — core slug observation (may rename once in this
      transaction); core picture caption if `image/*`; then each
      Field-declared observation prompt.

4b. **`--force`** reruns extraction and Field observations + caption;
    keeps `title_slug` and filename. Does not rerun the core slug
    observation.

A matching-only Field (Contacts) stops at step 1's cousin: write or replace
working data under `.fieldnotes/fields/<field-id>/`. No note is created. The
graph consumes those identifiers on its own pass.

**Walking every note per record, asking "is this identifier in your list," is
not the design.** It is the naive reading of a list-valued property, it is
O(notes) per record and O(records × notes) per sync, and at tens of thousands
of messages it is the difference between a sync that finishes and one that
does not. The uniqueness invariant plus the index in section 4 are what turn
step 2 into a single lookup. Any implementation that scans `notes/` per record
is wrong even if it produces correct files.

Fieldnotes does not de-duplicate notes against any destination the user
copies them into. Skip-unless-`--force` is a notebook-local courtesy so a
daily re-collect of the same window does not churn files the user is already
processing. It is not a promise to a second brain.

#### What a derived note carries, and what it does not

A derived note carries:

- `origin: derived`;
- `signals`: exactly one signal identifier;
- `title_slug`: stored once at first emit, kept across `--force` and
  re-collect. **New shared property; needs registry review.**
- Whatever properties its template's declared property set selects
  (section 5). `template_id`, `template_version`, `last_extraction_at`, and
  `last_observation_at` are **cut from this gate.**

A derived note does not carry `content_hash`, `collected_by`,
`source_version`, `captured_at`, or `instance_id`. Those describe collection,
they live on the signal, and their presence on a readable record is P6.

An authored note carries `origin: authored`, no `signals`, and the A1
required properties it already carries today.

**Nothing is ever written into a signal to record that a note exists.** A
"has a note" flag would let the readable tier mutate the evidence tier, so a
signal's bytes would depend on what had been rendered from it, an idempotent
re-collection could churn it, and its `fn-record-v1` fingerprint would change
for a reason that has nothing to do with the source. Coverage lives in the
index, which is a cache, and in the notes themselves, which are the truth.

#### Enrichment timestamps

**Cut from this gate.** `last_extraction_at` and `last_observation_at` wait
with the enhancement package. They are not required to review the split,
the index, or the collect path.

#### Alternatives considered

- **A scalar `signal_id`.** Simpler to read, simpler to index, and the shape
  the earlier draft of this package recommended. Rejected by the owner, and
  the reason holds up: the first multi-signal note type forces either a
  second property meaning almost the same thing or a type change on a
  published property.
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
  person browsing would have to look in two places for "my notes." The
  settled layout decision puts them together; section 11 carries the deletion
  rule that makes one directory safe.
- **Store enrichment timestamps when enrichment runs.** One less thing to
  recompute, and it puts non-reproducible state in a disposable file. The
  same mistake as change observations in section 8, at smaller scale.

#### Consequences

- `rebuild` becomes a destructive operation over `notes/` and must be keyed
  on `origin`. Section 11 states the rule.
- A note's frontmatter is not stable across enhancement state changes. A
  reader diffing two notebooks may see notes differ where signals agree; the
  signal is the thing to diff.
- `last_extraction_at` and `last_observation_at` require the render pass to
  read `extractions/` and `observations/`, so rendering depends on more than
  the signal and its template. Rendering remains deterministic — same inputs,
  same bytes — but "rebuild one note" needs the enrichment index, not just
  one signal file.

### 4. The signal-to-note index: required, disposable, never authoritative

**Settled: A3 requires a persistent index. A one-shot scan of `notes/` per
run is not an acceptable steady-state design.** The owner will sync a whole
mailbox — tens of thousands of messages — and step 2 of the collect path runs
once per record. Scanning per record is quadratic; scanning once per run is
linear in the notebook on every run, including the runs that collect three
new messages. Neither is acceptable when the notebook holds 10^5 notes, so
the map is part of the design rather than an optimization somebody adds later.

**Location: `.fieldnotes/cache/`.** That is [ADR
0001](../decisions/0001-current-state-and-state-classes.md)'s state class 4,
"disposable caches: indexes and acceleration stores under
`.fieldnotes/cache/`, always safe to delete and rebuild." No amendment to ADR
0001 is needed for this; class 4 already describes exactly this object, which
is the point of putting it there.

**What it holds.** Two maps:

- **signal identifier → note**: the note's record ID and its path. This is
  the map the collect path's step 2 reads, and the map the uniqueness
  invariant makes single-valued.
- **portable source key → signal**: the signal's ID and its path. This is the
  map reconciliation already needs conceptually — A1 section 7's "at most one
  active Note for an exact portable source key" is a lookup by portable key,
  and the only way to answer it without an index is to read every signal.
  Writing it down here is not a new requirement; it is naming a lookup the
  approved contract already assumes.

**It is not a new canonical record kind.** It has no record-kind prefix, no
record ID, no registry entry, no place in the public notebook contract, and
no frozen fixture bytes. Its on-disk representation is implementation-owned
and may change in a patch release without a format version. What A3 freezes
is the *rule*: it is rebuildable, it is never authoritative, and no correct
behavior may depend on its presence.

**It is built and updated during sync**, and by `rebuild`. Write ordering is
fixed: the public file becomes durable first, then the index entry is
updated. A crash between the two leaves the index missing an entry that
exists on disk, which is the recoverable direction; the reverse ordering
would leave the index naming a file that was never written.

**It is fully rebuildable from `.fieldnotes/signals/` plus notes'
frontmatter.** Every entry restates something already written on disk: the
signal map from each signal's `(source_scope, source_identity)` and `id`,
the note map from each note's `origin`, `signals`, and `id`. A rebuild reads
only frontmatter, so it can stop at each file's closing `---` rather than
parsing bodies.

#### Why a cache is safe here

**Losing the index costs a rescan and can never cause a wrong merge.** That
is the property that makes this a cache rather than a record, and it is worth
stating as a mechanism rather than a promise:

- **Merge and conflict detection never read the index.** They compare signal
  bytes and `fn-record-v1` fingerprints, exactly as A1 section 8 specifies.
  The index accelerates finding a candidate; it never decides whether two
  records are the same object, and it never supplies a value that ends up in
  a file.
- **Every write that acts on an index hit re-reads the file first.** Before
  re-rendering over a note the index names, the writer opens that file,
  confirms it parses, confirms it carries the ID the index claims, and
  confirms it still lists the signal. A hit that fails any of those checks is
  discarded, not trusted.
- **The delete path is gated on the file, never the index.** Section 11's
  rule — delete only what parses, validates, and proves `origin: derived` —
  reads the bytes on disk every time. No index entry can cause an authored
  note to be deleted, because no index entry is consulted when deciding to
  delete.

So the index can be wrong in exactly two directions, and both are bounded:

| Failure | Cause | Detection | Cost |
|---|---|---|---|
| **False hit** — entry names a note that is gone, renamed, or no longer lists the signal | user deleted or edited a note; crash after a file write | pre-write verification above | entry dropped; the record is treated as uncovered and a note is rendered |
| **False miss** — no entry for a signal that a note does list | index deleted, partial, or predates a hand-added note | the completeness check below, or `inspect` | a second derived note for one signal: an invariant violation, visible and repairable |

Neither touches a signal, neither deletes authored material, and neither
merges anything. The worst outcome in the whole failure space is a duplicate
disposable file and some wasted I/O.

#### Staleness, disagreement, and rebuild

**Header.** The index carries an index-format version, the notebook's
`instance_id`, and a generation counter. Any mismatch — a version it does not
recognize, an `instance_id` that is not this notebook's, a truncated or
unparseable header — is not repaired. It is discarded and rebuilt. A cache
that cannot prove which notebook it describes is worse than no cache.

**Positive answers are verified, not trusted.** Every hit is checked against
the file it names, per the previous subsection. This is one file open per
covered record, which is work the render path was going to do anyway when the
signal changed, and a stat-plus-frontmatter read when it did not.

**Negative answers need a completeness claim.** A miss is only meaningful if
the index is known to cover every note. So the index records a completeness
marker: the run that last performed a full scan of `notes/`, plus a cheap
directory-state summary — entry count and the maximum modification time
observed — for the notes directory. On open, that summary is recomputed. If it
matches, misses are trusted. If it does not, the index is **partial**: the
directory is re-enumerated, notes whose paths the index does not know are
read for their `origin` and `signals`, and the missing entries are added
before any note is rendered.

Be honest about that check: entry count and maximum mtime is a heuristic, not
a proof. Filesystem timestamp granularity differs (APFS and ext4 do not agree
on either resolution or update semantics), and a user who deletes one note
and adds another between runs can leave the count unchanged. That is why the
consequence of getting it wrong is capped at a duplicate derived note rather
than at data loss, and why `inspect` re-derives coverage by scanning. An
implementation that wants certainty rather than a heuristic can re-enumerate
`notes/` on every run and skip the summary: that is one `readdir` plus a stat
per file, not a parse per file, and it is affordable even at 10^5 notes.

**When the index disagrees with the files, the files win. Always.** There is
no case in which the index's answer is preferred, no case in which a
disagreement is recorded as a conflict, and no case in which a public file is
rewritten to match the index. A disagreement means the index is wrong, and
the repair is to correct the index.

**Rebuild cost, at scale.** A full rebuild reads the frontmatter of every
signal and every note. For a 40,000-message mailbox that is roughly 80,000
file opens and 80,000 partial parses. On a local SSD, dominated by syscall
and parse overhead rather than bytes, that is a minute or two — acceptable as
an occasional recovery and unacceptable as a per-run cost, which is exactly
why the index is required. Two further honest notes:

- The rebuild is I/O-bound and parallelizable, and it needs no ordering
  guarantee, so it can be split across threads. That is an implementation
  freedom, not a contract.
- **`inspect` has two modes.** Fast mode trusts the index, says so, and is
  the default at mailbox scale (settled point 5). Full mode remains a scan
  by design: validation must read the bytes it validates. Full mode is what
  CI and the cache-rebuild corpus run. Collect never waits on it.

#### Alternatives considered

- **One scan of `notes/` per run, no persistent index.** Simplest correct
  design, no staleness rules, no cache to invalidate, and it is what the
  earlier draft of this package implied. Rejected by the owner and rightly:
  it makes every sync cost O(notes) in parse work regardless of how little
  changed, so a three-message incremental sync reads 40,000 files.
- **A coverage flag on the signal.** Turns the lookup into a property read
  with no index at all. Rejected in section 3: it lets the readable tier
  mutate the evidence tier, and it churns signal fingerprints for
  render-layer reasons.
- **Derive the derived note's ID from the signal ID.** A genuinely attractive
  alternative that deserves recording. If `note_id = f(signal_id)`, a false
  miss overwrites the existing note instead of duplicating it, because both
  renders target the same ID — the invariant becomes structural rather than
  enforced, and the index degrades to pure acceleration whose loss cannot
  produce a duplicate. The costs are real: it is a second reversal of A1
  section 1's rejection of derived IDs, it couples note identity to signal
  identity so a note cannot outlive its signal or later cite two, and finding
  a note by ID still needs either a path convention or a directory scan.
  Recorded as the strongest alternative to the completeness heuristic; not
  recommended, because it constrains the multi-signal note the `signals` list
  exists to leave room for.
- **A relational store (SQLite) versus an append-only file versus a
  serialized map.** Deliberately not decided here. It is an implementation
  choice under `.fieldnotes/cache/`, it changes no public byte, and freezing
  it at an approval gate would be scope creep.

### 5. Type-specific rendering, and what a template is

A note is a **scaffold**. Core always writes it. The type template, plus
section declarations from the producing Field's `describe`, decide its
shape.

A template declares:

1. **A property set** — frontmatter this type's notes carry (`title`,
   `origin`, `signals`, `title_slug`, plus any approved shared names).
2. **A body layout** — Markdown sections in order. Each section is one of:
   - **extraction** — filled mechanically from named signal properties or
     literal spans. A mask lists the allowed sources. Nothing the signal
     does not contain may appear. No model is required; a model, if used,
     is still bound by the mask and rejected if it invents.
   - **observation** — filled by the local LLM using a prompt the Field
     declared for that section, or left as an empty heading for the human
     if the LLM is off. A generator stamp marks LLM fill. Absence of a
     stamp means human text; `--force` does not overwrite it.
   - **signals** — required, last. Traceability only: signal id, portable
     source key, `source_url` when present, relative path under
     `.fieldnotes/signals/`.
3. **A slug source** — properties the stored `title_slug` is derived from
   on first emit only.

The Field does not write the note. It declares sections and prompts on
`describe`. Core validates the declaration, writes the scaffold, runs
masked extraction, and optionally the local LLM. That keeps A2's rule that
a Field emits values (and now, section declarations) and never notebook
bytes.

**The eleven type templates are not part of this gate.** Mail plus authored
text is enough to review the scaffold. A mail-shaped example:

```markdown
---
title: "Migration Thursday"
id: note_01a0287d-acc0-7000-8000-000000000005
type: mail
origin: derived
signals:
  - sig_9c1f8ab2d4e60517
title_slug: migration-thursday
---

# Migration Thursday

## People

- alice@example.com (from)
- bob@example.com (to)

## Ask

- (empty heading if the LLM is off; otherwise a bounded observation)

## Signals

- `sig_9c1f8ab2d4e60517` — Outlook Mail (`outlook_mail_work`),
  collected 2026-08-22, source `message/AAMkAGI2MAIL01`
  ([signal](../.fieldnotes/signals/outlook_mail_work/20260822T080000Z_outlook_mail_work_mail_sig_9c1f8ab2d4e60517.md))
```

The contact-shaped example that used to live here is **withdrawn.** Contacts
are not notes.

Two properties in that withdrawn example (`organization`, `role`) were
never approved names; they stay unapproved. The mail example's `## People`
is an extraction section: those addresses must occur on the signal
(properties, identities, or body spans) or the section is empty.

The rest of this section's "template is reviewable but not frozen" reasoning
stands: a derived note is unhashed, skip-unless-`--force` keeps the slug,
and a template revision is a `--force` rebuild of notes the user asks to
refresh, not a format version.

**What it means for a template to be contract.** Reviewable but not frozen:
a template revision is a `--force` refresh, not a format version. That holds
because derived notes are unhashed, uncompared, and carry a stored slug.

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
- **Approve all eleven templates at this gate.** What the earlier draft of
  this package proposed. Rejected: eleven property sets and eleven fixture
  families are a larger review surface than the model they illustrate, every
  gap found while writing them becomes a registry-review item, and bundling
  them means the model cannot be approved until the last body layout is
  argued out.

#### Consequences

- A notebook's notes may have been rendered by different template versions at
  different times. `template_version` on each note is what makes that
  visible, and a re-render is what makes it uniform.
- A template that wants a property the registry does not have cannot invent
  it. Every gap becomes a registry review item, which is the correct pressure
  and will produce a list longer than the two names in the example above.
- Until the template package lands, types without an approved template have
  no derived notes. A1's fixture list already carried part of this debt — it
  noted that IG1 must add the missing `meeting`, `call`, and `document` body
  templates — and A3 makes the ordering explicit rather than urgent.

### 6. Property ordering for human reading

Approve a reading order for **readable records only** — notes, and only
notes. **Settled: `title` first, then A1's existing rule.**

1. `title` when present.
2. Then `id`, `type`, `origin`, and, for a derived note, `signals`.
3. Then every remaining property in ascending ASCII byte order.

Template-declared per-type order waits for the template package. Moving to
it later is a rebuild, not a migration.

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

**This one is open**, and the cheaper variant is genuinely defensible; see
the open recommendations at the end of this document.

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
  section 5 that is a rebuild, not a migration.

### 7. Layout and filenames

This section is last among the structural sections on purpose. It is the
consequence of the model, not the reason for it.

#### Layout

```text
notebook/
├── notes/                          # public, flat; the artifact
├── artifacts/
├── extractions/                    # later gate; not this package's reading surface
├── observations/
├── entities/
├── relationships/
├── proposals/
├── conflicts/
├── packages/
└── .fieldnotes/
    ├── instance.yaml
    ├── fields/<field-id>/          # config + connector working data
    ├── signals/<field-id>/         # private collected evidence
    ├── cache/                      # signal-to-note index; disposable
    └── state/                      # cursors, checkpoints
```

Public `notes/` stays flat. Authored and derived notes share it, distinguished
by `origin` rather than by location. This **preserves** [notebook
format](../notebook-format.md)'s approved statement that "`notes/` is flat
in v0.1".

Signals are **not** a second public directory. They live under
`.fieldnotes/signals/<field-id>/` so they travel with the vault, stay out of
Obsidian’s default browse, and can be partitioned by Field without a public
format change. Connector working data — the Contacts identifier arrays in
particular — live under `.fieldnotes/fields/<field-id>/`, next to the Field
configuration that already lives there.

**The honest consequence, restated.** A flat `notes/` holding tens of
thousands of *kept* mail notes is still not pleasant to browse in a file
tree. That is now a search-and-views problem, not a reason to invent
`notes/mail/` vs `notes/contact/`. Contacts no longer occupy 76% of the
listing. Signals at mailbox scale live in a hidden tree partitioned by Field,
so P2 for the collected layer is answered by putting that layer out of the
reading surface rather than by a public subdirectory scheme.

If the owner later wants `notes/` partitioned, it is still a public-directory
format change with a migration. It is cheaper than it was, because derived
notes can be re-rendered and authored notes are the only files that would
have to be moved. It is not taken here.

#### Filename grammar — settled: stored slug, keep `_`

```text
<readable-part>--<record-id>.md
```

`--` would be unambiguous and is **not taken**. Notes keep A1's `_`
delimiter. P4 stays documented rather than fixed. The substance of the
filename decision is the stored slug, not the separator.

The readable part is type-specific:

- **Event-like types** (`mail`, `message`, `event`, `meeting`, `call`,
  `ticket`, `text`, `voice`, `file`, `document`) keep a UTC timestamp, so the
  flat directory still sorts as a timeline: `<YYYYMMDDTHHMMSSZ>--<slug>`
- **Entity-like types** (`contact`) carry no timestamp, because P5: `<slug>`

Before and after, for the two live cases:

```text
# contact, today (91 characters, name absent)
notes/20250512T201356Z_outlook_contacts_work_contact_note_<uuid>.md

# contact, proposed
notes/alice-muller--note_01a02b40-0000-7000-8000-000000000001.md

# mail, today
notes/20260822T080000Z_outlook_mail_work_mail_note_01a0287d-acc0-7000-8000-000000000005.md

# mail, proposed
notes/20260822T080000Z--migration-thursday--note_01a0287d-acc0-7000-8000-000000000005.md
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

**Recommendation, open: the slug is stored, not recomputed.** This is the one
filename decision that is not cosmetic, because it is the one that decides
whether Fieldnotes renames files behind Obsidian's back.

A stored slug does not change when its source property changes. A recomputed
slug would mean that an upstream display-name edit renames a file, and a
rename Fieldnotes performs behind Obsidian's back breaks every wikilink and
backlink pointing at the old name — Obsidian updates links for renames *it*
performs, not for renames that appear underneath it. At 248 contacts, an
upstream directory-sync touching display names is a mass link-breaking event;
at mailbox scale it is worse. Renaming is therefore an explicit operation
(`rebuild --rename` or equivalent, a `0.1.x` CLI decision, not frozen here),
and the default is that names are stable for the life of the note. The same
rule covers the authored case for free: a person's own note keeps the name it
was given.

Storing it means one new shared property, `title_slug` (**registry review**):
a text property, written once when the note is first rendered, from which the
filename is thereafter computed. A1 section 3's rule that a validator
"computes the expected filename from validated frontmatter and compares the
whole name" is preserved exactly, because the slug is itself validated
frontmatter. That also means the folding rules below govern *initial
derivation* only — which matters, because if the slug were recomputed on
every render, the ASCII folding table would have to be frozen as fixture
bytes with the same rigour as A1's media-type-to-extension registry: two
implementations folding `Müller` differently would compute different expected
filenames and each would reject the other's notebook. With a stored slug,
divergent folding produces a cosmetically different name in a newly rendered
note and no validation failure anywhere.

**Derivation.** From the first present, non-empty property in the template's
declared slug-source list (`contact`: `title`; `mail`: `subject`, then
`title`): apply Unicode NFKD; drop combining marks; apply a frozen ASCII
folding table for the characters NFKD does not decompose (`ß`, `ø`, `đ`, and
their kin); lowercase, ASCII only; replace every run of characters outside
`[a-z0-9]` with one `-`; trim leading and trailing `-`; truncate to 48 bytes
at a `-` boundary. The folding table needs registry review; it is small, and
leaving it to implementation is how two notebooks end up with `alice-muller`
and `alice-mller`.

**Absence.** The slug segment and its delimiter are both omitted, and the
grammar is unambiguous without it because the ID segment always begins with a
known kind prefix:

```text
notes/note_01a02b40-0000-7000-8000-000000000001.md
notes/20260822T080000Z--note_01a0287d-acc0-7000-8000-000000000005.md
```

The slug is omitted when: no slug-source property is present; the derived
string is empty after filtering, which is the case for a title written
entirely in a script the folding table does not cover; the result matches a
reserved Windows device name (`con`, `prn`, `aux`, `nul`, `com0`–`com9`,
`lpt0`–`lpt9` — the same list A2 section 8 already excludes from artifact
handles); or the result would begin with a character a filesystem treats
specially. Omission is always available, always valid, and never a failure.

**Uniqueness.** Guaranteed unconditionally by the record ID, which is in
every filename. The slug is never load-bearing for identity, so no
disambiguating suffix, counter, or collision-retry logic is needed — the
failure mode those mechanisms exist to prevent cannot occur. Two notes whose
slugs differ only by case are still distinct files on a case-insensitive
filesystem because their IDs differ, and restricting the slug to lowercase
ASCII means no filesystem's Unicode normalization (APFS and HFS+ differ here)
can make two distinct slugs collide.

**Path length.** A note path is roughly `notes/` plus 48 slug bytes plus 2
plus 41 plus 3 — about 100 bytes, well under the 255-byte per-component limit
but consuming a real fraction of Windows' 260-character `MAX_PATH` once a
user's notebook sits a few directories deep. Signals, at `signals/` plus A1's
grammar, are similar. Worth a bound-checking test on Windows rather than a
rule.

#### Alternatives considered

- **Keep A1's `_` separator and add the slug as another `_`-delimited
  segment.** No new grammar, and it makes P4 strictly worse by adding a
  segment whose contents may contain the delimiter's cousin.
- **Drop the record ID from note filenames.** The only route to a genuinely
  short name. It makes uniqueness depend on slug plus timestamp, so two
  contacts with the same name collide and need a disambiguator, and the
  disambiguator must itself be a stored property for the filename to stay
  computable — which is a worse version of `title_slug`, doing more work for
  a weaker guarantee.
- **Recompute the slug on every render.** Names always match current content,
  and it costs a frozen folding table on the compatibility path plus mass
  renames on upstream edits. Rejected above.
- **Partition `notes/` by primary type.** What the earlier draft of this
  package recommended, as the answer to P2. Dropped by the owner's decision.
  It would have reversed notebook format's flat statement, given up the
  single global-timeline listing, and required every tool that globs
  `notes/*.md` to glob `notes/*/*.md`. It is recorded here because it is the
  obvious response to the browse problem named above, and because if that
  problem ever becomes acute this is the shape of the answer.

#### Consequences

- `notebook format`'s statement that "the filename contains no subject,
  participant, organization, or other mutable human content" is **directly
  reversed** for notes. A note filename contains a stored slug. Signals
  keep the old rule.
- One new shared property (`title_slug`) exists purely to make a filename
  stable. That is a real cost and the alternative is mass renames.
- `notes/` remains one directory listing, which keeps every tool that globs
  `notes/*.md` working and keeps the browse problem unsolved.

### 8. Change tracking: current state only

**Settled: current state only.** Signals hold current state; notes render
current state. Every approved invariant stands, every release gate stays
satisfiable, and the merge rule, the conflict rule, the pruning story, and
the product boundary are unchanged. That is what makes A3 a presentation and
storage change rather than a product-boundary change.

**What it costs: the owner does not get "the job title changed."** Showing
that a contact's job title changed requires retaining a value that
current-state reconciliation overwrites, and there is no way around that
fact. The roadmap states that "Fieldnotes represents the current collected
state, not an append-only history ledger" and "does not retain source
revisions merely because it observed them during earlier syncs." ADR 0001
makes the same commitment; A1 section 7 implements it ("v0.1 keeps no source
revision ledger"); the product boundary says Fieldnotes is not "an immutable
archive, audit trail, compliance store, or evidence ledger."

**What is still available.** Disagreement between *concurrent* current
sources. A contact note can carry a section that says: this contact record
states one role; a signature extraction from a mail record two weeks later
states another; here are both, with citations. That needs no retention, is
fully rebuildable, and is probably a large fraction of what "track changes
over time" was reaching for — because in practice the reason a job title
looks stale is usually that a directory record and a live signature disagree,
not that anyone needs an audit trail. Section 9 gives the mechanism.

The vocabulary must match: such a section says "disagrees with," never
"changed," so a reader is not misled about what the notebook knows.

#### The two rejected alternatives, recorded

**Change observations recorded at detection time — rejected outright.** When
reconciliation replaces a signal, write an observation recording what changed,
then discard the prior value. It looks derived, costs almost no storage, and
needs no merge rule. It is durable state wearing a disposable costume: the
prior value is gone once overwritten, so the observation cannot be
regenerated from anything, and deleting `observations/` makes that change
unrecoverable. It also breaks release gate R2 as written — "deleting all
caches and derived graph files reproduces the same semantic graph" — because a
change observation is not reproducible from current evidence, so a rebuild
produces a *different* graph and the gate cannot pass without exempting the
one record class it exists to constrain. ADR 0001 already states the
governing principle: "human decisions must be stored separately from
rebuildable generated projections." A change observation is neither a human
decision nor rebuildable, and it would sit in the directory reserved for
rebuildable things.

**Bounded versioned signals — a possible later package, not part of A3.**
Retain a bounded number of prior revisions per source object, in the evidence
tier, as first-class retained state. This is the only design where change
history is genuinely reconstructible: a note can say "job title changed from
X to Y between these two collections" and cite both revisions, and a rebuild
reproduces it because the evidence is on disk. It is also a larger package
than this one. It reverses the invariant openly; storage grows with revision
depth; A1 section 8's survivor rule assumes one current record per portable
source key, so two notebooks with different retained revision sets for the
same object need a union rule, an ordering rule, and a conflict rule that do
not exist; pruning needs a policy; the product boundary needs amending,
because a bounded revision store is a history ledger with a bound on it. And
the bound is not a detail: past it, a change is silently unrecoverable, so a
note that reports changes reports some of them.

If the owner ever wants it, nothing in sections 1 through 7 needs revisiting.
Bounded revisions are additional records in the evidence tier, which is
exactly where section 1 puts non-rebuildable state. The honest framing is
that "change observations" and "versioned signals" are the same decision at
different levels of honesty: if the change record must be durable, put it
where non-reproducibility is expected and accept the invariant reversal.

#### Consequences

- The notebook cannot answer "what changed" in v0.1, and a user who wants
  that answer must keep it elsewhere. The product boundary already says so.
- `source_version` still moves when a source object changes, and
  `content_hash` still differs, so the notebook can detect *that* something
  changed at collection time. It cannot report *what*, and it cannot report it
  after the fact, because both values are overwritten too.

### 9. Contacts are not notes; the graph answers "is this a new person?"

**Settled: the 248 contact markdown files were product drift.** A contact is
not an event and it is not something a person or an agent needs as a
standalone reading artifact. The Contacts Field does not produce public
notes.

What it does produce:

- **vCard 4 working files** under
  `.fieldnotes/fields/<contacts-field-id>/contacts/<uid>.vcf`. One card per
  source contact. Core validates a subset of RFC 6350 and installs it. The
  Field stages `text/vcard`; it does not write the notebook path. This is
  not a public record kind and is not in `notes/`.
- **Identity ammunition for the graph.** `UID` is the exact source id inside
  the Field's scope. `EMAIL` and `TEL` (with `TYPE` parameters, which is the
  reason to use the standard) become `email:` and `phone:` anchors. "Is this
  a new person?" is the resolver in
  [identity-and-graph.md](../identity-and-graph.md): exact and strong anchors
  join; medium and weak evidence produces a candidate; `FN` / display names
  never silently merge.

**vCard 4 subset (normative for this gate's Contacts working data):**
`BEGIN/END:VCARD`, `VERSION:4.0`, `UID` (required), `FN`, `N`, `KIND`,
`EMAIL` (repeatable, `TYPE` permitted), `TEL` (repeatable, `TYPE`
permitted), `ORG`, `TITLE`, `REV`, `ADR`, `URL`. Line folding per RFC 6350.
Rejected or dropped: inline `PHOTO`, `NOTE` dumping vendor blobs, arbitrary
`X-` properties above a size cap. The subset exists so Fieldnotes owns a
parser for a small language rather than a general vCard library whose
accepted language is wider than the contract — the same reason A1's YAML
is a subset.

A public note that mentions a person is emitted because *something happened*
— a mail, a meeting, an unmatched identity on an event-like signal, an
extraction that just introduced a property this notebook has not seen — not
because a directory record exists. That note cites its event-like signal.
The graph may link it to a person entity. There is no `type: contact` file
in `notes/` in v0.1.

**What the graph already does.** A person entity joins identity anchors
across Fields and cites the records that supplied them. The frozen corpus
already demonstrates this, with one entity citing eight records across five
channels. That is the matching mechanism. It does not require a contact
note.

**What the graph cannot do.** It only ever sees current state. It can report
that two *current* sources disagree; it cannot report that a value changed,
because the prior value was overwritten before the graph ran. Directory
change-history is not recovered by making contact files, and it is not
recovered by the graph. Section 8 already settled that.

Entity and relationship evidence citations move from Note IDs to signal IDs
(section 11), or, for matching-only Fields, to the working-data identity the
Field stored. The graph never derives from rendered notes.

### 10. What this package does not solve: P7

**The split does not give a contact typed, repeating, role-bearing contact
methods, and nothing in A3 moves that forward.** This is stated as its own
section because it is the defect most likely to be misread as something the
restructure fixes.

The mechanism is simple and it is not going away in this package. A2's
`record.properties` admits only scalars and homogeneous scalar lists, so a
Field cannot send `emails: [{address, kind, primary}, …]`, so core cannot
store it, so no template can present it. **A3 amends A2 not at all**, by
design and by the owner's decision on serialization. The evidence stands as
measured on the live notebook:

- 245 of 248 contacts carry an address as **Markdown body prose**; 232 carry
  a phone number the same way.
- **No `email` property, no `phone` property, and no address property exists
  on any of the 248.**
- All 248 carry `identities` anchors, which are role-erased by list
  semantics: A1 sorts and deduplicates them by normalized text, so a business
  phone and a mobile are two indistinguishable strings, and a primary address
  and an alias are two indistinguishable strings.

P7 was measured on contact *notes*, which this package no longer produces.
The underlying protocol limit remains: a matching-only Contacts Field still
cannot store role-bearing repeating methods, because A2 still admits only
scalars and scalar lists. That matters for the graph's identifier arrays
(business vs mobile is still erased if both land in one `phone:` list) and
it is still not this gate.

The route to actually fixing the nested-methods gap is the A2-amending
package section 2 describes. Approving A3 does not make it.

### 11. Derived, projected, and reconciliation records

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

**Extractions.** The most seriously affected, and this is unchanged in
substance from the earlier draft. An extraction cites one `source_note_id`
and validates its spans against "the deterministic normalized Markdown body
of the cited current Note" ([enhancement](../enhancement.md)). If notes are
rendered from templates, then every span offset is an offset into template
output, so **a template revision silently invalidates every extraction span
for that type** — precisely the "stale body coordinates" case enhancement's
validator is required to reject. **Extraction spans must therefore anchor to
the signal's normalized body, never to a rendered note body.** That requires
a signal-citing property in place of `source_note_id` (**registry review**),
and it means the `0.1.8` evidence coordinate system — whose unit,
byte-versus-scalar, is already flagged as an open question in
`enhancement.md` — must be defined over signal bodies. This is a genuine
simplification once made: a signal body is normalized source evidence with a
frozen normalization contract, which is a far better coordinate system than
template output, and it is the coordinate system enhancement was always
describing before notes and signals were separate things.

**Observations.** `supported_by` moves domain the same way, to signals and
extractions. Observations gain no change-tracking role (section 8).

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

**The deletion rule that makes one flat `notes/` safe.** Rebuild may delete
and regenerate a note **only** when that file parses, validates, and carries
`origin: derived`. A file it cannot prove is derived is left alone and
reported. The proof comes from the file's own bytes, never from the index in
section 4. This is the same shape as A1 section 12's protection for proposal
review state and ADR 0001's requirement that human decisions live separately
from rebuildable projections; the difference is that here the two populations
share a directory, so the check is per-file rather than per-directory and must
fail closed. It is also the single most dangerous line of code in this
proposal: a mis-scoped rebuild deletes the only copy of user-authored
material, and no source can refetch it.

#### Consequences

- Every derived-record fixture in the corpus changes: evidence citations move
  domain, and the entity, relationship, extraction, observation, proposal, and
  conflict fixtures must be regenerated rather than edited. Most of that
  regeneration belongs to the implementation gate; the review corpus below
  asks only for what proves the model.
- `enhancement.md`'s evidence-validation section needs revision before
  `0.1.8`, and its open coordinate-unit question is now a question about
  signal bodies.
- `notes/` acquires a per-file safety check that no other directory needs.

### 12. Amendments to the approved invariant documents

A3 does **not** propose that "Note" in approved documents silently comes to
mean "signal." The four statements below are load-bearing, and approving A3
should approve these specific words. They are written here rather than applied
to their documents because A3 is unapproved; on approval they are applied
verbatim, as part of the same change that lands the model.

#### Roadmap invariant 1 — canonical representation

Current:

> Notes and retained artifacts are the canonical representation. Caches,
> indexes, entities, relationships, Extractions, and Observations are
> disposable unless a document explicitly identifies user-maintained state.

Proposed replacement:

> Signals, authored notes, and retained artifacts are the canonical
> representation. Derived notes, caches, indexes, entities, relationships,
> Extractions, and Observations are disposable unless a document explicitly
> identifies user-maintained state. A derived note is rebuildable from its
> signal and its template; an authored note is not rebuildable from anything
> and is never removed by a rebuild.

#### ADR 0001 — state class 1

Current:

> 1. **Public notebook state:** portable Notes, artifacts, and generated
>    Markdown records. Collected Notes may be reconciled, removed, pruned, or
>    refetched.

Proposed replacement:

> 1. **Public notebook state:** portable authored notes, derived notes, and
>    artifacts. Derived notes are the reading surface rendered from signals;
>    they are not themselves the collected evidence. Authored notes are
>    evidence and are the only copy of their content.
> 1b. **Private portable evidence:** signals under
>    `.fieldnotes/signals/<field-id>/`, and Field working data under
>    `.fieldnotes/fields/<field-id>/`. These travel with the notebook, are
>    not the reading surface, and may be partitioned by Field without a
>    public format change. Collected signals may be reconciled, removed,
>    pruned, or refetched. Deleting them loses reprocess-without-refetch;
>    they are not a cache.

State classes 2, 3, and 4 need no amendment. In particular, the
signal-to-note index in section 4 is class 4 as written — "indexes and
acceleration stores under `.fieldnotes/cache/`, always safe to delete and
rebuild" — and A3 relies on that sentence rather than adding to it.

#### `docs/product.md` — the definition of a Note

Current, in core concepts:

> - **Note:** the primary readable file produced from a Field or user input.

Proposed replacement, one amended bullet and one new bullet directly above it:

> - **Signal:** the collected machine record of what a source said, reconciled
>   to current state. It is the evidence every derived record cites, and it is
>   not the file a person is expected to read.
> - **Note:** the primary readable file. A derived note is rendered from one
>   signal by a type-specific template and is disposable; an authored note is
>   written directly by the user, is the only copy of its content, and is
>   never removed by a rebuild.

The definition cannot stand alone, so one sentence in "The working-notebook
model" is amended with it. Current:

> Each configured Field is a stable, read-only producer of **Notes**. The
> reserved `self` Field records notes and artifacts supplied directly by the
> user.

Proposed replacement:

> Each configured Field is a stable, read-only producer. Note-producing
> Fields emit **signals**, from which Fieldnotes may render readable
> **notes**. Matching-only Fields (Contacts) emit identifier working data
> for the graph and do not produce notes. The reserved `self` Field records
> notes and artifacts supplied directly by the user; user-authored notes are
> readable files in their own right and are not rendered from anything.

#### Release gate R0

Current:

> **Release gate R0:** golden fixtures are stable; interrupted writes leave no
> valid-looking partial Note; repeated imports reuse artifacts; no secret-like
> CLI input is persisted accidentally; macOS, Linux, and Windows CI exercise
> path and rename behavior.

Proposed replacement:

> **Release gate R0:** golden fixtures are stable; interrupted writes leave no
> valid-looking partial signal and no valid-looking partial note; repeated
> imports reuse artifacts; a re-collect without `--force` does not rewrite an
> existing derived note; `--force` rewrites derived-note content and keeps
> `title_slug`; rebuilding or deleting `.fieldnotes/cache/` changes no public
> file; authored notes are never removed by collect, `--force`, or rebuild;
> no secret-like CLI input is persisted accidentally; macOS, Linux, and
> Windows CI exercise path and rename behavior.

#### Release gate R8

Current:

> **Release gate R8:** default installation performs no inference or model
> download; deleting `extractions/` and `observations/` leaves Notes
> byte-for-byte unchanged; rebuilding uses pinned generator contracts;
> evaluation fixtures meet approved evidence-precision, language, CPU/memory,
> packaging, and licensing thresholds.

Proposed replacement:

> **Release gate R8:** compile and CI perform no inference, model download,
> or GPU work; collect succeeds with the local engine missing or disabled
> and still writes a deterministic scaffold; deleting `.fieldnotes/cache/`
> leaves every signal, vCard, and note unchanged; `--force` does not
> overwrite human-written observation sections (no generator stamp);
> extraction sections validate against the cited signal body; rebuilding
> uses pinned generator contracts when the engine is on.

The R8 restatement protects the same thing the original did: enhancement
never mutates *evidence*. Notes may gain or lose observation text; signals
and vCards do not.

#### The rest of the sweep

Those five are the statements whose meaning changes. Every other occurrence of
"Note" in the approved documents is either mechanical or already correct, and
each falls into one of three groups:

1. **Occurrences that mean the evidence record** — A1 sections 3 and 5 through
   9, ADR 0001's consequences, ADR 0002, ADR 0004, the roadmap's
   reconciliation and deletion invariants, `notebook-format.md`,
   `property-registry.md`, `identity-and-graph.md`, and `enhancement.md`.
   These become "signal" in a mechanical rename, applied per document as part
   of the implementation gate, each with its fixtures regenerated in the same
   change.
2. **Occurrences that mean the readable file** — parts of `product.md` and
   `cli.md`, and the CLI's `note` command, which keeps its name because it
   creates an authored note.
3. **Occurrences in A2** — none require change: A2 speaks of `note_type` and
   of never emitting "a rendered Note," and both statements remain true. A2 is
   untouched.

Scoping the sweep this way is deliberate. Sweeping every file in one change
would produce a diff nobody can review against fixtures that no longer
exist; leaving a standing equivalence rule would make the vocabulary
permanently ambiguous. The middle path is per-document amendment with the four
load-bearing statements settled up front, which is what this section does.

## Compatibility and migration

A3 changes the notebook format. Under A1's change policy this is exactly the
case that "requires an explicit format version/migration proposal," and the
roadmap's scope controls say the same: "if implementation evidence
invalidates an approved contract, return to the relevant approval milestone
with an explicit migration and compatibility proposal." This document is that
proposal.

**What is invalidated.** Every Note filename; the directory a collected record
lives in; the identity of the record kind a derived record cites; the `0.1.0`
golden fixtures behind gate R0; the entity, relationship, extraction,
observation, proposal, and conflict fixtures; `fieldnotes.base` default views;
and the notebook the owner has already collected.

**What is not.** Every hash domain and every hash vector (section 2). The A1
canonical serializer, flat-YAML grammar, scalar spelling, datetime rules,
artifact identity, extension registry, merge survivor rule, and conflict
bundle layout. `notes/` being flat. The whole of A2 and all three Microsoft
Fields. Every credential, cursor, and checkpoint mechanism.

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
a population of one notebook that can be refetched in minutes.

**Anything a user authored is the exception**, and it is the only thing a
re-sync cannot recover. Authored notes have no source. Migration guidance must
say plainly that authored material must be copied out first, and the CLI
should refuse to delete a notebook containing `origin: authored` notes without
an explicit acknowledgement.

**This is far cheaper now than later.** One notebook, one user, 328 records,
no published fixture consumers outside this repository, and no shipped
connector whose property vocabulary depends on the readable layer. Every one
of those numbers grows monotonically — and the mailbox sync that motivates
section 4 is what will grow them. The same argument ADR 0010 made about
fixing a crate dependency at `0.1.1` — "the last point at which this was
cheap" — applies here with a much larger multiplier.

**Where approved invariants are touched.** Recorded explicitly, because
several are:

| Approved statement | Where | Effect |
|---|---|---|
| "Notes and retained artifacts are the canonical representation" | roadmap invariants | Reversed as written; exact replacement wording in section 12 |
| "`notes/` is flat in v0.1" | notebook format | **Preserved.** Type partitioning is dropped |
| "The filename contains no subject, participant, organization, or other mutable human content" | notebook format | Reversed for notes if the slug recommendation is taken; retained for signals |
| Structural keys first, then ascending ASCII | A1 section 5 | Replaced for notes only; unchanged everywhere else |
| Note filename grammar | A1 section 3 | Replaced for notes; retained for signals |
| "Note: the primary readable file produced from a Field or user input" | product, core concepts | Redefined; exact replacement wording in section 12 |
| `fn-content-v1` applicability | A1 section 9 | Narrowed to signals and authored notes; no domain or vector changes |
| "at most one active Note for an exact portable source key" | A1 section 7 | Reads as "at most one active signal"; mechanical rename, section 12 |
| Rejection of deterministic/content-derived record IDs | A1 section 1 | Partially reversed for signal IDs, derived from the portable source key rather than from content — open recommendation |
| Four state classes, class 1 | ADR 0001 | Restated; exact replacement wording in section 12 |
| Four state classes, class 4 | ADR 0001 | **Unchanged**, and it is what makes the required index in section 4 admissible as written |
| "deleting `extractions/` and `observations/` leaves Notes byte-for-byte unchanged" | gate R8 | Restated over signals and authored notes; strengthened, section 12 |
| Golden fixtures are stable | gate R0 | Invalidated; restated in section 12; the corpus must be regenerated |
| "deleting all caches and derived graph files reproduces the same semantic graph" | gate R2 | Unchanged, and it is the gate that rejects change observations |
| "notebooks remain useful offline and without Fieldnotes installed" | gate R9 | Unchanged in substance; section 2 records the one way the claim weakens |
| "no release ... may require a model ... for notebook use" | roadmap invariants | Unchanged, and it is why deterministic rendering is non-negotiable |
| "Fieldnotes represents the current collected state, not an append-only history ledger" | roadmap invariants, ADR 0001, A1 section 7 | **Untouched**, per the settled change-tracking decision |
| The whole of A2 | A2 | **Untouched** |

**New shared property names proposed, none settled.** Every name below
requires registry review with fixtures before use, per A1's change policy and
the roadmap's control that "no release may introduce unapproved shared
property names opportunistically". The list is long, and its length is itself
a reason to keep the template package separate — most of the growth will come
from templates, not from the model:

`origin`, `signals`, `title_slug`, and a signal-citing replacement for
`source_note_id` on derived records that currently cite Notes. Also requiring
registry review: the `sig_` record-kind prefix, the ASCII folding table for
slug derivation, and the 96-bit signal-ID derivation.

**Cut from this gate:** `template_id`, `template_version`,
`last_extraction_at`, `last_observation_at`, and any contact-note property
(`organization`, `role`, `contact_kind`). Those wait for the template
package, the enhancement package, or they never arrive because contacts are
not notes.

## Review corpus required at this gate

The earlier draft of this package asked for a corpus covering eleven
templates, every slug edge case, and every regenerated derived-record
fixture. That is implementation-gate work. **The model must be reviewable from
a small corpus, and this is it:**

1. **A mail message.** One signal in the A1 byte form at
   `.fieldnotes/signals/<field-id>/`, and the derived note rendered from it:
   `origin: derived`, `signals` with exactly one identifier, `title` first,
   stored `title_slug`, and no `content_hash`.
2. **An authored text note.** `origin: authored`, no `signals`, and the A1
   required properties.
3. **Skip unless `--force`.** Re-collect the same mail signal: the existing
   derived note is byte-for-byte untouched. `--force` rewrites the body and
   keeps `title_slug` and the filename.
4. **A rebuild that must not delete an authored note.** One directory holding
   both derived notes and the authored note from case 2, with a rebuild
   transcript proving the authored file is byte-for-byte untouched — plus a
   negative case where a malformed file whose `origin` cannot be read is left
   alone and reported rather than deleted.
5. **A cache rebuild.** The index deleted and rebuilt from
   `.fieldnotes/signals/` plus note frontmatter, reproducing the same map; a
   stale entry naming a note that no longer lists its signal, showing the
   entry dropped and the file preferred; and a false-miss case producing two
   notes for one signal, showing that `inspect --full` reports the invariant
   violation.

There is **no contact-note fixture.** Contacts contribute identifier working
data under `.fieldnotes/fields/`, not a file in `notes/`.

Plus, because they are nearly free and they are the evidence for section 2's
central claim: the `fn-content-v1` and `fn-record-v1` vectors from
`tests/fixtures/hashes/proposed-v1/`, reproduced unchanged against the renamed
signals.

Plus the rejection cases that fall directly out of the model: a note with
`origin: derived` and no `signals`; a note with `origin: derived` and two
entries in `signals`; a note with `origin: authored` and a non-empty
`signals`; two notes listing the same signal identifier; a note carrying
`content_hash`; a signal carrying a note-only property.

**Explicitly not required at this gate, and the gate does not close on
them:** the eleven type templates and their per-type golden fixtures; the
template registry; the slug folding table and its vector family; the
regenerated entity, relationship, extraction, observation, proposal, and
conflict corpus; the enrichment-timestamp cases. Each belongs to the package
that decides the thing it demonstrates.

Every fixture must state whether it is normative at A3 or illustrative for a
later gate, in the style the A1 and A2 corpus READMEs already use. Hash
vectors must state exact input bytes reproducibly.

## Explicit approval checklist

**Nothing below is approved.** The five decisions in "Settled by the owner"
are not repeated here, because they are settled; approval of A3 is approval of
those plus every box below.

### Model and vocabulary

- [ ] Three tiers — signal, note, enrichment — with the vocabulary applied by
  explicit per-document amendment and no standing equivalence rule.
- [ ] Deterministic first emit always: slug, scaffold, masked extractions,
  empty observation headings, `## Signals` last. Collect never fails because
  a model is absent.
- [ ] Small local LLM on by default once installed; config flag (and later
  onboarding) can turn it off. Compile/CI stay model-free. BYO is not a
  hard "No" and is not shipped here.
- [ ] A derived note is Fieldnotes output the user may keep, move, or delete.
  Collect does not rewrite it unless `--force`. `title_slug` and
  human-written observation sections are durable on the note.
- [ ] The exact replacement wording in section 12 for roadmap invariant 1,
  ADR 0001 state class 1, `product.md`'s definition of a Note, and gates R0
  and R8.

### Signals

- [ ] `sig_` as a new record-kind prefix, and private
  `.fieldnotes/signals/<field-id>/` with A1 section 3's filename grammar.
- [ ] Signal IDs derived from the portable source key at 96 bits (24 hex),
  with `self` signals keeping UUIDv7.
- [ ] `fn-content-v1` and `fn-record-v1` unchanged in domain, input, and
  vectors, with `fn-content-v1` narrowed in applicability to signals and
  authored notes.

### Notes

- [ ] A required `origin` property with the closed vocabulary
  `authored derived`.
- [ ] `signals` as a set-like list property, length exactly one on a derived
  note and absent on an authored note.
- [ ] The uniqueness invariant: a signal identifier appears on at most one
  note, so coverage is a map lookup, and a second note citing one signal is a
  reported violation rather than a tolerated state.
- [ ] The collect path in section 3: skip unless `--force`; `--force` keeps
  `title_slug`; scanning `notes/` per record is explicitly not the design.
- [ ] Contacts are not notes. The Contacts Field writes identifier working
  data under `.fieldnotes/fields/<field-id>/`. "New person?" is the graph.

### The index

- [ ] A persistent signal-to-note index is required, lives in
  `.fieldnotes/cache/` as ADR 0001 class 4, carries the portable-source-key
  map as well, and is not a new canonical record kind.
- [ ] Losing it costs a rescan and can never cause a wrong merge, by the three
  mechanisms in section 4 rather than by assertion.
- [ ] The staleness rules: a verified header, verified positive answers, a
  completeness marker plus directory summary for negative answers, and files
  winning every disagreement.
- [ ] `inspect` defaults to a fast mode that trusts the index and says so;
  `inspect --full` remains a scan.

### Rendering

- [ ] A template declares a property set, a body layout, a slug source, and a
  required signals section.
- [ ] A template is reviewable but not frozen: a revision is a rebuild, not a
  format version bump, which holds only because notes are unhashed,
  uncompared, and carry a stored slug.
- [ ] The eleven type templates, the template registry, and their golden
  fixtures are follow-on work, and **this gate does not close on them**.
- [ ] Templates are release-owned, with no user-editable template surface in
  v0.1.
- [ ] `title` first, then A1's existing rule, for notes only. A1 section 5
  unchanged for every other record kind.

### Filenames

- [ ] Keep A1's `_` delimiter.
- [ ] Event-like notes keep a UTC timestamp in the readable part.
- [ ] `title_slug` as a stored property, kept across `--force`.
- [ ] Slug omission as always available and never a failure, with the frozen
  folding table, the reserved-device-name exclusion, and the 48-byte bound.
- [ ] Uniqueness carried entirely by the record ID, with no disambiguator.
- [ ] Human content in a note filename, reversing notebook format's rule for
  notes while retaining it for signals.

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
  `origin: derived` from its own bytes, failing closed otherwise.

### Migration and policy

- [ ] Migration is a re-sync, with the in-place migrator explicitly declined.
- [ ] Authored material must be copied out before a notebook is deleted, and
  the CLI must refuse without acknowledgement.
- [ ] Every new shared property name, the `sig_` prefix, the folding table,
  the signal-ID derivation, and the new conflict types go through registry
  review with fixtures before use; none is approved by this checklist.
- [ ] The five-case review corpus above is required A3 evidence, and the gate
  is not closed until it exists.

## Approval effect

Approval would unblock: a signal store reusing the existing A1 serializer and
hashes; the render layer and the two templates the review corpus needs; the
required signal-to-note index; the new filename grammar; registry review for
every new name the compatibility section lists; regeneration of the notebook
fixture corpus; and the exact amendments in section 12 to the roadmap
invariants, ADR 0001, `product.md`, and gates R0 and R8.

It would not approve: nested signal structure or JSON serialization; change
tracking in any form; the eleven type templates; a user-editable template
surface; BYO inference (lifted as a prohibition, not implemented); the
pinned local model choice; or any new shared property name without registry
review. It does change the Contacts Field's product role, and it adds an
optional `describe.note_sections` member. A2 record/checkpoint frames stay.

## 13. If approved: planner's brief

A later session (or a later agent) must be able to plan and rework from
**this file**, the approved A0–A2 packages, and the crate tree. It must not
need the chat that produced A3. This section is that handoff.

### 13.1 Product in one page

Fieldnotes collects a **window of source material** into private **signals**,
then emits public **notes** the user can process (move, delete, keep). It is
a work queue by default. Keep-everything is the same tree with notes left in
place.

```text
Field (read-only, A2 child process)
        |
        v
.signal YAML under .fieldnotes/signals/<field-id>/     (evidence, current state)
.vcf    under .fieldnotes/fields/<contacts-id>/contacts/ (matching only)
        |
        v
index in .fieldnotes/cache/   (signal_id → note; source key → signal)
        |
        +-- already has a note?  skip (unless --force; keep title_slug)
        +-- no note?
              1. deterministic note (fold slug)
              2. extraction (masked, converter for docs)
              3. observations: core slug, core caption, Field sections
                 (Apple FM → llama → leave headings empty)
        |
        v
notes/<timestamp>_<slug>_note_<uuid>.md     (the artifact a person or agent opens)
```

- A **signal** is A1 YAML+Markdown, `sig_` id derived from
  `(source_scope, source_identity)` at 96 bits, UUIDv7 only for `self`.
- A **derived note** is created in three stages: deterministic scaffold,
  masked extraction, then observations. Slug is a **core** observation
  (not a Field add-in). Field add-ins only declare their own observation
  section prompts. `## Signals` is last.
- An **authored note** is user writing. Never deleted by collect/`--force`.
- **Contacts are not notes.** vCard 4 subset, graph matches `UID`/`EMAIL`/`TEL`.
- Downstream duplicates (second brain, CRM) are not Fieldnotes' problem.

### 13.2 What stays frozen

Do not reopen these unless a new ADR says so:

- A0 crate boundaries, toolchain, no `unsafe`, no model on `cargo test`.
- A1 YAML subset, hashes (`fn-content-v1`, `fn-record-v1`), datetime, artifacts.
- A2 record / checkpoint / diagnostic / credential frames and the sixteen
  transcripts, except an **additive** `describe.note_sections` member.
- Current-state reconciliation. No revision ledger. Option 2 change
  observations remain rejected.
- Core is the only durable writer. Fields emit envelopes (and now section
  declarations, and staged `text/vcard`). They never write `notes/`.
- Default compile/CI: no model download, no GPU.

### 13.3 Documents to amend on approval (verbatim from section 12)

Apply section 12's replacement wording to: roadmap invariant 1, ADR 0001
class 1 (plus private portable evidence), `product.md` Note/Signal bullets,
gates R0 and R8. Also restated on approval, not by silent equivalence:

- [enhancement.md](../enhancement.md): extractions/observations become
  **sections in the note**, not `extractions/` / `observations/` files;
  mechanical+masked vs LLM-or-human; local LLM on-by-default-once-installed.
- [notebook-format.md](../notebook-format.md): `notes/` stays flat; no
  public `signals/`; human content allowed in note filenames via stored
  `title_slug`.
- [fields.md](../fields.md) / Contacts: matching Field, not a note producer.
- Roadmap 0.1.8 and ADR 0006's "no BYO" sentence: prohibition lifted; BYO
  still not implemented.
- `Agents.md` "default build free of model downloads" **stays**. The product
  default after install is a different sentence.

### 13.4 Code that must change

| Area | Today | After A3 |
|---|---|---|
| `fieldnotes-domain` | `note_` ids, no `origin` | `sig_` kind, 96-bit derived ids, `origin`, `signals` list |
| `fieldnotes-format` | one emit order, Note filenames, `notes/` assumed | title-first emit for notes; `title_slug`; note vs signal paths |
| `fieldnotes-store` | `notes/` writer, source index scan | private signal tree; vCard installer; persistent cache index; skip/`--force` |
| `fieldnotes-app` collect | every record → Note | upsert signal; lookup; skip or scaffold; Contacts → vCard not notes |
| `fieldnotes-cli` | `sync`, `inspect` | `--force`; `inspect` fast-by-default; LLM on/off config |
| `fieldnotes-graph` | evidence = Note ids | evidence = signal ids; read vCard `UID`/`EMAIL`/`TEL` |
| `fieldnotes-field-protocol` | `describe` as frozen | additive `note_sections`; `text/vcard` as a staged working payload |
| Outlook Mail | maps to Note | still A2 records; `describe` grows mail sections/prompts |
| Outlook Contacts | maps to `type: contact` Notes | stop public notes; stage vCard 4 subset |
| Outlook Calendar | event Notes | keep as note-producing Field (events are a timeframe) |
| Fixtures | A1 Note corpus | signals at new paths + mail/authored note corpus in section "Review corpus" |
| `RESERVED_DIRECTORIES` | public `notes/` etc. | do **not** add public `signals/`; private dirs under `.fieldnotes/` |

Do not rewrite A2 Fields' Graph mapping except Contacts' destination and
Mail's `describe` sections. HTML-to-markdown of bodies/attachments is
**renderer** work (13.6), not a protocol change.

### 13.5 Suggested implementation order

1. Registry + domain: `origin`, `signals`, `title_slug`, `sig_`, 96-bit
   derivation. Tests first.
2. Store layout: `.fieldnotes/signals/<field-id>/`, cache index with the
   "files always win / lose cache = rescan never wrong merge" rules.
3. Collect path: skip unless `--force`; `--force` keeps slug; authored
   fail-closed.
4. Mail scaffold (deterministic only) + authored notes. Review corpus cases
   1–5.
5. Contacts: vCard 4 subset writer/validator; graph reads cards; no
   `notes/contact_*`.
6. `describe.note_sections` schema patch (additive A2); Mail declares
   extraction + observation + signals sections.
7. Masked extraction fill (no model).
8. Engine cascade: detect Apple Foundation Models; if unavailable, fold
   (llama.cpp later). First-emit jobs: slug proposal, then observation
   sections from `describe` prompts. CI stays on the fold path.
9. Only then: score the decided use cases on a sanitized fixture
   (invented entities = 0).
10. Engine **evaluation** (13.6) for converters and signature extraction
    before pinning. PII is postponed. Picture *captions* ship with slug
    once 13.6 E cases pass; document OCR stays on the converter.
11. Apply section 12 wording to the invariant docs. Re-sync the live
    notebook; copy authored notes out first.

### 13.6 Engine evaluation the implementation gate must run

A3 does **not** pin tools. It requires a measured comparison against the
constraints below, recorded as a short findings note (same shape as A1/A2
implementation findings) before any of them enter the default path.

**Hard constraints for anything on the default path**

- Local. No network at collect time.
- CPU-capable. GPU allowed as acceleration, never required.
- Compile and `cargo test` still download no model and need no Python.
- Optional engines are sidecars or explicit installs, like the local LLM.
- Extraction output is **masked**: every emitted string must occur in the
  cited signal body or declared property. Spans use the frozen coordinate
  unit (still open in `enhancement.md`; decide it in that findings note).
- Conversion used as evidence must be **deterministic** under A1 body
  normalization, or it cannot be a span source.

**A. HTML/Office/PDF → Markdown (attachment and rich-body rendering)**

ADR 0007 already stores original binaries and does **not** restrict
retention to markitdown types. Conversion is for *note sections*, not for
what is kept.

Evaluate at least:

- **Microsoft MarkItDown** — Python, broad formats (Office, PDF, HTML,
  optional OCR/audio). Process-spawn from Rust. Format-specific quality.
  License: MIT. Optional Azure paths must stay off.
- **Firecrawl AnyDoc** (local parser, Rust) — 14-format local convert,
  bindings for Rust/Node/Python/WASM. Closer to this workspace's language.
  Hosted Firecrawl Parse is **out** (network, vendor).

Measure, on a fixture set of: one HTML mail body, one docx, one PDF with a
table, one multilingual (at least DE/FR/IT besides EN) PDF or HTML, one
hostile/malformed HTML. Record: wall time on CPU, peak RSS, license,
whether output is byte-stable across two runs, table fidelity, whether
it fetches the network if unplugged.

Until that note exists, the deterministic mail scaffold uses the Graph
plain-text/HTML body the Field already maps. Do not block A3 collect on a
converter. **Do not use the vision model as a PDF/Office/screenshot
transcriber** — that is this converter's job, whether MarkItDown (Python
sidecar, as today) or Firecrawl AnyDoc (Rust, preferred if the bake-off
wins so collect does not grow a Python runtime). Hosted Firecrawl Parse
stays out.

**B. Multilingual signature extraction**

This is an **extraction**, not an observation. Chat-LLM signature parsing
will invent titles. Evaluate:

- Mechanical first: signature-boundary heuristics (quoted text, `-- `,
  `Sent from`), then copy spans. This is the no-model floor.
- Specialized extractors (Talon-style, GLiNER-multilingual, or a small
  token-classifier) **inside the mask**: they may only label spans that
  exist in the signal.
- Reject any engine that emits a role/org not present as literal text.

Languages that matter for this notebook: at least EN + the owner's live
mail languages. Record precision/recall on a sanitized signature fixture
set, not on live PII.

**C. PII — postponed; Presidio is out**

**Do not implement. Do not evaluate Presidio.** It requires Python and
spaCy model downloads, which is enough to reject it for this project.
ADR 0006's optional PII-span candidate remains recorded and is **not
in v0.1**. A later gate may look at a non-Python tagger; that search
is not this package.

**D. Observations and slugs — cascade, not a pinned GGUF**

The engine is a **runtime detection**, not a model file in the repo.

| Order | When | What it does |
|---|---|---|
| 1 | macOS, `SystemLanguageModel` available (Apple Silicon, macOS 26+, Apple Intelligence on) | On-device AFM. Slug + observation sections. Image attach is available on current OS trains — measure in 13.6 E, do not ship captions until measured. |
| 2 | llama.cpp (or equivalent) present and enabled | Same jobs. Win/Linux parity, and Mac fallback. Optional; not a v0.1 ship gate. |
| 3 | neither | Fold + empty observation headings. **This is the Windows/Linux default** and the CI path. |

Why this is faster than pinning a 3B GGUF first: the owner's Mac already
has the model; there is no download UX, no quant bake-off, and no Python.
A small macOS adapter (Swift helper or the existing `foundation-models` /
`fm-rs` class of binding) plus availability detection unblocks slug and
per-section prompts. llama.cpp packaging is a later parity package.

Do **not** shell out to an unpinned third-party CLI (`apfel`, `fm`) as the
product path. Those prove the OS can do it. Fieldnotes owns the adapter,
checks availability, and stays on-device (no PCC).

Order on **first emit** is fixed: note → extraction → observations.

Observations (engine cascade; skip the LLM bits if unavailable):

1. **Core: slug** — JSON `{"slug": string}`. Not declared by the Field.
   May replace the fold `title_slug` and rename only in this transaction.
   `--force` does not rerun it.
2. **Core: picture caption** — JSON `{"caption": string}` for `image/*`
   only. Not a transcription.
3. **Field: typed sections** — prompts from `describe.note_sections`
   (ask, etc.), run against *already extracted* text plus a bounded
   signal slice. Scoring those use cases waits until this exists.

**Do not score the product use cases until both pieces exist:** the
pinned (or trial) local engine, **and** the per-section prompt
declaration. Then run the already-decided use cases (ask, deadline if
literal, FYI vs action, one-line summary, signature-derived people as
*extraction* not observation) on a sanitized fixture. Invented entities
must be 0.

Out of scope for that first engine: cross-note synthesis, "what
changed," and **document OCR** (converter, 13.6 A).

**E. Picture captions — ship; document OCR does not go through the model**

Settled point 19. On-device AFM on this Mac already captions (13.7). The
next implementation wave ships:

- **Caption:** one sentence on `image/*` artifacts (`{"caption": "..."}`).
  Omit the section if the engine is off, the attachment is not an image,
  or the model refuses.
- **Not caption:** PDF, Office, HTML, and “this screenshot is a
  document, please transcribe it.” Those go to MarkItDown or Firecrawl
  AnyDoc and land as **extraction** text, masked.

**Eval / test cases required before captions ship** (synthetic is enough
to start; live mail screenshots later, sanitized):

| ID | Fixture | Pass if |
|---|---|---|
| C1 | Geometric scene (house/sun/grass — `/tmp/fn_spike_scene.png` class) | One sentence naming the obvious objects; no claim of readable text |
| C2 | Text-heavy slide (CUTOVER WINDOW… — `/tmp/fn_spike_slide.png` class) | Caption is *about* the image (“slide announcing a cutover window”), **not** a line-by-line transcript. Transcript is the converter’s job |
| C3 | Blank / nearly blank image | Empty caption or omitted section; no invented scene |
| C4 | Engine unavailable (CI / Windows fold) | Note still valid; no caption section; collect succeeds |
| C5 | Non-image artifact (pdf/docx) | No caption section from the LLM |
| C6 | Prompt discipline | JSON only; caption ≤ ~140 characters / one sentence |
| C7 | Timing | Caption < 3 s on the owner Mac class of hardware |

Do **not** fail C2 if the model *can* OCR; fail it if the shipped prompt
asks it to. The product prompt is “one-line caption,” not “quote the
text.”

Win/Linux: no caption until llama parity exists (fold). Do not block
Mac captions on that.

### 13.7 Spike evidence (owner Mac, 2026-08-24)

Ran on this machine: **Apple M5 Pro, macOS 27.0, Apple Intelligence available.**
`SystemLanguageModel.default.isAvailable == true`. Direct Swift
`FoundationModels` (not `fm`; that CLI is blocked on an unsigned
machine-wide license).

| Job | Result | Wall time |
|---|---|---|
| Probe | exact `hello-from-on-device` | — |
| Slug: "Re: Migration Thursday — please confirm the cutover window" | `migration-confirmation` | 814 ms |
| Slug: "Besprechung mit Alice Müller nächste Woche" | `meeting-with-alice` (translated; umlaut gone) | 394 ms |
| Ask, free-text markdown | messy (slug-like lines plus a stray `(none)`) | 842 ms |
| Ask, JSON `{"hasAsk","bullets"}` | `hasAsk: true`, two bullets copied from the mail, **no invented names** | 1207 ms |
| No-ask digest, JSON | `hasAsk: false`, `bullets: []` | 452 ms |

Implications for the adapter:

- The cascade's step 1 **works on this Mac today.** Slug + observation fill
  are in the 0.4–1.2 s range, fine for first-emit.
- Free-text observation prompts are not reliable. Ask for **JSON** (or,
  once Xcode/macros are on the build, `@Generable`). Command Line Tools
  `swiftc` cannot load `FoundationModelsMacros`; guided-generation macros
  need the Xcode plugin. JSON-in / JSON-out is enough to start.
- Do not depend on `/usr/bin/fm` until `sudo fm license` is accepted;
  the framework itself does not need that license.
- CI stays on the fold path (Linux). This spike is owner-machine
  evidence, not a golden fixture.

**Pictures (same session, `Attachment(NSImage)` in a `PromptBuilder`).**

Two synthetic fixtures: a text slide (CUTOVER WINDOW / Thursday 18:00-20:00 /
Confirm by Wed 16:00) and a geometric “house under a sun” scene.

| Job | Result | Wall time |
|---|---|---|
| Slide, free-text “quote the text” | Exact three lines copied | 1.8 s |
| Slide, JSON `{visibleText, ask}` | `visibleText` exact; `ask` mixed the *operator* instruction with the document (“extract the text…”) — prompt bug, not a vision miss | 2.0 s |
| Scene caption | “white building with a brown door on a green surface, under a blue sky with a yellow sun” | 1.2 s |

So on-device AFM **can see**, on this Mac, with no extra model. C1/C2-class
fixtures already run. Ship **caption only**; keep slide transcription on
the converter. The C2 prompt must not ask for a quote.

### 13.8 What a plan must not do

- Partition `notes/` by type, or invent `notes/contact/`.
- Store signals as a second public vault directory.
- Recompute slugs; rename out from under Obsidian.
- Scan `notes/` per record.
- Treat the cache as authoritative.
- Emit contact markdown.
- Require a model for collect to succeed.
- Download models in CI.
- Implement BYO providers in the first A3 implementation wave.
- Build an in-place notebook migrator; re-sync, after copying authored
  notes out.

## What remains open

Not frozen here, and not a menu. Section 13.6 is how a later session
**decides** 1–2; it must not invent a pin in chat.

1. **llama.cpp pin, if/when Win/Linux parity is wanted.** The Mac path is
   Foundation Models when available; no GGUF is required to start.
   Packaging and first-run download wait for that later package.
2. **`describe.note_sections` JSON Schema bytes.** Existence and the three
   section kinds are settled.
3. **Converter / signature engine pins.** Evaluate per 13.6 A–B. Do not
   guess MarkItDown vs AnyDoc. **PII is postponed; Presidio is out.**
4. **Span coordinate unit** (UTF-8 bytes vs Unicode scalars) — still open
   in `enhancement.md`; freeze it when masked extraction is implemented,
   not for PII.
5. **Windowed collect vs keep-everything** is CLI policy (`--since` /
   `--until`), not format. Same files. Keep-everything is "don't delete
   notes."
6. **Caption eval C1–C7** must pass on Mac before captions ship. llama
   captions on Win/Linux are later. Converter pin (MarkItDown vs AnyDoc)
   is independent and must not block slug+caption.
