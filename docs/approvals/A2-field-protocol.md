# A2 approval: Field process protocol

**Status:** Ready for review. Not approved. Approval is explicit and is never
inferred from silence or from implementation.  
**Scope:** The transport and process boundary between Fieldnotes core and an
external Field process: framing, version negotiation, the `describe` manifest,
the collection request, the `record`, `checkpoint`, and `diagnostic` events,
artifact transfer, portable exact-source identity and identity anchors, cursor
and checkpoint durability, authoritative deletion, protected credential
delivery, diagnostics and redaction, exit codes and partial failure, and
untrusted-output bounds

## Decision requested

A2 freezes protocol v1 as the shared contract between core and every external
Field, in every language, for the whole `0.1.x` line. It is the third and last
of the three explicit approval gates, and it blocks all local and live Field
implementation: `0.1.1`'s Field SDK and `local` Field, and every Microsoft and
Jira Field after it.

The recommendation is to approve the choices in the numbered sections below
together with the attached candidate schemas and transcripts. Nothing in this
document is in force until the user says so.

The review corpus is attached as:

- [candidate protocol schemas and transcripts](../../tests/fixtures/protocol/proposed-v1/README.md).

Those bytes make the proposal reviewable. A2 approval would freeze them as the
implementation target; the Rust DTOs, the conformance kit, the fixture Field,
and the executable crash, hostile-output, and secret-canary tests are the
subsequent IG2 implementation evidence, not a prerequisite for choosing the
contract. This mirrors how A1 was reviewed and then implemented.

The detailed background lives in:

- [Field process protocol](../field-protocol.md)
- [Fields](../fields.md)
- [Operations and lifecycle](../operations.md)
- [Security and privacy](../security.md)
- [Architecture](../architecture.md)
- [Identity and deterministic graph](../identity-and-graph.md)
- [Property and record-type registry](../property-registry.md)
- [Artifacts and renditions](../artifacts.md)
- [Field process boundary ADR](../decisions/0005-field-process-boundary.md)
- [Source identity and merge ADR](../decisions/0002-source-identity-updates-and-merge.md)
- [A1 implementation rulings ADR](../decisions/0006-a1-implementation-rulings.md)

## What A2 must not do

The roadmap is explicit that A2 "cannot silently define A1 record vocabulary."
[A1](A1-notebook-contract.md) froze record types, property names, ID grammars,
datetime serialization, filenames, hashes, and canonical bytes. A2 defines the
transport and process boundary only.

Where the protocol must refer to notebook vocabulary it cites A1 rather than
restating or extending it. Concretely, and by design:

- the schemas constrain the *grammar* of a primary Note type but do not
  enumerate A1's eleven approved types; core validates the value against the A1
  registry, so there is no second copy of that vocabulary to drift;
- the record envelope does not state which unprefixed property names are legal,
  because that is A1's closed shared registry. It states only that a Field may
  use registry names and its own declared prefixed names, and it structurally
  excludes the names core owns;
- datetime, artifact-identity, filename, hash, key-ordering, and canonical
  scalar rules appear nowhere in the protocol as rules. A Field emits values;
  core alone turns them into canonical bytes.

Transport-level grammars in the schemas are **well-formedness guards** for an
untrusted child process, not definitions. A1 remains authoritative and core
still performs full A1 validation after a guard passes. Section 14 explains why
guards exist at all.

## Recommended protocol

### 1. Transport, operations, and framing

An external Field is a short-lived child process that core starts for one
bounded operation. Two operations exist in v1, selected by a single argv token:

```text
<field-executable> describe
<field-executable> collect
```

Protocol data uses standard input and standard output. Logs use standard error.
Credential material uses a separate protected channel and nothing else.

