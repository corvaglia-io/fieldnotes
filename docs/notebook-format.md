# Fieldnotes notebook format

**Status:** Proposed v0.1 contract

This document defines the portable, file-based interface of a Fieldnotes
notebook. It is intentionally narrower than the product specification: it
describes what other tools may rely on when reading or combining notebooks.

## Meaning of canonical

Notebook files are the canonical representation of the notebook's **current
state**. Canonical means that Fieldnotes, humans, and other tools treat these
files as the authoritative public representation. It does not mean that the
files are append-only, irreplaceable, or a compliance archive.

In particular:

- a source update may reconcile and overwrite the Note for the same stable
  source object;
- a source deletion may remove the collected Note;
- pruning may remove Notes and unreferenced artifacts;
- refetching from a source is the recovery mechanism when source data remains
  available;
- v0.1 does not require a revision ledger, event log, or tombstone history.

Fieldnotes must not imply that a notebook is the only durable copy of business
state. A user who needs immutable history must use a system designed for that
purpose.

## Layout

```text
notebook/
├── README.md
├── fieldnotes.base                 # optional Obsidian convenience
├── .fieldnotes/
│   ├── instance.yaml
│   ├── config.yaml
│   ├── fields/
│   ├── state/                      # operational state; not a cache
│   │   └── sync/
│   └── cache/                      # safe to delete
├── notes/
├── artifacts/
├── extractions/                    # optional, generated notebook records
├── observations/                   # optional, generated notebook records
├── entities/                       # generated current-state projections
├── relationships/                  # generated current-state projections
├── proposals/                      # generated proposals; review state is private state
├── conflicts/                      # unresolved reconciliation bundles, not active Notes
└── packages/                       # prepared handback packages; never destination writes
```

`notes/` is flat in v0.1. Its filenames form a useful global timeline without
requiring Fieldnotes. Other public record directories may choose their own
collision-safe filename convention, provided every filename contains its
record ID.

Credentials never live in the notebook tree. They live in an operating-system
credential store or another explicitly selected credential provider.

## Record IDs

The proposed v0.1 logical-record ID representation is a lowercase UUIDv7 with
a record-kind prefix:

```text
fn_01a02837-2de0-7a2b-8c41-f2481851192a
note_01a028d5-90c0-7248-a74b-c8bc1085ab19
ext_01a028e9-b500-7ed2-8138-02d96c291d7d
obs_01a028ee-48e0-798e-9e91-9283ea42fd60
ent_01a028f2-dcc0-7743-8951-4f4ad91727ec
rel_01a028f4-b180-777a-8679-5e7dc41d8f8a
prop_01a028f7-70a0-7806-a8bd-496d2da773e8
```

UUIDv7 is standardized, globally unique in normal operation, filesystem-safe,
and time-sortable by creation time. Its timestamp is the time the Fieldnotes
record ID was created, not `occurred_at`. Correcting a Note's event time must
not change its ID.

IDs are opaque to readers. Consumers must not derive business meaning from
their embedded creation time.

Immutable original artifacts are the proposed exception: their public ID is
`artifact_sha256_<64-lowercase-hex>` over exact bytes, and their path is
`artifacts/<artifact-id>.<canonical-extension>`. This exposes and reuses the
content address directly instead of assigning a second logical UUID to the same
byte object.

## Note filename

```text
<utc-timestamp>_<field-id>_<type>_<note-id>.md
```

For example:

```text
20260822T093614Z_teams_acme_message_note_01a028d5-90c0-7248-a74b-c8bc1085ab19.md
```

The timestamp is the UTC rendering of the instant in `occurred_at`, at whole
second precision, using `YYYYMMDDTHHMMSSZ`. A filename timestamp is a locator,
not an identity. If `occurred_at` changes, Fieldnotes atomically renames the
file while preserving its Note ID.

Field IDs and types use lowercase ASCII letters, digits, and underscores. The
filename contains no subject, participant, organization, or other mutable
human content.

## Required Note properties

Every Note contains flat YAML frontmatter with these properties:

