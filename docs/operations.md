# Operations and lifecycle

**Status:** Proposed v0.1 operational contract; command flags and archive
formats are approval-gated

Fieldnotes operates a disposable, current-state notebook. Operations preserve
portable evidence and producer provenance without turning collection into an
append-only history service.

## State locations

Operational behavior follows four state classes:

| Class | Examples | Loss behavior |
|---|---|---|
| Public notebook | Notes, retained original artifacts, generated public Markdown | Current collected evidence is lost; refetch may recover source-backed records |
| Durable local intent/config | instance/Field IDs, aliases, merge decisions, proposal review state, retention policy | Cannot be reconstructed reliably; restore from backup or reconfigure |
| Operational sync state | opaque cursors, checkpoints, last-run summaries, retry/backoff metadata, per-run artifact staging | Refetch/backfill and reconcile; completeness depends on source capability |
| Disposable cache | search/index/graph acceleration, parsed-file caches | Delete and rebuild at any time |

Operational cursors live under `.fieldnotes/state/sync/`, not
`.fieldnotes/cache/`. Each Field's committed cursor is
`.fieldnotes/state/sync/<field_id>.cursor` — the opaque token paired with the
`cursor_format_version` it was committed at, since a token is never replayed at a
version the Field no longer declares — and its last-run summary is
`.fieldnotes/state/sync/<field_id>.status.json`. A run's artifact staging
directory is `.fieldnotes/state/sync/staging/<field_id>/<run_id>/`, also
operational sync state and deliberately **not** the disposable cache: artifact
bytes must not transit a directory whose entire contract is "always safe to
delete", even briefly and even before they are durable. Startup recovery removes
staging left behind by a crashed run; no Note references it, because the record
it belonged to was never accepted.

Credentials live outside the notebook; only named credential-profile references
appear in configuration.

## Initialization

`fieldnotes init [path]` creates a notebook, a stable instance ID, the built-in
`self` Field configuration, and required directories. Initialization refuses to
silently adopt a non-empty incompatible directory. Re-running it is idempotent
when the existing notebook is valid.

Instance creation metadata uses RFC 3339 with an explicit numeric offset. The
instance ID is immutable; its friendly name may change.

The exact initial files, permissions, default configuration values, and
interactive UX are part of the repository/notebook approval gates for `0.1.0`.

## Locks and concurrent access

v0.1 should assume one Fieldnotes writer per notebook. Read-only tools may
inspect completed files concurrently. A mutating command acquires a notebook
writer lock, while collection additionally acquires a per-Field run lock so two
syncs cannot race one cursor.

Lock files contain no credentials or full source payloads. Stale-lock detection
must be conservative and platform-tested. Exact locking primitive, timeout,
owner metadata, and force-unlock UX are approval-gated before concurrent command
behavior is promised.

## Collection and current-state upsert

For each external record, core receives and validates a portable source key:

```text
(source_scope, source_identity)
```

Producer provenance remains:

```text
(instance_id, field_id)
```

The disposable source-key index accelerates lookup, but a current notebook scan
can rebuild it. A source update is an upsert of the current Note, not creation
of a history Note.

The durable sequence is:

1. validate protocol ordering, sizes, source key, properties, datetime, and
   artifact declarations;
2. locate the current Note by portable source key;
3. normalize content and compute domain-separated hashes;
4. stage, verify, and atomically install or reuse original artifacts;
5. stage the complete replacement Note on the destination filesystem;
6. atomically install it under the same Note ID;
7. when `occurred_at` changed, remove the old UTC-derived filename only after
   the replacement exists durably;
8. commit a checkpoint only after all preceding record changes are durable.

Frontmatter datetimes retain an explicit source/configured/client-local numeric
offset. Filenames render the represented instant in UTC. An offset change that
preserves the same instant does not require a different filename.

No prior body, revision event, or update tombstone is required. While the Note
remains present, upsert preserves its Note ID. Refetching a deliberately removed
Note may create a new Note ID while retaining the same portable source key.

Unexpected destination collisions and same-ID divergence are conflicts, never
last-writer-wins. Startup recovery identifies incomplete temporary files and
either removes or safely completes them under the approved storage rules.

## Checkpoints and retries

A checkpoint means that every preceding event has reached its durable current
state. Core, not the Field, commits it. A Field may emit several checkpoints in
one bounded run.

On process failure:

- durable Note and artifact changes before the last committed checkpoint may
  remain;
- the cursor does not advance past an undurable change;
- replay reconciles through portable source keys and hashes instead of creating
  uncontrolled duplicates;
- partial frames and valid-looking partial Notes are rejected.

