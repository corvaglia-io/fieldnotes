# ADR 0008: Extend the canonical extension registry for the default retention set

- **Status:** Accepted on 2026-08-23
- **Date:** 2026-08-23

## Context

[ADR 0007](0007-attachment-retention-policy.md) approved a default,
per-run-configurable media-type retention include set: twenty media types
across documents/text, images, and audio, chosen because a notebook is
disposable working material that should keep what is useful for work and
context. That ADR also cross-checked the twenty types against A1's frozen
canonical media-type-to-extension registry
(`crates/fieldnotes-format/src/extension.rs`, mirrored in
`docs/artifacts.md`) and found that **nine had no entry**:

```text
application/vnd.openxmlformats-officedocument.wordprocessingml.document (.docx)
application/vnd.openxmlformats-officedocument.spreadsheetml.sheet (.xlsx)
application/vnd.openxmlformats-officedocument.presentationml.presentation (.pptx)
application/vnd.oasis.opendocument.text (.odt)
application/vnd.oasis.opendocument.spreadsheet (.ods)
application/vnd.oasis.opendocument.presentation (.odp)
text/csv (.csv)
application/rtf (.rtf)
image/heic (.heic)
```

ADR 0007 deliberately left this gap open rather than closing it, on the
grounds that extending A1's frozen registry is itself a notebook-format
compatibility change requiring its own registry review (`docs/artifacts.md`
says so explicitly) — separate from the retention question ADR 0007 was
answering.

That review is this ADR. The gap is not cosmetic. A user who deliberately
retains a Word document — precisely the material the ADR 0007 default exists
to keep — gets it stored as `artifacts/artifact_sha256_<64hex>.bin`. A `.bin`
file does not open on double-click, does not preview in Obsidian or a file
manager, and tells the operating system nothing about its type. The policy
whose entire purpose is to keep useful work material produces, for most of
its default document types, the least usable possible representation of that
material. That is worse than a cosmetic gap: it defeats the purpose of the
default for the majority of the document formats it was written to cover.

**Note on a related discrepancy found while preparing this ADR.** The doc
comment on `default_artifact_media_types()` in
`crates/fieldnotes-field-protocol/src/limits.rs` states "**Ten** of these
twenty media types have no entry" where ADR 0007's own body, and an
independent recount performed for this ADR, both say **nine** (the list
above). Counting the default set against the pre-ADR-0008 fifteen-row
registry confirms nine, not ten: eleven of the twenty default types already
had an entry (`application/pdf`, `text/plain`, `text/markdown`, `image/png`,
`image/jpeg`, `image/gif`, `image/webp`, `audio/mp4`, `audio/mpeg`,
`audio/wav`, `audio/ogg`), leaving exactly nine without one. This ADR does
not correct that comment: `limits.rs` belongs to `fieldnotes-field-protocol`,
outside this ADR's file set, and the comment's numeric claim is stale in a
different way after this ADR ships regardless (the honest count of
*undetectable-by-content* types is smaller still — see below). It is
recorded here so a follow-up correcting that comment does not have to
re-derive the count.

## Decision

### Extend the registry with the nine missing rows

The nine media types above are added to the canonical registry in
`crates/fieldnotes-format/src/extension.rs` and to the mirrored table in
`docs/artifacts.md`, keeping both files in the same order (the registry's
existing alphabetical-by-media-type order) and byte-consistent in content.
No existing row's extension changes.

This is a **pure addition**. It cannot invalidate anything already stored:

- An original's identity is its content hash, never its extension
  (`docs/artifacts.md` "Original byte identity"; A1 section 2). Adding a
  registry row does not touch how any existing hash is computed.
- `fieldnotes-store`'s artifact lookup
  (`crates/fieldnotes-store/src/artifact.rs::find_artifact`) does not trust
  the canonical-extension-derived filename alone: when that expected path is
  absent it scans the artifacts directory for any file whose **stem** (not
  extension) equals the artifact ID, because "the same bytes may have been
  stored earlier under a different canonical extension." A file already
  stored as `artifact_sha256_<hex>.bin` before this ADR is found and reused
  by that stem-matching scan; it is never re-stored under a new extension,
  and no duplicate is created. `store_artifact` calls `find_artifact` first
  and only writes a new file when nothing matches.