```yaml
---
id: note_01a028d5-90c0-7248-a74b-c8bc1085ab19
instance_id: fn_01a02837-2de0-7a2b-8c41-f2481851192a
field_id: teams_acme
type: message
occurred_at: 2026-08-22T11:36:14+02:00
---
```

`instance_id` and `field_id` identify the producer that first wrote this Note.
When a merge reconciles exact copies collected by other producers, their
producer references are preserved in `collected_by`:

```yaml
collected_by:
  - fn_01a02837-2de0-7a2b-8c41-f2481851192a/teams_acme
  - fn_01a02837-31c8-75ef-b4df-95d5cdcf09cc/teams_acme
```

`collected_by` is a list of `<instance_id>/<field_id>` strings. On a freshly
collected Note it may be omitted because the required producer pair already
records the only known producer. Once more than one producer is known, the
sorted, deduplicated list is self-contained and includes that required primary
pair as well as every additional producer.

External Notes also contain portable source identity:

```yaml
source_scope: "microsoft-graph:tenant/8d82..."
source_identity: "chat-message/19:abc.../174531..."
```

`source_scope` is a connector-namespaced, non-secret, stable authority or
account scope. It must be identical when two Fieldnotes instances collect from
the same upstream scope. `source_identity` is stable within that scope and
includes an object-kind namespace when the source does not guarantee IDs are
unique across kinds.

The portable source-object key is:

```text
(source_scope, source_identity)
```

The producer provenance key remains:

```text
(instance_id, field_id)
```

These keys answer different questions: the former identifies the upstream
object across instances, while the latter identifies who collected it. Notes
created by the built-in `self` Field do not have a portable source-object key.

## Datetimes

Datetime properties use RFC 3339 with an explicit numeric UTC offset. A
timezone-less datetime is invalid.

```yaml
occurred_at: 2026-08-22T11:36:14+02:00
captured_at: 2026-08-22T11:38:02+02:00
```

The offset should represent the source-local time when the source supplies a
reliable offset. Otherwise it represents the configured Field timezone or the
collecting client's local offset at that instant. The value always preserves
the exact instant, and consumers may normalize it to UTC.

Even for UTC, the canonical serializer uses a numeric offset (`+00:00`) rather
than a timezone-less value. Filenames always use UTC and an explicit `Z`.

Calendar dates without a time remain `YYYY-MM-DD` and are a distinct property
type from datetimes.

## Frontmatter value model

Frontmatter supports only:

- text;
- finite numbers;
- booleans;
- dates and offset-bearing datetimes;
- one-dimensional, homogeneous lists of those scalar types.

It does not support nested mappings, arrays of objects, mixed-type lists,
duplicate keys, aliases, anchors, application-specific tags, non-finite
numbers, or binary values. A list property remains a list even when it has one
element. Missing values are omitted rather than written as `null`.

The canonical serializer uses UTF-8, LF line endings, deterministic property
ordering, and exactly one blank line between closing frontmatter and Markdown
body. Parsers may accept UTF-8 files with CRLF line endings but serializers
always write LF.

