# Field process protocol

**Status:** Exact protocol v1 is approval milestone A2, approved by the user on
2026-08-23: see the [A2 approval package](approvals/A2-field-protocol.md) and
its [frozen schemas and transcripts](../tests/fixtures/protocol/proposed-v1/README.md).
This document remains the conceptual boundary; A2 is the exact, now-frozen
contract for it.

## Purpose

External Fields are bounded, read-only child processes. The protocol isolates vendor dependencies and failures while keeping Fieldnotes core the sole validator, ID authority, reconciler, notebook writer, and checkpoint committer. It is a trusted adapter boundary, not a sandbox for malicious plugins.

The built-in `self` Field does not use this protocol.

## Conceptual exchange

Core starts one configured Field for one bounded operation, supplies non-secret configuration plus protected credential access, and consumes ordered newline-delimited events.

```text
core -> collect request
field -> record
field -> record
field -> checkpoint
field -> diagnostic
field -> process exit
```

Logs use standard error. Protocol data uses standard input/output. No secret may enter arguments, environment inherited by default, event streams, logs, cursors, or notebook material.

## Operations and events frozen at A2

### `describe`

Returns protocol compatibility, registered Field type and prefix, supported source-object capability slices, authentication kind, identity-scope rules, and incremental/snapshot/tombstone/refetch behavior.

It also declares every connector-prefixed property the Field may emit, with that property's name, scalar type, and list semantics (set-like versus order-bearing). Core validates emitted records against the declaring manifest and rejects prefixed properties the manifest does not declare. This closes the gap [ADR 0006](decisions/0006-a1-implementation-rulings.md) ruling 4 deferred to A2, where a prefixed property's type was inferred from canonical spelling because no registry entry existed.

### Collection request

Carries protocol version, configured Field ID, optional opaque cursor, optional bounded time/window request, non-secret connector configuration, cancellation/deadline information, and an approved protected credential reference/channel.

The A2 package adds three members this list did not anticipate: an explicit collection `mode` distinguishing an incremental run from a reconciling snapshot, the declared `snapshot_scope` a snapshot run claims to cover, and the core-created per-run artifact staging directory. It deliberately omits the notebook `instance_id`, which is producer provenance the Field has no use for.

### `record`

Carries a bounded normalized source envelope containing portable `source_scope`, `source_identity`, optional reliable source version, primary Note-type candidate, timestamps, flat shared/prefixed property candidates, body content, identity anchors, artifact descriptors, and damage/truncation information.

The A2 package approves that boundary as **post-mapping and pre-serialization**: the Field maps vendor structures onto A1 vocabulary and does none of the serialization, so core stays the single canonical serializer, the final validator, and the sole durable writer. Record IDs, producer provenance, capture time, hashes, canonical key order and scalar spelling, filenames, and rebuildable projection lists are core's and are structurally absent from the record envelope. A Field never emits a path that core trusts as a notebook destination and never writes a rendered Note directly. The rejected alternative, a nearly rendered Note candidate, is argued in [A2 section 6](approvals/A2-field-protocol.md).

### `checkpoint`

Carries an opaque cursor that becomes eligible for commit only after every preceding record change is durable. A checkpoint never contains credentials or full sensitive payloads.

### `diagnostic`

Carries bounded structured health, permission, rate-limit, truncation, skipped-content, refetch, or damage information. Sensitive source payloads are excluded by default.

## Ordering and current-state rules

- Events are ordered within one process run.
- Replaying from the last committed checkpoint is safe.
- Duplicate records with the same portable source key reconcile to one current Note.
- A changed object updates that Note atomically without creating revision history.
- Deletion occurs only from an approved authoritative tombstone event or complete-snapshot reconciliation; missing partial results never delete.
- Artifact bytes become durable before a Note references them.
- A checkpoint commits only after all preceding artifact and Note changes are durable.
- A non-zero exit means the run did not complete normally; already durable work before the last committed checkpoint may remain and must replay safely.

## Trust and resource boundary

Core runs only first-party or explicitly trusted/configured Field executables. It does not silently execute an arbitrary matching binary from `PATH` and grant it credentials.

Protocol v1 must bound frame size, body size, artifact size/streaming, stderr capture, process lifetime, idle time, and cancellation. It must define invalid UTF-8, partial frame, unknown event, version mismatch, hang, stderr flood, and child-crash behavior. Filesystem paths, symlinks, and artifact handles must be validated by core.

## A2 decisions

The [A2 package](approvals/A2-field-protocol.md) carries the approved decision
for every item below, together with the frozen schemas and transcripts. The
user approved A2 on 2026-08-23; everything here is now in force.

A2 approves:

- exact JSON Schemas and examples for manifest, request, records, checkpoints, diagnostics, and authoritative deletion/snapshot signaling;
- protocol negotiation and compatibility rules;
- executable discovery/configuration and trust confirmation;
- normalized envelope versus rendered-candidate boundary;
- artifact streaming/handle mechanism and limits;
- authentication operations and protected credential transport;
- exit codes, cancellation, deadlines, and resource bounds;
- source-scope derivation declarations;
- diagnostic codes and redaction behavior.

## A2 conformance evidence

The approval package includes example transcripts, generated schema validation, a fake Field, and tests for normal collection, duplicate replay, changed objects, authoritative deletion, partial snapshots, pagination checkpoints, crash before/after checkpoint, malformed/oversized/invalid output, version mismatch, stderr floods, hangs, malicious paths, cancellation, and secret canaries.

The reviewable half of that — the schemas and the transcripts — is attached to the A2 package as [frozen protocol fixtures](../tests/fixtures/protocol/proposed-v1/README.md), approved and now the IG2 implementation target. The fixture Field and the executable conformance suite are the subsequent IG2 implementation evidence.

Every live Field must later pass this shared suite plus its vendor fixtures. Connector work cannot amend protocol v1 privately.