- A source filename never selects the stored extension (A1 section 2;
  `docs/artifacts.md` "Import and collection"), so this change cannot alter
  naming based on anything the previous ingestion also had available; it can
  only change what a *newly computed* canonical extension resolves to for a
  media type that previously had no row at all.

### What content detection can and cannot actually deliver

`canonical_extension` is looked up **only** from `detect_media_type`'s
content-based result (confirmed by every caller:
`crates/fieldnotes-store/src/artifact.rs` and
`crates/fieldnotes-app/src/note.rs::store_import`, which pass
`detect_media_type(&bytes)` and nothing else). A declared or source-supplied
media type is never consulted for naming. So a new registry row only
produces a real, user-visible extension for material whose media type
`detect_media_type` can actually determine from the bytes.

Of the nine, that split as follows:

- **Detected reliably, real benefit today:** `application/rtf` (a fixed
  `{\rtf1` header, distinct from the generic text fallback) and `image/heic`
  (the ISO base media `ftyp` box's brand field — `heic`, `heix`, `heim`,
  `heis`, `mif1`, `msf1` — read the same way the existing code already reads
  the `M4A `/`M4B `/`M4P ` audio brands to distinguish `audio/mp4` from
  `video/mp4`). Both checks are small, single-purpose magic-byte comparisons
  consistent with every other check already in `detect_media_type`, so both
  are added alongside the registry rows in this same change. Without the
  `image/heic` addition, a real HEIC photo's `ftyp` box would keep falling
  through the existing brand check as an unrecognized brand, which the code
  already unconditionally treats as `video/mp4` — i.e. today, HEIC photos
  are actively misdetected as `video/mp4`, not merely undetected. Adding the
  brand check both gives the new `.heic` row real content and fixes that
  existing misclassification as a side effect.
- **Not detectable by content, no benefit from detection alone:**
  `text/csv` has no content signature whatsoever — it is ordinary delimited
  text, byte-for-byte indistinguishable from any other UTF-8 text, so it
  reads as `text/plain` today and will continue to.
- **Not detectable by content without deeper inspection than this ADR
  implements:** the three Office Open XML formats
  (`.docx`/`.xlsx`/`.pptx`) and the three OpenDocument formats
  (`.odt`/`.ods`/`.odp`) are all ZIP containers. Their local-file-header
  magic bytes (`PK\x03\x04`) are identical to a plain `application/zip` and
  to each other; the only way to tell them apart is to open the archive and
  read an internal member — `[Content_Types].xml` for Office Open XML, or
  the fixed-content `mimetype` member for OpenDocument. `detect_media_type`
  deliberately does **not** do this. Parsing an untrusted ZIP's central
  directory to read a named member is a materially different kind of
  operation from a fixed-offset magic-byte comparison: it means trusting
  compressed-size and offset fields from untrusted input, handling malformed
  or adversarial archives, and reasoning about the same
  decompression-bomb/traversal concerns `docs/artifacts.md`'s security
  section already requires for a full archive-extraction path — which this
  crate does not have and should not grow as a side effect of a naming
  lookup. That is not "genuinely small and safe," so it is not implemented
  here.

**Honest bottom line.** This ADR closes the naming gap for two of the nine
types outright (`.rtf`, `.heic`) through content detection alone. For the
other seven, the new registry rows exist and are correct, but content
sniffing alone will not select them: a `.docx`/`.xlsx`/`.pptx`/`.odt`/`.ods`/
`.odp`/`.csv` file imported today, with nothing but its bytes to go on, still
resolves to `.bin`. The rows are not wasted, though: they are exactly what a
future caller needs the moment it has a declared or source-supplied media
type available (an A2 Field that reports `ArtifactRef.media_type` from a
connector's own metadata, or a later explicit-import path that lets a user
assert a type) — today's importer simply has no such caller wired up to
supply one. This ADR states that limitation plainly rather than claiming the
registry addition alone fixes retrieval for the majority of the nine types.