Opaque cursor loss is not cache deletion. Recovery resets or re-establishes the
Field and performs the connector's supported refetch/backfill. If the source no
longer exposes older data, Fieldnotes reports the recovery gap rather than
claiming completeness.

Cursor schema, protocol event shape, batch/transaction granularity, retry
classification, and backoff policy require approval at the `0.1.1` Field
protocol gate.

## Authoritative deletions

A collected Note may be removed only when one of these is true:

- the connector emits an explicit, authoritative deletion for the portable
  source key under the approved protocol;
- an approved complete-snapshot reconciliation proves absence and that Field
  declares the snapshot authoritative for deletion;
- the user explicitly prunes or deletes the Note.

Missing pages, pagination failures, expired permissions, throttling, partial
history, an unavailable account, truncation, process failure, or an incomplete
snapshot are not deletions.

An authoritative deletion removes the current Note atomically and does not
create a required tombstone or revision ledger. Shared artifacts remain until
reference analysis proves them unreferenced. A later source refetch may recreate
the Note because Fieldnotes intentionally keeps no hidden deletion history.

The exact deletion event/schema, source version preconditions, snapshot
capability declaration, and deletion race behavior are approval-gated with the
Field protocol. Until a connector proves deletion authority, it is add/update
only.

## Status and diagnostics

`fieldnotes status` and `fieldnotes fields status` should distinguish:

- healthy, running, interrupted, backoff, re-authentication, and recovery states;
- last attempted and committed checkpoints without exposing opaque cursor data;
- counts for collected, updated, deleted, reused, skipped, damaged, truncated,
  conflicted, and failed records;
- source capability limitations and known recovery gaps;
- cache health separately from public notebook integrity.

Human-readable output is default. Stable JSON output is provided where
automation is a documented use case. Exact fields and exit statuses are
approval-gated with the CLI and protocol contracts.

## Validation and inspect

Validation checks public files without requiring a network source. It reports:

- invalid filenames, IDs, source keys, datetimes, and flat property types;
- source-specific keys without the registered prefix;
- missing or mismatched artifacts and hashes;
- duplicate Note IDs or portable source keys;
- dangling evidence, entity, relationship, or rendition references;
- same-ID and merge conflicts;
- obvious credential leakage;
- stale or damaged generated projections separately from source Notes.

`inspect` resolves a record ID independently of its current filename.
`explain` shows the evidence and rule/generator provenance for a derived claim.

## Rebuild

Rebuild never requires a source network call. It operates on current notebook
files, durable non-secret intent/configuration, and pinned deterministic rules.

A deterministic rebuild may remove and recreate:

- disposable indexes and search caches;
- entity and relationship projections;
- graph explanations and other explicitly rebuildable generated files.

It must not remove or rewrite:

- source Notes or retained original artifacts;
- instance/Field identity and source configuration;
- operational cursors;
- identity aliases, manual merge decisions, proposal review state, or other
  durable human intent.

Optional Extractions and Observations have their own enhancement rebuild in
`0.1.8`, using pinned generator contracts. A normal deterministic rebuild does
not silently download a model or run inference.

For identical current inputs, config, and rule versions, rebuild produces the
same semantic graph. Byte identity for generated files that contain generation
timestamps or new opaque IDs must be decided by approved projection fixtures;
semantic determinism is required regardless.

The exact generated directory set and preservation mechanism for reviewed
proposal state are approval-gated before `0.1.2`/`0.1.7` implementation.

## Merge

`fieldnotes merge <path>` treats the input as untrusted and should offer a
dry-run report before mutation. It validates and stages incoming records rather
than copying over existing paths blindly.

Merge applies these rules:

- same Note ID and same semantic content is the same Note;
- same Note ID with divergent content is preserved as a visible conflict;
- different Note IDs with the same portable source key are the same upstream
  object and may reconcile under source version/current-state rules;
- matching source key and matching content collapses to one Note while all
  producer references are retained;
- a reliably newer source version may become the current Note;
- unresolved divergent current states are preserved as conflicts;
- matching content or artifact hashes without matching source keys never
  discards contextual Notes;
- caches and graph projections are rebuilt after a successful merge.

Merge does not manufacture a revision history. A conflict file or staging copy
exists to prevent data loss during the merge decision; it is not a permanent
event ledger.

The exact conflict directory, deterministic survivor rule, `collected_by`
property name, source version comparison, transaction size, rollback mechanism,
and direct-copy repair UX are approval-gated for `0.1.2`.

## Field removal and reconfiguration

Removing a Field disables future collection and may remove its operational
cursor and credential reference. It does not silently delete Notes already
collected by that producer. Those Notes remain portable and attributable until
the user prunes them or an approved explicit removal option is invoked.

