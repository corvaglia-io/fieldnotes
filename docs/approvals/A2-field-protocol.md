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
them with.

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
- the registered A1 Field stem and the registered property prefix, or `null`
  for a Field that contributes none;
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
   type from the declaration.

A manifest may not declare unprefixed properties. Those belong to A1's closed
shared registry and take their type from it; a Field uses them by name or not
at all.

Core snapshots each configured Field's manifest. If a later manifest changes a
declared property's `value_type` or `cardinality`, or changes
`cursor_format_version`, core refuses to sync that Field until an explicit
migration, rather than retyping notebook data in place. This is A1's rule that
"a property name never changes meaning or scalar/list type within v0.1", made
enforceable at the boundary where the change would arrive.

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
- Changing one is a migration, and core says so instead of guessing.
- The interim inference rule survives only as the reading rule for notebooks
  written before `0.1.1`, which by construction contain no prefixed property at
  all, because only `self` ships at `0.1.0` and `self` has no prefix.

### 5. The collection request

Core writes one `collect_request` carrying: the negotiated version and
revision; the configured `field_id`; the `mode`, either `incremental` or
`snapshot`; the last committed opaque `cursor` and the format version it was
stored at, when one is replayable; an optional bounded `window`; the
`snapshot_scope` in snapshot mode; non-secret `config` as flat scalars and
homogeneous scalar lists; at most a credential *reference* and channel
descriptor; the `artifact_staging_dir`; the effective `limits`; and the
`deadline`.

Two omissions are deliberate. The request carries no credential material — see
section 12. It also carries no `instance_id`: producer provenance is core's,
the Field has no use for it, and not sending it means a Field cannot embed it in
a cursor, a diagnostic, or an upstream request.

`config` is non-secret by construction, because core never puts credential
material there. A Field must not treat any value in `config` as a secret.

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
- `note_type`: the primary Note type candidate;
- `occurred_at`: the event instant with an explicit offset;
- `properties`: flat property candidates keyed by A1 property names — shared
  registry names plus this Field's declared prefixed names;
- `body`: deterministically normalized source evidence as Markdown text;
- `artifacts`: original-byte references in role order, per section 8;
- `identity_anchors`: structured anchors, per section 7;
- `integrity`: `damaged`, `truncated`, and measured `lost_characters`.

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
names. It admits no dot, no separator, and no traversal sequence, so a handle
cannot be a path however it is spelled. Core joins the handle to the staging
directory, opens it without following symlinks, requires a regular file whose
identity is unchanged between check and use, and bounds the read by the
declared length.

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
- The staging directory is operational, not notebook state: it lives under the
  disposable cache class and its removal is always safe.
- A crash mid-run leaves staged bytes that startup recovery removes; no Note
  references them because the record was never accepted.

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

**Core commits a cursor only after** every record with a sequence number at or
below `covers_record_seq_through` has reached durable current state and the
store's durability barrier has returned. The order is fixed:

1. validate and bound the event;
2. normalize it into core domain values;
3. locate the current Note by portable source key;
4. stage, verify, and install or reuse original artifacts;
5. make artifacts durable;
6. stage and atomically install the Note replacement;
7. remove a superseded filename only after the replacement exists durably;
8. atomically commit the cursor.

Never the reverse, and never partially. `records_covered` disagreeing with what
core received fails the run, because it means the two sides disagree about what
was transferred.

**A cursor may advance** only at a checkpoint whose covered records are all
durable, and only monotonically within a run in emission order. A run in which
core rejected a record commits no further checkpoint.

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
semantic payloads are a no-op, within a run or across runs. Two frames for one
key with divergent payloads and no declared version ordering are a Field
defect, not a conflict: within a single run, one producer asserting two
different current states for one object with no ordering is a bug, and core
rejects the second frame rather than manufacturing a conflict bundle out of it.

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
inventing a Field-level meaning for it.

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

Protocol v1 freezes these ceilings. A notebook may configure a value lower;
raising one above its ceiling requires a protocol revision.

| Bound | Ceiling |
|---|---|
| One frame, including its terminating LF | 1 MiB |
| Record `body.text` | 1 MiB |
| Property candidates per record | 256 |
| One property value | 64 KiB |
| List members | 1024 |
| Artifact references per record | 64 |
| One staged artifact | 512 MiB |
| Staged bytes per run | 8 GiB |
| Standard output per run | 1 GiB |
| Records per run | 1,000,000 |
| Diagnostics per run | 10,000 |
| Captured standard error per run | 256 KiB, ring-buffered |
| Cursor | 4 KiB |
| Run wall clock | 3600 s |
| Idle without a frame or artifact progress | 120 s |
| Cancellation grace before termination | 10 s |

Rejection is uniform: **any protocol violation fails the run.** Core stops
consuming output, terminates the child after the grace period, removes the run's
staging directory, and commits no further checkpoint. Checkpoints already
committed stand, because they were durable before they were committed.

