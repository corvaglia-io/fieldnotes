# Proposed A2 Field protocol corpus, version 1

**Status:** Approved at A2 on 2026-08-23; frozen as the implementation target.
The user approved the
[A2 package](../../../../docs/approvals/A2-field-protocol.md), so these bytes
are now frozen rather than review material.
**Implementation:** None yet. IG2 adds the executable conformance suite,
the fixture Field, and the DTOs that round-trip these schemas. The
verification that was possible before an executable suite existed is
described under "How this corpus was checked" below.

This corpus makes the A2 recommendation reviewable as bytes rather than prose:
exact JSON Schemas for every message that crosses the Field process boundary,
and worked transcripts for the cases the boundary exists to get right.

## What A2 does and does not define

A2 defines the **transport and process boundary only**. A1 froze the
byte-visible notebook contract — record types, property names, ID grammars,
datetime serialization, filenames, hashes, and canonical bytes — and A2 may not
redefine any of it.

Where a value in this corpus also appears in a notebook record, the schema
carries a **well-formedness guard**, not a definition. Guards exist so an
untrusted child process cannot make core do work on obvious nonsense. The
authoritative rule stays in A1 and core still performs full A1 validation after
the guard passes. Two deliberate consequences:

- `noteType` constrains the 32-byte primary-type grammar but **does not
  enumerate** A1's eleven approved primary Note types. Core validates the value
  against the A1 registry. A protocol schema that listed them would be a second
  copy of A1 vocabulary that could drift.
- The record schema's `properties` map excludes every core-owned and hoisted
  A1 property name by name-grammar rather than approving a property vocabulary
  of its own. It cannot state which unprefixed names are legal, because that is
  A1's closed shared registry.

## Schemas

JSON Schema dialect `2020-12` throughout. Each file declares `$schema` and
`$id`, sets `additionalProperties: false` wherever the contract is closed, and
makes required-versus-optional explicit.

`$id` values use the reserved-by-RFC-2606 host `schemas.fieldnotes.invalid`, so
a `$id` is unambiguously an identifier and can never be mistaken for a URL a
validator should fetch. Cross-file `$ref` values are relative filenames and
resolve entirely within this directory; no schema resolution touches the
network.

| File | Message | Direction |
|---|---|---|
| `schemas/common.schema.json` | shared `$defs`: grammars, scalars, source key, identity anchor, limits, deadline, closed diagnostic vocabulary | — |
| `schemas/describe-request.schema.json` | `describe_request` | core to Field |
| `schemas/describe-manifest.schema.json` | `manifest` | Field to core |
| `schemas/collect-request.schema.json` | `collect_request` | core to Field |
| `schemas/cancel-control.schema.json` | `cancel` | core to Field |
| `schemas/record-event.schema.json` | `record`, both `upsert` and `delete` | Field to core |
| `schemas/checkpoint-event.schema.json` | `checkpoint` | Field to core |
| `schemas/diagnostic-event.schema.json` | `diagnostic` | Field to core |
| `schemas/credential-channel.schema.json` | `credential_request`, `credential_response` | both, on the protected channel only |
| `schemas/core-frame.schema.json` | union of every core-to-Field frame | core to Field |
| `schemas/field-event.schema.json` | union of every Field-to-core standard-output frame | Field to core |
| `schemas/transcript.schema.json` | the fixture format of this directory's transcripts | not a wire message |

The five message types the roadmap names for A2 are the manifest, the
collection request, and the `record`, `checkpoint`, and `diagnostic` events.
The other schemas are not additions to that scope: the describe request, the
cancel frame, and the credential channel are the exact mechanisms A2's required
scope calls for in "protocol-version negotiation", "exit codes and
partial-failure behavior", and "credential references and protected secret
delivery", and the two unions and the transcript schema are review and
conformance machinery rather than new protocol surface.

## Transcripts

