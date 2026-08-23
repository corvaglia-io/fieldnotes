# A2 implementation findings

**Status:** Findings recorded during the `0.1.1` `sync` implementation, awaiting
coordinator rulings  
**Scope:** Places where the approved [A2 package](A2-field-protocol.md) turned
out to be ambiguous, incomplete, or awkward once core's side of the boundary was
actually built. Nothing here amends A2; each finding names the workaround the
implementation shipped so the behavior is reviewable while the ruling is pending.

A2's compatibility policy requires exactly this: "a connector workstream may not
amend protocol v1 privately. If implementation evidence invalidates a choice
here, it returns to this gate as a recorded finding and a coordinator ruling,
exactly as [IG1 did for A1](A1-implementation-findings.md)."

## Finding 1: a snapshot run must send a scope core has no way to learn

**A2 sections 5 and 10.** A snapshot run's collection request carries a required
`snapshot_scope`, and deletion by absence is refused unless the Field's final
checkpoint claims completeness for **exactly** that scope. But a Field's
`source_scope` *value* is computed by the Field at run time — the `local` Field
derives `local-root:<sha256-of-canonical-root-path>` from its configured root —
and the manifest declares only the scope's `scope_shape`, for review, never its
value. Core is therefore required to name a value it is given no way to obtain
before the run.

The three obvious candidates all fail:

- the manifest's `source_key.scope_shape` is a human-readable shape, and A2 is
  explicit that it exists "for review";
- `describe` has no place to report a computed value, and adding one would be a
  new required manifest member, which A2 makes a `protocol_version` 2 change;
- running an incremental pass first to observe a scope, then a snapshot, makes
  the *first* snapshot of a notebook impossible, which is exactly the recovery
  case a snapshot exists for (a lost or unusable cursor).

**What shipped.** `sync` infers the scope from the notebook: the single distinct
`source_scope` the Field's own active Notes carry. When the Field has no Note in
the notebook, or its Notes span more than one scope, the run is refused with an
actionable message rather than guessing, and a `--scope` flag lets the operator
name it explicitly. This is safe — a scope inferred from Notes can never be
wider than the Notes it was inferred from, so removal still cannot reach beyond
it — but it means a Field's very first run in a notebook cannot be a snapshot
unless the operator already knows the scope value.

**Candidate rulings.** Either (a) accept the inference plus the explicit flag as
the permanent shape; or (b) add an optional `source_key.scope_value` (or a
`scope` echo on the manifest) as an additive `protocol_revision` member, so a
Field can report the scope it will use for the run it was just asked to describe.
Option (b) is a revision rather than a version bump only if the member is
optional and core keeps the inference as its fallback.

## Finding 2: no shipping Field can exercise an authorized tombstone

**A2 section 10 and section 12.** Deletion authority must be declared before it
can be exercised, and A2 requires both authorities to be independently
exercisable. In practice the two split across a boundary `0.1.1` cannot cross:

- the `local` Field declares `deletion.snapshot: authoritative` and
  `deletion.tombstones: unsupported`, because a directory walk is authoritative
  for absence but a removed file is never *reported*;
- the Field that declares `deletion.tombstones: authoritative` is
  `outlook_mail`, whose manifest also declares
  `auth.credential_profile_required: true` and
  `auth.protected_channel_required: true` — and the protected channel does not
  exist until the `0.1.3` authentication gate, so `sync` refuses to start it.

So at `0.1.1` there is no Field a sync will run that can emit an authorized
tombstone at all, and the release gate nevertheless requires the evidence.

**What shipped.** The fixture Field gained one manifest flavor,
`local_with_tombstone_authority`, and two scenarios (`tombstone-local`,
`tombstone-local-unauthorized`): same registered stem, same declared properties,
same cursor format version, no credential, and both deletion authorities
declared. That closes the evidence hole without touching the protocol or the
shipping Fields' manifests. It is fixture-only and illustrative, exactly as the
fixture's other capability slices are.

**Candidate ruling.** Confirm that fixture-only coverage satisfies R1 for the
tombstone path and that the end-to-end authorized-tombstone-against-a-live-Field
evidence is inherited by `0.1.3` alongside the credential canary, which A2
section 12 already defers there.

## Finding 3: `attachment_ref` collides with `source_identity` for a Field whose objects *are* files

ADR 0007 defines `attachment_ref` as "a stable connector-namespaced upstream
attachment reference, following the same object-kind-namespace convention
`source_identity` uses". For a mail Field the two are plainly different things
(`mail-message/…` versus `mail-attachment/…`). For the `local` Field, the object
and its only attachment are the same file, so the Field emits
`file/projects/rollout/readme.md` as *both* the source identity and the
attachment reference. The resulting Note carries the same string in
`source_identity` and in `skipped_attachments`.