Framing is newline-delimited JSON: one JSON object per line, UTF-8 without a
BOM, terminated by one LF. There is no envelope wrapping the stream and no
length prefix. Every frame carries `v` (the protocol major version), `type`
(the discriminator), and `run_id` (core's identifier for this run). Every
Field-to-core event on standard output additionally carries `seq`.

```text
core  -> describe_request                 (describe run)
field -> manifest
field -> exit

core  -> collect_request                  (collect run)
field -> record        seq 1
field -> record        seq 2
field -> checkpoint    seq 3
field -> diagnostic    seq 4
core  -> cancel                           (optional, at any time)
field -> exit
```

`seq` starts at 1 and increases by exactly 1 across every event of one collect
run, records, checkpoints, and diagnostics sharing one sequence. A gap, a
repeat, or a regression is a protocol violation, because it is the cheapest
possible detection of a truncated, interleaved, or reordered stream. Ordering
is a protocol rule that a single-frame schema cannot express, so core enforces
it separately; the transcripts mark such frames with the code core must reject
them with. A repeat is `protocol.duplicate_seq`, a regression is
`protocol.seq_regression`, and a gap is `protocol.seq_gap` — its own code,
distinct from `protocol.unexpected_order`, which is reserved for a frame
arriving somewhere the protocol does not allow it at all rather than for a
hole in an otherwise well-ordered stream. **This corrects the proposal as
originally drafted**, which overloaded `protocol.unexpected_order` for a gap
and, separately, for a checkpoint's `records_covered` disagreeing with what
core actually received — see section 9's `protocol.coverage_mismatch`. Both
are now distinguishable in logs, metrics, and tests from an ordering violation
that has nothing to do with counting.

The built-in `self` Field does not use this protocol. It is a registered A1
Field with no process, no credential, and no prefix.

#### Alternatives considered

- **Length-prefixed binary framing:** removes the frame-size scan and carries
  bytes natively, but is unreadable in a terminal, unreviewable as a fixture,
  and hostile to a connector written in a scripting language. Fieldnotes'
  boundary is a trusted adapter interface whose fixtures must be reviewable by
  a person; NDJSON keeps every transcript a diffable text file.
- **One long-lived Field daemon with request multiplexing:** amortizes process
  and authentication startup, but turns failure isolation into shared state,
  makes cancellation and resource bounds much harder, and gives a connector's
  memory an unbounded lifetime. `0.1.x` has no throughput requirement that
  needs it.
- **A single stream carrying both logs and protocol:** fewer file descriptors,
  but a connector library that prints a warning to standard output would
  corrupt the protocol, and every diagnostic would need to be protocol-aware.
  Splitting them means an accidental `print` is noise on standard error rather
  than a failed run.

#### Consequences

- Core must scan for LF with a hard byte bound rather than reading whole lines
  optimistically.
- A Field must not write anything but frames to standard output; conformance
  includes a case where it does.
- Transcript fixtures are text and can be reviewed, diffed, and hand-edited.

### 2. Protocol-version negotiation

`v` and `protocol_version` are the major version, fixed at `1` for this
proposal. `protocol_revision` is an additive-only revision within a major
version.

Negotiation happens entirely inside the describe run, before any credential
grant, staging directory, or collect run exists:

1. core writes `describe_request` naming every major version it supports and
   the highest revision it understands;
2. the Field selects one major version from that set and answers with a
   `manifest` declaring the version it selected, its own revision, and every
   version it supports;
3. the negotiated revision is the minimum of the two declared revisions.
   Neither peer may emit a member introduced above it.

`describe_request`'s `limits` member is **optional**, unlike `collect_request`'s
required one: a describe run emits at most one manifest and reads no records,
artifacts, or diagnostics, so it has almost nothing to bound, and requiring the
full thirteen-member ceiling table would make every Field parse numbers it
cannot act on. When core does state `limits` on a describe run, it still
enforces the frame-size ceiling within it.

Failure is closed and actionable in both directions. A Field that supports no
version core offered emits no manifest — a manifest it cannot express correctly
is worse than none — writes one actionable line naming both version sets to
standard error, and exits with the negotiation exit code. Core aborts that
Field's sync, reports both version sets and the concrete remedy, resolves no
credential, and does not attempt collection. A Field that answers with a
version core did not offer has its manifest rejected as invalid rather than
partially interpreted.

#### Alternatives considered

- **Ignore unknown members and continue:** the usual forward-compatibility
  posture, and wrong here. An unknown member from a child process is exactly
  the case that must fail closed: silently dropping a member could discard a
  deletion authority, a completeness claim, or a declared property type, and
  the failure would be invisible.
- **Only a major version, with no revision:** simpler, but then adding one
  optional manifest member is a breaking change for every Field, which
  guarantees that either the version churns or people add members informally.
- **Negotiate per operation instead of per Field:** would allow a Field to
  change version mid-life, which is churn without a use case.

#### Consequences

- Every schema sets `additionalProperties: false`, and an unknown member is a
  failed run rather than a warning.
- A Field build and a core build can be mismatched, and the mismatch surfaces
  as one actionable message rather than a mysterious rejection later.
- Version negotiation costs one extra process start per sync. That is
  acceptable and is also where the manifest snapshot check in section 4 happens.

### 3. The `describe` manifest

The manifest is a Field's complete self-declaration and the only thing core
consults about a Field's powers. It declares:

- the negotiated protocol version, its revision, and every version supported;
- the driver name and version;
- the registered A1 Field stem and the registered property prefix, **absent**
  for a Field that contributes none. *This corrects the proposal as originally
  drafted*, which made `property_prefix` required-but-nullable — a shape that
  traps an implementer twice over: an omitted member and an explicit `null`
  are indistinguishable failure modes at the type level, and the omitted case
  was meant to be read as "contributes no prefix" while a stray `null` could
  silently disable all prefix checking instead of failing loudly. The member
  is now absent-or-present like every other optional manifest member: present
  with a string means that prefix, absent means none, and `null` is simply not
  a value the schema admits;
- `declared_properties`: the exhaustive prefixed-property declaration in
  section 4;
- `capabilities`: the source-object slices this release actually supports, each
  naming its connector-local object kind, the primary Note type it maps to, and
  whether it emits artifacts or identity anchors;
- `source_key`: how the Field derives the portable exact-source key, per
  section 7;
- `identity_anchors`: the anchor namespaces it may emit, per section 7;
- `auth`: authentication kind, whether a credential profile and the protected
  channel are required, least-privilege scopes, refresh ownership, and a
  constant `writes_to_source: false`;
- `collection`: incremental support, the cursor format version, supported
  modes, window support, refetch capability, and declared deletion authority;
- `limitations`: known permission, history, or coverage limits, which the
  roadmap's release controls require a narrowing connector to surface.

A capability list documents what a release supports. It is never a claim that
the Field covers everything its vendor offers, and narrowing is legitimate as
long as it is declared here and surfaced by status output.

Three manifest members are constants rather than choices, because their only
honest value is one value: `writes_to_source` is always false, a portable
scope's dependence on the user's Field label is always false, and an identity
anchor never substitutes for the source key. Encoding them as constants makes a
connector that disagrees fail schema validation instead of quietly behaving
differently.

#### Alternative considered: infer capabilities from behavior

Core could learn what a Field does by watching what it emits. That is
attractive because it cannot go stale, and it is rejected for one decisive
reason: deletion authority and snapshot completeness must be declared *before*
they are exercised. A Field that could acquire the power to delete Notes by
emitting a frame has an unreviewable privilege. Declaration first, enforcement
on every frame, is the only ordering that makes "absence is not deletion"
enforceable rather than aspirational.

### 4. Declared connector-prefixed properties

**This section closes the gap [ruling 4](../decisions/0006-a1-implementation-rulings.md)
assigned to A2 and is required content, not optional.**

A1 registered property *prefixes* but no registry entry exists for an
individual prefixed property, so IG1's interim rule inferred a prefixed
property's type from its canonical spelling. That inference is round-trip
stable but it is inference, and it cannot express list semantics at all.

Each Field's `describe` manifest declares every connector-prefixed property it
may emit. Each declaration carries:

- `name`: the full prefixed property name, which must begin with the manifest's
  own registered `property_prefix`;
- `value_type`: one of A1's scalar types — `text`, `number`, `boolean`, `date`,
  `datetime`;
- `cardinality`: `scalar` or `list`;
- `list_semantics`: required for a list and forbidden for a scalar, either
  `set` (sorted and deduplicated by normalized value) or `ordered` (source or
  role order preserved), matching A1's two list classes;
- `description`: a short human explanation for review.

Core then enforces, on every record:

1. a prefixed property the declaring manifest does not list is **rejected**;
2. a declared property whose emitted JSON shape contradicts its declared
   `value_type` or `cardinality` is **rejected**;
3. a prefixed property belonging to another Field's registered stem is
   **rejected**, which is A1 section 4's prefix-to-producer binding;
4. an unprefixed name outside A1's closed shared registry is **rejected**;
5. spelling-based inference is retired for declared properties. Core takes the
   type from the declaration;
6. an unprefixed name that the registry types for a **derived record** only —
   `confidence`, `generated_at`, `evidence_spans`, `binding_status`, and the
   rest of that subset, listed precisely alongside the rest of A1's shared
   registry entries — is **rejected**, even though it is a registered A1
   property type. **The Note-applicable subset of A1's shared registry is
   named precisely and enforced**: a Field collects a Note, never a derived
   record, so it never has an evidence span, a confidence score, or a binding
   status to report, and the fact that the registry also carries names for
   generated Notes does not make those names available to a collecting Field.
   Rejected as `record.unknown_property`, the same code an unregistered name
   gets, because from a collecting Field's point of view the name is equally
   unusable either way;
7. a record's `note_type` that disagrees with the `note_type` its
   `object_kind`'s capability slice declares is **rejected** as
   `record.note_type_not_declared`, even when the value itself is one of A1's
   eleven approved types. **A capability slice's declared `note_type` is now
   enforced**, not merely descriptive: "declare before exercise" is this
   package's own principle for capability, deletion authority, and snapshot
   authority, and without this check a slice's declared `note_type` would be
   decoration a Field is free to ignore, which contradicts that principle for
   the one manifest member it had not yet been applied to.

A manifest may not declare unprefixed properties. Those belong to A1's closed
shared registry and take their type from it; a Field uses them by name or not
at all.

Core snapshots each configured Field's manifest. **Adding** a declared
property is a Field release change, not a migration, and needs none: a name
the Field starts emitting carries no prior notebook data to retype. **Changing
or removing** one requires a migration; core refuses to sync that Field until
it happens, rather than either retyping notebook data in place or losing track
of what a no-longer-declared name used to mean. If a later manifest changes a
declared property's `value_type` or `cardinality`, removes a declared property
outright, or changes `cursor_format_version`, core refuses to sync closed. This
is A1's rule that "a property name never changes meaning or scalar/list type
within v0.1", made enforceable at the boundary where the change would arrive.
*This corrects the reference implementation as first written*, which treated
removal as freely allowed on the reasoning that "a name the Field no longer
emits cannot retype anything" — true as far as it goes, but the manifest is the
only place that property's type and list semantics were ever recorded, so once
the declaration disappears core can no longer tell a previously-collected value
of that property from an undeclared one on the next divergence check. Removal
is now a migration for the same reason a type change is.

#### Alternatives considered

- **Keep spelling-based inference:** no new mechanism, and it is round-trip
  stable. But it cannot express set-versus-ordered list semantics, so the
  canonical serializer would not know whether to sort a connector's list — and
  A1 section 5 requires that the semantics be declared *before* a connector
  emits the property.
- **A central registry file for prefixed properties:** consistent with shared
  properties and reviewable in one place, but it puts every connector's vendor
  vocabulary in a core-owned file, so adding a Jira field becomes a core
  change. The manifest keeps connector vocabulary with the connector while
  still making it declared, snapshotted, and enforced.
- **Allow undeclared prefixed properties with a default of text:** maximally
  permissive and quietly destructive: a number would silently become text in
  one notebook and a number in another.

#### Consequences

- Adding a prefixed property is a Field release change reviewed with that
  Field's fixtures, and it is visible in the manifest diff.
- Changing **or removing** one is a migration, and core says so instead of
  guessing or silently forgetting.
- The interim inference rule survives only as the reading rule for notebooks
  written before `0.1.1`, which by construction contain no prefixed property at
  all, because only `self` ships at `0.1.0` and `self` has no prefix.
- A Field's own vendor vocabulary is bounded by two independent checks now,
  not one: the declared-property mechanism above governs its *prefixed*
  names, and the Note-applicable subset governs which of A1's *unprefixed*
  shared names it may use at all.

### 5. The collection request

Core writes one `collect_request` carrying: the negotiated version and
revision; the configured `field_id`; the `mode`, either `incremental` or
`snapshot`; the last committed opaque `cursor` and the format version it was
stored at, when one is replayable; an optional bounded `window`; the
`snapshot_scope` in snapshot mode; non-secret `config` as flat scalars and
homogeneous scalar lists; at most a credential *reference* and channel
descriptor; the `artifact_staging_dir`; the effective `limits`; the required
`artifact_media_types` retention policy; an optional `recollect_targets`; and
the `deadline`.

Two omissions are deliberate. The request carries no credential material — see
section 12. It also carries no `instance_id`: producer provenance is core's,
the Field has no use for it, and not sending it means a Field cannot embed it in
a cursor, a diagnostic, or an upstream request.

`config` is non-secret by construction, because core never puts credential
material there. A Field must not treat any value in `config` as a secret.

**`artifact_media_types` (ADR 0007) is required, for the same reason `limits`
is.** It states the effective media-type retention include set — an exact
`type/subtype` media type or a subtype wildcard such as `image/*` — in
addition to the size threshold `limits.max_artifact_bytes` already states. A
Field compares a known attachment's declared media type against this set
*before* staging it, exactly as it already compares size against
`max_artifact_bytes`: included, stage normally; excluded, emit a
`not_retained` reference instead of bytes core would otherwise have to
reject. This is orthogonal to A1's frozen media-type-to-extension registry,
which governs how a *retained* original is named rather than whether it is
retained at all; a type may be included here and still have no canonical
extension, falling back to `.bin` per A1 section 2. The default v1 include
set (documents and text, images, and audio; video, archives, disk images, and
installers/executables excluded) is fixed in
`fieldnotes_field_protocol::limits::default_artifact_media_types`, and, like
`max_artifact_bytes`, is a per-run/settings-configurable default rather than
a ceiling: there is no absolute media type list that protects core from a
hostile child, only a sane bound on how long the list may be.

**`recollect_targets` (ADR 0007) is optional and orthogonal to `mode`.** It
names a bounded list of previously-collected source objects, each by its
portable exact-source key alone, that core asks the Field to explicitly
recollect: refetch current metadata and re-evaluate every known attachment
against the *currently* effective retention policy, regardless of the last
committed cursor. It exists because normal incremental sync moves forward
from a cursor and never revisits settled objects, so raising
`max_artifact_bytes` or widening `artifact_media_types` could otherwise never
reach an attachment a Field already reported and declined. When present,
`cursor` and `window` are absent — recollection is scoped exactly to its
named targets, not to a cursor-bounded or windowed range — and it is never
combined with `snapshot` mode, which already reconciles everything inside its
declared scope. No new manifest capability was added for it: whether a
configured Field can honor a recollection request at all is exactly what its
existing `collection.refetch` declaration (`supported` / `bounded` /
`unsupported`) already governs. A2 specifies this request shape; it does not
build the re-collection *operation* that issues it, which is `0.1.1`
sync-command scope (see `docs/roadmap.md`'s `0.1.1` entry).

### 6. The record envelope: a normalized source envelope

**The roadmap requires A2 to be explicit about whether a `record` carries a
normalized source envelope or a nearly rendered Note candidate. The
recommendation is the normalized source envelope.**

A record is **post-mapping and pre-serialization**. The Field has already done
the work only it can do — mapping vendor structures onto Fieldnotes vocabulary —
and has done none of the work only core may do.

A record carries:

- `change`: `upsert` or `delete`;
- `source`: the portable exact-source key, plus optional non-identity source
  metadata (`version`, `url`, `parent_identity`);
- `object_kind`: the declared capability slice it belongs to;
- `note_type`: the primary Note type candidate, which must also equal the
  `note_type` the manifest's capability slice for this `object_kind` declares.
  **This equality is now enforced**, not merely descriptive: a value that
  clears the A1 registry check but disagrees with the declared slice is
  rejected as `record.note_type_not_declared` (section 4);
- `occurred_at`: the event instant with an explicit offset;
- `properties`: flat property candidates keyed by A1 property names — shared
  registry names from the **Note-applicable subset** of the closed registry
  (section 4), plus this Field's declared prefixed names;
- `body`: deterministically normalized source evidence as Markdown text;
- `artifacts`: original-byte references in role order, per section 8;
- `identity_anchors`: structured anchors, per section 7;
- `integrity`: `damaged`, `truncated`, and measured `lost_characters`.

**A number crosses the boundary in its wire spelling, verbatim.** The
reference implementation keeps a numeric property value as the JSON number the
Field sent — not decoded to a binary floating-point value and re-encoded —
because decoding to `f64` and back would turn `8814423` into `8814423.0`,
breaking the round-trip the evidence list requires. A1 owns canonical number
spelling (RFC 8785); A2 deliberately says nothing about it and simply carries
the Field's spelling through unchanged until core's own canonicalization step
decides the final bytes.

A `date` value that is not a well-formed A1 date-only string is rejected as
`record.invalid_date`, distinct from `record.invalid_datetime` (a manifest may
declare a property's `value_type` as `date`, and a Field that sends
`"August 20"` for one is a different mistake from one that sends a malformed
instant). **The envelope's own instants are guarded at decode**: `occurred_at`
and `observed_at` are explicit-offset RFC 3339 values checked by the transport
grammar itself, before a record is even accepted, so `record.invalid_datetime`
can in practice only fire for a *declared or registered temporal property*
inside `properties` — never for the envelope's own timing fields, which fail
earlier and more generically as `protocol.schema_invalid` if malformed.

Core owns, and a Field never supplies: the Note ID; `instance_id` and
`field_id`; `captured_at`; `collected_by`; `content_hash`; the projected
`identities`, `entities`, `related`, `artifacts`, and `attachments` lists; the
canonical key order; the canonical scalar spelling; the filename; and every
durable write. The record schema's property-name grammar **structurally
excludes** all of them, so a Field that tries to assign a record ID or a
content hash fails validation rather than being overruled somewhere later.

A record is never a rendered Note, never carries a notebook path or filename,
and never carries a path core would treat as a destination.

#### Alternative considered: a nearly rendered Note candidate

The alternative is for a Field to emit something very close to the final Note:
canonically ordered frontmatter, canonical scalar spellings, the computed
content hash, perhaps the computed filename, with core validating and writing
the result.

Its genuine attraction is that a connector author sees exactly what will land
in the notebook, and that byte-level bugs surface in the connector's own tests.

It is rejected for four reasons, in descending order of weight.

First, it duplicates the A1 canonical serializer into every Field, in every
language a Field may be written in. A1's canonical form is not casual: RFC 8785
number spelling, an exact plain-versus-double-quoted text rule, structural keys
first and then ascending ASCII, offset-preserving datetimes, deduplicated and
sorted set-like lists, and exactly one blank line and one final LF. Every
correct implementation of that is a place it can be implemented incorrectly.

Second, it makes core's role incoherent. Core must remain the final validator,
so it would have to re-parse and re-canonicalize the candidate anyway to be
sure — the rendering step's only durable effect would be a second place where
canonical bytes are decided. One canonical serializer is the whole point of A1.

Third, it turns A1 clarifications into breaking changes for connectors. Under
ruling 2, IG1 discovered that the semantic-record encoding differs from the
public emitter in two ways rather than one. That was a prose correction with no
byte change. If Fields rendered bytes, every such clarification would be a
protocol-visible change to every Field.

Fourth, it creates exactly the pressure the security model forbids. A Field
that renders a Note is one step from supplying its filename, and one more from
supplying its path. Keeping the Field on the far side of serialization means
there is no filename in the protocol to be tempted by.

The rejected alternative's real benefit — connector authors seeing the notebook
outcome — is served instead by the conformance kit: a fixture Field plus core
produces actual notebook bytes in a test, without moving the serializer.

The opposite extreme, a **raw vendor-payload passthrough**, is rejected for the
mirror-image reason: it would push vendor mapping into core, contradicting A0's
crate responsibilities, where vendor SDKs must not enter format or storage
crates, and `fields/*` owns source-specific mapping.

#### Consequences

- Core remains the single canonical serializer and the sole durable writer,
  which is non-negotiable and follows from A0 and A1.
- A Field is easier to write and much easier to write correctly: it maps values
  and never spells bytes.
- Core does more work per record — normalization, hashing, ordering, filename
  derivation — which is the right place for it to be done once.
- A connector cannot see final bytes from its own unit tests alone. The
  conformance kit closes that gap.

### 7. Portable exact-source identity and identity anchors

These are two different things and A2 keeps them structurally separate.

**The portable exact-source key** is `(source_scope, source_identity)`, carried
on every record as `source.scope` and `source.identity`. It is the only thing
that collapses independently collected copies of one upstream object, and the
only thing core reconciles Notes by. Producer provenance stays
`(instance_id, field_id)`, which core owns and the Field never sees.

The manifest declares how a Field derives the key: a named `scope_rule` and its
version, a non-secret `scope_shape` for review, the `identity_shape`, a
constant `identity_includes_object_kind: true`, a constant
`stable_across_instances: true`, and a constant
`scope_depends_on_field_label: false`. That last constant is the one that makes
the key portable: a scope derived from the user's local Field label would differ
between two instances collecting the same mailbox, and exact cross-instance
deduplication would silently stop working. Declaring the derivation is what
lets a reviewer confirm the scope is non-secret and label-independent before a
Field ships, rather than discovering it from a merge that failed to deduplicate.

The manifest also declares `source_version_ordering`. Core compares two
`source_version` values only through the rule the Field declares, and
`unsupported` means divergence with no other evidence is a visible conflict
rather than a silent overwrite. A connector may declare an ordering only if it
can prove one.

**Identity anchors** are separately declared and carry a namespace, a
normalized value, a `scope_class` from the identity contract's scope classes, a
normalization rule and version for a normalized channel identity, the declared
authority scope for an authority- or namespace-scoped anchor, and an optional
role. Each declaration carries the constant
`substitutes_for_source_key: false`.

Anchors may relate graph entities. They never identify an upstream object, are
never used for Note reconciliation, and never make two source objects one.
Core projects them onto A1's `identities` list; the Field does not supply that
list. A source value a connector cannot scope safely must be emitted as a weak
descriptive anchor or not at all — never promoted to exact because it looks like
a well-formed address or ID.

#### Consequences

- The two identity questions cannot be conflated by a connector, because they
  are different members with different declared metadata.
- A reviewer can audit portability from the manifest alone, before any
  collection.
- A connector that cannot produce a stable portable scope must say so, and its
  Notes then deduplicate only by Note ID across copies.

### 8. Artifact transfer

Original bytes reach core by **staged file with a core-derived path**. Core
creates a per-run staging directory, names its absolute path in the collection
request, and removes it when the run ends. The Field writes each original into
that directory under a `handle`, and the record references the handle.

A handle is a single path segment from a closed character set:
`^[a-z0-9][a-z0-9_-]{0,63}$`, additionally excluding reserved Windows device
names — `con`, `prn`, `aux`, `nul`, `com0`-`com9`, and `lpt0`-`lpt9`. `com0` and
`lpt0` are excluded alongside the numbered range even though the conventional
numbering starts at 1, because Windows reserves both. It admits no dot, no
separator, and no traversal sequence, so a handle cannot be a path however it
is spelled. Core joins the handle to the staging directory, opens it without
following symlinks, requires a regular file whose identity is unchanged
between check and use, and bounds the read by the declared length.

A grammar failure and a filesystem-shape failure are distinguishable
rejections. `artifact.invalid_handle` is a malformed handle string, refused
before any filesystem call. `artifact.not_regular_file` is a *grammatically
valid* handle whose staged entry turned out to be a symlink, a directory, or
any other non-regular file — a defect no grammar can catch, because the handle
itself was fine. Logs, metrics, and tests can then distinguish "the Field sent
a hostile string" from "the Field staged the wrong kind of thing," which the
original proposal's single overloaded code could not.

**The two-layer validity model.** The transcript corpus's `valid` field means
*wire-schema* validity — would a validator checking only the JSON Schema
accept this frame's shape — and `expect_reject` names the code from whichever
pipeline stage actually rejects the frame, which is not always the schema. An
artifact handle is the clearest example: the wire schema's `artifactRef.handle`
carries the closed handle-character-set pattern as a well-formedness guard (so
a validator checking the schema alone correctly flags a traversal string as not
matching it), but **the reference implementation's own data-transfer object
carries `handle` as an unvalidated string**, and applies the grammar itself, by
hand, as a distinct artifact-validation step that runs before any filesystem
call. This is not an inconsistency: it is what keeps the *code* right. A DTO
field typed to enforce the grammar during deserialization would fail a hostile
handle at decode time with a generic `protocol.schema_invalid` — "the frame
does not validate" — when the code this package actually specifies, and the
one the transcripts pin, is `artifact.invalid_handle` from the later,
purpose-built step. An implementer translating this package's schemas
literally into a strongly-typed DTO must not collapse the wire-schema guard
into the type used for decoding, or every hostile handle silently becomes the
wrong rejection code.

Core always computes its own SHA-256 over the staged bytes and derives A1's
`artifact_sha256_<hex>` identity and notebook path from **its own** digest. A
Field-declared `sha256` is a detection aid: a disagreement rejects the record.
The canonical extension comes from A1's media-type registry, never from
`source_filename`, which is retained as display evidence only. Artifacts become
durable before any Note that references them.

A second reference kind, `digest_only`, lets a Field say "this is the artifact
with this digest, and I did not transfer its bytes." Core accepts it only when
it already stores that digest, and otherwise rejects the record so the Field
retries with bytes. This is what stops a mail Field from re-downloading every
forwarded attachment on every sync, and it is safe because A1 already
establishes that the same digest is the same bytes. Identical artifact bytes
deduplicate storage; they never collapse Notes from different source objects.

A third reference kind, `not_retained`, lets a Field say "I saw this artifact
at the source and chose not to retain it." See section 14's retention
threshold for when and why a Field does this, and why it is a policy decision
rather than a failure. As of ADR 0007, retention is filtered by declared media
type as well as by size — see section 5's `artifact_media_types` — and a
type-excluded attachment produces exactly the same `not_retained` outcome as
an oversize one.

**A `not_retained` reference carries `attachment_ref` (ADR 0007), required
exactly for this kind and forbidden for `staged` or `digest_only`.** A staged
or digest-only reference already has an A1 artifact ID derived from a digest;
a declined artifact has no bytes and computes no digest, so `attachment_ref`
— a stable connector-namespaced upstream attachment reference, following the
same object-kind-namespace convention `source_identity` uses — is the only
stable identity it carries. Core projects it onto the shared
`skipped_attachments` Note property (see the
[property registry](../property-registry.md)), which A1 was amended to
register for exactly this purpose. A2 does not otherwise interpret
`attachment_ref`: it stores no byte size and no skip reason, because
re-collection (below) re-evaluates each reference against whichever policy is
current when it runs, and a stored size or reason would only be a stale copy
of something the source, or the policy, has since superseded.

#### Alternatives considered

- **Base64 inline in the record frame:** one mechanism, no filesystem, no
  staging lifetime. Rejected because it forces whole-artifact buffering in both
  processes and collides head-on with the frame bound: a 20 MB attachment
  either breaks the frame limit or the frame limit stops bounding anything.
- **A second multiplexed binary stream:** avoids the filesystem and streams
  naturally, but requires freezing a second framing protocol with its own
  chunking, interleaving, back-pressure, and error semantics. That is a large
  contract to approve for a benefit the staging directory already provides.
- **A Field-supplied source path that core reads:** the cheapest option for the
  `local` Field, which would avoid copying files it is already reading.
  Rejected outright: it is precisely the "connector-supplied filesystem
  destination" the security model forbids, and it would let a Field make core
  read any file the user can read. The local Field pays one extra copy; that is
  the correct price.
- **Trusting the Field's declared digest:** would allow skipping the hash of
  staged bytes. Rejected because the artifact ID *is* the digest, so trusting a
  declared digest would make a connector bug or a corrupted download produce a
  Note pointing at an artifact identity that does not describe its bytes.

#### Consequences

- Core does one extra pass over artifact bytes to hash them. Hashing is
  streaming, so this is bounded and predictable.
- **The staging directory is operational sync state, not the disposable cache
  class.** *This corrects the proposal as originally drafted*, which placed it
  under the disposable cache class; that recommendation is now settled the
  other way — see the "Questions the reviewer may want to settle" section.
  Artifact bytes must not transit a directory users are told is always safe to
  delete at any time, even briefly and even before they are durable.
- A crash mid-run leaves staged bytes that startup recovery removes; no Note
  references them because the record was never accepted.
- A `not_retained` reference never touches the staging directory, the digest
  index, or the run's staged-byte budget: core does no filesystem work and no
  hashing for it at all, and it never counts against `max_run_artifact_bytes`.

### 9. Cursors, checkpoints, and crash safety

**A cursor** is an opaque, non-secret, bounded UTF-8 string produced only by
the Field and meaningful only to that Field's driver at a declared
`cursor_format_version`. Core never parses, orders, or interprets it. It is
operational sync state under `.fieldnotes/state/sync/`, not notebook truth.

If the manifest's declared `cursor_format_version` differs from the version the
stored cursor was written at, core does not replay it. It starts unbounded and
reports a recovery gap, rather than handing a Field a token it may misread.

**A checkpoint** is the Field's *offer* of a resume point. The Field proposes;
core commits. A checkpoint carries the cursor, its format version,
`covers_record_seq_through` (the last record sequence number the cursor
accounts for), `records_covered` for cross-checking against what core actually
received, an optional snapshot completeness claim, an optional window, and a
`final` flag.

**Core commits a cursor only after every accepted *record* with a sequence
number at or below `covers_record_seq_through` has reached durable current
state** and the store's durability barrier has returned. That italicized word
is deliberate and is **the single most dangerous ambiguity in this package**,
worth making impossible to misread: `seq` is shared across event kinds — a
record, a checkpoint, and a diagnostic all draw from the same per-run counter —
so "every seq at or below N" and "every accepted record with seq at or below N"
are different claims, and only the second one is checkpoint eligibility.

Concretely: a diagnostic at seq 1 and a record at seq 2 means a checkpoint
declaring `covers_record_seq_through: 2` covers exactly **one** record — the
one at seq 2 — not two. An implementation that instead tracks a naive
contiguous-seq watermark (waiting for every seq from 1 through N to be
individually accounted for as "durable," including seq values that were never
records at all) never commits, silently and forever, the moment a run emits
even one diagnostic or one earlier checkpoint before the record a later
checkpoint covers. The failure is silent because nothing rejects the run: the
checkpoint offer is simply never eligible, the cursor never advances, and every
subsequent run replays from scratch, indefinitely, with no error anywhere. The
reference implementation tracks durability per accepted record specifically —
never per raw `seq` value — for exactly this reason.

The order of the underlying work is fixed:

1. validate and bound the event;
2. normalize it into core domain values;
3. locate the current Note by portable source key;
4. stage, verify, and install or reuse original artifacts;
5. make artifacts durable;
6. stage and atomically install the Note replacement;
7. remove a superseded filename only after the replacement exists durably;
8. atomically commit the cursor.

Never the reverse, and never partially. `records_covered` disagreeing with what
core actually received fails the run as `protocol.coverage_mismatch` — a code
of its own, not the overloaded `protocol.unexpected_order` the original
proposal used — because it means the two sides disagree about what was
transferred, which is a different defect from a frame arriving somewhere the
protocol forbids it.

**Coverage range edge cases**, stated explicitly because an implementation that
gets them wrong either commits nothing or panics:

- `covers_record_seq_through: 0` means **no records are covered at all** — the
  cursor advances without any record in between, which a Field may emit once it
  has proven a page contained nothing new. `records_covered` must then be `0`.
- **Repeated coverage of an already-covered range is legal, and a no-op.** A
  checkpoint whose `covers_record_seq_through` equals the previous checkpoint's
  value accounts for zero new records; core commits it (or would, if offered)
  without effect, and `records_covered` must be `0`.
- **The implementation trap**: naively computing the covered count as
  `(previous_coverage + 1)..=covers_record_seq_through` and asking a sorted set
  of accepted-record sequence numbers for that range panics the moment
  `covers_record_seq_through` has not strictly advanced past `previous_coverage`
  — a start bound after the end bound is a logic error for most sorted-range
  APIs, not silently zero. The range must only ever be constructed in the
  branch where coverage has actually advanced; the no-advance case is handled
  separately and yields zero without ever building the range.

**A cursor may advance** only at a checkpoint whose covered records are all
durable, and only monotonically within a run in emission order. A run in which
core rejected a record commits no further checkpoint.

**Durability is conservative on purpose, in v0.1.** Core refuses to consume the
next event at all while a checkpoint's durability barrier is outstanding: the
Field's next frame is not read until the pending offer is resolved, committed
or withheld. Core could instead pipeline — keep consuming and durably writing
records while an earlier checkpoint's barrier is still outstanding, and
reconcile which records a given commit actually covers after the fact. That is
a permitted future optimization, and it is deliberately **not** done in v0.1:
without pipelining, "which records does this commit cover" is a question with
one answer, decided before the next event is even read, rather than a question
about in-flight state that a reviewer or a test has to reconstruct.

**On crash before a checkpoint commit**, records already made durable remain,
and the committed cursor lags them. The next run replays from that cursor, the
Field re-emits those objects, and reconciliation through the portable
exact-source key makes the replay a no-op: same Note ID, same bytes, same
filename. If the object changed upstream meanwhile, it is an in-place atomic
update under the same Note ID.

**On crash after a checkpoint commit**, the next run resumes from it. The Field
need not re-emit strictly earlier objects, and core tolerates it if it does.

The asymmetry is the whole design: **a lagging cursor costs work; an advanced
cursor loses an object forever.** Over-collection is always safe and
under-collection never is, so the cursor is required to lag.

Duplicates are explicitly safe. Two frames for one source key with identical
semantic payloads are a no-op, within a run or across runs. **Cross-run and
cross-instance idempotence is guaranteed by the store, not by the protocol
boundary**: recognizing a replayed object as the same current state requires
comparing it against the notebook's *current* durable state, which is
information the protocol library cannot see — it only ever sees the events of
one run. The store, which reconciles by portable exact-source key and can
consult what is actually on disk, is what makes replay after a crash
idempotent across runs and across instances of the same Field.

Two frames for one key with divergent payloads and no declared version
ordering are a Field defect, not a conflict, and **this recommendation is
settled**: within a single run, one producer asserting two different current
states for one object with no ordering is a bug in that Field, and core
rejects the second frame — `record.duplicate_divergent_in_run` — rather than
manufacturing a conflict bundle out of it. Turning a same-run self-contradiction
into user-visible conflict material would hide the bug instead of surfacing it.
This scope is deliberately narrow: **cross-run and cross-instance divergence
still becomes a visible conflict**, at the store, which is where evidence
preservation applies — two different producers, or two different runs of the
same producer, disagreeing about an object's current state is exactly the
situation a conflict exists to record. Only the in-run, single-producer,
self-contradictory case is treated as a defect instead.

#### Alternatives considered

- **Core computes the cursor:** would make cursors inspectable and portable,
  but a delta token, a continuation URL, or a change-feed watermark is
  vendor-defined and cannot be synthesized by core. Opacity is honest.
- **A Field commits its own cursor:** removes a round trip, and removes the
  entire durability guarantee. Only core knows whether a Note is on disk.
- **One checkpoint per record:** simplest possible reasoning, but a durability
  barrier per record makes a large sync unusably slow. Several checkpoints per
  run, each covering a batch, keeps the guarantee and the throughput.
- **Store the cursor before the Notes and repair afterwards:** faster in the
  happy path, and it turns any crash into silent data loss.

#### Consequences

- Replay after a crash re-does work and changes no notebook byte.
- A cursor cannot be reconstructed from Notes, which is why sync state is its
  own state class rather than a disposable cache.
- Crash-injection tests at every checkpoint boundary are required evidence, not
  optional hardening.

### 10. Authoritative deletion versus partial results

A Note may be removed for a source disappearing in exactly two protocol ways,
and both must be **declared in the manifest before they can be exercised**.

**An explicit tombstone.** A `record` with `change: "delete"` carries the
portable source key, `authority: "tombstone"`, the observation instant, and
**no content at all**. Content is structurally forbidden on a delete, so a
deletion can never be confused with an empty or partial collection result. Core
rejects the record unless the manifest declares
`deletion.tombstones: authoritative`.

**A completed authoritative snapshot.** Absence may remove Notes only when all
of these hold:

1. the manifest declares `deletion.snapshot: authoritative` and lists
   `snapshot` among its supported modes;
2. core requested `mode: "snapshot"` with an explicit `snapshot_scope`;
3. the run's final checkpoint claims `snapshot.state: "complete"` for **exactly
   that scope** — a wider claim is rejected;
4. no diagnostic of severity `error` appeared anywhere in the run;
5. the process exited zero.

Only then may core remove Notes whose portable source key falls inside that
declared scope and which the run did not report. Notes outside the scope,
including every `self` Note and every Note from another Field, are untouched: a
snapshot claim narrower than the notebook can never reach beyond its scope.

**Everything else is not deletion.** A partial completeness claim, a bounded
window, a missing page, a pagination failure, throttling, a permission loss,
unavailable history, a cancellation, a deadline, an error diagnostic, a
non-zero exit, or a crash. Each of those independently disqualifies removal;
any one is sufficient. Absence from a partial result is never evidence of
deletion.

Partial results are made **distinguishable** rather than left to inference, by
three independent signals: the explicit `snapshot.state`, diagnostic severity,
and the exit code. A reviewer or a test can tell a complete run from an
incomplete one without reading the records.

An authoritative deletion removes the current Note atomically. No tombstone
record and no revision entry is written, per A1 section 7, so a later refetch
may recreate the Note under a new Note ID with the same portable source key.
Shared artifacts are reclaimed only by a separate verified reference-analysis
pass.

#### Alternatives considered

- **Deletion by absence in incremental mode:** cheap and extremely dangerous. A
  filtered query or a failed page is indistinguishable from a deleted object
  unless the connector has explicitly declared and completed a full snapshot.
- **A deletion frame with no declared authority:** would let a connector
  acquire the power to delete Notes by emitting a frame, which is not
  reviewable.
- **Deriving completeness from a zero exit code:** exit zero means the process
  ended normally, not that it enumerated everything. Completeness is a separate
  claim about scope, and it has to be made explicitly.
- **A persistent deletion tombstone in the notebook:** would suppress refetch,
  and A1 rejected it: it creates hidden historical state and complicates
  direct-copy merge.

#### Consequences

- A connector without proven deletion authority is add-and-update only, which
  is the correct default.
- A snapshot run is more expensive than an incremental one, so deletion
  reconciliation is a deliberate operation rather than a side effect of sync.
- A Note may outlive its source object until a snapshot or a tombstone proves
  otherwise. That is the intended trade: a stale Note is recoverable, a wrongly
  deleted one is not.

### 11. Diagnostics, logging, and redaction

A `diagnostic` carries a severity, a code from a **closed v1 vocabulary**, an
already-redacted human message, optionally the portable source key it concerns,
an advisory `retry_after_seconds`, bounded structured `detail`, and a
`redacted` list naming what the Field removed.

Severity is load-bearing: `error` means the run cannot be reported complete. It
disqualifies any snapshot completeness claim and any deletion by absence, even
if the process later exits zero.

The redaction obligation is two-layered, as the security model requires:

1. **the Field classifies and sanitizes before emission**, replacing each
   removed value with the exact marker `[redacted]` and naming it in
   `redacted`, so a reviewer can see that redaction happened rather than
   guessing;
2. **core applies its own second pass** over every diagnostic member and over
   captured standard error before display or persistence, covering
   authorization and cookie headers; token, password, secret, code, and
   signature fields; credentials embedded in URLs or error strings; protected
   channel material; and credential-like values in pagination errors and
   cursors.

Core never persists raw standard error. It captures it into a bounded ring
buffer, redacts it, and notes truncation.

Redaction is defense in depth, not permission to log a secret first. And it is
an obligation on **Fieldnotes' own output only**. Per
[ruling 3](../decisions/0006-a1-implementation-rulings.md), Fieldnotes performs
no secret scanning of notebook content and never rejects collected evidence for
containing secret-looking text. A credential a colleague pasted into an email is
evidence; a credential Fieldnotes holds is a secret. A2 governs the second and
says nothing about the first.

A diagnostic about a source object never implies its deletion. A record is the
only thing that can change notebook state.

#### Alternative considered: an open diagnostic-code vocabulary

Free-form codes would let a connector describe anything. Rejected because core
must *act* on some of these — severity `error` blocks deletion, throttling
drives backoff, `cursor.reset_required` triggers recovery — and it cannot act
on a string it does not know. Adding a code is an additive revision, which is
cheap; a code core silently ignores is not.

### 12. Credential references and protected delivery

The collection request carries a **reference, never a value**: the non-secret
`profile_ref` naming the configured credential profile, a single-use per-run
`grant_id`, a channel descriptor, an expiry, and the granted scopes. The
`grant_id` authorizes nothing outside this run's channel and is not source
credential material.

Material crosses only on the **protected channel**, which is separate from
standard input, output, and error. The Field writes a `credential_request`
naming its grant and purpose; core answers `credential_response` with either
`material` or an actionable non-granted outcome. The channel is an inherited
descriptor, a duplicated handle, or an OS-appropriate per-run endpoint, all
named in the request rather than in the environment. Core closes it when the
run ends and refuses an expired grant.

**The channel descriptor's flat object shape — one `kind` discriminator plus
every mechanism's fields sitting as siblings, rather than a tagged union of
four distinct shapes — is deliberate**, not an oversight to tidy up later.
`additionalProperties: false` and serde's internally tagged enums fight each
other: a Rust enum tagged on `kind` with per-variant payload structs would
either have to relax `additionalProperties` for the whole object (defeating
the closed-schema guarantee everywhere else) or reject legitimate members with
a spurious "unknown field" the moment a variant's own fields are checked
against the union of every variant's schema. A flat shape sidesteps the
conflict entirely, and every implementation language pulls toward a union here
for the same reason serde does — worth stating plainly so the shape is not
"fixed" into something more idiomatic-looking that quietly reopens the schema.

`credential_response.material` is the **only member in the entire protocol that
carries a secret**, and it exists on no other channel. Nowhere else: not in
process arguments, not in the inherited environment, not in `config`, not in
records, not in checkpoints, not in cursors, not in diagnostics, not on
standard error, not in notebook material. Core builds a sanitized allowlisted
environment for the child; the `0.1.3` environment-variable provider reads into
core's memory only and never into a child's environment.

Refresh ownership is declared in the manifest. When core owns refresh, a Field
asks again on the channel rather than holding long-lived material. A Field must
hold material for the minimum useful time and must never write it anywhere.

The exact per-platform channel mechanism, refresh semantics, and memory
clearing remain the `0.1.3` authentication gate's business; A2 freezes the shape
and the invariant, which is what the boundary needs.

#### Alternatives considered

- **Environment variables:** by far the easiest, and rejected. An inherited
  environment leaks to grandchild processes and, on some systems, to other
  processes; it appears in crash dumps and debugging tools; and it has the
  process's whole lifetime.
- **Command-line arguments:** rejected by every governing document in this
  repository, because argv is world-readable on common systems.
- **A credential file with restrictive permissions:** simple and portable, and
  it puts a secret at rest on disk with a lifetime longer than the run and a
  cleanup path that fails on crash.
- **Core proxying every upstream request so the Field never holds a token:**
  the strongest option, and out of scope: it would put vendor HTTP semantics
  into core, which contradicts why the process boundary exists.

#### Consequences

- Every Field needs channel-handling code, so the shared SDK crate must provide
  it rather than leaving each connector to reimplement it.
- Secret-canary tests are structural evidence: a unique canary is granted on
  the channel and asserted absent from argv, the inherited environment,
  standard output, standard error, logs, diagnostics, cursors, Notes, and
  artifacts. That is release gates R1, R3, and R9.
- The A2-level evidence stands as designed and is not weakened: both channel
  frames are typed, `credential_response.material` is the only secret-bearing
  member anywhere in the protocol, `CredentialMaterial`'s `Debug`
  implementation redacts, and a canary-absence scan runs with its negative
  control (a scenario that deliberately leaks, proving the scan is not
  vacuous). **The end-to-end credential canary — granting a canary through a
  real per-platform channel mechanism and confirming its absence everywhere —
  is assigned to the `0.1.3` authentication gate**, because section 12 already
  defers the per-platform channel mechanism, refresh semantics, and memory
  clearing to that gate, and A2 cannot freeze evidence for a mechanism it does
  not itself freeze. This is an explicit addition to the evidence list, not an
  obligation dropped: `0.1.3` inherits it by name.

### 13. Exit codes, partial failure, and cancellation

A Field's exit code is part of the contract, because it is the one signal that
survives a crashed or hung process.

| Code | Meaning |
|---|---|
| 0 | Run completed normally |
| 1 | Unclassified Field failure; partial results possible |
| 2 | Usage or invocation error, such as an unknown operation |
| 3 | Protocol version negotiation failure |
| 4 | Authentication or credential failure; re-authentication needed |
| 5 | Authorization or permission denied by the source |
| 6 | Source unavailable or throttled beyond the retry budget |
| 7 | Cursor unusable; resume impossible without reset or backfill |
| 8 | Cancelled by core, acknowledged within the grace period |
| 9 | Configuration invalid |
| 10 | Internal Field error |
| 11–63 | Reserved for additive protocol revisions |
| 64–125 | Must not be used by a Field; reserved by convention to shells and `sysexits` |
| 126–255 | Operating system, shell, and signal territory |

Core treats any signal termination — 128 plus the signal number on POSIX, and
its Windows equivalent — as a failed run, and normalizes it rather than
inventing a Field-level meaning for it. **The Windows equivalent is specified
explicitly**, because "its Windows equivalent" as originally written could be
misread as another small integer: Windows has no POSIX-style signal number.
Where a process ends by unhandled structured exception — including the one
`std::process::abort()` raises on Windows, which surfaces as the NTSTATUS code
`0xC0000409` (`STATUS_STACK_BUFFER_OVERRUN`) — the exit-status value core
observes is a full NTSTATUS-shaped 32-bit code, which **does not fit the `u8`**
every ordinary exit code and every POSIX 128-plus-signal value fits into.
Core's exit-observation type must therefore carry a value wide enough for this
case on Windows specifically, distinct from the ordinary 0–255 exit-code
observation, and classify any such value as a failed run exactly like a POSIX
signal termination — never attempting to narrow it into the 0–255 range, which
would silently alias an abnormal termination onto an ordinary exit code (for
example, `0xC0000409`'s low byte is `0x09`, which would misread as exit code 9,
"configuration invalid," if naively truncated).

**Partial failure does not roll back durable work.** Notes, artifacts, and the
cursor committed before the failure remain, and they are correct because they
were committed only after they were durable. A run's outcome is one of:

- **complete:** exit 0, no error-severity diagnostic, and, in snapshot mode, a
  completeness claim for the requested scope;
- **partial:** durable work happened and the run did not complete;
- **failed:** a protocol violation, a rejected record, a crash, or a hang.

Only **complete** can authorize deletion by absence.

For a multi-Field sync, one Field's failure does not abandon the others.
Every Field's outcome is reported individually along with which cursors were
committed. The CLI's own exit-code table and summary format remain a CLI
contract decision and are not frozen here.

**Cancellation** is cooperative. Core writes one `cancel` frame with a reason
and a grace period. The Field stops starting new work, may emit one final
checkpoint for material it already emitted, and exits with code 8. If it does
not exit within the grace period, core terminates it and reports a failed run.
A cancelled run is never complete, so it can never authorize a removal.

### 14. Untrusted output, bounds, and rejection

Field process output is untrusted input. A Field is trusted *code* — the process
boundary isolates dependencies and failures and is explicitly not a sandbox
against a malicious connector — but its output is data from outside, and a
buggy connector is far more likely than a malicious one.

Protocol v1 freezes these ceilings: absolute technical bounds that protect core
against a hostile or buggy child, which no configuration may cross, and which
only a protocol revision can raise. For most of them the ceiling is also the
sensible default. **Two are not**: the single-artifact bound and the run wall
clock each have a configurable *default* distinct from their ceiling, settled
by the "Questions the reviewer may want to settle" section below. A notebook
may configure either bound — and, in general, may configure any bound in this
table — anywhere from the product's own minimum up to the ceiling, in either
direction from the default; configuring *up* toward the ceiling is exactly as
legal as configuring down from it, and only crossing the ceiling itself
requires a protocol revision. This is a change from the original proposal,
which stated the weaker rule "a notebook may configure a value lower"; that
rule is retained for every bound that keeps ceiling equal to default, and
loosened for the two that do not, precisely because the ceiling — not the
default — is what actually protects core.

| Bound | Default | Ceiling |
|---|---|---|
| One frame, including its terminating LF | 1 MiB | 1 MiB |
| Record `body.text` | 1 MiB | 1 MiB |
| Property candidates per record | 256 | 256 |
| One property value | 64 KiB | 64 KiB |
| List members | 1024 | 1024 |
| Artifact references per record | 64 | 64 |
| One staged artifact, and the retention threshold (see below) | 25 MiB (26,214,400 bytes) | 512 MiB (536,870,912 bytes) |
| Staged bytes per run | 8 GiB | 8 GiB |
| Standard output per run | 1 GiB | 1 GiB |
| Records per run | 1,000,000 | 1,000,000 |
| Diagnostics per run | 10,000 | 10,000 |
| Captured standard error per run | 256 KiB, ring-buffered | 256 KiB |
| Cursor | 4 KiB | 4 KiB |
| Run wall clock | 600 s (10 minutes) | 3600 s (1 hour) |
| Idle without a frame or artifact progress | 120 s | 3600 s |
| Cancellation grace before termination | 10 s | 120 s |

**The single-artifact bound doubles as the retention threshold**, and this is
a deliberate one-number design rather than two separate numbers (a transfer
ceiling and a retention policy). A Field compares a known artifact's size
against this same value *before* staging it: at or under it, stage normally;
over it, emit a `not_retained` reference (section 8) instead of bytes core
would otherwise have to reject as oversized. One number communicated in the
request is simpler to explain and to configure than a transfer ceiling and a
retention policy that could drift apart, and it means a Field that stages
something larger really is violating a limit it was told, not guessing at an
unstated one. The default of 25 MiB reflects the product's own boundary: a
notebook is disposable working material, not a system of record — deleting and
refetching is a supported lifecycle action — so the default keeps what is
useful for work and context, and larger original material stays at its source
rather than being copied in by default. Exceeding the threshold is **never** a
run failure and **never** a hostile-output violation: the Note is still
created, referencing the source object, without the retained bytes. See the
`not_retained` rejection-free reference kind in section 8.

Rejection is uniform: **any protocol violation fails the run.** Core stops
consuming output, terminates the child after the grace period, removes the run's
staging directory, and commits no further checkpoint. Checkpoints already
committed stand, because they were durable before they were committed.

The v1 rejection codes are closed and grouped by what went wrong:

- `protocol.invalid_utf8`, `protocol.not_json`, `protocol.oversized_frame`,
  `protocol.truncated_frame`, `protocol.schema_invalid`,
  `protocol.unknown_event`, `protocol.unexpected_order`,
  `protocol.duplicate_seq`, `protocol.seq_regression`, `protocol.seq_gap`,
  `protocol.limit_exceeded`, `protocol.timeout`, `protocol.idle_timeout`,
  `protocol.stderr_flood`, `protocol.version_unsupported`,
  `protocol.coverage_mismatch`;
- `record.unknown_property`, `record.undeclared_property`,
  `record.foreign_prefix`, `record.property_type_mismatch`,
  `record.invalid_note_type`, `record.note_type_not_declared`,
  `record.invalid_date`, `record.invalid_datetime`,
  `record.missing_source_key`, `record.duplicate_divergent_in_run`;
- `artifact.invalid_handle`, `artifact.not_regular_file`,
  `artifact.digest_mismatch`, `artifact.length_mismatch`,
  `artifact.missing_staged_file`, `artifact.unknown_digest`,
  `artifact.oversized`, `artifact.type_excluded`;
- `deletion.unauthorized`, `snapshot.completeness_contradicted`,
  `snapshot.scope_widened`;
- `credential.unknown_grant`, `credential.grant_expired`,
  `credential.channel_closed`;
- `manifest.property_type_changed`, `manifest.cursor_format_changed`,
  `manifest.undeclared_capability`.

**Five codes are new relative to the original proposal**, each closing a
specific ambiguity or gap surfaced by implementation, and each recorded where
it is decided: `protocol.seq_gap` and `protocol.coverage_mismatch` (section 1
and section 9, splitting a gap and a coverage disagreement out of the
overloaded `protocol.unexpected_order`); `artifact.not_regular_file` (section
8, splitting a filesystem-shape failure out of `artifact.invalid_handle`);
`record.note_type_not_declared` (section 4 and section 6, enforcing a
capability slice's declared `note_type` as a bound rather than decoration);
and `record.invalid_date` (section 6, distinguishing a malformed date-only
value from a malformed datetime).

**One further code was added by the ADR 0007 attachment-retention-policy
pass**: `artifact.type_excluded` (section 5 and section 8), mirroring
`artifact.oversized` for the media-type retention gate rather than the size
gate, and kept distinct from it so the two kinds of retention refusal remain
distinguishable in logs, metrics, and tests.

**Path safety is structural, not defensive.** No Field-supplied string is ever
a path component:

- an artifact handle is a single segment from a closed character set, excluding
  dots, separators, traversal sequences, and reserved Windows device names, so
  traversal is a grammar error rather than something to sanitize;
- core resolves a handle only inside the run's staging directory, opens it
  without following symlinks, requires a regular file, and requires its
  identity to be unchanged between check and use;
- `source_filename` and any path-shaped property value are display metadata
  only;
- the notebook path is derived from core's own digest and A1's extension
  registry, never from anything a Field said;
- an unexpected destination collision is a conflict, not overwrite permission.

Core also treats a valid-looking partial write as a failure: startup recovery
removes or completes incomplete temporary files so no partial Note ever looks
valid.

#### Consequences

- Every bound needs a hostile-output conformance case, and the corpus provides
  one transcript per shape.
- Bounds are visible to the Field in the request, so a well-behaved connector
  can self-police rather than discovering a limit by being killed.
- A single malformed frame costs a run, not a notebook.

### 15. Executable trust and discovery

Core runs only a Field executable at a **configured, pinned absolute path**.
There is no `PATH` search and no name-based discovery: v0.1 must not execute a
plausible binary and hand it credentials because it returned a plausible
manifest.

Adding a Field records its pinned path, the manifest snapshot from section 3,
and a digest of the executable. A changed digest requires explicit user
confirmation before the next run, and a manifest change that alters declared
property typing or cursor format requires the migration in section 4.

Installation provenance, upgrade behavior, and the trust-confirmation user
experience are product and security concerns that the security document already
gates separately; A2 freezes only what the protocol needs: pinned path, no
implicit discovery, recorded manifest, and confirmation on change.

## Compatibility and change policy

If A2 is approved:

- protocol v1 is frozen. Every schema closes with
  `additionalProperties: false`, and an unknown member or event is a failed run
  rather than a warning;
- additive-only changes — a new optional member, a new diagnostic code, a new
  reserved exit code, a lower default limit — increment `protocol_revision`.
  The negotiated revision is the minimum of the two peers' declared revisions,
  and neither may emit a member above it;
- a semantic change to any member, a new required member, a removed member, a
  raised limit ceiling, a changed exit-code meaning, or a changed diagnostic
  code meaning requires `protocol_version` 2 and an explicit migration
  proposal;
- the diagnostic-code and rejection-code vocabularies are closed. Adding one is
  a revision; silently ignoring an unknown one is never permitted;
- a Field's declared property types and list semantics are frozen for that
  Field within v0.1. Changing one is a migration, not a manifest edit;
- A2 approves no A1 vocabulary. New shared property names, primary types,
  prefixes, or record kinds still require A1 registry review and fixtures;
- A2 approves no connector's actual capability slices, property names, scopes,
  or source-scope rule. Each of those is approved with its Field's release
  gate: `local` at `0.1.1`, Microsoft Fields at `0.1.3` through `0.1.5`, Jira
  at `0.1.6`;
- A2 approves no renderer or rendition contract (`0.1.7`), no handback package
  schema (`0.1.7`), no enhancement capability (`0.1.8`), no CLI exit-code
  table, and no destination write of any kind;
- a connector workstream may not amend protocol v1 privately. If implementation
  evidence invalidates a choice here, it returns to this gate as a recorded
  finding and a coordinator ruling, exactly as
  [IG1 did for A1](A1-implementation-findings.md).

## Evidence required for implementation

The attached schemas and transcripts make the recommended choices concrete
enough for contract review. They are not the executable conformance suite. If
A2 is approved, IG2 must produce, before `0.1.1` closes:

- protocol DTOs in `fieldnotes-field-protocol` that round-trip every attached
  schema, with the schemas checked in as the generation or validation source so
  a DTO cannot drift from the approved contract silently;
- a reusable Field SDK covering framing, negotiation, the protected credential
  channel, artifact staging, and self-policing against the request's limits, so
  no connector reimplements the boundary;
- `fields/fieldnotes-field-fixture`, a fixture Field that can be driven into
  every failure mode in the corpus on demand: each malformed shape, each
  hostile artifact reference, each undeclared property, a hang, a stderr flood,
  and a crash at any chosen point;
- one executing conformance case per attached transcript, asserting the
  observable core actions the transcript records, not merely that no error
  occurred;
- crash-injection tests at **every** checkpoint boundary, proving that a
  committed cursor never precedes an undurable write and that replay from any
  boundary changes no notebook byte;
- an idempotence test proving that a full replay of a completed run leaves the
  notebook byte-for-byte identical;
- deletion tests: an authorized tombstone removes a Note; an unauthorized one
  is rejected; a complete snapshot removes only inside its declared scope; a
  partial snapshot, a windowed run, an error diagnostic, a cancellation, and a
  non-zero exit each independently remove nothing;
- prefixed-property enforcement tests for every case in section 4, including a
  manifest whose declared type changed between runs;
- artifact tests: staged install, digest-only reuse, digest mismatch, unknown
  digest, oversized, missing staged file, symlink escape, traversal handle,
  a non-regular staged entry distinguished from a hostile handle by code,
  reserved device name (including `com0` and `lpt0`), a `not_retained`
  reference that neither stages nor fails, and a hard-link and
  check-versus-use race case;
- media-type retention tests (ADR 0007): a Field declining a type-excluded
  attachment as `not_retained` with `attachment_ref`, alongside a retained
  attachment on the same record; a hostile Field staging a type-excluded
  attachment anyway, rejected as `artifact.type_excluded` and distinguished
  from `artifact.oversized`; and a round-trip/validation test for
  `recollect_targets`, including that it is rejected together with `cursor`,
  together with `window`, and under `mode: "snapshot"`;
- secret-canary tests with a unique canary granted on the protected channel and
  asserted absent from argv, the inherited environment, standard output,
  standard error, logs, diagnostics, cursors, Notes, artifacts, and any crash
  recovery file, using the in-process fixture channel A2 can exercise now;
  **the end-to-end version, over a real per-platform channel mechanism, is
  `0.1.3` authentication-gate evidence** (section 12), because A2 does not
  freeze that mechanism;
- bound-enforcement tests for each ceiling in section 14, including a frame
  that exceeds the limit without core buffering the remainder;
- negotiation tests for an unsupported version in each direction, a revision
  mismatch, and a cursor-format change;
- cross-platform execution of the whole suite on Linux, macOS, and Windows,
  since path, rename, symlink, handle-inheritance, and process-termination
  behavior differ;
- a fuzz target over the frame parser, treating output as untrusted input.

Every fixture must state whether it is normative at A2 or illustrative for a
later capability gate, in the same style the A1 corpus READMEs use. Together
these are release gate R1's evidence.

Consistent with [ruling 3](../decisions/0006-a1-implementation-rulings.md), no
test may require Fieldnotes to reject collected evidence for containing
secret-looking text. The canary tests examine Fieldnotes' own output only.

## Explicit approval checklist

Nothing below is approved. Each box records one choice the user is asked to
accept, reject, or amend.

### Transport and negotiation

- [ ] Newline-delimited JSON on standard output, logs on standard error,
  credential material only on a separate protected channel.
- [ ] Two argv-selected operations, `describe` and `collect`, each one bounded
  short-lived child process.
- [ ] A per-run monotonic `seq` shared by records, checkpoints, and
  diagnostics, with any gap, repeat, or regression failing the run.
- [ ] Major version plus additive-only `protocol_revision`, negotiated as the
  minimum of the two declared revisions.
- [ ] Negotiation entirely inside the describe run, before any credential grant
  or staging directory exists, failing closed and actionably in both
  directions.
- [ ] `additionalProperties: false` everywhere, with an unknown member or event
  a failed run rather than a warning.

### Manifest and declared properties

- [ ] The manifest as a Field's complete self-declaration: stem, prefix,
  capability slices, source-key derivation, identity anchors, authentication,
  collection behavior, deletion authority, cursor format, and limitations.
- [ ] Capability, deletion authority, and snapshot authority must be declared
  before they can be exercised, rather than inferred from behavior.
- [ ] `writes_to_source: false`, `scope_depends_on_field_label: false`, and
  `substitutes_for_source_key: false` as schema constants rather than
  configurable values.
- [ ] Every connector-prefixed property declared with name, scalar type,
  cardinality, and set-versus-ordered list semantics, closing the gap ruling 4
  assigned to A2.
- [ ] Core rejects prefixed properties the declaring manifest does not list,
  values contradicting a declared type or cardinality, another Field's prefix,
  and unprefixed names outside A1's closed shared registry.
- [ ] Spelling-based type inference retired for declared properties, and a
  declared-type change, a declared-property removal, or a cursor-format change
  each treated as a migration that blocks sync rather than a silent retype or
  a silently forgotten type; adding a declared property needs no migration.
- [ ] The Note-applicable subset of A1's shared registry named precisely and
  enforced, so a Field collecting a Note cannot emit a name the registry types
  for a derived record only.
- [ ] A record's `note_type` enforced against its capability slice's declared
  `note_type`, closing the one manifest member "declare before exercise" had
  not yet been applied to.

### Record envelope and identity

- [ ] **The normalized source envelope, rather than a nearly rendered Note
  candidate**, with core remaining the single canonical serializer, the final
  validator, and the sole durable writer.
- [ ] Core-owned values — record IDs, producer provenance, capture time,
  hashes, canonical order and spelling, filenames, and rebuildable projections
  — structurally excluded from the record schema rather than merely overruled.
- [ ] The `upsert` and `delete` split, with content structurally forbidden on a
  delete.
- [ ] `(source_scope, source_identity)` as the only key that collapses
  independently collected copies, with `(instance_id, field_id)` retained by
  core as producer provenance and never sent to the Field.
- [ ] Declared source-key derivation, declared `source_version_ordering`, and
  `unsupported` meaning divergence becomes a visible conflict rather than a
  silent overwrite.
- [ ] Separately declared identity anchors with namespace, scope class, and
  normalization rule and version, which may relate graph entities and never
  substitute for the exact-source key or reconcile a Note.
- [ ] A property number crossing the boundary in its wire spelling, verbatim,
  with A1 alone owning canonical number spelling.
- [ ] A malformed `date` value (`record.invalid_date`) distinguishable from a
  malformed `datetime` value (`record.invalid_datetime`), and the envelope's
  own instants guarded at decode so the latter can only fire for a declared or
  registered temporal property.

### Artifacts

- [ ] Staged-file transfer into a core-created, core-named, per-run staging
  directory, with a single-segment closed-character-set handle and no
  connector-supplied path anywhere.
- [ ] Core always computing its own digest and deriving A1 artifact identity
  and path from it, with a declared digest as a detection aid only.
- [ ] The `digest_only` reference, accepted only for a digest the notebook
  already stores and otherwise rejected so the Field retries with bytes.
- [ ] The `not_retained` reference, for an artifact the Field saw and declined
  to retain per the single-artifact bound's default; never a rejection, and
  the record and its Note are still accepted without those bytes.
- [ ] Artifacts durable before any Note that references them, and identical
  bytes deduplicating storage without ever collapsing Notes.
- [ ] A grammar failure (`artifact.invalid_handle`) and a filesystem-shape
  failure (`artifact.not_regular_file`) as distinguishable rejection codes,
  and the handle grammar applied by the implementation as an artifact-
  validation step distinct from wire-schema validity, never folded into the
  DTO type used for decoding.
- [ ] A required `artifact_media_types` retention policy on the collection
  request (ADR 0007), mirroring `max_artifact_bytes` in shape and default-
  versus-configurable behavior, orthogonal to A1's frozen media-type-to-
  extension registry, with `artifact.type_excluded` as its own rejection code
  distinct from `artifact.oversized`.
- [ ] `attachment_ref`, required exactly for `not_retained` and forbidden for
  `staged` or `digest_only`, projected by core onto the new A1 shared
  property `skipped_attachments` (see the A1 amendment and
  [ADR 0007](../decisions/0007-attachment-retention-policy.md)) rather than
  interpreted by A2 itself.
- [ ] An optional `recollect_targets` request shape naming previously-
  collected source objects by their portable exact-source key alone,
  orthogonal to `mode`, excluding `cursor` and `window` when present, never
  combined with `snapshot` mode, and gated by the manifest's existing
  `collection.refetch` declaration rather than a new manifest member.

### Cursors, checkpoints, and crash safety

- [ ] An opaque, non-secret, bounded, Field-owned cursor with a declared format
  version that core never parses, and a format change starting unbounded with a
  reported recovery gap.
- [ ] Checkpoints proposed by the Field and committed only by core, after every
  covered record is durable and the durability barrier has returned.
- [ ] The fixed persistence order, with the cursor committed last and never
  advancing past an undurable write.
- [ ] A run in which core rejected a record commits no further checkpoint.
- [ ] A lagging cursor as the deliberately required direction, with replay made
  idempotent by portable-source-key reconciliation **at the store**, which is
  the only place that can see the notebook's current state; the protocol
  boundary itself guarantees only within-run duplicate detection.
- [ ] Identical duplicate records as no-ops within and across runs, and
  in-run divergence without declared ordering as a rejected Field defect —
  settled, not reopened by cross-run or cross-instance divergence, which
  still becomes a visible conflict at the store.
- [ ] Checkpoint eligibility stated precisely as "every accepted *record* with
  seq at or below the covered value," not "every seq," because `seq` is
  shared across records, checkpoints, and diagnostics and a naive contiguous
  watermark over raw `seq` values never commits.
- [ ] `covers_record_seq_through: 0` meaning no records covered, and repeated
  coverage of an already-covered range as a legal no-op.
- [ ] `protocol.coverage_mismatch` as its own code for a `records_covered`
  disagreement, and `protocol.seq_gap` as its own code for a sequence gap,
  neither overloading `protocol.unexpected_order`.
- [ ] Durability handled conservatively in v0.1: core refuses the next event
  while a checkpoint's durability barrier is outstanding, with pipelining left
  as a permitted future optimization.
- [ ] The cursor grammar excluding every C0 control character, not only NUL.

### Deletion and partial results

- [ ] Removal only by an explicit declared-authority tombstone or a completed
  authoritative snapshot, and by nothing else.
- [ ] Snapshot removal requiring declared authority, requested snapshot mode, a
  completeness claim for exactly the requested scope, no error diagnostic, and
  a zero exit, with each condition independently sufficient to refuse.
- [ ] Partial results made distinguishable by explicit completeness state,
  diagnostic severity, and exit code, rather than inferred.
- [ ] No tombstone or revision record written on deletion, with refetch
  recovery under a new Note ID, as A1 section 7 requires.

### Credentials, diagnostics, and redaction

- [ ] A credential reference in the request and material only on the protected
  channel, with `credential_response.material` the only secret-bearing member
  in the protocol.
- [ ] No secret in process arguments, the inherited environment, `config`,
  event streams, logs, cursors, or notebook material, with a sanitized
  allowlisted child environment.
- [ ] Single-use per-run grant, declared expiry, declared refresh ownership,
  and core closing the channel when the run ends.
- [ ] The closed diagnostic-code vocabulary, and `severity: error`
  disqualifying completeness and any deletion by absence.
- [ ] Two-layer redaction: the Field sanitizes with the exact `[redacted]`
  marker and names what it removed; core redacts again over diagnostics and
  captured standard error before display or persistence, and never persists raw
  standard error.
- [ ] Redaction as an obligation on Fieldnotes' own output only, with no secret
  scanning of collected evidence, per ruling 3.
- [ ] The channel descriptor's flat object shape as deliberate, not an
  oversight: `additionalProperties: false` and an internally tagged union
  fight each other, and a flat shape is what every implementation language
  will be pulled toward for the same reason.
- [ ] The end-to-end credential canary — over a real per-platform channel
  mechanism — assigned to the `0.1.3` authentication gate, since section 12
  defers that mechanism there and A2 cannot freeze evidence for a mechanism it
  does not itself freeze.

### Failure, bounds, and trust

- [ ] The exit-code table, with reserved ranges and signal termination
  normalized to a failed run, including Windows abnormal termination — a full
  NTSTATUS-shaped 32-bit value such as `0xC0000409`, which does not fit the
  `u8` an ordinary exit code fits into and must never be narrowed into one.
- [ ] Complete, partial, and failed run outcomes, with only complete
  authorizing deletion by absence, and durable work before a failure retained.
- [ ] Per-Field outcomes in a multi-Field sync, with the CLI's own exit-code
  table left to the CLI contract.
- [ ] Cooperative cancellation with a grace period, exit code 8, and a
  cancelled run never complete.
- [ ] The frozen bound ceilings — absolute technical bounds no configuration
  may cross — echoed to the Field in the request. For most bounds the ceiling
  is also the default and configuration is downward only; the single-artifact
  bound and the run wall clock instead have a configurable default distinct
  from their ceiling and may be configured in either direction up to it, per
  the "Questions the reviewer may want to settle" section.
- [ ] Any protocol violation failing the run, with the closed rejection-code
  vocabulary and previously committed checkpoints standing.
- [ ] Structural path safety: no Field-supplied string is ever a path
  component, handles resolved only inside the staging directory without
  following symlinks, and notebook paths derived only from core's own digest.
- [ ] Pinned configured executable paths with no `PATH` discovery, a recorded
  manifest snapshot and executable digest, and explicit confirmation on change.

### Corpus and policy

- [ ] The candidate schemas and transcripts are correct and complete enough to
  freeze as the IG2 implementation target, within the A2-versus-later-gate
  classification each corpus README states.
- [ ] A2's compatibility and change policy, and the boundaries it leaves to the
  `0.1.3` authentication gate, the per-Field release gates, and the CLI
  contract.
- [ ] The IG2 evidence list as release gate R1's required evidence.

## Approval effect

Approving A2 would unblock the `0.1.1` release: the protocol DTO and SDK
crates, the fixture Field, the connector conformance kit, the `local` Field,
`fields add/list/status/remove`, `sync`, durable cursors and checkpoint
recovery, source-identity reconciliation, authoritative deletion handling, and
diagnostic and credential-redaction infrastructure. Every later Field —
Outlook Mail, Calendar, Contacts, Teams, and Jira — then becomes a conforming
connector plus fixtures plus registry review, rather than a protocol
negotiation.

It would not approve any A1 vocabulary change, any connector's actual
capability slices or property names, the per-platform credential channel
mechanics that close at `0.1.3`, renderers or handback packaging at `0.1.7`,
enhancement at `0.1.8`, the CLI exit-code table, or any destination write.

## Questions the reviewer may want to settle

These are places where the existing documents did not determine the answer and
the recommendation above made a defensible choice that a reviewer may prefer to
make differently.

1. **In-run divergence for one source key.** *Settled by the coordinator on
   2026-08-22: the recommendation stands.* The second frame for one source key
   within a single run, with divergent payloads and no declared version
   ordering, is rejected as a Field defect (`record.duplicate_divergent_in_run`)
   rather than turned into a conflict bundle. One producer asserting two
   unordered current states within one run is a bug in that Field, and turning
   it into user-visible conflict material would hide the bug instead of
   surfacing it. This is deliberately narrow: cross-run and cross-instance
   divergence still becomes a visible conflict, at the store, which is where
   evidence preservation applies — nothing about this settles item 2 differently
   or reduces conflict-preservation across runs or instances.
2. **`source_version_ordering: unsupported` for the v0.1 Fields.** *Settled by
   the coordinator on 2026-08-22: intended.* Neither a Graph change key nor a
   file content hash gives a reliable order, so both candidate manifests declare
   `unsupported`. A1's merge rule 4 — reliable newer version selects content —
   therefore never fires for a shipping v0.1 Field, and cross-instance
   divergence becomes a visible conflict instead. A source version is
   deliberately not required for these Fields, and preserving divergence as a
   conflict rather than guessing an order is the intended behavior.
3. **The single-artifact ceiling.** *Settled by the coordinator on 2026-08-22:
   default 25 MiB (26,214,400 bytes), ceiling unchanged at 512 MiB
   (536,870,912 bytes).* The ceiling remains the absolute technical bound
   protecting core against a hostile or buggy child, chosen as a number large
   enough for ordinary mail attachments and documents and small enough to
   bound a run; no configuration may exceed it, and it is unchanged from the
   original recommendation. The **default** — what core requests absent
   configuration — is set well below it: a notebook is disposable working
   material, not a system of record, and copying every large blob by default
   contradicts that boundary, so the default keeps what is useful for work and
   context and larger original material stays at its source. This same number
   doubles as the retention threshold that decides when a Field emits a
   `not_retained` reference instead of staging bytes (section 8, section 14). A
   notebook may configure the effective value anywhere from the product's
   minimum up to the 512 MiB ceiling, in either direction from the default —
   settings plumbing for this is `sync`-command scope (`0.1.1`) and is not
   built here; A2 states the default, the ceiling, and the behavior a
   configured value must have.
4. **The run and idle ceilings.** *Settled by the coordinator on 2026-08-22:
   run default 600 seconds (10 minutes), run ceiling unchanged at 3600 seconds
   (1 hour); idle ceiling unchanged at 120 seconds; cancellation grace
   unchanged at 10 seconds.* The run ceiling remains the absolute bound past
   which no configuration may push a run. The **default** is set to 10
   minutes, well under it: a first full sync expected to run longer is handled
   by windowed, resumable runs — the cursor and checkpoint machinery already
   exists for exactly that — rather than by one long-running process, because a
   crash late in a long run discards far more durable-but-uncommitted work
   than a crash late in a short one. The idle and grace values stay as
   recommended, sensibly proportioned to the 10-minute run default. As with
   item 3, a notebook may configure the run length between the product's
   minimum and the 3600-second ceiling; settings plumbing is `sync`-command
   scope and is not built here.
5. **Where the staging directory lives.** *Settled by the coordinator on
   2026-08-22: operational sync state, not the disposable cache class.*
   Artifact bytes must not transit a directory users are told is always safe to
   delete at any time, even before they are durable. This reverses the
   recommendation as originally drafted, which placed it under the disposable
   cache class for cleanup simplicity; that convenience does not outweigh
   staged bytes passing through a directory whose entire contract is "always
   safe to delete."
6. **Whether `describe` should run on every sync.** *Settled by the coordinator
   on 2026-08-22: yes, on every sync.* That is where negotiation and the
   manifest-change check happen, and the cost — one extra process start per
   Field per sync — is accepted as the price of never syncing against a stale
   or incompatible manifest.
7. **Transcript file format.** *Settled by the coordinator on 2026-08-22: keep
   the fixture envelope.* It carries the `expect_reject` information the
   executable conformance tests need — which pipeline stage rejects a frame and
   with which code — that a literal wire capture has no place to record. A
   reviewer who wants transcripts to be literal wire captures would need a
   different arrangement, probably one file per direction plus a separate
   expectations file, at the cost of losing the single-file, side-by-side
   review the current format gives a human reader.

The single-artifact ceiling's *default* (item 3) and the run ceiling's
*default* (item 4) were the two questions this package originally left open
for the owner; both are now settled above, alongside the four questions that
were already resolved. No question in this section remains open.