Newline-delimited JSON, one JSON object per line, UTF-8, LF, exactly one final
LF. **The transcript file format is a fixture format, not the wire format.** A
single file has to show both directions, two channels, core's own observable
behavior, and input that is deliberately not valid protocol — none of which a
raw wire capture can carry. So every line is a tagged wrapper:

- the **first line** is a `header` stating which case the transcript
  demonstrates, its operation and mode, and the outcome and exit code expected;
- a `frame` line carries one wire frame plus its `direction`, its `channel`,
  whether it is expected to validate (`valid`), and, when it must be rejected,
  the code core should reject it with (`expect_reject`);
- a `raw` line carries bytes that are not valid protocol, either literally
  (`bytes_utf8`) or described (`bytes_description`) when they cannot be embedded
  in a JSON string at all, such as invalid UTF-8 or an oversized frame;
- a `core` line records an observable core action — what it committed, wrote,
  removed, reused, refused, or reported;
- an `exit` line records the process exit code and the resulting run outcome.

A `frame` line's payload is validated against the wire schema its `type`
selects. A line marked `valid: false` is asserted **not** to validate, so the
negative cases are checked as negatives rather than merely asserted in prose.

`expect_reject` also distinguishes the two kinds of rejection that matter here.
Some frames are invalid in isolation and a schema catches them. Others are
perfectly well-formed frames that violate a rule no single-frame schema can
express — sequence ordering, declared deletion authority, registry membership,
declared property typing, or what is actually on the filesystem. Both must
fail; only the first is a schema matter.

### The two-layer validity model

`valid` in every `frame` line means **wire-schema validity only**: would a
validator checking this frame's `type` against its corresponding schema in
`schemas/` accept its shape. `expect_reject` names the code from whichever
pipeline stage actually rejects the frame — the wire schema itself, or a later
stage that a single frame's shape cannot express an opinion about at all.

The artifact handle is the case worth being explicit about, because it is easy
to implement in a way that produces the wrong code. `record-event.schema.json`
carries the handle-character-set pattern on `artifacts[].handle` as a
well-formedness guard, so a validator checking the wire schema alone correctly
reports a traversal string as not matching it. **The reference implementation's
own data-transfer object nonetheless carries `handle` as an unvalidated
string**, and applies the grammar itself, by hand, as a distinct
artifact-validation step that runs before any filesystem call. This is not
carelessness: a DTO field typed to enforce the grammar at deserialization would
fail a hostile handle at decode time with a generic `protocol.schema_invalid`
— "the frame does not validate" — when the code this package actually
specifies, and the one the transcripts pin, is `artifact.invalid_handle` from
the purpose-built later step. **An implementer translating these schemas into
a strongly-typed DTO in any language must not collapse the wire-schema guard
into the type used for decoding**, or a hostile handle silently produces the
wrong rejection code. Transcript 11 pins `artifact.invalid_handle` for a
grammar failure and `artifact.not_regular_file` for a *grammatically valid*
handle whose staged entry turned out to be a symlink — two different pipeline
stages, two different codes, and only the grammar failure is something the
wire schema itself has an opinion about.

