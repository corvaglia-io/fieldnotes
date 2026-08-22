# Fieldnotes v0.1 architecture

**Status:** A0 workspace scaffold, A1 notebook contract, and A2 Field protocol
approved; connector-specific and later-release decisions remain proposed

Fieldnotes is a local-first, current-state context collector. The architecture
keeps the public notebook portable, source connectors isolated, deterministic
behavior useful by default, and every performance database replaceable.

## Architectural boundaries

```text
external source
      │
      ▼
Field child process ── records/checkpoints/diagnostics ──► sync application
                                                               │
user input ─────────────── built-in self Field ─────────────────┤
                                                               ▼
                                                  validation + normalization
                                                               │
                                                               ▼
                                                atomic notebook persistence
                                                               │
                                            ┌──────────────────┴──────────────┐
                                            ▼                                 ▼
                                  deterministic graph                 optional enhancement
                                            │                        Extractions/Observations
                                            └──────────────────┬──────────────┘
                                                               ▼
                                                    proposals for external action
```

The core never writes to an external Field. A proposal is the final Fieldnotes
stage; a human or external agent decides whether to act on it.

## Four state classes

The implementation must distinguish these classes explicitly.

### 1. Public notebook state

`notes/`, `artifacts/`, and public generated Markdown directories are the
canonical file interface for the notebook's current state. They are portable
and understandable without Fieldnotes. Canonical does not mean irreplaceable:
collected Notes may be updated, deleted, pruned, or recovered by refetching.

Generated public files such as entities and relationships conform to the public
notebook format while present, but they are disposable projections rather than
source evidence. They remain reproducible from current evidence and rules.

### 2. Durable local intent and configuration

This class contains state that cannot be inferred from current source evidence:

- stable instance identity;
- Field definitions and immutable Field IDs;
- portable source-scope configuration when a connector needs it;
- user identity aliases, merge decisions, and resolution overrides;
- proposal review decisions or other human-authored workflow state;
- retention policy.

It lives under `.fieldnotes/` but not under `cache/`. It should be included in a
notebook backup when the user wants behavior and review decisions to survive.
Secrets are references only; credential values live outside the tree.

### 3. Operational synchronization state

Opaque cursors, checkpoint metadata, and source backoff state live under
`.fieldnotes/state/sync/`. They are not public notebook truth and may be lost,
but they are not advertised as a freely disposable cache because an opaque
cursor cannot always be reconstructed from Notes.

Loss recovery is connector refetch/backfill followed by reconciliation through
portable source keys. A connector that cannot refetch must report that limit
clearly; Fieldnotes cannot promise recovery the source does not support.

### 4. Disposable caches

Search indexes, graph acceleration stores, parsed-file caches, and temporary
work live under `.fieldnotes/cache/`. Deleting this directory is always safe.
It can be rebuilt from public notebook state plus non-secret durable config and
rules.

Temporary files used during atomic writes are not durable state. Startup
recovery removes or finishes them according to the store transaction rules.

## Core invariants

- Notebook frontmatter is sparse, flat, type-stable, and strictly validated.
- Filesystem paths are derived by the core, never trusted from a connector.
- A source object is portable across instances through
  `(source_scope, source_identity)`.
- Producer provenance remains `(instance_id, field_id)` and survives merge.
- Source updates reconcile the current Note; v0.1 has no revision-ledger or
  tombstone-history requirement.
- Artifacts become durable before Notes reference them, and checkpoints advance
  only after preceding Note changes are durable.
- Hashes are domain-separated; content equality is not source identity.
- Deterministic graph output is stable for the same notebook, config, and rule
  versions.
- Credentials and obvious secret material never enter notebooks, process
  arguments, cursors, or normal diagnostics.

## Field process boundary

External Fields are short-lived child processes. The boundary isolates vendor
SDKs and authentication dependencies without making notebook schemas
connector-defined.

For v0.1 the protocol direction is:

- a versioned machine-readable `describe` operation;
- one bounded collection request from core;
- ordered record, checkpoint, and diagnostic events from the Field;
- logs on standard error;
- no direct notebook writes by a Field;
- no secrets in command-line arguments or event/log streams;
- explicit cancellation, resource bounds, and failure reporting;
- checkpoints committed by core only after preceding writes are durable.

Exact event schemas, artifact transfer, auth-operation messages, exit codes,
and compatibility rules require a focused protocol proposal and conformance
fixtures before real connectors are implemented. They should not be inferred
independently by connector teams.

Field executables are trusted adapters, not a security sandbox. v0.1 should run
only built-in/first-party or explicitly configured executables. Conventional
name discovery must not silently execute an arbitrary program from `PATH` or
give it credentials.

## Atomic persistence sequence

A collection unit follows this order:

1. validate and bound the Field event;
2. normalize it into core domain values;
3. locate an existing current Note by portable source key;
4. stage new artifacts on the target filesystem;
5. make artifacts durable;
6. stage and atomically install the Note replacement;
7. remove the old filename after a successful time-changing replacement;
8. atomically commit the checkpoint after all preceding records are durable.

