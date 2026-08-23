# A0 approval: repository scaffold

**Status:** Approved and implemented on 2026-08-22  
**Decision:** Use the proposed Rust workspace and Field process layout.

## Approved amendments

**2026-08-23:** Release `0.1.1` added a reusable Field-authoring SDK crate,
`fieldnotes-field-sdk`, extracted from what the working `local` and `fixture`
Fields actually needed rather than designed speculatively at A0 time. The
rationale, its contents, what it deliberately excludes, and its dependency
rule are recorded in
[ADR 0009](../decisions/0009-field-sdk-extraction.md). The workspace tree and
the crate-responsibility table below are updated to include it; A0's original
tree and table did not name a separate SDK crate, instead folding "host/SDK
support" into `fieldnotes-field-protocol`'s row, and that phrase is now
removed from that row since the SDK crate owns it.

**2026-08-23:** The A1 shared property registry moved from
`fieldnotes-format` to `fieldnotes-domain`. ADR 0009 flagged, but did not
revisit, that `fieldnotes-field-protocol` depended on `fieldnotes-format`
solely to reach the registry, so every Field binary transitively linked the
entire canonical notebook serializer — exactly the notebook-byte-work
coupling A2 section 6's normalized source envelope decision exists to keep
out of a Field process. [ADR 0010](../decisions/0010-property-registry-relocation.md)
records the fix: the registry (names, scalar types, list semantics) is
vocabulary with no I/O and no byte form of its own, which is
`fieldnotes-domain`'s row below already, so it now lives there;
`fieldnotes-format` re-exports it for its own internal use and for existing
callers such as `fieldnotes-app`. `fieldnotes-field-protocol` no longer
depends on `fieldnotes-format` at all, confirmed by `cargo tree
-p fieldnotes-field-local -e normal` and `cargo tree -p fieldnotes-field-fixture
-e normal`. The `fieldnotes-domain` row below is updated to name the
registry explicitly.

## Recommendation

Use a Cargo workspace with small core crates organized by dependency boundary, and keep each external Field as a sibling process under `fields/`.

```text
fieldnotes/
├── Cargo.toml
├── rust-toolchain.toml
├── README.md
├── AGENTS.md
├── crates/
│   ├── fieldnotes-domain/
│   ├── fieldnotes-format/
│   ├── fieldnotes-store/
│   ├── fieldnotes-field-protocol/
│   ├── fieldnotes-field-sdk/
│   ├── fieldnotes-app/
│   ├── fieldnotes-graph/
│   ├── fieldnotes-credentials/
│   ├── fieldnotes-cli/
│   ├── fieldnotes-msgraph/
│   └── fieldnotes-test-support/
├── fields/
│   ├── fieldnotes-field-fixture/
│   ├── fieldnotes-field-local/
│   ├── fieldnotes-field-outlook-mail/
│   ├── fieldnotes-field-outlook-calendar/
│   ├── fieldnotes-field-outlook-contacts/
│   ├── fieldnotes-field-teams/
│   └── fieldnotes-field-jira/
├── docs/
└── tests/
    └── fixtures/
        ├── notebooks/
        ├── protocol/
        └── hashes/
```

The workspace starts with directories and minimal compiling crates only. Enhancement is added near `0.1.8` after its engine and packaging boundary is approved.

## Responsibilities

| Path | Responsibility |
|---|---|
| `fieldnotes-domain` | IDs, scalar property algebra, the shared property registry, shared vocabulary, source and producer keys; no I/O |
| `fieldnotes-format` | strict Markdown/frontmatter parsing and canonical serialization |
| `fieldnotes-store` | atomic Note/artifact/state operations and merge mechanics |
| `fieldnotes-field-protocol` | versioned process DTOs, schemas, host support, conformance helpers |
| `fieldnotes-field-sdk` | Field-authoring helpers (cursor encoding, artifact staging, truncation, scope derivation, frame emission, request dispatch); no vendor logic, no notebook byte work |
| `fieldnotes-app` | sync orchestration and application use cases |
| `fieldnotes-graph` | deterministic identities, entities, relationships, explanation, proposals |
| `fieldnotes-credentials` | credential-provider interface, OS adapters, protected delivery |
| `fieldnotes-cli` | thin executable and human/JSON presentation |
| `fieldnotes-msgraph` | shared Microsoft authentication, transport, retry, and fixture support |
| `fieldnotes-test-support` | injected clocks/IDs, notebook builders, fake processes, fixture helpers |
| `fields/*` | source-specific read-only collection and mapping; never notebook writes |

## Dependency rule

Core domain and format crates know nothing about vendor APIs. Field processes depend on the protocol library, not the notebook store. The CLI composes application services but does not duplicate their rules. Root manifests, protocol schemas, shared vocabulary, and golden fixtures have one coordinator-owned integration path.

## Toolchain policy

- Rust edition 2024.
- Pin one stable Rust toolchain in `rust-toolchain.toml` when the scaffold is generated.
- Keep the pinned compiler as the 0.1-line MSRV unless a documented dependency or security requirement forces a reviewed increase.
- Run formatting, Clippy with warnings denied, tests, and Rustdoc warnings in CI.
- Execute checks and tests on native x64 and ARM64 GitHub-hosted runners for Windows, Linux, and macOS, covering all six target triples. Reconfirm runner availability and platform behavior before each release claim.
- Keep inference/model dependencies out of the default workspace dependency graph until the enhancement milestone.

The verified official installer was used to install the pinned Rust 1.98.0 minimal toolchain with Rustfmt and Clippy. Shell startup files were not modified; local verification uses the installed Cargo path explicitly.

## Alternatives considered

### One large core crate

This lowers initial manifest overhead but creates overlapping ownership for parallel agents and makes it easier for vendor, filesystem, CLI, and model dependencies to leak across boundaries.

### One crate per command or every tiny concept

This maximizes isolation but creates excessive compilation and coordination overhead. The proposed split follows meaningful dependency and failure boundaries instead.

### Connectors inside the main CLI process

This simplifies early calls but couples vendor SDKs and authentication failures to the notebook writer. It also fails to exercise the specification's language-neutral Field boundary.

## Approval effect

A0 authorized and produced this workspace, minimal crate manifests, baseline CI, formatting/lint configuration, and empty test-fixture directories. It did not approve record names, Markdown schemas, Field protocol event shapes, or connector capability slices; those remain blocked by A1 and A2.

## Verification evidence

The generated 17-package workspace passes formatting, metadata resolution, `cargo check --workspace --all-targets`, `cargo test --workspace --all-targets`, Clippy with warnings denied, and Rustdoc generation under Rust 1.98.0. All packages are private/unpublished and no external Rust dependencies or feature implementations were added at A0.