| Transcript | Case demonstrated | Expected outcome |
|---|---|---|
| `01-successful-incremental-collection.ndjson` | describe, negotiation, two upserts, one staged artifact, one committed checkpoint | complete, exit 0 |
| `02-resumption-from-cursor.ndjson` | resumption from the cursor committed by transcript 01 | complete, exit 0 |
| `03-duplicate-record-replay.ndjson` | a lagging cursor replays an object, and one run emits it twice; both are no-ops | complete, exit 0 |
| `04-authoritative-deletion-tombstone.ndjson` | explicit authoritative deletion under declared tombstone authority | complete, exit 0 |
| `05-partial-result-is-not-deletion.ndjson` | a snapshot run that stops part-way; nothing is removed | partial, exit 6 |
| `06-diagnostic-with-redaction.ndjson` | credential delivery on the protected channel, then a redacted authentication failure | partial, exit 4 |
| `07-version-negotiation-failure.ndjson` | negotiation fails closed before any credential exists, in both directions | failed, exit 3 |
| `08-malformed-output-rejection.ndjson` | non-JSON, unknown event, sequence regression, invalid UTF-8, oversized frame, truncated frame | failed, exit 10 |
| `09-snapshot-complete-authorizes-absence.ndjson` | the one path by which absence removes a Note | complete, exit 0 |
| `10-artifact-transfer-and-dedup.ndjson` | staged original bytes installed, a digest-only reference reused, and an oversize attachment declined and left at its source | complete, exit 0 |
| `11-hostile-artifact-references.ndjson` | traversal, absolute path, reserved device name, symlink escape (rejected as `artifact.not_regular_file`, distinct from a handle-grammar failure), digest mismatch, unknown digest | failed, exit 10 |
| `12-crash-before-checkpoint.ndjson` | durable write, commit, durable write, crash, resume, idempotent replay | failed then complete, exit 137 then 0 |
| `13-undeclared-prefixed-property-rejection.ndjson` | ruling 4 enforcement: undeclared, foreign-prefixed, unknown unprefixed, mistyped, and core-owned properties | failed, exit 10 |
| `14-cancellation-and-deadline.ndjson` | cooperative cancellation of a snapshot run; partial, so nothing is removed | partial, exit 8 |
| `15-attachment-retention-policy.ndjson` | the media-type retention policy (ADR 0007): a well-behaved decline carrying `attachment_ref` alongside a retained attachment on the same record, then a hostile Field staging an excluded type anyway (`artifact.type_excluded`, distinct from `artifact.oversized`) | failed, exit 10 |
| `16-explicit-recollection.ndjson` | the explicit re-collection request shape (ADR 0007): `recollect_targets` names a known source object with no cursor and no window, and the Field re-emits it with a previously declined attachment now staged under a widened policy | complete, exit 0 |

## Reused approved material

The transcripts deliberately reuse values that are already frozen, so a
reviewer can trace them rather than take them on trust:

- the artifact digests `449d6bf4…aab17` and `cf741b83…b36f` and the 40-byte
  length are the A1 vectors in
  [`tests/fixtures/hashes/proposed-v1/`](../../hashes/proposed-v1/README.md);
- the mail record's source scope, source identity, participants, subject,
  conversation and thread values, `outlook_mail_importance`, and
  `outlook_mail_internet_message_id` are those of the frozen Note
  `20260822T080000Z_outlook_mail_work_mail_note_01a0287d-acc0-7000-8000-000000000005.md`;
- the local records reuse `local_relative_path`, `local_media_type`,
  `local_document_date`, and `local_document_flag` exactly as the frozen local
  and document Notes spell them, including `local_document_flag: "true"` as
  text rather than a boolean.

Every credential value in the transcripts is a marked fixture canary of the
form `FIXTURE-NOT-A-REAL-TOKEN-canary-…`. No real credential, account, tenant,
or vendor payload appears here.

## Gate classification

