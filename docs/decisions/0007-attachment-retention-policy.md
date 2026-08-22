# ADR 0007: Skipped attachments, link semantics, re-collection, and a media-type retention policy

- **Status:** Accepted on 2026-08-22
- **Date:** 2026-08-22

## Context

A2's artifact-retention threshold (`Limits::max_artifact_bytes`, default 25
MiB, ceiling 512 MiB) already lets a Field decline to retain an oversize
attachment's bytes: the record is accepted, the Note is created, and the
attachment stays at its source. Four gaps around that mechanism needed a
ruling before it can be built out further:

1. A1's shared registry has no property meaning "this Note had an attachment
   that was deliberately not retained." `damaged`, `truncated`, and
   `lost_characters` all imply corruption or cutoff, not a policy choice.
2. A1 does not say what the Markdown body's attachment link should point at
   once bytes may or may not exist locally.
3. Raising the retention threshold, or widening what is retained, must later
   pick up attachments a Field previously declined — but ordinary incremental
   sync moves forward from a cursor and never revisits settled objects, so
   this cannot happen as a side effect of a normal sync.
4. Retention was size-only. The project no longer restricts retained
   originals to markitdown-supported types (it stores original binaries), so
   nothing currently stops an unbounded set of media types from being kept by
   default.

The coordinator has ruled on all four. This ADR records those rulings, their
rationale, the alternatives rejected, and their consequences.

## Decision

### 1. A new shared property: `skipped_attachments`

Approved: a new A1 shared, Note-applicable, set-like `list[text]` property
named `skipped_attachments`, registered in
`crates/fieldnotes-format/src/registry.rs` and
`docs/property-registry.md`. Each member is a stable connector-namespaced
upstream attachment reference, following the same object-kind-namespace
convention `source_identity` uses (for example
`mail-attachment/AAMkAGI2TQABAAACattach02`).

**Why this shape, and no other.** A1 freezes frontmatter as scalars and
one-dimensional scalar lists: no nested objects, no arrays of objects. A Note
may have several attachments with some retained and some skipped, so a single
boolean cannot carry the fact (there is no per-attachment slot to hang it on),
and an array of objects is forbidden outright. The remaining tempting shape —
two index-correlated parallel lists, one of references and one of sizes — is
also rejected, because **A1 sorts and deduplicates set-like lists**: two
parallel lists would have their positional correlation destroyed the moment
either one is canonicalized independently. One flat set-like list of
references, with nothing else riding alongside it positionally, is the only
shape that survives canonicalization intact.

**Deliberately excluded: byte size and skip reason.** The property records
*which* attachments were skipped and nothing else. Re-collection (ruling 3
below) re-evaluates each reference against the retention policy *in force at
the time of re-collection* and refetches metadata from the source at that
time. A stored size would therefore be a stale copy of something the source
already knows more currently; a stored reason would be a stale copy of a
policy decision that may since have changed (the threshold or the include set
can be reconfigured between collection and re-collection). Per-attachment
human detail — names, approximate sizes, why a particular attachment was
skipped — belongs in the Markdown body as deterministic evidence, which is
unconstrained by the flat-frontmatter rule and is exactly where source
evidence like this already lives.

`skipped_attachments` is **not** added to the `fn-record-v1` semantic-comparison
exclusions: unlike `collected_by` or `content_hash`, it is source-semantic
content (what a source object actually contains), matching how `artifacts`
already participates in semantic comparison.

**Alternatives rejected:**

- **A boolean `has_skipped_attachments` flag:** cannot say *which* attachment,
  which is the entire value of the property — a reader could not act on it or
  distinguish one skipped attachment from three.
- **An array of `{ reference, size, reason }` objects:** the most naturally
  readable shape, and structurally forbidden by A1's flat-frontmatter rule.
- **Two parallel lists (`skipped_attachment_refs` / `skipped_attachment_sizes`):**
  destroyed by canonicalization, as above — A1's own sort-and-deduplicate rule
  for set-like lists makes positional correlation between two lists an
  unreliable carrier of meaning.
- **Storing size and reason anyway, accepting staleness:** rejected because a
  stale size or reason is worse than none — it would read as current evidence
  while actually describing a policy that reprocessing may have already
  superseded.

### 2. Link semantics: local path when retained, source when not

Approved: the Markdown body's attachment link targets the derived relative
artifact path when the artifact's bytes are retained, and the original source
location when they are not. `source_url` remains present in frontmatter in
**both** cases, so provenance is never lost regardless of the retention
outcome.

This is the minimal reading consistent with A1 section 6: source identity and
provenance are never allowed to depend on what Fieldnotes chose to copy.
Removing `source_url` when bytes are skipped — the stronger reading a
retention policy might tempt an implementer toward — was considered and
rejected: it would make the record of *where the material lives* depend on a
retention decision that can change (ruling 3), which is exactly backwards.
The source reference must survive so re-collection has something to refetch
from and so a human can always reach the original regardless of what the
notebook currently holds.