Canonical scalar spelling is part of A1, not a YAML-library preference:
booleans use lowercase; finite binary64 numbers use the RFC 8785 number rule;
dates and offset-bearing datetimes use their exact approved lexical forms; and
text uses the restricted safe plain form or JSON-escaped YAML double quotes.
See the [A1 contract](approvals/A1-notebook-contract.md#5-canonical-markdown-and-flat-yaml)
for the complete emitter rule.

Source-specific properties use the connector's registered prefix. A property
name has one meaning and one type everywhere in a notebook.

## Hash domains

All v0.1 public hashes use SHA-256 and include a textual domain/version prefix.
Different domains must never be compared as though they were interchangeable.

### Artifact byte hash

```text
sha256:<lowercase-hex>
```

This is the SHA-256 digest of the exact artifact bytes. It is suitable for
artifact storage and byte-for-byte deduplication. The artifact's extension and
media type are metadata, not hash input.

### Normalized content hash

```text
fn-content-v1-sha256:<lowercase-hex>
```

This hashes the v1 deterministic normalized content representation produced by
a Field and the core. It excludes Fieldnotes IDs, producer provenance,
`captured_at`, filenames, and merge bookkeeping. It detects identical content;
it does not prove that two Notes have the same source context.

The exact byte-level normalization vectors must be frozen as golden fixtures
before v0.1 compatibility is declared. Changing normalization requires a new
domain version rather than silently changing existing hashes.

### Semantic record hash

An internal `fn-record-v1-sha256` hash covers the canonical semantic Note
payload whenever deterministic conflict comparison or candidate ordering is
performed. It is not required public Note frontmatter in v0.1. It excludes
serialization-only differences, producer/capture/source-version bookkeeping,
and rebuildable `entities`/`related` projections. Golden test vectors define
its input. Datetimes in this internal comparison encoding are normalized by
instant to UTC `+00:00`; the public Note keeps its meaningful source/client-
local offset.

## Current-state source reconciliation

Within one notebook, Fieldnotes maintains at most one active collected Note for
a portable source-object key.

When a source returns an updated version of that object, Fieldnotes:

1. finds the current Note by `(source_scope, source_identity)`;
2. preserves its Note ID while it remains in the notebook;
3. renders the new current content and metadata;
4. writes the replacement atomically;
5. renames the file if `occurred_at` changed;
6. commits a source checkpoint only after the replacement is durable.

No prior revision or tombstone is required. A source deletion event may remove
the current Note. Shared artifacts are removed only after a reference scan or
garbage-collection pass proves that they are unreferenced.

If a deliberately deleted or pruned object is fetched again, Fieldnotes may
assign a new Note ID. The stable upstream identity remains its portable source
key, not its historical Fieldnotes Note ID.

## Merge reconciliation

Two external Notes with the same portable source-object key represent the same
upstream object even when different instances collected them.

- If their current semantic content matches, merge keeps one Note and unions
  all producer references into `collected_by`.
- If the connector exposes a reliable `source_version` with an approved
  comparison rule, merge uses it to select the demonstrably newer current
  state and unions producer references. Event time, capture time, filesystem
  time, and merge order never select a winner.
- If current content differs and neither side is demonstrably newer, merge
  must preserve both inputs as a visible conflict until resolution. It must not
  silently choose one.
- Same `content_hash` without the same portable source-object key means only
  identical content. Both contextual Notes remain.
- Same Note ID with divergent content is always a conflict.

Conflict preservation is a safety mechanism during reconciliation, not a
general revision ledger.

## Markdown body and artifacts

The Markdown body preserves coherent source evidence and remains useful
without parsing frontmatter. Deterministic normalization may clean encoding,
line endings, vendor link wrappers, or quoted-history boundaries, but it must
not replace available source content with an inferred summary.

Artifacts are copied into `artifacts/` before their referencing Note becomes
durable. `artifacts` and `attachments` frontmatter contain artifact IDs only;
readable relative paths may appear in the Markdown body. Artifact filenames
are derived from the byte hash and the canonical media-type extension registry.
Input paths are never stored as the only copy, and untrusted connector paths
must not be used directly as notebook paths.

An attachment a Field declines to retain under the effective retention policy
(a size threshold or a media-type include set) never appears in `artifacts` or
`attachments`; its reference lands instead in the shared `skipped_attachments`
property (see the [property registry](property-registry.md) and
[ADR 0007](decisions/0007-attachment-retention-policy.md)). The Markdown
body's attachment link follows the retention outcome: it targets the derived
relative artifact path when bytes are retained, and the original source
location when they are not. `source_url` remains present in frontmatter
either way, so a reader can always reach the source regardless of what was
copied in. Per-attachment detail such as name, approximate size, or why it was
skipped belongs in the Markdown body as deterministic evidence; frontmatter
deliberately stores neither, because re-collection re-evaluates each reference
against whatever policy is current when it runs.

## Derived public records

Extractions, Observations, entities, relationships, and generated proposals are
also public notebook records while present. Their flat frontmatter obeys the
same scalar, datetime, ID, no-secret, and stable-property rules. Unlike Notes
and retained artifacts, they are disposable projections rather than source
evidence.

They may be regenerated or removed even though their files are the canonical
serialized representation exposed to readers at a given moment. Human review
decisions, identity overrides, and other non-reconstructable intent belong in
durable private state and must not be lost when generated files are rebuilt.