| Corpus area | A2 approval meaning if approved | Later gate work |
|---|---|---|
| `schemas/common.schema.json` grammars, limits, deadline, closed diagnostic vocabulary | Normative for protocol v1 transport well-formedness and for the frozen limit ceilings — absolute bounds no configuration may cross | Configuring within a ceiling is `sync`-command scope, including the single-artifact bound's and the run wall clock's configurable defaults; raising a ceiling or adding a diagnostic code is an additive protocol revision |
| `schemas/describe-manifest.schema.json` envelope, `declared_properties`, `source_key`, `identity_anchors`, `auth`, `collection` | Normative for the manifest envelope and for prefixed-property declaration and enforcement, closing the gap [ruling 4](../../../../docs/decisions/0006-a1-implementation-rulings.md) assigned to A2 | Each Field's actual declared property names, capability slices, and scopes are approved with that Field's release: `local_` at `0.1.1`, `outlook_mail_` at `0.1.3` |
| `schemas/collect-request.schema.json`, `cancel-control.schema.json` | Normative for the request envelope, mode, window, snapshot scope, staging directory, cooperative cancellation, the required `artifact_media_types` retention policy, and the `recollect_targets` re-collection request shape (ADR 0007) | The CLI's own exit-code table and multi-Field summary remain a CLI-contract decision; the re-collection *operation* that issues `recollect_targets` is `0.1.1` sync-command scope and is not built by A2 |
| `schemas/record-event.schema.json` | Normative for the normalized-source-envelope boundary, the upsert/delete split, deletion authority, the three artifact reference kinds including `not_retained`, the retention threshold's and the media-type policy's protocol-level effect, `attachment_ref`, the Note-applicable registry subset, and the core-owned property exclusion | Property names inside `properties` remain governed by the A1 registry; the "attachment was seen but is not retained" name this row previously left as a possible future gap is now `skipped_attachments`, approved by A1 amendment via [ADR 0007](../../../../docs/decisions/0007-attachment-retention-policy.md) |
| `schemas/checkpoint-event.schema.json` | Normative for cursor commit eligibility, coverage accounting, and the snapshot completeness claim | Retry classification and backoff policy are implementation policy inside these bounds |
| `schemas/diagnostic-event.schema.json` | Normative for the diagnostic envelope, the closed code vocabulary, severity semantics, and the redaction obligation | `0.1.5` Teams and `0.1.6` Jira may need additional codes, which is an additive revision |
| `schemas/credential-channel.schema.json` | Normative for the shape of protected delivery and for the rule that `material` is the only secret-bearing member in the protocol | The exact per-platform channel mechanism, refresh ownership, and memory-clearing behavior close at the `0.1.3` authentication gate |
| `schemas/core-frame.schema.json`, `field-event.schema.json` | Normative as the fail-closed union: a frame matching no branch is a failed run | — |
| `schemas/transcript.schema.json` and the transcripts | Normative as the executable target of the `0.1.1` conformance kit: each transcript is one required case | IG2 turns each into an executing case with a fixture Field, crash injection, and secret canaries, and adds vendor fixtures per Field |
| Connector-specific values inside transcripts: driver versions, folder paths, scopes, cursor encodings, `outlook_mail_folder_path`, `outlook_mail_has_attachments`, `outlook_mail_categories`, `local_tags` | Illustrative only. They show the shape the schemas require | Each becomes normative when its Field's release gate approves its manifest and fixtures |

## How this corpus was checked

No Rust conformance suite exists yet, by design: A2 is what unblocks writing
one. What was verified, and how:

1. every schema file parses as JSON, is UTF-8 and LF, and ends with exactly one
   final LF;
2. every schema validates against the JSON Schema 2020-12 meta-schema, and
   declares the 2020-12 dialect and an `$id`;
3. every `$ref` in every schema resolves inside a registry built only from
   these twelve files, proving the set is self-contained and needs no network;
4. targeted accept-and-reject probes confirm the guards do what they claim,
   including that `-00:00` and a `Z` datetime are rejected while `+00:00` is
   accepted, that `../etc/passwd`, `/Users/joe/.ssh/id_ed25519`, `sub/dir`,
   `a.b`, `nul`, and `com3` are all rejected as artifact handles, that a mixed
   list is rejected, and that a declared list property without
   `list_semantics` and a declared scalar property with it are both rejected;
5. every transcript line parses as JSON and validates against
   `transcript.schema.json`;
6. every `frame` line's payload validates against the wire schema its `type`
   selects, and additionally against the direction's union schema; every line
   marked `valid: false` is asserted to fail that schema;