### 3. Explicit re-collection, not a side effect of sync

Approved: reprocessing that raises the retention threshold or widens the
media-type include set must be able to pick up previously skipped
attachments, but **not** as a side effect of an ordinary sync. Normal
incremental sync moves forward from a cursor and never revisits settled
objects; there is no other mechanism by which a widened policy would ever
reach an object a Field already reported.

This requires an explicit re-collection pass: an operation that finds Notes
carrying `skipped_attachments` references and asks the owning Field to
refetch exactly those known source objects, re-evaluating each of their
attachments against the *current* policy. When bytes finally arrive, the Note
is rewritten in place under the same Note ID — exactly A1 section 7's
current-state update rule for an update to a stable source object. No new
identity behavior is needed: this is an ordinary current-state update whose
trigger happens to be a policy change rather than an upstream edit.

**The collection request could not already express "collect these specific
known references."** Its only scoping mechanisms are `cursor` (moves forward
only, and cannot target arbitrary past objects), `window` (time-bounded, not
object-targeted), and `snapshot_scope` (claims an entire scope for complete
reconciliation, forcing expensive full enumeration and engaging deletion-by-
absence semantics a targeted maintenance pass does not want at all). None of
the three can name a bounded, arbitrary set of already-known source objects.

**What was added to close the gap.** `CollectRequest` gains an optional
`recollect_targets`: a bounded list of `(scope, identity)` pairs — the
portable exact-source key alone, nothing else — naming exactly the source
objects core asks the Field to recollect. When present, `cursor` and `window`
must be absent (recollection is scoped exactly to the named targets, not to a
cursor-bounded range), and it is rejected in `snapshot` mode (a snapshot
already reconciles everything in its scope; naming individual targets inside
one is redundant and would blur two different completeness claims). Whether
a configured Field can honor a recollection request at all is exactly what
the manifest's existing `collection.refetch` declaration
(`supported` / `bounded` / `unsupported`) already governs — no new manifest
member was needed, because "can this Field refetch material it already
reported" is precisely the capability recollection depends on.

This ADR **specifies** the shape; it does not build the re-collection
*operation*. The `sync` command and cursor persistence do not exist until
`0.1.1`, and building an operation with nothing to invoke it from would be
speculative. `docs/roadmap.md`'s `0.1.1` entry now names the operation so it
is not lost between approval and implementation.

**Alternatives rejected:**

- **A third `mode: "recollect"` value:** considered, and rejected in favor of
  an orthogonal optional member. A new mode would force every place that
  branches on `CollectionMode` (manifest capability checks, deletion-authority
  reasoning, checkpoint/cursor semantics) to grow a third arm for a case that
  does not actually change collection *mode* — it changes collection *scope*.
  Keeping `recollect_targets` orthogonal to `mode` means it composes with
  `incremental` mode's existing machinery instead of duplicating it.
- **Reusing `window` with a synthetic instant range:** unrelated in meaning
  (time-bounded, not object-targeted) and would make a targeted maintenance
  pass indistinguishable from an ordinary bounded incremental run.
- **A new manifest capability member for recollection:** redundant with the
  existing `collection.refetch` declaration, which already answers the exact
  question ("can this Field refetch material it already reported") that
  recollection depends on.

### 4. A configurable media-type include set, orthogonal to the extension registry

Approved: retention is also filtered by media type, with a default include
set that may be overridden in settings or per run, exactly like the size
limit. The earlier intent to restrict retention to markitdown-supported types
no longer applies, because the project now stores original binaries rather
than only rendering them.

**This is orthogonal to A1's frozen media-type-to-extension registry.** That
registry governs how a *retained* original is named on disk; this policy
governs *whether* it is retained at all. A media type may be in the include
set and still have no canonical extension mapping (it falls back to `.bin`,
per A1 section 2) — the two questions are independent, and conflating them
would make naming policy gate retention policy for no reason connected to
either.

**Expressed as media types, never as extensions, and never derived from a
source filename.** A1 section 2 is explicit that a source filename never
selects the stored extension; the same reasoning applies here; a
filename-based retention policy would create exactly the untrustworthy
filename-as-authority pattern A1 already rejected for naming. The grammar
admits an exact `type/subtype` or a subtype wildcard (`image/*`), though the
approved v1 default deliberately uses only exact media types (see
"Cross-check against the extension registry" below).

**Default include set** (documents/text, images, audio):

```text
application/pdf
application/vnd.openxmlformats-officedocument.wordprocessingml.document
application/vnd.openxmlformats-officedocument.spreadsheetml.sheet
application/vnd.openxmlformats-officedocument.presentationml.presentation
application/vnd.oasis.opendocument.text
application/vnd.oasis.opendocument.spreadsheet
application/vnd.oasis.opendocument.presentation
text/plain
text/markdown
text/csv
application/rtf
image/png
image/jpeg
image/gif
image/webp
image/heic
audio/mp4
audio/mpeg
audio/wav
audio/ogg
```

