# A1 approval: public notebook contract

**Status:** Approved by the user on 2026-08-22  
**Scope:** Public file identities, filenames, frontmatter, hashes, merge behavior,
artifact references, derived records, conflicts, proposals, and package envelopes

## Approved amendments

**2026-08-22:** IG1 implementation surfaced five contradictions and
under-specifications, recorded in
[A1 implementation findings](A1-implementation-findings.md). The coordinator
ruled on all five; the rulings, rationale, and rejected alternatives are
recorded in [ADR 0006](../decisions/0006-a1-implementation-rulings.md):

1. The 32-byte type grammar in section 4 applies only to the eleven primary
   Note types. Non-Note record types (Extraction, Observation, entity,
   relationship, proposal, conflict, package) use a separate 63-byte grammar
   (sections 4 and 10).
2. The semantic-record encoding used for `fn-record-v1-sha256` differs from
   the public emitter in two ways, not one: UTC datetime normalization and
   unconditional ascending-key order with no structural-keys-first exception.
   It is a hash input, never a publishable notebook record (section 8).
3. **Withdrawn.** Fieldnotes performs no secret or password scanning of
   notebook content. The `security.secret_detected` rejection and its
   negative fixture are withdrawn; credential handling remains an internal
   concern of how Fieldnotes handles credentials it holds (section 4's
   fixture-evidence list; see [security](../security.md)).
4. A Note's prefixed properties must belong to its own `field_id`'s
   registered stem, plus unprefixed shared registry properties; derived and
   projection records may carry any registered prefix; the built-in `self`
   Field is a registered Field that contributes no property prefix (section 4).
5. The `collected_by` example in [notebook format](../notebook-format.md) is
   corrected to plain style, matching the approved quoting rule and the
   frozen `semantic-record-source.md` fixture.

Rulings 1, 2, and 5 change no approved byte; every file valid under the
prior contract prose remains valid. Ruling 3 withdraws an A1 requirement.
Ruling 4 adds an enforcement rule A1 left unstated.

## Decision requested

A1 freezes the byte-visible notebook contract that core writers, Fields,
fixtures, generic readers, and later releases must share. The recommendation was
to approve the choices in this document together with the integrated candidate
golden fixtures and hash vectors. The user has explicitly approved every choice
below, together with the integrated candidate fixture corpus and hash vectors,
on 2026-08-22; the contract in this document is now frozen.

The review corpus is attached as:

- [candidate valid notebook files](../../tests/fixtures/notebooks/proposed-v1/README.md);
- [candidate invalid notebook files](../../tests/fixtures/notebooks/proposed-v1-invalid/README.md);
- [candidate canonical hash vectors](../../tests/fixtures/hashes/proposed-v1/README.md).

These bytes make the proposal reviewable. A1 approval freezes them as the
implementation target; parser/validator code and executable positive/negative
tests are the subsequent IG1 implementation evidence, not a prerequisite for
choosing the contract.

The detailed background lives in:

- [Notebook format](../notebook-format.md)
- [Property and record-type registry](../property-registry.md)
- [Fields](../fields.md)
- [Artifacts](../artifacts.md)
- [Identity and deterministic graph](../identity-and-graph.md)
- [Operations and lifecycle](../operations.md)
- [Handback packages](../handback.md)
- [Current-state ADR](../decisions/0001-current-state-and-state-classes.md)
- [Source identity and merge ADR](../decisions/0002-source-identity-updates-and-merge.md)
- [Datetime ADR](../decisions/0003-datetime-serialization.md)
- [ID and hash ADR](../decisions/0004-record-ids-and-hash-domains.md)

## Recommended contract

### 1. Logical record IDs

Use lowercase, hyphenated UUIDv7 values with a readable kind prefix:

| Prefix | Kind |
|---|---|
| `fn_` | Fieldnotes instance |
| `note_` | Note |
| `ext_` | Extraction |
| `obs_` | Observation |
| `ent_` | Entity projection |
| `rel_` | Relationship projection |
| `prop_` | Proposal |
| `pkg_` | Handback package |
| `conf_` | Reconciliation conflict bundle |

Example:

```text
note_01a028d5-90c0-7248-a74b-c8bc1085ab19
```

The UUIDv7 timestamp is ID-creation time, not `occurred_at`. IDs are opaque and
must not be decoded for business behavior. Correcting event time, updating a
current source object, or renaming a file never changes a retained logical
record ID.

