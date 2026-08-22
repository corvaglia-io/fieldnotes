# ADR 0004: UUIDv7 logical IDs, content-addressed artifacts, and hash domains

- **Status:** Proposed for user approval
- **Date:** 2026-08-22

## Context

Fieldnotes needs globally unique, filesystem-safe IDs and clear deduplication
semantics. One undifferentiated `content_hash` cannot safely represent artifact
bytes, normalized text, semantic record equality, and source identity.

## Decision

Use lowercase UUIDv7 values with readable logical-record prefixes such as
`fn_`, `note_`, `ext_`, `obs_`, `ent_`, `rel_`, `prop_`, `pkg_`, and `conf_`.

Immutable original artifacts are the deliberate exception. Identify them as
`artifact_sha256_<64-lowercase-hex>` using SHA-256 of the exact bytes, so the
same byte object deduplicates transparently across instances without a second
logical ID.

The UUIDv7 timestamp records ID creation time, not `occurred_at`. IDs are opaque
and stable for the lifetime of a record in the current notebook.

Use SHA-256 with separate domains:

- `sha256:<hex>` hashes exact artifact bytes; the same digest forms the suffix
  of the public artifact ID;
- `fn-content-v1-sha256:<hex>` hashes the frozen v1 normalized content
  representation and excludes IDs/provenance/capture metadata;
- an internal `fn-record-v1-sha256` hashes canonical semantic Note payloads
  whenever conflict comparison or deterministic candidate ordering is needed;
  it is not required public Note frontmatter.

Portable source identity is not a hash domain. It is the explicit
`(source_scope, source_identity)` pair.

Canonical byte inputs and normalization are fixed by golden test vectors. Any
incompatible normalization change creates a new versioned domain.

## Consequences

- IDs remain unchanged when `occurred_at` is corrected.
- Exact artifact reuse is distinct from normalized-content equality.
- Identical content does not collapse Notes from different source contexts.
- Tests need deterministic clock/ID injection and published hash fixtures.
- The textual UUID representation is longer than ULID but follows an Internet
  standard and is broadly supported.