A Field ID is immutable after producing Notes. Account display names and
credentials may change without changing the ID. A change that points the same
Field ID at a different portable `source_scope` is rejected; the user configures
a new Field instead.

Exact credential deletion, cursor preservation, and explicit collected-Note
removal flags are approval-gated with Field-management UX.

## Archive

Archive is introduced in `0.1.7`. It prepares a portable, verifiable copy of an
approved selection and its dependency closure. Archive is not upload, hosted
backup, immutable retention, or proof of delivery.

An archive operation should:

1. select Notes by validated datetime instant and explicit filters, not filename
   text alone;
2. close over required original artifacts and approved evidence references;
3. include a checksum-covered, secret-free manifest;
4. preserve explicit-offset frontmatter and UTC filenames;
5. validate and round-trip before reporting success;
6. leave the live notebook unchanged by default.

Archive format, compression, encryption, directory layout, manifest schema,
selection syntax, and whether generated projections are included are approval-
gated for `0.1.7`.

## Prune

Prune is conservative and explicit. Its default mode is a preview that reports
selected Notes, dependent generated records, shared/unshared artifacts,
proposal-review implications, and the fact that refetch may recreate source
objects. A mutating prune requires confirmation or an explicit non-interactive
flag under an approved CLI contract.

Prune evaluates age using the instant represented by `occurred_at`, not a
timezone-less value or filename parsing alone. It never removes an artifact
still referenced by a retained Note, archive dependency, or other retained
public record under policy.

Pruning a source Note does not create a tombstone and does not write to the
source. The operational cursor may remain ahead of the pruned object, so it may
stay absent until a refetch; after refetch it may return with a new Note ID.
Users who require durable suppression need a separately approved retention
policy, not an implicit source-history ledger.

Exact duration grammar, period boundaries, dependency closure, confirmation,
grace periods, and reviewed-proposal handling are approval-gated for `0.1.7`.

## Recovery matrix

| Failure or loss | Recovery |
|---|---|
| Disposable cache deleted | Rebuild locally from current notebook and durable non-secret config |
| Generated graph files deleted | Deterministic rebuild |
| Sync cursor lost | Connector refetch/backfill, exact source-key reconciliation, visible gap if source history is unavailable |
| Current source Note deleted locally | Refetch if upstream still exposes it; a new Note ID is allowed |
| Source authoritatively deleted object | Current Note may be removed; no tombstone; upstream restore/refetch may recreate it |
| Original artifact missing | Refetch/re-import if available; otherwise report a damaged reference |
| Same-ID divergent merge | Preserve conflict; require deterministic or user resolution |
| Durable user intent/config lost | Restore its backup or reconfigure; graph rebuild cannot infer it |
| Credential removed/revoked | Re-authenticate; never recover it from notebook or logs |
| Interrupted archive/prune | Live notebook remains unchanged until the approved transaction commits; recover staged temporary files |

## Release alignment

- **0.1.0:** initialization, self Notes, file/voice import, validation, atomic
  local writes, and baseline status/inspect.
- **0.1.1:** Field protocol, local Field, operational cursors, checkpoint/replay,
  and current-state upsert.
- **0.1.2:** deterministic rebuild, graph inspection, exact source-key merge,
  and conflict preservation.
- **0.1.3:** OS credential providers, protected Microsoft auth, and Outlook Mail
  sync/recovery.
- **0.1.4:** independent Calendar/Contacts cursors, recurrence/contact current
  state, and source-scoped identity.
- **0.1.5:** Teams permission diagnostics, partial history, refetch, and thread
  reconciliation.
- **0.1.6:** Jira current-state issue reconciliation and connector conformance.
- **0.1.7:** archive, prune, artifact lifecycle, proposal review preservation,
  handback packaging, and Obsidian lifecycle views.
- **0.1.8:** explicit enhancement lifecycle and separate derived rebuild.
- **0.1.9:** cross-platform packaging, upgrade behavior, soak/recovery tests,
  complete operations/troubleshooting reference, and traceability closure.

## Approval gates

The following details remain intentionally unsettled until their roadmap gate:

- writer lock implementation and recovery UX;
- exact protocol, checkpoint, and authoritative-deletion schemas;
- source-version comparison and current-state upsert transaction boundaries;
- merge conflict layout, provenance-list property, and survivor rule;
- archive container, manifest, encryption, and round-trip format;
- prune selection, dependency closure, confirmation, and retention policy;
- rebuild-generated file set and durable review-state storage;
- Field removal's cursor, credential-reference, and collected-Note options;
- stable JSON status/diagnostic schemas and exit-code table.

