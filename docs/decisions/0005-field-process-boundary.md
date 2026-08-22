# ADR 0005: Minimal trusted Field process boundary

- **Status:** Accepted. The dedicated protocol-schema follow-up this ADR
  called for is the [A2 Field protocol package](../approvals/A2-field-protocol.md),
  approved on 2026-08-23.
- **Date:** 2026-08-22

## Context

Vendor connectors need independent dependencies and authentication logic, while
the core must remain the sole authority for notebook validation and writing.
A process boundary helps dependency and failure isolation but is not a security
sandbox.

## Decision

External Fields run as bounded, short-lived child processes. Protocol v1 has:

- a versioned machine-readable description operation;
- one collection request from core;
- ordered record, checkpoint, and diagnostic events from the Field;
- diagnostic logs on standard error;
- core-owned validation, IDs, normalization, filesystem paths, atomic writes,
  source reconciliation, and checkpoint commit;
- no credentials in command-line arguments, event output, logs, or cursors.

The core advances a checkpoint only after every preceding record change is
durable. It imposes event-size, output, time, and cancellation bounds and treats
malformed output as a failed run.

Field processes are trusted code with access to the credentials explicitly
granted to them. v0.1 does not silently execute discovered `PATH` binaries or
present process isolation as protection against a malicious connector.

Before connector teams implement real Fields, a follow-up protocol proposal
must freeze the JSON/NDJSON schema, artifact transfer mechanism, authentication
operations, exit statuses, negotiation rules, limits, and conformance fixtures.
Those details are deliberately not invented in this ADR.

## Consequences

- Vendor SDKs do not enter notebook-format or storage crates.
- Connectors can be tested using a fixture executable and language-neutral
  conformance cases.
- A hung, noisy, or malformed connector cannot hold an unbounded core run.
- Connector installation and trust UX are product/security requirements.
- Artifact transfer and authentication protocol work block production
  connectors but not the initial domain/format/store scaffold.