Entity and relationship files are disposable projections. If their files and
all non-canonical ID mappings are deleted, rebuild may assign new UUIDv7 IDs;
consumers must follow current evidence rather than treat projection IDs as
cross-notebook real-world identity. A retained record that points to a
projection must also carry a stable evidence/identity anchor and follow the
explicit rebinding rule in section 12.

#### Instance metadata exception

The notebook instance identity is operational metadata at
`.fieldnotes/instance.yaml`, not a frontmatter-bearing public record. Its A1
schema and canonical key order are:

```yaml
instance_id: fn_<uuidv7>
created_at: <explicit-offset datetime>
name: <optional canonical text scalar>
```

`instance_id` and `created_at` are required; `name` is optional, non-secret,
and display-only. No other keys are accepted in v0.1. The UUIDv7 timestamp is
the same creation instant as `created_at`, to millisecond precision. This file
uses UTF-8, LF, exactly one final LF, and the A1 scalar rules, but no `---`
delimiters or Markdown body.

#### Alternatives considered

- **ULID:** shorter and naturally sortable, but UUIDv7 is an Internet standard
  with broad library and database support.
- **UUIDv4:** standardized and widely supported, but loses useful creation-time
  ordering without improving opacity.
- **Deterministic/content-derived IDs for every record:** make rebuild IDs
  stable, but couple identity to mutable serialization, leak equality, and turn
  ordinary edits into identity changes.

#### Consequences

- ID generation requires a secure random source and injectable clock/generator
  for tests.
- IDs sort by creation time, while Note filenames sort by event time.
- Derived IDs are locators for the current projection, not durable entity keys.

### 2. Original artifact identity is the deliberate exception

Use a content-addressed identifier for immutable original bytes:

```text
artifact_sha256_<64-lowercase-hex-digits>
```

The digest is SHA-256 over the exact original byte sequence. The same bytes
have the same artifact identity across instances and are stored once per
notebook. Contextual metadata such as source filename, sender, attachment role,
or source object remains on Notes/relationships and does not alter byte
identity.

Store an original at:

```text
artifacts/<artifact-id>.<canonical-extension>
```

The extension is not identity. It comes from an approved deterministic media-
type-to-extension registry; unknown or conflicting types use `.bin`. A merger
that encounters the same artifact ID with different bytes reports corruption.
Different extension claims for the same bytes reconcile through the approved
registry rather than retaining duplicate originals.