The v1 rejection codes are closed and grouped by what went wrong:

- `protocol.invalid_utf8`, `protocol.not_json`, `protocol.oversized_frame`,
  `protocol.truncated_frame`, `protocol.schema_invalid`,
  `protocol.unknown_event`, `protocol.unexpected_order`,
  `protocol.duplicate_seq`, `protocol.seq_regression`,
  `protocol.limit_exceeded`, `protocol.timeout`, `protocol.idle_timeout`,
  `protocol.stderr_flood`, `protocol.version_unsupported`;
- `record.unknown_property`, `record.undeclared_property`,
  `record.foreign_prefix`, `record.property_type_mismatch`,
  `record.invalid_note_type`, `record.invalid_datetime`,
  `record.missing_source_key`, `record.duplicate_divergent_in_run`;
- `artifact.invalid_handle`, `artifact.digest_mismatch`,
  `artifact.length_mismatch`, `artifact.missing_staged_file`,
  `artifact.unknown_digest`, `artifact.oversized`;
- `deletion.unauthorized`, `snapshot.completeness_contradicted`,
  `snapshot.scope_widened`;
- `credential.unknown_grant`, `credential.grant_expired`,
  `credential.channel_closed`;
- `manifest.property_type_changed`, `manifest.cursor_format_changed`,
  `manifest.undeclared_capability`.

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
  reserved device name, and a hard-link and check-versus-use race case;
- secret-canary tests with a unique canary granted on the protected channel and
  asserted absent from argv, the inherited environment, standard output,
  standard error, logs, diagnostics, cursors, Notes, artifacts, and any crash
  recovery file;
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
  declared-type or cursor-format change treated as a migration that blocks sync
  rather than a silent retype.

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

### Artifacts

- [ ] Staged-file transfer into a core-created, core-named, per-run staging
  directory, with a single-segment closed-character-set handle and no
  connector-supplied path anywhere.
- [ ] Core always computing its own digest and deriving A1 artifact identity
  and path from it, with a declared digest as a detection aid only.
- [ ] The `digest_only` reference, accepted only for a digest the notebook
  already stores and otherwise rejected so the Field retries with bytes.
- [ ] Artifacts durable before any Note that references them, and identical
  bytes deduplicating storage without ever collapsing Notes.

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
  idempotent by portable-source-key reconciliation.
- [ ] Identical duplicate records as no-ops within and across runs, and
  in-run divergence without declared ordering as a rejected Field defect.

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

### Failure, bounds, and trust

- [ ] The exit-code table, with reserved ranges and signal termination
  normalized to a failed run.
- [ ] Complete, partial, and failed run outcomes, with only complete
  authorizing deletion by absence, and durable work before a failure retained.
- [ ] Per-Field outcomes in a multi-Field sync, with the CLI's own exit-code
  table left to the CLI contract.
- [ ] Cooperative cancellation with a grace period, exit code 8, and a
  cancelled run never complete.
- [ ] The frozen bound ceilings, configurable downward only, and echoed to the
  Field in the request.
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

1. **In-run divergence for one source key.** The recommendation rejects the
   second frame as a Field defect rather than creating a conflict bundle,
   reasoning that one producer asserting two unordered current states within
   one run is a bug. A reviewer who would rather never lose evidence may prefer
   a conflict bundle even here.
2. **`source_version_ordering: unsupported` for the v0.1 Fields.** Neither a
   Graph change key nor a file content hash gives a reliable order, so both
   candidate manifests declare `unsupported`. A1's merge rule 4 — reliable
   newer version selects content — would then never fire for a shipping v0.1
   Field, and cross-instance divergence becomes a visible conflict instead.
   That is the safe reading, and it is worth confirming it is the intended one.
3. **The 512 MiB single-artifact ceiling.** Chosen as a number large enough for
   ordinary mail attachments and documents and small enough to bound a run.
   No existing document states a size expectation.
4. **The 3600 s run and 120 s idle ceilings.** Likewise chosen rather than
   derived. A first full mailbox sync may legitimately exceed an hour, in which
   case the answer is either a larger ceiling or windowed runs, and the reviewer
   may have a preference.
5. **Where the staging directory lives.** The recommendation places it under
   the disposable cache class, which makes cleanup unambiguous but means
   artifact bytes transit a directory a user may be told is safe to delete at
   any time. Placing it under operational sync state instead would be
   defensible.
6. **Whether `describe` should run on every sync.** The recommendation runs it
   every sync, because that is where negotiation and the manifest-change check
   happen. The cost is one extra process start per Field per sync.
7. **Transcript file format.** The corpus wraps wire frames in a fixture
   envelope so one file can show both directions, both channels, core's
   actions, and deliberately invalid bytes. A reviewer who wants transcripts to
   be literal wire captures would need a different arrangement, probably one
   file per direction plus a separate expectations file.