That is not wrong — the reference is stable, namespaced, and it is what a
re-collection pass would need — but it reads oddly, and a reader cannot tell from
the value alone whether a `skipped_attachments` member names a distinct
attachment or the object itself.

**What shipped.** Nothing: the value is carried through as the Field emitted it,
because A2 says core "does not otherwise interpret `attachment_ref`".

**Candidate ruling.** Either accept the collision as a property of Fields whose
objects are their own attachments, or ask such a Field to namespace its
attachment references distinctly (`file-attachment/…`) at its own release gate.
This is a per-Field manifest question, not a protocol one.

## Finding 4: the body's attachment link has no owner in the record envelope

**ADR 0007 ruling 2** requires the Markdown body's attachment link to target the
derived relative artifact path when bytes are retained and the source location
when they are not. Neither party can do this alone: the Field writes `body.text`
but never learns the local artifact path, which is derived from **core's** digest
and A1's extension registry; core owns the path but the body is the Field's
deterministic source evidence.

**What shipped.** Core appends one deterministic "Artifacts:" section to the
Field's body evidence, listing each retained artifact with its derived
notebook-relative path and each declined one with its source location (the
Field's `source.url` when it supplied one, otherwise the portable exact-source
key). The Field's own body text is otherwise untouched, and the appended section
is part of the `fn-content-v1` body hash like any other body byte.

**Candidate ruling.** Confirm that core composing this section is the intended
division of labor, and whether the section's exact wording should be frozen as a
fixture (it is currently an implementation detail that a reviewer can read but
no golden file pins).

## Finding 5: `source_url` cannot be present "either way" for a Field that has no URL

ADR 0007 ruling 2 says `source_url` "remains present in frontmatter in **both**
cases, so provenance is never lost regardless of the retention outcome". But
`source.url` is optional on the record envelope, and the `local` Field supplies
none — a local file has no URL, and A1 section 2 forbids deriving one from a
filename.

**What shipped.** `source_url` is projected when, and only when, the Field
supplied one. When it did not, the declined artifact's body link names the
portable exact-source key instead, which is the only source location core was
ever given.

**Candidate ruling.** Read ADR 0007 ruling 2 as conditional on the Field
supplying a URL ("`source_url` is not *removed* because bytes were skipped")
rather than as a requirement that every Note carry one. The implementation
assumes this reading.

## Finding 6: `covers_record_seq_through: 0` reports as "covering records through seq 0"

A2 section 9 makes `covers_record_seq_through: 0` legal and meaningful: the
cursor advances with no record in between, which a Field emits once it has proven
a page contained nothing new. The `local` Field does this on every no-op
incremental run. Core commits it correctly, but the number is meaningless as a
*report*, and status output that renders it literally reads as though a record at
sequence zero existed.

**What shipped.** The value is reported as-is, because inventing a second
representation for "no records" would put a display concern into the durable
cursor file.

**Candidate ruling.** A CLI presentation decision, not a protocol one; noted here
only because it surfaced from A2's own edge case.

## Non-findings worth recording

These were checked against implementation and needed no change:

- **Checkpoint eligibility.** A2 section 9's warning about a naive
  contiguous-`seq` watermark is real and load-bearing. The `local` Field emits an
  `info` diagnostic for a skipped symlink *before* any record, and the fixture's
  `resume` scenario does the same; a watermark over raw `seq` values would have
  silently stopped committing forever. Tracking durability per accepted record,
  as the protocol crate already does, is correct.
- **The two-layer artifact validity model.** Resolving an artifact through the
  protocol crate and then reading the staged bytes again to install them means
  core opens the staged file twice. The second read is bounded by the same
  declared length and its digest is compared against the digest the verified read
  produced, so a file swapped between the two reads is rejected as
  `artifact.digest_mismatch` rather than stored under an identity that does not
  describe its bytes. The cost is one extra bounded read per artifact.
- **Cancellation.** `sync` never writes a `cancel` frame, because nothing in the
  `0.1.1` command surface cancels a run. The protocol crate and its conformance
  kit already cover the cooperative-cancellation path; core's side of it has no
  caller yet.
- **Per-Field run locks.** `docs/operations.md` calls for a notebook writer lock
  and a per-Field run lock, and explicitly gates the locking primitive, timeout,
  owner metadata, and force-unlock behavior behind a separate approval. `sync`
  therefore takes no lock, and two concurrent syncs of one Field could race one
  cursor. That remains the documented, approval-gated gap it already was.