7. transcript-internal consistency: exactly one header and it is first, one
   exit outcome matching the header, per-run sequence numbers starting at 1 and
   incrementing by exactly 1, checkpoint coverage strictly below its own
   sequence number and never regressing, `records_covered` equal to the record
   frames actually present in the covered range, and at most one final
   checkpoint per run and it last.

The check ran with `python3` and `jsonschema` 4.26.0 in a throwaway virtual
environment outside the repository: 75 assertions, 0 failures. The generator
and checker are scratch tooling and are deliberately not committed; IG2 replaces
them with the Rust conformance kit, which is the real evidence.

### Re-verification after the implementation-finding pass

The reference implementation named in the A2 package (`fieldnotes-field-protocol`
and `fields/fieldnotes-field-fixture`) has since been built and, in the course
of building it, surfaced the defects and ambiguities this pass corrects — new
rejection codes, the checkpoint-eligibility clarification, the two-layer
validity model, the `not_retained` artifact kind, and the rest recorded
throughout the A2 package above. Every schema and transcript byte this pass
touched was re-verified the same way, again with `python3` and `jsonschema`
4.26.0 in a throwaway virtual environment outside the repository: every schema
still parses, is UTF-8, and ends with exactly one final LF; every schema still
validates against the 2020-12 meta-schema; every `$ref` still resolves inside
this twelve-file set; every transcript line still parses and validates against
`transcript.schema.json`; every `frame` line's payload still validates against
the wire schema its `type` selects, with every `valid: false` line still
failing that schema exactly as before. This second pass also re-ran targeted
probes for every grammar this revision touched: the artifact handle grammar
now also refuses `com0` and `lpt0`; the cursor grammar now refuses an embedded
LF or TAB, not only NUL; `describe_request` validates without a `limits`
member; a manifest's `property_prefix` validates when present and fails
schema validation when explicitly `null`; and an `artifactRef` of kind
`not_retained` validates with a `byte_length` far past the 512 MiB transfer
ceiling, which applies only to `staged`. This is still scratch tooling,
deliberately not committed, and still not a substitute for the Rust
conformance kit, which by this point exists and is the real evidence — this
second check exists only to keep the schemas and transcripts honest while they
were being hand-edited alongside the code.

### Re-verification after the attachment-retention-policy pass

[ADR 0007](../../../../docs/decisions/0007-attachment-retention-policy.md)
added a required `artifact_media_types` member to `collect_request` (every
existing transcript's `collect_request` frame gained it), an optional
`recollect_targets` member and its `recollectTarget` shape, an optional
`attachment_ref` member on `artifactRef` (required exactly for
`not_retained`, forbidden otherwise), the `mediaTypeMatcher` and
`attachmentRef` shared definitions, and two new transcripts,
`15-attachment-retention-policy.ndjson` and
`16-explicit-recollection.ndjson`. This pass re-verified every schema and
transcript the same way as before, again with `python3` and `jsonschema`
4.26.0 in a throwaway virtual environment outside the repository: every
schema still parses, is UTF-8, and ends with exactly one final LF; every
schema still validates against the 2020-12 meta-schema; every `$ref` still
resolves inside this set; every transcript line still parses and validates
against `transcript.schema.json`; every `frame` line's payload still
validates against the wire schema its `type` selects, with every
`valid: false` line still failing that schema; and the two new transcripts'
internal sequence, checkpoint-coverage, and header/exit consistency hold.
Targeted probes additionally confirmed that `image/*` and `application/pdf`
are accepted as `mediaTypeMatcher` values while `image/`, `Image/*`, and a
bare `image` are rejected, that a `not_retained` reference without
`attachment_ref` is rejected while one with it validates, that a `staged` or
`digest_only` reference carrying `attachment_ref` is rejected, and that
`recollect_targets` together with `cursor`, together with `window`, or under
`mode: "snapshot"` is rejected in each case. This is still scratch tooling,
deliberately not committed, and still not a substitute for the Rust
conformance kit.
