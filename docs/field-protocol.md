# Field process protocol

**Status:** Boundary proposed; exact protocol v1 is approval milestone A2.

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

## Operations and events to freeze at A2

### `describe`

Returns protocol compatibility, registered Field type and prefix, supported source-object capability slices, authentication kind, identity-scope rules, and incremental/snapshot/tombstone/refetch behavior.

### Collection request

Carries protocol version, configured Field ID, optional opaque cursor, optional bounded time/window request, non-secret connector configuration, cancellation/deadline information, and an approved protected credential reference/channel.

### `record`

Carries a bounded normalized source envelope containing portable `source_scope`, `source_identity`, optional reliable source version, primary Note-type candidate, timestamps, flat shared/prefixed property candidates, body content, identity anchors, artifact descriptors, and damage/truncation information.

The A2 decision must choose the exact normalized-envelope boundary. A Field never emits a path that core trusts as a notebook destination and never writes a rendered Note directly.

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

A2 must approve:

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

Every live Field must later pass this shared suite plus its vendor fixtures. Connector work cannot amend protocol v1 privately.