### Alternative considered: narrow the default retention set instead

Rejected. The alternative would drop the seven still-undetectable formats
(the six Office/OpenDocument types and `text/csv`) from ADR 0007's default
include set, so nothing in the default ever produces a `.bin` original. This
was rejected because:

- It solves the symptom by deleting the feature. ADR 0007's default exists
  specifically because Office documents are exactly the "material useful for
  work and context" a work-notes tool should retain by default; removing
  them from the default because naming cannot fully resolve them yet is a
  worse outcome for users than keeping them retained under an imperfect
  extension.
- A `.bin`-named original is still strictly better than no original at all.
  The bytes are exact, content-addressed, and durable; a user can still open
  a `.bin` file by renaming it or via "open with," and any future rendition
  or explicit-media-type path recovers the correct name without re-import.
  Silently declining to keep the file is unrecoverable without going back to
  the source; a wrong extension is not.
- Narrowing the default would need to happen again the moment any future
  improvement — an explicit declared-media-type path, or deeper archive
  inspection — closed the remaining detection gap, since there would be no
  reason to keep the narrower default once naming caught up. That is
  churn in the wrong direction: better to keep the useful default now and
  improve naming under it over time, which is exactly what this ADR does
  for two of the nine types today.

### Alternative considered: implement ZIP-member inspection now

Considered and rejected for this ADR, for the reasons in "What content
detection can and cannot actually deliver" above: reading
`[Content_Types].xml` or `mimetype` from an untrusted ZIP is a real archive-
parsing feature with its own hardening requirements, not a small, safe,
fixed-offset check like the RTF header or the HEIC `ftyp` brand. It is left
for a future ADR if evidence shows the seven-of-nine gap matters enough in
practice to justify that scope, and if so it should be designed alongside
the archive-extraction hardening `docs/artifacts.md`'s security section
already anticipates, not bolted onto this lookup in isolation.

## Consequences

- `crates/fieldnotes-format/src/extension.rs`: `REGISTRY` grows from fifteen
  rows to twenty-four (the nine additions above); `detect_media_type` gains
  an `image/heic` branch on the existing `ftyp`-brand check and a new
  `{\rtf1` header check; new tests cover every added mapping, the unlisted-
  type `.bin` fallback (including the still-excluded legacy binary Office
  types `application/msword` etc.), parameter-stripping/lowercasing for the
  new rows, the two now-detectable formats, and the still-undetectable ones
  (documenting, not just asserting, that CSV and the six ZIP-based formats
  resolve to `text/plain`/`application/zip` from content alone).
- `docs/artifacts.md`'s registry table gains the same nine rows in the same
  order, with the detection caveat stated plainly rather than implied; its
  A2 retention-policy section's reference to the "real gap this policy does
  not close" is updated to point at this ADR instead of describing an open
  gap.
- `docs/approvals/A1-notebook-contract.md`'s approved-amendments block gains
  an entry recording this registry addition, since A1 section 2 froze the
  registry and the compatibility-and-change-policy section requires registry
  review and fixtures for any addition.
- `tests/fixtures/hashes/proposed-v1/` gains media-type-to-extension and
  artifact-path vectors for the nine new rows, documented in that corpus's
  README the way the existing artifact vectors are documented.
- The `limits.rs` "Ten of these twenty" comment discrepancy noted above is
  not corrected by this ADR (out of this ADR's file set) but is recorded so
  a follow-up in `fieldnotes-field-protocol` can fix it without
  re-deriving the count; that comment will also need to drop its framing of
  the registry gap as fully "not solved" once this ADR ships, since two of
  the nine are now solved and the rest are only partially open (declared-type
  callers benefit even though content sniffing does not).
- Nothing here changes A2's approval status or ADR 0007's rulings; this ADR
  is exactly the registry review ADR 0007 deferred, nothing more.
