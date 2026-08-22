# Artifacts and renditions

**Status:** Original-byte identity/path proposed at A1; rendition details remain
approval-gated

Artifacts let a Note retain useful source material that does not fit safely or
faithfully in Markdown. Examples include an imported voice recording, a PDF,
an image, an email attachment, or a document collected by a Field.

The governing rule is simple: original bytes and derived renditions are
different records with different trust and retention semantics.

## Terms

An **original artifact** is the exact byte sequence imported or collected from
a source. Fieldnotes does not rewrite those bytes in place.

A **rendition** is a derived representation of one original artifact, such as
deterministically extracted text or Markdown. A rendition is disposable and
rebuildable when the same renderer version remains available.

An **artifact reference** is a flat Note property or body link that associates
a Note with an original or rendition. The exact v0.1 reference and manifest
schema is part of the notebook-contract approval gate and is not fixed here.

An inference-generated transcript is an Extraction with audio-time evidence,
not an original and not a deterministic rendition. It belongs to the optional
enhancement layer delivered in `0.1.8`.

## Original byte identity

Original artifacts are content-addressed with SHA-256 over the exact bytes:

```text
sha256:<lowercase-hex>
```

The proposed public artifact ID and flat original path are:

```text
artifact_sha256_<64-lowercase-hex>
artifacts/<artifact-id>.<canonical-extension>
```

No filename, source URL, media type, timestamp, producer, or source metadata is
included in this hash. Two inputs with the same byte hash may share one stored
original even when they have different source names or appear in different
Notes. Their contextual relationships remain distinct.

The store must stream hashing and copy operations, verify the completed digest,
and refuse to replace an existing hash path with different bytes. A hash match
deduplicates original bytes only; it does not deduplicate Notes or prove that
two source objects have the same context.

The ID exposes the byte digest deliberately; it is not a UUID or a contextual
attachment identity. The extension comes from an approved deterministic media-
type registry and is not part of identity. Unknown or conflicting types use
`.bin`. Mutable document/attachment-occurrence metadata, if later required,
must use a separate approved logical record rather than changing byte identity.

## Initial canonical extension registry

Strip media-type parameters and ASCII-lowercase the type/subtype before exact
lookup. The v0.1 A1 mapping is:

| Media type | Extension |
|---|---|
| `application/json` | `.json` |
| `application/pdf` | `.pdf` |
| `application/zip` | `.zip` |
| `audio/mp4` | `.m4a` |
| `audio/mpeg` | `.mp3` |
| `audio/ogg` | `.ogg` |
| `audio/wav` | `.wav` |
| `image/gif` | `.gif` |
| `image/jpeg` | `.jpg` |
| `image/png` | `.png` |
| `image/svg+xml` | `.svg` |
| `image/webp` | `.webp` |
| `text/markdown` | `.md` |
| `text/plain` | `.txt` |
| `video/mp4` | `.mp4` |

An unlisted, unavailable, or conflicting detected media type uses `.bin`.
Source filenames and source-declared types are labels and cannot override this
result. Adding or changing a mapping is a notebook-format compatibility change;
aliases do not create several stored paths for the same artifact ID.

## Import and collection

For user imports and collected attachments, Fieldnotes must:

1. open the input without trusting its filename or declared media type;
2. enforce configured and implementation size limits while streaming;
3. hash and copy the bytes to a same-filesystem temporary file;
4. verify the final hash and byte count;
5. atomically install or reuse the content-addressed original;
6. make the Note durable only after every original it references is durable.

Fieldnotes must not leave the only useful copy at an ephemeral input path.
Source paths, archive members, and connector-supplied filenames are labels,
never trusted notebook paths. Import behavior for symlinks must be explicit and
must prevent a check/use race from substituting a different file.

The source filename may be retained as sanitized display metadata. It is not
used as the storage key. MIME declarations are hints and may be accompanied by
a deterministically detected media type; disagreement is reported rather than
silently hidden.

## Voice Notes

An imported voice Note references the exact original audio bytes and records
an offset-bearing `occurred_at`, media type, and duration where these can be
measured safely. The Note remains playable and attributable with enhancement
disabled.

Direct microphone recording is not required in v0.1. The original artifact
contract supports it later without changing how imported recordings are
represented.

Optional speech-to-text in `0.1.8` creates a separate Extraction. Its transcript
and time ranges never replace the original audio or mutate the voice Note.

## Renditions

A rendition must retain enough flat provenance to answer:

- which original artifact byte hash it came from;
- which renderer contract and version produced it;
- which output media type it claims;
- whether rendering was truncated or damaged;
- any measurable loss, such as lost characters or skipped pages;
- its own exact-byte or versioned normalized-content hash.