The storage implementation uses same-filesystem temporary files, refuses
unexpected destination collisions, and treats directory synchronization as a
platform-specific durability concern. Crash-injection tests define the actual
v0.1 guarantee. Shared artifacts are reclaimed only by a separate verified
garbage-collection pass.

## Deterministic graph and enhancement

The graph package resolves exact identities, source-provided relationships,
threads, duplicates, first/last seen values, and evidence counts. It does not
need a graph database. An on-disk index is an optimization and belongs in the
disposable cache class.

Optional enhancement consumes existing Notes and emits separate Extractions
and Observations. It never changes a canonical Note. Enhancement should remain
behind a separate crate/process boundary so a default build does not acquire a
model runtime, model download, GPU requirement, or provider abstraction.

The precise enhancement engine remains a separate product decision. The file
contracts may be implemented and tested before an engine ships.

## Approved Rust workspace scaffold

This scaffold was approved at A0 and generated with placeholder-only crates:

```text
fieldnotes/
├── Cargo.toml                       # workspace, shared deps and lints
├── rust-toolchain.toml
├── README.md
├── docs/
│   ├── architecture.md
│   ├── notebook-format.md
│   └── decisions/
├── crates/
│   ├── fieldnotes-domain/           # IDs, values, vocabulary, source keys
│   ├── fieldnotes-format/           # strict parse/canonical serialization
│   ├── fieldnotes-store/            # atomic files, artifacts, merge
│   ├── fieldnotes-field-protocol/   # wire DTOs, schema, conformance support
│   ├── fieldnotes-app/              # sync orchestration and use cases
│   ├── fieldnotes-graph/            # deterministic entities/relationships
│   ├── fieldnotes-credentials/      # provider interface and OS adapters
│   ├── fieldnotes-cli/              # thin composition root and binary
│   ├── fieldnotes-msgraph/          # shared Microsoft transport/auth support
│   └── fieldnotes-test-support/     # fixtures/fakes; not a runtime dependency
├── fields/
│   ├── fieldnotes-field-fixture/    # protocol/crash conformance executable
│   ├── fieldnotes-field-local/      # first external process implementation
│   ├── fieldnotes-field-outlook-mail/
│   ├── fieldnotes-field-outlook-calendar/
│   ├── fieldnotes-field-outlook-contacts/
│   ├── fieldnotes-field-teams/
│   └── fieldnotes-field-jira/
└── tests/
    └── fixtures/
        ├── notebooks/
        ├── protocol/
        └── hashes/
```

The workspace deliberately separates ownership areas suitable for parallel
development while keeping the number of runtime processes small.

### Dependency direction

```text
fieldnotes-cli ──► fieldnotes-app ──► fieldnotes-store ──► fieldnotes-format
                         │                    │                     │
                         │                    └─────────────────────┤
                         ├──► fieldnotes-graph                      ▼
                         ├──► fieldnotes-credentials         fieldnotes-domain
                         └──► fieldnotes-field-protocol

fields/* ──► fieldnotes-field-protocol
```

`fieldnotes-domain` has no filesystem, process, network, vendor SDK, or model
dependency. Field binaries do not depend on notebook storage. The CLI is a thin
composition root rather than a second implementation of application rules.

An enhancement crate/process is intentionally absent from the 0.1.0 scaffold.
The engine is a required capability of the later 0.1.8 increment, but its
crate/process shape should follow the engine, licensing, packaging, and resource
decision rather than burdening inference-disabled releases prematurely.

## Verification strategy

The architecture should be verified at its boundaries:

- golden byte fixtures for Markdown/frontmatter and hash domains;
- property tests for IDs, source keys, merge idempotence, and order-independent
  exact deduplication;
- fake Field processes for replay, partial frames, oversized input, invalid
  encoding, stderr floods, hangs, and crashes around checkpoints;
- failpoints around artifact, Note, rename, and checkpoint persistence;
- deterministic graph rebuild tests with clocks and ID generators injected;
- cross-platform filesystem tests on Linux, macOS, and Windows;
- secret-canary tests across files, diagnostics, process arguments, and cursors;
- malformed-format corpora and fuzz targets for YAML/Markdown and protocol
  parsers;
- mocked credential-provider contracts plus gated OS-keychain smoke tests.

Cross-compiling is useful but does not prove runtime support. Release CI should
execute the test suite on each operating-system family and run ARM64 smoke tests
on real ARM64 systems when release support is claimed.

## Decisions still requiring user approval

- UUIDv7-prefixed IDs and the public hash domains proposed here;
- the exact object/capability slices supported by each already-selected
  external Field;
- the enhancement engine, licensing, packaging, and resource envelope for the
  required 0.1.8 increment;
- the exact treatment of human proposal-review state in the CLI;
- the minimum Linux runtime baseline and release packaging.
