# ADR 0006: A1 implementation rulings from IG1 findings

- **Status:** Accepted on 2026-08-22
- **Date:** 2026-08-22

## Context

IG1 implemented the approved A1 notebook contract
(`docs/approvals/A1-notebook-contract.md`) as domain types, canonical
serialization, hashes, and an executable conformance suite. Implementation
surfaced five places where contract prose and the approved fixture corpus
could not both be satisfied, or where A1 left a rule open. Each was recorded
without deciding, in `docs/approvals/A1-implementation-findings.md`, because
changing an approved shared contract requires a coordinator ruling and an
explicit migration proposal rather than an implementation guess.

The coordinator has now ruled on all five. This ADR records those rulings,
their rationale, the alternatives rejected, and their consequences.

Three of the five rulings — record-type length (1), semantic-record key
ordering (2), and the `collected_by` documentation example (5) — resolve the
contradiction in favor of the already-frozen fixture bytes and correct
contract prose that stated something the fixtures already contradicted.
**They change no approved byte.** One ruling (3) withdraws an A1 requirement,
its `security.secret_detected` error, and its negative fixture. One ruling (4)
adds an enforcement rule A1 left unstated.

## Decision

### 1. Record-type length is split by record class

A1 section 4's grammar `[a-z][a-z0-9_]{0,31}` (32-byte cap) applies only to
primary Note types, a closed vocabulary of eleven short values. Non-Note
record types (Extraction, Observation, entity, relationship, proposal,
conflict, package) use `[a-z][a-z0-9_]{0,62}` — 63 bytes, matching the
property-name bound.

Derived types are an open, registry-reviewed vocabulary where descriptive
multi-word names are the point. The frozen fixture type
`organization_affiliation_candidate` is 34 bytes and revealed that the 32-byte
bound was scoped to the wrong record class: every other derived fixture type
is 16 bytes or fewer, so nothing else depended on the tighter bound. Filename
headroom is ample: `obs_` + a 36-byte UUID + `_` + a 63-byte type + `.md` is
107 bytes, far under the 255-byte filesystem limit.

**Alternative rejected:** renaming the fixture to fit 32 bytes. This would
change approved bytes and push a deliberately readable vocabulary toward
cramped abbreviations for no safety benefit.

This needs no format version bump: the change is purely permissive for
non-Note types, so every file valid under the strict reading stays valid and
nothing can require migration.

### 2. The semantic-record encoding has two differences from the public emitter, not one

A1 section 8 stated that the semantic encoding "uses the public canonical
emitter after removing excluded properties, except that datetime values are
rendered as their instant in UTC with `+00:00`" — one exception. The frozen
`fn-record-v1-sha256` vector proves there are two:

1. datetimes render as their UTC instant with `+00:00`, as already stated;
2. every retained key sorts in ascending ASCII byte order with **no**
   structural-keys-first exception, so `type` sorts among the ordinary keys.

Structural-first ordering is a human-readability affordance for public files
(A1 section 5's own alternatives discussion states its purpose is keeping the
five structural properties visible in a larger record). The semantic record
is a machine-only hash input, so that affordance does not apply. One
unconditional ordering rule makes the fingerprint independent of record kind
and removes a real divergence hazard: two implementations disagreeing about
whether structural-first survives the bookkeeping exclusions would silently
produce different fingerprints for identical evidence, surfacing to users as
phantom merge conflicts. The benefit of keeping structural-first would have
been near-zero anyway, since `id`, `instance_id`, and `field_id` are all
bookkeeping exclusions from the semantic payload, leaving only `type` and
`occurred_at` of the structural five to reorder.

A1 section 8 is amended to state both differences explicitly, and to state
that the semantic encoding is a hash input and never a publishable notebook
record — the checked-in `semantic-record-canonical.md` vector is a hash-input
fixture, not a notebook file.

**Alternative rejected:** keeping structural-first ordering in the semantic
encoding to mirror the public form. Rejected because it is the divergence the
frozen vector already contradicts, and preserving it would not have made any
real record easier to read, since the structural keys it would preserve are
almost entirely excluded already.

This needs no format version bump: it corrects contract prose to match the
already-frozen, already-approved hash vector. No implementation that follows
the fixture bytes needs to change.

### 3. Fieldnotes performs no secret or password scanning of notebook content

This reverses an A1 requirement. A1 required rejecting frontmatter containing
secret indicators (error `security.secret_detected`, with a negative fixture).
That conflated two different things, and is withdrawn.

The real invariant, from `AGENTS.md`, is "never write credentials or tokens
into notebooks, diagnostics, fixtures, or process arguments" — a rule about
how Fieldnotes handles credentials **it holds**. It is enforced by design and
by release-gate scanning of Fieldnotes' own output:

- the `CredentialProvider` abstraction and OS keychain integrations (`0.1.3`);
- protected secret delivery to Field processes rather than command-line
  arguments;
- diagnostic and log redaction;
- release gates R3 and R9 scanning argv, logs, diagnostics, cursors, Notes,
  and artifacts for credential leakage.

It is **not** content validation of collected evidence. Collected evidence
must never be rejected for containing secret-looking text: a credential in a
Note body was put there by a person or upstream system, so rejecting it would
discard real evidence, be unfixable by the user (who cannot edit the upstream
mail), and permanently break sync for that source — one colleague pasting an
API key into an email must not brick a mail Field.