The initial canonical mapping is frozen in
[Artifacts and renditions](../artifacts.md#initial-canonical-extension-registry).
Media-type parameters are removed and the type/subtype is ASCII-lowercased
before lookup. Detection disagreement or a type absent from the table selects
`.bin`; a source filename never selects the stored extension.

Notes reference artifact IDs, not absolute paths:

```yaml
artifacts:
  - artifact_sha256_6f4d...
attachments:
  - artifact_sha256_6f4d...
```

`artifacts` lists every original carried by the Note. `attachments` is the
role-specific subset received as attachments. A readable Markdown body may
also link to the derived relative artifact path.

Renditions never replace originals. Their final path/manifest contract is
approved at the `0.1.7` renderer gate; A1 freezes only that a rendition cites
the original artifact ID, renderer contract/version, damage state, and its own
hash in a separate location.

#### Alternative considered: UUIDv7 artifact records

The alternative is `artifact_<uuidv7>` metadata records that point to a byte
hash. It supports several logical artifact records for the same bytes and gives
metadata an independent lifecycle. It also requires an ID-to-blob manifest,
creates merge reconciliation for immutable bytes, and makes direct filesystem
deduplication less transparent.

The recommendation prefers content-addressed artifact IDs because Fieldnotes'
v0.1 artifact is the retained original byte object. If later releases need a
mutable logical document or attachment-occurrence object, that should be a
separate typed record that references the content-addressed original.

### 3. Datetimes and Note filenames

Serialize every datetime as RFC 3339 with an explicit numeric offset:

```yaml
occurred_at: 2026-08-22T11:36:14+02:00
captured_at: 2026-08-22T11:38:02+02:00
```

Use a reliable source-local offset when supplied; otherwise use the configured
Field or client-local offset at that instant. UTC serializes as `+00:00`, not a
timezone-less value. Date-only properties remain `YYYY-MM-DD`.

Note filenames render the `occurred_at` instant in UTC at whole-second
precision:

```text
<YYYYMMDDTHHMMSSZ>_<field-id>_<type>_<note-id>.md
```

Example:

```text
20260822T093614Z_outlook_mail_work_mail_note_01a028d5-90c0-7248-a74b-c8bc1085ab19.md
```

Frontmatter is authoritative. Readers must not naively split the underscore-
delimited filename, because registered Field stems and labels may contain
underscores. A validator computes the expected filename from validated
frontmatter and compares the whole name. If event time changes, core atomically
installs the new filename before removing the old one.

#### Alternatives considered

- **All datetimes in UTC:** simpler byte comparison but discards source-local
  wall-clock context.
- **Timezone-less Obsidian-native datetimes:** attractive display behavior but
  loses the exact instant for generic readers and moving notebooks.
- **Offset in filenames:** harms filename portability/readability and global
  lexical ordering.

#### Consequences

- Offset-aware parsing and instant comparison are mandatory.
- Two frontmatter values may denote the same instant with different offsets;
  the filename is identical in that case.
- Obsidian compatibility must be demonstrated with offset-bearing fixtures,
  not achieved by dropping timezone information.

### 4. Field IDs, primary types, and source-property prefixes

Reserve `self` as the only one-part Field ID. External IDs are:

```text
<registered-field-stem>_<user-label>
```

Both stem and label match:

```regex
[a-z][a-z0-9]*(?:_[a-z0-9]+)*
```

Each is at most 31 ASCII bytes; the complete Field ID is at most 63 bytes.
Validation uses the configured registered stem and label rather than guessing
their split from underscores. The ID is unique within an instance and immutable
after producing Notes.

Approve these v0.1 stems and property prefixes:

| Field | Stem | Prefix |
|---|---|---|
| built-in self | `self` | none |
| local reference | `local` | `local_` |
| Outlook Mail | `outlook_mail` | `outlook_mail_` |
| Outlook Calendar | `outlook_calendar` | `outlook_calendar_` |
| Outlook Contacts | `outlook_contacts` | `outlook_contacts_` |
| Microsoft Teams | `teams` | `teams_` |
| Jira | `jira` | `jira_` |

Property names match `[a-z][a-z0-9_]*` and are at most 63 ASCII bytes. A source-
specific property begins with its registered prefix. Shared properties are
closed by the approved registry; a Field cannot invent an unprefixed property.

A Note may carry prefixed properties only for its own `field_id`'s registered
stem, plus unprefixed shared registry properties; a `teams_`-prefixed property
on a mail Note is a connector-boundary violation. The built-in `self` Field is
a registered Field like the others: its ID is one-part and it contributes no
property prefix, so a `self` Note may carry only unprefixed registry
properties. This does not introduce a `self_` prefix or permit `self_<label>`
Field IDs. Derived and projection records (`ext_`, `obs_`, `ent_`, `rel_`,
`prop_`, `conf_`, `pkg_`) may carry any registered prefix, because they
legitimately aggregate evidence across Fields. See
[ADR 0006](../decisions/0006-a1-implementation-rulings.md).

A primary Note type matches `[a-z][a-z0-9_]{0,31}` and must be one of:

```text
text message mail meeting call ticket document file contact event voice
```

This 32-byte grammar and vocabulary bound only the eleven primary Note types
above. Non-Note record types use a separate 63-byte grammar approved in
section 10.

Approve the semantic distinctions:

- `mail` remains distinct from chat-like `message`;
- Calendar source objects use `event`; a meeting record uses `meeting` when the
  meeting itself, rather than a calendar reservation, is primary;
- a generic imported/collected binary uses `file`; a source document whose
  text-bearing document identity is primary uses `document`;
- a playable user recording uses `voice`; `call` is an observed call record.

#### Alternatives considered

- **One `message` type for mail and chat:** smaller vocabulary but loses useful
  generic distinctions and pushes ordinary mail semantics into prefixes.
- **Hyphens in IDs/properties:** human-friendly but less uniform with Rust/YAML
  keys and existing examples.
- **Connector-defined primary types:** flexible but destroys cross-source views
  and stable property typing.

### 5. Canonical Markdown and flat YAML

Public record files are UTF-8 without BOM, use LF line endings, begin with one
frontmatter document delimited by `---`, and contain exactly one blank line
between closing frontmatter and Markdown body. The canonical body ends with one
LF.

Allowed frontmatter values are:

- text;
- finite JSON-compatible numbers;
- `true` or `false` booleans;
- `YYYY-MM-DD` dates;
- explicit-offset RFC 3339 datetimes;
- homogeneous, one-dimensional lists of one scalar type.

Missing values and empty lists are omitted. `null`, nested mappings, inline or
nested objects, mixed lists, duplicate keys, anchors, aliases, tags, binary
values, non-finite numbers, explicit YAML document-end markers, and multiple
YAML documents are invalid. Canonical lists use block style and remain lists
with one member. The serializer emits no YAML comments.

Canonical scalar spelling is deterministic:

- booleans are exactly `true` or `false`;
- numbers are finite IEEE 754 binary64 values serialized by the JSON
  Canonicalization Scheme number rule (RFC 8785 section 3.2.2.3); negative zero
  becomes `0`, and integers outside the exactly representable binary64 range
  are invalid rather than rounded;
- dates are exactly `YYYY-MM-DD`;
- datetimes use uppercase `T`, a numeric `+HH:MM` or `-HH:MM` offset (`-00:00`
  is invalid), and omit fractional seconds when zero or otherwise emit one to
  nine digits with trailing zeroes removed;
- text uses YAML plain style when and only when it matches
  `[A-Za-z0-9_./@+-]+(?: [A-Za-z0-9_./@+-]+)*` and resolves as a string under
  the YAML 1.2 Core Schema; every other text value uses double-quoted style
  with the RFC 8785 section 3.2.2.2 JSON string-serialization rule (including
  literal non-control Unicode and no optional solidus escaping);
- list items use two spaces, `-`, and one space before the scalar; an empty
  list is never emitted.

These rules, rather than a particular YAML library's defaults, define the byte
form. Parsers use the registry's property type and must not infer a different
public type from a scalar's spelling.

Order properties as follows:

1. For Notes: `id`, `instance_id`, `field_id`, `type`, `occurred_at`, omitting
   none of them.
2. For non-Note public records: `id`, then `type`.
3. Every remaining property follows in ascending ASCII byte order by key.

Ordering is serialization, not meaning. Parsers may accept a different key
order but the canonical serializer rewrites to this order. Set-like lists
(`collected_by`, `identities`, `entities`, and other registry-declared sets)
are deduplicated and sorted by their normalized text value. Role/order-bearing
lists preserve their registered semantic order. The property registry must
mark list semantics explicitly before a connector emits the property.

The Markdown body contains deterministically normalized source evidence, not a
model summary. Candidate fixtures demonstrate representative heading/body
templates; IG1 adds the missing `meeting`, `call`, and `document` examples
before the `0.1.0` compatibility suite is complete.

#### Alternatives considered

- **Lexicographic order for every key:** simpler, but hides the five structural
  Note properties in larger records.
- **Registry-table order for all keys:** attractive grouping, but adding a
  property changes ordering policy and requires every generic writer to carry
  the full registry.
- **Nested YAML or JSON objects:** preserve vendor structures but violate flat
  Obsidian/generic-tool behavior and stable property types.
- **Preserve arbitrary input formatting/comments:** friendly to hand edits but
  prevents one canonical byte form and complicates atomic source updates.

### 6. Required Note provenance and portable source identity

Every Note requires:

```text
id, instance_id, field_id, type, occurred_at
```

Every external Note with a stable source object additionally requires:

```text
source_scope, source_identity
```

`source_scope` is connector-namespaced, non-secret, and stable across
Fieldnotes instances connected to the same upstream authority/account.
`source_identity` is stable within that scope and includes an object-kind
namespace where upstream IDs are not unique across kinds.

The exact source-object identity is `(source_scope, source_identity)`.
Producer provenance is `(instance_id, field_id)`. Content hashes, filenames,
subjects, timestamps, and entity matches never substitute for either identity.

### 7. Current-state updates and authoritative deletion

Within a notebook there is at most one active Note for an exact portable source
key, except while a visible conflict is unresolved.

An update to that source object:

1. preserves the current Note ID;
2. replaces its current frontmatter/body atomically;
3. retains the demonstrably current `source_version` when supplied;
4. renames the file if the UTC event filename changed;
5. advances the Field cursor only after the replacement and referenced
   originals are durable.

v0.1 keeps no source revision ledger or persistent deletion tombstone. An
authoritative source deletion may remove the current Note. Absence caused by a
partial result, time window, pagination failure, permission loss, unavailable
history, or failed sync is not authoritative deletion. A pruned/deleted object
may receive a new Note ID if later refetched.

#### Alternatives considered

- **Append every update as a Note:** creates an audit/event ledger that conflicts
  with the disposable current-state product boundary.
- **Persist tombstones:** suppresses refetch but creates hidden historical state
  and complicates direct-copy merge.
- **Delete on snapshot absence:** efficient but unsafe unless the connector has
  explicitly declared and completed an authoritative full snapshot.

### 8. Merge survivor and `collected_by`

For this decision, two source Notes have equivalent current semantic payloads
when these values match after canonical validation:

- primary type and event/content metadata, including `occurred_at`;
- every registered shared and connector-prefixed property other than the
  bookkeeping exclusions below;
- normalized Markdown body bytes;
- ordered artifact references.

Exclude `id`, `instance_id`, `field_id`, `captured_at`, `collected_by`,
`source_version`, the derived `content_hash`, and the rebuildable Note
projection properties `entities` and `related` from semantic comparison.
`source_scope` and `source_identity` remain part of source-object comparison;
`source_version` is compared separately only through the connector's approved
ordering rule.

For internal conflict detection, compute
`fn-record-v1-sha256:<hex>` over a domain-separated canonical encoding of that
semantic payload. It is an implementation/checking fingerprint, not public
required frontmatter. The byte input is ASCII `fieldnotes-record-v1`, one NUL
byte, and the canonical semantic record bytes specified by the
[A1 hash vectors](../../tests/fixtures/hashes/proposed-v1/README.md).
Implementations must not substitute language-specific map or debug
serialization.

The semantic-record encoding uses the public canonical emitter after removing
excluded properties, with two differences from the public form. First,
datetime values are rendered as their instant in UTC with `+00:00`; thus two
source/client-local offsets for the same instant compare equally while the
surviving public Note retains its meaningful local offset, and date-only
values are unchanged. Second, every retained key sorts in ascending ASCII
byte order with no structural-keys-first exception, so `type` sorts among the
ordinary keys rather than first; the public five-structural-keys-first rule in
section 5 is a human-readability affordance for public files and does not
apply to this machine-only hash input. The semantic encoding is a hash input
and is never a publishable notebook record; the checked-in
`semantic-record-canonical.md` vector is a hash-input fixture, not a notebook
file. See [ADR 0006](../decisions/0006-a1-implementation-rulings.md).

Approve this deterministic survivor rule:

1. Same Note ID plus identical semantic content is one Note.
2. Same Note ID plus divergent semantic content is a conflict, even when one
   candidate appears newer; this protects against corruption and tampering.
3. Different Note IDs plus the same portable source key and equivalent current
   semantic content collapse to the lexicographically smaller Note ID.
4. If the same portable source key has different content and the connector
   supplies a reliable comparable `source_version`, use the newer content under
   the lexicographically smaller Note ID.
5. If no reliable ordering exists, preserve both candidates as a conflict.
6. Same content hash without the same portable source key never collapses Notes.

The survivor's original `(instance_id, field_id)` remains the required primary
producer pair. `collected_by` is a sorted, deduplicated list of **all** known
`<instance_id>/<field_id>` producer references, including that primary pair,
when more than one producer is known. It is omitted when exactly the required
primary producer is known.

This slightly tightens the earlier “additional producers” wording: once
present, `collected_by` is self-contained rather than requiring consumers to
remember to union an implicit producer.

#### Alternatives considered

- **Keep the first merge operand:** makes results depend on copy/command order.
- **Always keep the newer UUIDv7:** ID creation time does not establish source
  freshness.
- **Keep the record carrying newer content and its ID:** preserves one local
  pairing but makes survivor identity change with merge direction/version.
- **Always emit `collected_by`:** simpler reading but repeats the required pair
  on every unmerged Note.

#### Consequences

- Merge is order-independent for non-conflicting exact-source duplicates.
- The selected Note ID may originate from an older content candidate; source
  version determines current content, while UUID ordering determines identity.
- Producer history is preserved without nested YAML.

### 9. Normalized body content hash

Use this public value form:

```text
fn-content-v1-sha256:<64-lowercase-hex-digits>
```

Hash only the canonical normalized Markdown body bytes, not frontmatter,
filenames, IDs, timestamps, producer/source identity, or artifact bytes. Before
hashing, v1 normalization:

1. requires valid UTF-8;
2. removes a leading UTF-8 BOM if present at ingestion;
3. converts CRLF and lone CR to LF;
4. preserves Unicode code points without NFC/NFKC normalization;
5. preserves all other whitespace/content bytes;
6. ensures exactly one final LF.

The required empty line after the closing frontmatter delimiter is a file
separator, not the first byte of the Markdown body, and is not hashed. The body
begins with the first Markdown content byte after that separator.

The SHA-256 input is domain-separated:

```text
fieldnotes-content-v1\0<canonical-body-bytes>
```

where `\0` is one zero byte. Attachments are represented by their independent
artifact IDs/hashes and do not enter this body hash.

The hash detects normalized body equality. It is not Note identity, source
identity, a complete semantic record hash, or permission to discard context.
Any normalization change requires a new domain version.

#### Alternatives considered

- **Hash the complete Markdown file:** causes provenance/capture/serialization
  changes to appear as content changes.
- **Hash body plus selected metadata:** moves semantic-schema policy into a
  value intended to identify normalized content.
- **Unicode normalization and whitespace trimming:** increase superficial
  matches but change evidence and invalidate exact extraction offsets.
- **Hash raw source bytes:** impossible across Fields that deterministically
  render different vendor envelopes into one notebook contract.

### 10. Derived record IDs and filenames

Approve these directories and filename forms:

```text
extractions/<ext-id>_<type>.md
observations/<obs-id>_<type>.md
entities/<ent-id>_<type>.md
relationships/<rel-id>_<type>.md
proposals/<prop-id>_<type>.md
packages/<pkg-id>/manifest.md
```

The ID already carries the kind prefix. Non-Note record types match
`[a-z][a-z0-9_]{0,62}` — 63 ASCII bytes, matching the property-name bound —
rather than the 32-byte primary-Note-type grammar in section 4. Derived types
are an open, registry-reviewed vocabulary where descriptive multi-word names
are the point; filename headroom stays ample (`obs_` plus a 36-byte UUID plus
`_` plus a 63-byte type plus `.md` is 107 bytes, far under the 255-byte
filesystem limit). See [ADR 0006](../decisions/0006-a1-implementation-rulings.md).
Derived records use flat YAML, explicit-offset datetimes, the deterministic
ordering rule, readable Markdown bodies, and evidence references.

Extraction and Observation capability-specific types/properties are not all
approved merely by reserving their envelopes. Each enhancement capability in
`0.1.8` still requires evidence fixtures and generator validation. Package
selection/dependency schema remains an `0.1.7` gate.

#### Alternative considered: timestamped derived filenames

Timestamps help directory browsing but make regenerated projections look like
source events and create rename churn. Derived records are located by ID/type;
their meaningful evidence times remain explicit properties.

### 11. Visible conflict layout

Store unresolved reconciliation material outside active `notes/`:

```text
conflicts/<conf-id>/
├── conflict.md
├── candidate_1.md
└── candidate_2.md
```

`conflict.md` has ID prefix `conf_`, a registered conflict `type`, explicit-
offset `detected_at`, sorted `fn-record-v1-sha256` candidate fingerprints,
involved Note/source IDs, producer references, and a readable explanation.
Candidate files preserve the complete validated input bytes. Candidate
numbering follows ascending record fingerprint, with SHA-256 of the exact
canonical candidate file bytes as a tie-breaker, so input order does not change
the bundle content order.

When a conflict moves two otherwise-active source Notes into a bundle, no Note
is silently declared current. Status/inspect exposes the gap. Resolution writes
one validated current Note, preserves the decision in durable private intent
when human judgment was required, and removes the resolved working bundle.

A conflict bundle is safety material for unresolved current state, not a source
revision ledger. Conflict IDs are UUIDv7 and may differ when independently
created; merge reconciles equivalent bundles by their candidate hashes.

#### Alternatives considered

- **Keep one candidate active and store only the loser:** makes the apparent
  current state depend on merge direction.
- **Suffix conflicting Note filenames in `notes/`:** breaks the one filename
  contract and lets generic readers mistake both candidates for active Notes.
- **Last-writer-wins:** silently loses evidence and is prohibited.

The conflict property names and candidate fixture bytes are included in the A1
registry and review corpus.

### 12. Proposal and handback record envelopes

A proposal is a public, readable working record:

```text
proposals/<prop-id>_<type>.md
```

It uses flat frontmatter, cites current evidence, describes vendor-neutral
existing/proposed values in Markdown, and never contains an executable vendor
API payload. `prop_` IDs are stable for the lifetime of the proposal. Human
review state is durable private intent under `.fieldnotes/state/proposals/` and
may be projected as a registered public `status` text value. Ordinary graph
rebuild must not discard proposal files or accepted/rejected review intent.

An entity-targeting proposal carries both the current `entity_id` and a stable
`subject_identity` anchor such as a normalized qualified email identity. On
graph rebuild, core rebinds `entity_id` only when that anchor resolves to
exactly one current entity. It also carries `binding_status` from this closed
vocabulary:

```text
bound unresolved ambiguous
```

`bound` requires exactly one current `entity_id`. `unresolved` and `ambiguous`
require `entity_id` to be omitted, so a stale projection ID is never presented
as current. The stable `subject_identity`, evidence, status, and Markdown
explanation remain. Missing or ambiguous resolution never silently selects a
candidate or discards the proposal. Rebinding rewrites the public projection
atomically while durable review intent remains keyed by the stable `prop_` ID.

Approve the initial public status vocabulary:

```text
proposed accepted rejected superseded
```

Approve `entity_update` as the initial proposal type. Additional proposal types
require registry review rather than ad hoc connector properties.

A handback package is preparation output, never delivery or writeback:

```text
packages/<pkg-id>/manifest.md
```

The `pkg_` UUIDv7 and flat manifest envelope are reserved at A1 so later files
can reference a package consistently. Exact selection semantics, dependency
closure, copied-versus-referenced artifacts, checksum manifest fields,
encryption, destination hints, and package status remain explicitly unapproved
until the `0.1.7` handback gate.

#### Alternatives considered

- **Proposal status only in public Markdown:** portable but a generated rebuild
  can erase human review intent.
- **Proposal state only in private config:** rebuild-safe but copied proposals
  lose visible review status.
- **Vendor-neutral JSON patch:** appears structured but becomes a destination
  write schema and encourages automatic action beyond Fieldnotes' boundary.
- **Approve the full package schema now:** blocks A1 on lifecycle questions that
  are not needed for the 0.1.0 notebook kernel.

## Compatibility and change policy

After A1 approval:

- core serializers must emit the approved canonical bytes;
- validators may accept explicitly documented non-canonical key order or CRLF,
  but rewrites produce the canonical form;
- a property name never changes meaning or scalar/list type within v0.1;
- new shared properties, primary types, prefixes, or record kinds require
  registry review and fixtures;
- source-specific additions remain under the approved connector prefix;
- changing ID grammar, datetime semantics, filename grammar, hash input,
  artifact identity, or merge survivor behavior requires an explicit format
  version/migration proposal;
- A1 does not approve A2 Field JSON schemas, executable discovery, credential
  IPC, connector capability details, renderers, model outputs, or destination
  writes.

## Fixture and hash-vector evidence required for implementation

The integrated candidate inputs make the recommended choices concrete enough
for contract review; they are not presented as the complete executable
compatibility suite. IG1 implementation expands the corpus before `0.1.0`
release to cover at least:

- instance metadata;
- `self` text, file, and voice Notes;
- one external Note for local, Outlook Mail, Calendar, Contacts, Teams, and Jira;
- explicit-offset values crossing UTC date boundaries and a UTC `+00:00` value;
- every approved primary type and Field prefix;
- single-member lists, quoting-sensitive text, booleans, numbers, and dates;
- damaged/truncated material;
- exact artifact bytes, canonical extension, artifact ID, relative link, and
  byte hash;
- normalized body vectors for LF, CRLF, BOM, preserved Unicode code points,
  trailing whitespace, and exactly-one-final-LF behavior;
- semantic-record fingerprint vectors proving that producer, capture,
  `collected_by`, source-version, and derived-content-hash bookkeeping does not
  change current payload equality;
- equivalent-source merge with deterministic survivor and sorted
  `collected_by`;
- reliable newer `source_version`, unordered divergence, and same-ID conflict;
- Extraction, Observation, entity, relationship, proposal, package manifest
  envelope, and conflict bundle;
- rejected nested/mixed/null/tagged/duplicate-key/timezone-less/unprefixed
  inputs.

Content secret-scanning is withdrawn per
[ADR 0006](../decisions/0006-a1-implementation-rulings.md) ruling 3: a fixture
or implementation must not reject collected evidence merely for containing
secret-looking text, and no `security.secret_detected` rejection is required.

Every fixture must identify whether it is normative at A1 or illustrative for
a later capability gate. Hash vectors must state exact input bytes in a form
that reviewers can independently reproduce.

## Explicit approval checklist

A1 is approved because the user explicitly accepted each checked choice below,
and the integrated fixtures match it.

### Identity and naming

- [x] Lowercase hyphenated UUIDv7 with `fn_`, `note_`, `ext_`, `obs_`, `ent_`,
  `rel_`, `prop_`, `pkg_`, and `conf_` prefixes for logical records.
- [x] The `.fieldnotes/instance.yaml` operational metadata exception, exact
  three-key schema/order, and creation-time agreement with its `fn_` UUIDv7.
- [x] Content-addressed `artifact_sha256_<hex>` as the exception for immutable
  original bytes, instead of UUIDv7 artifact metadata IDs.
- [x] Flat original path `artifacts/<artifact-id>.<canonical-extension>`, the
  initial media-type extension registry, and artifact-ID-only references in
  `artifacts`/`attachments`.
- [x] UTC Note filename grammar and explicit-offset RFC 3339 frontmatter.
- [x] Field stem/label, property name, primary type, byte-length, and registered
  prefix grammars exactly as recommended.
- [x] The eleven primary Note types and the mail/message, event/meeting,
  file/document, and voice/call distinctions.

### Serialization and hashes

- [x] UTF-8/LF/frontmatter/body boundary and the strict flat YAML subset.
- [x] Required structural keys first, then remaining keys in ascending ASCII
  order; registered set-like lists sorted/deduplicated.
- [x] `fn-content-v1-sha256` over the exact recommended normalized body bytes
  with the `fieldnotes-content-v1\0` domain separator.
- [x] Internal `fn-record-v1-sha256` canonical semantic comparison and vectors,
  with the approved bookkeeping exclusions.
- [x] Original artifact SHA-256 hashes exact bytes and is never compared as a
  Note/source/content identity.
- [x] Candidate fixtures and hash vectors are byte-for-byte correct within the
  A1-versus-later-gate classification stated by each corpus README.

### Current state, merge, and conflicts

- [x] Required Note properties and external `(source_scope, source_identity)`
  identity.
- [x] Atomic current-state update under the same Note ID, authoritative deletion,
  no revision ledger/tombstone history, and refetch recovery.
- [x] Lexicographically smaller Note-ID survivor for exact-source duplicates,
  reliable `source_version` selecting content, and conflicts when unordered.
- [x] `collected_by` contains all producers including the primary pair when
  emitted, and is omitted for a single producer.
- [x] Same Note ID with divergent content is always a conflict; content-hash
  equality alone never removes contextual Notes.
- [x] `conflicts/<conf-id>/` bundle layout, no active candidate while unresolved,
  and durable private intent for human resolution.

### Derived and downstream-facing records

- [x] Non-timestamped derived filename forms and UUIDv7 projection IDs.
- [x] Proposal file envelope, `entity_update`, public projected status
  vocabulary, closed binding-status states with no stale `entity_id`, stable
  subject-anchor rebinding, and private durable review intent.
- [x] `pkg_` package ID and `packages/<pkg-id>/manifest.md` envelope are reserved,
  while full handback schema remains deferred to `0.1.7`.
- [x] A1's compatibility/change policy and the explicit boundaries left for A2
  and later release gates.

## Approval effect

Approval unblocks implementation of domain ID/value types, canonical
serialization and validation, atomic Note/artifact storage, exact-source merge,
golden fixtures, and the `0.1.0` local notebook kernel.

It does not unblock real Field protocol implementation or live connector work.
Those remain behind A2's exact JSON schemas, protocol transcripts, artifact
transfer, protected credential delivery, and conformance suite.
