# ADR 0010: Move the property registry from `fieldnotes-format` to `fieldnotes-domain`

- **Status:** Accepted on 2026-08-23
- **Date:** 2026-08-23

## Context

`fieldnotes-field-protocol` depends on `fieldnotes-format` for exactly one
thing: the A1 shared property registry (`PropertyRegistry`, `PropertyType`,
`ListSemantics`, `SEMANTIC_EXCLUSIONS`, `DERIVED_RECORD_ONLY`, and
`is_note_applicable`), which `DeclaredPropertyIndex` needs to enforce two of
the ruling-4 rules: reject an unprefixed name outside the closed registry,
and reject a name the registry types for a derived record only. That
dependency was flagged as pre-existing and not revisited when
[ADR 0009](0009-field-sdk-extraction.md) extracted the Field SDK: "core
computes its own digest ... `fieldnotes-field-protocol` itself already
depends on `fieldnotes-format` for the A1 shared property registry — a
pre-existing, separately justified dependency this ADR does not revisit —
so it remains transitively present in any Field binary via the protocol
crate regardless of the SDK's existence." The recommendation at the time was
to fix it before more Field binaries were built on top of the protocol
crate. That did not happen before `0.1.1` shipped the `local` Field and the
Field SDK, so it is fixed now, before any live connector (`outlook_mail`,
`outlook_calendar`, `outlook_contacts`, `teams`, `jira`) is built.

`cargo tree -p fieldnotes-field-local -e normal` confirms the shape of the
problem directly: `fieldnotes-field-local` depends on
`fieldnotes-field-protocol`, which depends on `fieldnotes-format`, which is
the crate that owns the RFC 8785 canonical number emitter, the
plain-versus-quoted text rule, the flat-YAML frontmatter parser, and the
Note filename computation. Every Field binary — including every future
Microsoft or Jira connector — therefore links the entire canonical notebook
serializer, for a dependency that needs only vocabulary: names, scalar
types, and list semantics, never a byte form.

