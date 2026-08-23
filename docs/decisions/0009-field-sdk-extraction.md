# ADR 0009: A Field-authoring SDK, extracted from two working Fields

- **Status:** Accepted on 2026-08-23
- **Date:** 2026-08-23

## Context

The roadmap's `0.1.1` entry calls for "a reusable Field SDK crate," but A0's
scaffold deliberately did not create one: A0's crate table folded "host/SDK
support" into `fieldnotes-field-protocol`'s stated responsibility, and the
actual shape of a Field-authoring SDK was left open rather than designed
before any real Field existed to design it against. The plan all along was to
build a real Field first, let it report what it had to invent locally because
no shared helper existed, and extract the SDK from that report rather than
from speculation.

Two Fields now exist against the approved A2 protocol: `local`, a real
read-only connector for a configured directory, and `fixture`, a deliberately
misbehaving conformance counterparty used to drive `sync`'s 39 process-
boundary tests. Building `local` first, and holding `fixture` up as a second,
adversarial consumer of the same abstraction, is what this ADR's extraction
is checked against: **a helper that does not fit both a well-behaved Field
and a hostile one is the wrong helper**, and this review found one place
where that distinction mattered (see "What changed shape" below).

`local`'s own source carried five things it had to write itself because no
shared crate existed:

1. Percent-encoding/decoding for embedding path-shaped strings (the cursor's
   tie-break relative paths) inside a bounded, delimiter-bearing cursor
   token, byte-safe against splitting multi-byte UTF-8.
2. A "stage bytes while hashing them once" routine: read once, SHA-256 once,
   write to the run's staging directory under a handle, return the hex
   digest — plus the `a<seq>`-style handle-naming convention it used.
3. A byte-bounded, UTF-8-boundary-safe text truncation helper, for fitting
   body text and diagnostic messages under their declared byte bounds
   without panicking or splitting a character, returning how many
   *characters* (not bytes) were lost for the `Integrity.lost_characters`
   member.
4. A frame emitter that self-polices against the run's own declared limits
   (diagnostics, records, cursor size) and stops writing after the first
   frame-write failure.
5. A "hash a local identifier to a stable, non-secret scope" pattern
   (`local-root:<sha256-of-canonical-root-path>`), so a portable scope never
   embeds anything user-identifying.

## Decision

### Create `fieldnotes-field-sdk`

A new workspace crate, `crates/fieldnotes-field-sdk`, added to the workspace
`members` and to `[workspace.dependencies]`. It depends on
`fieldnotes-field-protocol` for the wire contract and, directly, on nothing
else in this workspace. It contains **no vendor logic**: nothing in it knows
about a filesystem, a mailbox, a calendar, or any other specific source.

**Dependency rule, restated for this crate specifically:** it must never
depend on `fieldnotes-format`, `fieldnotes-store`, or `fieldnotes-app`. A
Field does no notebook byte work — that is the entire point of A2's
normalized-envelope decision (`docs/approvals/A2-field-protocol.md` section
6) — and an SDK that every Field binary links against is exactly the wrong
place to reintroduce the canonical serializer by convenience. `Cargo.lock`
and `cargo tree -p fieldnotes-field-sdk` confirm the crate's own dependency
list holds to this. (`fieldnotes-field-protocol` itself already depends on
`fieldnotes-format` for the A1 shared property registry — a pre-existing,
separately justified dependency this ADR does not revisit — so it remains
transitively present in any Field binary via the protocol crate regardless
of the SDK's existence; the rule above binds what the SDK crate itself may
add on top of that.)

> **Resolved by [ADR 0010](0010-property-registry-relocation.md).** The
> parenthetical above flagged exactly the coupling that ADR 0010 later
> closed: the registry moved to `fieldnotes-domain`, which has no byte form
> of its own, and `fieldnotes-field-protocol` no longer depends on
> `fieldnotes-format` at all. `cargo tree -p fieldnotes-field-local -e
> normal` and `cargo tree -p fieldnotes-field-fixture -e normal` both now
> show `fieldnotes-format` entirely absent, not merely unused by the SDK.