Video, archives, disk images, and installers/executables are excluded by
default. Legacy binary Office formats (`application/msword`,
`application/vnd.ms-excel`, `application/vnd.ms-powerpoint`) are also excluded
from the v0.1 default set for simplicity; a later registry review can add them
if evidence shows they matter in practice.

**Cross-check against the extension registry.** Of the twenty default media
types above, **nine have no entry** in A1's canonical extension registry
(`docs/artifacts.md`, `crates/fieldnotes-format/src/extension.rs`), so a
retained original of one of these nine types gets the `.bin` fallback
extension today:

```text
application/vnd.openxmlformats-officedocument.wordprocessingml.document (.docx)
application/vnd.openxmlformats-officedocument.spreadsheetml.sheet (.xlsx)
application/vnd.openxmlformats-officedocument.presentationml.presentation (.pptx)
application/vnd.oasis.opendocument.text (.odt)
application/vnd.oasis.opendocument.spreadsheet (.ods)
application/vnd.oasis.opendocument.presentation (.odp)
text/csv (.csv)
application/rtf (.rtf)
image/heic (.heic)
```

The other eleven — `application/pdf`, `text/plain`, `text/markdown`,
`image/png`, `image/jpeg`, `image/gif`, `image/webp`, `audio/mp4`,
`audio/mpeg`, `audio/wav`, and `audio/ogg` — already have a canonical
extension. This is a real gap the extension registry does not close, and
this ADR deliberately does not close it either: expanding A1's frozen
extension registry is a notebook-format compatibility change
(`docs/artifacts.md` says so explicitly) requiring its own registry review,
separate from the retention question this ADR answers.
A retained Word document is real and useful evidence even with a `.bin`
extension; it is not blocked on this ADR.

**Enforcement mirrors the size threshold.** `CollectRequest` gains a required
`artifact_media_types` member — a Field needs it to self-police exactly as it
already self-polices against `max_artifact_bytes`. A Field compares a known
attachment's declared media type against the effective include set before
staging it: included, stage normally; excluded, emit a `not_retained`
reference instead of bytes core would otherwise have to reject. A
type-excluded attachment produces the same `not_retained` outcome as an
oversize one: it never stages, never hashes, never fails a run, and its
reference lands in `skipped_attachments`. Core additionally enforces the
policy itself, exactly as it enforces `max_artifact_bytes` today: a Field
that stages an excluded type anyway is rejected with the new
`artifact.type_excluded` code, distinct from `artifact.oversized` so the two
kinds of retention refusal remain distinguishable in logs and tests.

Enforcement is best-effort on a declared media type: `ArtifactRef.media_type`
is optional, and this policy has no other signal to classify staged bytes by
at record-acceptance time. An attachment staged with no declared media type
is not rejected for type; only the size threshold applies to it. A Field is
expected to declare an accurate media type whenever it can determine one.

**Alternatives rejected:**

- **A wildcard-only default (`image/*`, `audio/*`, `text/*`,
  `application/pdf`, plus the Office/OpenDocument types):** simpler to write,
  and rejected because it would silently retain every image or audio subtype
  a vendor might send — including ones the extension registry cannot name
  today and nobody has reviewed — rather than the specific, auditable list the
  owner actually approved. Wildcard matching is supported by the grammar for
  later settings use; the v0.1 default itself stays exact.
  Deriving the include set from a source-declared filename extension:
  rejected for the same reason A1 rejects filename-driven extension
  selection — a filename is a label, and labels lie or go missing.
- **Folding the include set into `Limits`:** rejected because `Limits` is
  specifically the frozen-ceiling-plus-configurable-default numeric bound
  table; a media-type set has no numeric ceiling to freeze, and forcing it
  into that shape would blur the one concept (an absolute technical ceiling)
  `Limits` exists to keep clear.

## Consequences

- A registry addition needs an approved fixture, not only a schema/code
  change: `tests/fixtures/notebooks/proposed-v1/` gains one Note exercising
  `skipped_attachments` alongside a retained attachment on the same Note, and
  the corpus README's gate-classification table is updated.
- Every existing A2 `collect_request` transcript frame gains the new required
  `artifact_media_types` member, because the schema now requires it exactly as
  it already requires `limits`.
- `ArtifactRef` gains a field (`attachment_ref`) required exactly for
  `not_retained` and forbidden otherwise, so a declined artifact always
  carries the stable reference `skipped_attachments` needs; this is the same
  requiredness pattern the schema already uses for `handle` (staged) and
  `sha256` (digest-only).
- The rejection-code vocabulary gains exactly one new closed member,
  `artifact.type_excluded`, following the same "declare before exercise"
  discipline as every other retention-adjacent code.
- A2 remains unapproved. Nothing here is in force until the user says so; this
  ADR records a ruling on scope and shape, not an approval of A2 itself.