Renditions live separately from originals and must never overwrite them. The
precise directory layout and rendition manifest are approval-gated for `0.1.7`.

A deterministic renderer may normalize text encoding, line endings, and safe
link wrappers when the transformation is documented and the original remains
recoverable. If a renderer cannot faithfully process an input, it emits a
visible diagnostic and damage/truncation metadata rather than presenting
corrupted output as complete.

Renderer output must not be treated as more authoritative than the original.
Rendering untrusted files is resource-bounded, and a renderer may reject a
format or feature it cannot handle safely.

## URLs and wrappers

A Field may recover an underlying URL from a vendor safety wrapper only when
the transformation is deterministic. The original wrapped URL remains
available as source evidence or provenance. URL normalization never fetches or
executes the destination as a side effect.

URLs containing credentials, bearer tokens, signed query strings, or other
obvious secrets must be redacted or omitted from notebook metadata and normal
diagnostics. A redacted URL must not be used as though it were the exact source
identity.

## Time metadata

Artifact-related datetime properties follow the notebook contract: RFC 3339
with an explicit numeric offset that represents reliable source-local time or
the configured/client-local offset at that instant. Timezone-less datetimes are
invalid. Any Note filename derived from that instant is rendered in UTC with
`YYYYMMDDTHHMMSSZ`.

Filesystem modification times are not portable evidence and do not substitute
for explicit metadata.

## References and garbage collection

An original can be referenced by many Notes and by multiple renditions.
Removing or updating one Note must not eagerly remove shared bytes.

Artifact garbage collection is a separate operation that:

1. scans all retained public records and archive/prune dependencies;
2. computes the complete set of referenced originals and renditions;
3. reports the proposed removals;
4. removes only objects proven unreferenced under the approved policy.

An authoritative source deletion may remove its current Note without retaining
a tombstone. Its artifacts remain until reference analysis proves they are no
longer used. Refetch may restore the Note and reuse an original that is still
present; if the original was pruned, it is collected again.

The exact retention policy, grace period, reference-manifest shape, and garbage
collection command are approval-gated for `0.1.7`.

## Archive and handback

An archive or handback package that includes a Note must either include every
required original artifact or contain an explicit, valid external reference
allowed by the approved format. Silent broken links are invalid.

Archives preserve original bytes exactly and record checksums in their approved
manifest. Renditions may be included for convenience but never stand in for an
original when policy requires retaining that original. Archive container
format, path layout, compression, encryption, and round-trip rules remain an
`0.1.7` approval gate.

## Security boundary

Artifacts are untrusted data. Fieldnotes does not execute them, enable macros,
or treat embedded scripts as trusted. Parsing and rendering are bounded by
input size, output size, time, nesting/page limits, and decompression limits.

Archive extraction rejects absolute paths, parent traversal, device names,
unsafe links, duplicate path confusion, and decompression bombs. Temporary
files use restrictive permissions and do not expose original bytes through
predictable shared paths.

Deletion is logical filesystem deletion, not a secure-erasure guarantee.
Backups, snapshots, SSD behavior, and copied notebooks may retain prior bytes.

## Release alignment

- **0.1.0:** exact-byte hashing, safe import, playable voice originals,
  content-addressed reuse, and atomic artifact-before-Note persistence.
- **0.1.1:** the local Field proves artifact transfer through the Field process
  contract.
- **0.1.2:** graph and merge treat artifact byte equality separately from Note
  and source-object identity.
- **0.1.3:** Outlook Mail introduces remote attachments and Microsoft transport
  limits.
- **0.1.4:** Calendar and Contacts reuse the same artifact rules where their
  supported source objects carry files or images.
- **0.1.5:** Teams attachments pass the common process and artifact contracts.
- **0.1.6:** Jira attachments and referenced files use the same boundaries.
- **0.1.7:** approved deterministic renderers, damage reporting, garbage
  collection, archive/prune integration, and round-trip fixtures ship.
- **0.1.8:** voice transcripts and other inference outputs remain separate
  Extractions/Observations.
- **0.1.9:** all release targets, large-artifact tests, parser hardening,
  dependency/license review, and release metadata close v0.1.

## Approval gates

Before implementation freezes the public contract, A1 approval is required for
the proposed original ID/path/reference rules. Later renderer/lifecycle gates
still require approval for:

- rendition reference and manifest properties;
- rendition directory/filename layout;
- maximum sizes and per-Field retention policy;
- approved deterministic formats and renderer versions;
- rendition damage metrics and normalized-text fixtures;
- archive inclusion, encryption, container, and round-trip behavior;
- garbage-collection grace period and command UX.