### What it contains

Five modules, one per extracted helper, plus one for scaffolding that
generalizes across both Fields' `main`:

- `percent`: `encode`/`decode`, the byte-safe percent-encoding from finding 1,
  generalized by taking any `&str` rather than assuming a relative path
  specifically — any Field with a path- or identifier-list-shaped cursor
  fragment can reuse it.
- `stage`: `stage_and_hash` (finding 2's combined read-once/hash/write), plus
  `sha256_hex` (hash alone, for bytes that end up not staged — see "what
  changed shape" below) and `handle_for_seq` (the naming convention).
- `truncate`: `truncate_utf8`, finding 3, unchanged in shape from `local`'s
  version — it was already fully generic.
- `scope`: `derive(prefix, identifier)`, finding 5, generalized by taking the
  prefix and the identifier bytes as parameters instead of hard-coding
  `local`'s `"local-root"` prefix and path-shaped identifier.
- `emit`: `Emitter<W>`, finding 4, generalized to self-police against
  records and cursor size in addition to diagnostics (see below), plus
  `checked_json`/`raw_json`/`raw_bytes` escape hatches `fixture` needs and
  `local` does not.
- `dispatch`: not one of the five findings, but scaffolding that fell out of
  comparing `local`'s and `fixture`'s `main` functions side by side and
  finding them nearly identical: argv-parsing into the one v1 operation
  token, and reading/matching the one request frame each operation expects,
  down to matching error wording in all but one message (see below).

### What changed shape, and why

**The frame emitter grew two self-policing axes `local`'s own version did
not have**, because the report was "what `local` needed," not "the complete
list of what any Field needs." `local`'s own `Emitter` only counted
diagnostics; its record-count ceiling was enforced by a loop-level check in
`collect.rs`, and its cursor-size ceiling was enforced by its own
`encode_within_limit` cursor-shrinking logic, called before the checkpoint
ever reached the emitter. Both remain in `local` as the smarter, Field-owned
policy for *what to do* when a bound is close (stop iterating before staging
files that won't be referenced; widen a cursor's tie-break set to a flag).
What moved to the SDK is the backstop: the emitter itself now also refuses to
exceed `max_run_records` or write a checkpoint whose cursor exceeds
`max_cursor_bytes`, so a Field that forgets its own smarter policy fails
closed rather than emitting a limit-violating frame. This is additive and
harmless for `local` (its own checks already prevent the backstop from ever
firing) and is exercised directly by the SDK's own unit tests.

**The staging helper split into two functions**, `sha256_hex` and
`stage_and_hash`, where `local`'s own code had one hash computation shared by
two branches (stage the bytes, or decline them under retention policy while
still declaring their digest as a detection aid). A single `stage_and_hash`
that always writes would force the declined-artifact branch to write bytes
core must never receive just to obtain their digest. Splitting the hash out
on its own keeps both branches at exactly one hash computation, matching the
original's efficiency, and makes the "read once, hash once" property of the
helper's name literally true in both call sites rather than true only in the
one `local` happened to write first.

**`Emitter::last_write_error` was added**, not present in `local`'s original
`Emitter` at all, because collapsing the write path into a boolean return
value (needed for `fixture`'s self-policing dispatch through
`checked_json`) would otherwise have discarded the `FrameError` detail
`describe.rs` previously logged on a failed manifest write. This is a case
where fitting the abstraction to a second consumer (`fixture`, whose
`checked_json`-based dispatch needs a uniform `bool`) would have quietly
regressed the first consumer's diagnostics if left unaddressed. `local`'s
`collect.rs` uses the same accessor to preserve its original "report the
error the first time, and only the first time, a frame write fails" behavior
on top of the shared emitter's own count-based bookkeeping.

**`dispatch`'s functions return a raw `u8`, not a `std::process::ExitCode`**,
for the same reason. `local`'s `main` wraps an exit code exactly once, at the
point of return, so an opaque `ExitCode` would have been enough for it alone.
`fixture` cannot use an opaque type there: its `FIELDNOTES_FIXTURE_EXIT_CODE`
environment variable overrides whatever code a scenario would otherwise
produce, which means the code has to survive as an inspectable value inside
`ScenarioOutcome` until the very end of `fixture`'s `main`. A raw `u8` serves
both — wrapped immediately by `local`, threaded through `fixture`'s override
logic — and an opaque `ExitCode` would have served only `local`.

### What was judged too local-specific and left alone

`local`'s cursor *format* itself (`local-walk/v1;hw=...;at=...`), its
`CursorState` advance/widen logic, its walk, its classification and
media-type sniffing, and its manifest construction all stay in `local`.
None of the five findings asked for these, and none of them are Field-agnostic
protocol bookkeeping — they are exactly the vendor-mapping decisions A2
section 6 assigns to a Field, not to shared infrastructure. `local`'s
`encode_within_limit` (its own cursor-shrinking strategy) was considered for
extraction alongside the emitter and rejected: *how* to shrink a cursor is
inseparable from that cursor's own format, which is Field-owned by
definition; only the backstop check that a cursor fits at all generalizes,
and that backstop now lives in `Emitter::checkpoint`.

### `fixture` proves the abstraction fits a hostile Field too

`fixture` now uses the same `Emitter`, `stage::stage_and_hash`, and
`dispatch` functions as `local`, with no helper altered to make this
possible. Its need to emit malformed, oversized, or hostile frames on
purpose is served by `Emitter::raw_json`/`raw_bytes` — unchecked escape
hatches that bypass every self-policing rule the same type otherwise
enforces — and by `Emitter::checked_json` for the well-formed scenarios,
which decodes a JSON value through the protocol's own types before writing
it, exactly replacing `fixture`'s previous bespoke `frame()` method. Nothing
about the emitter's design prevents misbehavior; it only makes good behavior
convenient.

## Consequences

- `crates/fieldnotes-field-sdk` is a new workspace member, added to
  `Cargo.toml`'s `members` and `[workspace.dependencies]`, with 41 unit
  tests of its own covering every extracted helper, including the
  percent-encoding round trip for multi-byte UTF-8 and characters needing
  escape, and the truncation boundary case landing exactly mid-character.
- `fields/fieldnotes-field-local` loses `src/hexutil.rs` entirely and sheds
  131 net lines across `main.rs`, `describe.rs`, `collect.rs`, `cursor.rs`,
  `record.rs`, and `scope.rs`, gaining a dependency on `fieldnotes-field-sdk`
  and losing its now-redundant direct dependency on `sha2`.
- `fields/fieldnotes-field-fixture` sheds 101 net lines from `main.rs` and
  `scenarios.rs`, losing its bespoke `Emitter` type entirely in favor of the
  shared one.
- Every pre-existing test in both Fields' suites passes unchanged in
  meaning: no assertion was edited to make this refactor work. That includes
  `fixture`'s full 68-test suite across `declared_properties.rs`,
  `roadmap_evidence.rs`, and `sync_durability.rs`, most of which spawn
  `fieldnotes-field-fixture` as a real child process over real pipes, and
  `local`'s full 47-test suite. The full workspace test suite (432 tests
  across every crate) passes, `cargo clippy --workspace --all-targets -- -D
  warnings` is clean, and `RUSTDOCFLAGS="-D warnings" cargo doc --workspace
  --no-deps` succeeds.
- `docs/approvals/A0-repository-scaffold.md` gains an amendment note and a
  new crate-responsibility row for `fieldnotes-field-sdk`, and narrows
  `fieldnotes-field-protocol`'s row to drop the "host/SDK support" phrase
  A0 originally used for what this crate now owns.