[A2 section 6](../approvals/A2-field-protocol.md#6-the-record-envelope-a-normalized-source-envelope)
chose the normalized source envelope specifically to prevent this. Its first
and heaviest-weighted reason for rejecting the alternative (a Field emitting
a nearly rendered Note) is that doing so "duplicates the A1 canonical
serializer into every Field, in every language a Field may be written in,"
and that "every correct implementation of that is a place it can be
implemented incorrectly." A2 section 6 also says plainly that a Field "does
no byte work" — mapping vendor structure onto A1 vocabulary, never spelling
canonical bytes. A Field binary that *links* the serializer, even without
calling it, is one refactor away from someone using it by convenience, and
it makes every connector binary larger and more coupled to core's
serialization internals than the approved design intends. This is exactly
the coupling A2 section 6 was written to keep out of a Field process.

A0's crate-responsibility table already assigns `fieldnotes-domain` "IDs,
scalar property algebra, shared vocabulary, source and producer keys; no
I/O" — which is precisely what a property registry is. It has no byte form
of its own; the byte form is `fieldnotes-format`'s job, and only
`fieldnotes-format`'s.

## Decision

Move the property registry — `PropertyRegistry`, `PropertyType`,
`ListSemantics`, the registry contents, `SEMANTIC_EXCLUSIONS`,
`DERIVED_RECORD_ONLY`, and `is_note_applicable` — from
`crates/fieldnotes-format/src/registry.rs` into
`crates/fieldnotes-domain/src/property/registry.rs`, as a new `registry`
submodule of the existing `fieldnotes-domain::property` module (which
already owned the property-name grammar, `is_valid_property_name`).

`fieldnotes-format/src/registry.rs` becomes a thin re-export of
`fieldnotes_domain::property::registry::*`, so `crate::registry::*` still
resolves everywhere inside `fieldnotes-format` (`build.rs`, `emit.rs`,
`record.rs`) and `fieldnotes_format::{PropertyRegistry, PropertyType,
ListSemantics}` still resolves for every existing caller outside it
(`fieldnotes-app`'s `sync::project`), with no changes required in either.
This keeps the diff to the two crates that actually needed to change:
`fieldnotes-domain` gains the registry, and `fieldnotes-field-protocol`
stops depending on `fieldnotes-format` and depends on `fieldnotes-domain`'s
registry directly instead.

`fieldnotes-field-protocol`'s one other use of `fieldnotes-format` —
`session::semantic_fingerprint` calling `fieldnotes_format::sha256_hex` to
fingerprint a duplicate-detection payload — is *not* a registry concern and
is not moved. `fieldnotes-field-protocol` already depends on `sha2` directly
(to derive its own artifact digest rather than trusting a Field's declared
one), and `artifact.rs` already has its own private lowercase-hex-encoding
helper next to that digest logic. That helper is made `pub(crate)` and
reused by `session.rs`, so `semantic_fingerprint` computes the identical
SHA-256-then-lowercase-hex value it always did, without reaching
`fieldnotes-format` for it. This is a mechanical substitution of one
already-duplicated hex-rendering routine for another; no hashing behavior
changes.

**This is a pure relocation.** Every registry entry's name, scalar type, and
list semantics is unchanged; `SEMANTIC_EXCLUSIONS` and
`DERIVED_RECORD_ONLY` are unchanged, in the same order; `is_note_applicable`
has the same signature and behavior. Every unit test that exercised the
registry moved with it, unedited. The full conformance suite — byte-for-byte
round trips over the valid corpus, rejection of the invalid corpus with the
same conceptual errors, and hash-vector reproduction — passes unchanged,
which is the strongest evidence available that no approved A1 byte
changed.

### Alternative considered: leave it, and revisit only when a live connector needs to

Rejected. `0.1.1` was the last point at which this was cheap: exactly two
Field binaries (`local`, `fixture`) existed, and both are already the ones
this change's `cargo tree` and full test suite were re-verified against.
Every connector after this one (`outlook_mail`, `outlook_calendar`,
`outlook_contacts`, `teams`, `jira`) would have shipped carrying the same
coupling, and unwinding it later means re-verifying against a larger set of
Field binaries for the same zero functional benefit today's fix provides.
ADR 0009 already flagged this dependency as one that should not be carried
forward silently a second time; declining to fix it now would be doing
exactly that.

## Consequences

- `crates/fieldnotes-domain/src/property/registry.rs` is the authoritative
  definition of the A1 shared property registry. `fieldnotes-domain` gained
  no new dependency: the registry's only import, `ScalarKind`, was already a
  domain type.
- `crates/fieldnotes-format/src/registry.rs` is now a re-export shim with no
  logic of its own.
- `fieldnotes-field-protocol`'s `Cargo.toml` no longer lists
  `fieldnotes-format`. `cargo tree -p fieldnotes-field-local -e normal` and
  `cargo tree -p fieldnotes-field-fixture -e normal` both confirm
  `fieldnotes-format` is absent from the resolved dependency graph of either
  Field binary.
- `fieldnotes-field-protocol`'s dev-dependency on `fieldnotes-format` in
  `fields/fieldnotes-field-local` and `fields/fieldnotes-field-fixture`
  (used only by their end-to-end `sync` tests, which need
  `CARGO_BIN_EXE_<name>` to live in the package that owns the binary) is
  untouched and out of scope: `-e normal` excludes dev-dependencies, and a
  Field's shipped binary never links test-only code.
- All 432 pre-existing tests pass unchanged in meaning: no assertion was
  edited to make this move work. `cargo clippy --workspace --all-targets --
  -D warnings` is clean, `cargo fmt --all --check` is clean, and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` succeeds.
- [A0](../approvals/A0-repository-scaffold.md) gains an amendment note and
  an updated `fieldnotes-domain` crate-responsibility row naming the
  registry explicitly.
- The pre-existing dependency ADR 0009 flagged and deferred is resolved;
  that entry in ADR 0009 and the corresponding item in
  [A2 implementation findings](../approvals/A2-implementation-findings.md)
  are marked resolved by this ADR.