**Alternative rejected: entropy-based detection.** `content_hash` values,
artifact IDs, and UUIDs are legitimately high-entropy and would false-positive
under an entropy heuristic, which would reject the approved fixtures
themselves.

The masking use case this withdrawn rule was reaching for is instead a
**future, optional** PII-detection capability, candidate for the `0.1.8`
enhancement gate, modelled as an Extraction: evidence-backed spans over exact
normalized-body offsets that point at text a user may choose to mask, never
altering the Note. It must remain optional, outside the default build, and
subject to the `0.1.8` rules (no model download, GPU, or network required by
default) — relevant because tools such as Microsoft Presidio pull Python and
spaCy model assets. Its schema is not approved now; it is recorded as a
candidate capability for that gate.

This withdraws the `security.secret_detected` rejection and its negative
fixture. Any notebook that was rejected only for that reason is now accepted;
no previously-accepted notebook is affected, so no format version bump is
needed.

### 4. Property prefixes bind to the producing Field, and `self` is a first-class Field

A Note may carry prefixed properties only for its own `field_id`'s registered
stem, plus unprefixed shared registry properties. A `teams_`-prefixed property
on a mail Note means the mail connector invented a Teams property — a
connector-boundary violation that A1 did not state a rule against, because no
fixture exercised the combination.

Derived and projection records (`ext_`, `obs_`, `ent_`, `rel_`, `prop_`,
`conf_`, `pkg_`) may carry any registered prefix, because they legitimately
aggregate evidence across Fields.

The built-in `self` Field is treated as a proper Field like the others rather
than a special case scattered through validation: it is a registered Field
whose ID is one-part and which contributes no property prefix, so a `self`
Note may carry only unprefixed registry properties. This does **not**
introduce a `self_` property prefix and does **not** permit `self_<label>`
Field IDs — `self` remains the only one-part Field ID exactly as A1 section 4
approved.

Separately, connector-prefixed property *types* remain undeclared in v0.1
because no registry entry exists for them. The interim rule is that
spelling-based inference is round-trip stable: canonical text of a number,
date, datetime, or boolean shape must be double-quoted, so plain implies typed
and quoted implies text, and re-emission reproduces the input bytes. A
property's type therefore cannot drift within a notebook even without a
registry entry. The real fix is deferred to gate A2: each Field's `describe`
manifest declares its prefixed properties with name, scalar type, and list
semantics (set-like versus ordered), and core rejects prefixed properties the
declaring manifest does not list.

This is not urgent for `0.1.0` because only `self` ships and `self` has no
prefix, so no notebook can contain a prefixed property before `0.1.1`.

This adds an enforcement rule A1 left unstated; it changes no approved byte,
since every A1 fixture already satisfies prefix-to-producer binding.

### 5. The `collected_by` documentation example is a non-canonical erratum

`docs/notebook-format.md` showed `collected_by` list members in double-quoted
style. The approved quoting rule (A1 section 5) uses plain style when and only
when the value matches `[A-Za-z0-9_./@+-]+(?: [A-Za-z0-9_./@+-]+)*` and
resolves as a string under the YAML 1.2 Core Schema. A `<instance_id>/<field_id>`
value contains only letters, digits, underscore, hyphen, and solidus and has
no colon, so the rule requires plain style. The frozen fixture
`tests/fixtures/hashes/proposed-v1/semantic-record-source.md` confirms it by
using plain style.

**Alternative rejected:** none considered; this is a documentation defect, not
a design choice. The fix is to correct the example to plain style.

## Consequences

- Rulings 1, 2, and 5 correct contract and documentation prose to match bytes
  that were already approved and frozen; no fixture, hash vector, or
  previously-valid notebook file changes meaning, and no format version bump
  is required.
- Ruling 3 removes `security.secret_detected` from the contract, the approval
  checklist, and the required negative-fixture corpus; a notebook that
  contains secret-looking evidence text is valid. Implementations must not
  scan Note bodies or frontmatter text values for credential-shaped patterns
  as a rejection condition. Credential protection remains fully in force as an
  internal-boundary concern (`CredentialProvider`, protected Field delivery,
  diagnostic/log redaction, release-gate scanning of Fieldnotes' own output).
- Ruling 3 adds a candidate, schema-unapproved optional PII-span Extraction to
  the `0.1.8` roadmap as the masking path, explicitly outside the default
  build and requiring no model download, GPU, or network by default.
- Ruling 4 adds a validation rule: a Note's prefixed properties must belong to
  its own `field_id`'s registered stem; derived/projection records may carry
  any registered prefix; `self` carries none. This does not create a `self_`
  prefix or `self_<label>` Field IDs. Full prefixed-property type declaration
  and enforcement remain deferred to the A2 `describe` manifest gate.
- `docs/approvals/A1-notebook-contract.md` gains a dated "Approved amendments"
  section recording all five rulings, with the affected sections (4, 5, 8, 10,
  the approval checklist, and the fixture-evidence list) corrected in place.
- `docs/approvals/A1-implementation-findings.md` moves from open questions to
  resolved, each finding linking to this ADR.
