# Proposed v1 hash and canonicalization vectors

**Status:** Approved at A1 on 2026-08-22; frozen as the compatibility contract  
**Implementation:** None yet. These files specify the approved byte inputs and expected digests; IG1 adds the executable implementation.

## Rules shared by these vectors

- SHA-256 output is lowercase hexadecimal.
- A public hash value carries the textual label shown in its expected file; the label is not part of the SHA-256 input unless a section explicitly says it is.
- Files in this directory use UTF-8 and LF. Every non-empty input fixture has exactly one final LF. The current text happens to use precomposed Unicode characters, but the algorithm does not normalize Unicode.
- Hashing is streaming and binary-safe. No platform newline conversion, YAML parsing, text decoding, path, media type, filename, timestamp, or filesystem metadata is applied unless the specific algorithm says so.

The `.bin` artifact fixture deliberately contains only patch-safe ASCII bytes so it can be added with `apply_patch`. A later approved suite should add at least one true arbitrary-byte vector containing NUL and high-bit bytes through a binary-safe repository mechanism. Until then, binary safety is an algorithm requirement that this particular fixture does not fully exercise.

## Artifact byte hash

Candidate public form:

```text
sha256:<lowercase-hex>
```

Input is the exact bytes of `artifact-input.bin`, with no domain prefix and no normalization. The exact input bytes are:

```text
Fieldnotes artifact bytes.\nSecond line.\n
```

Hexadecimal input:

```text
46 69 65 6c 64 6e 6f 74 65 73 20 61 72 74 69 66 61 63 74 20 62 79 74 65 73 2e 0a 53 65 63 6f 6e 64 20 6c 69 6e 65 2e 0a
```

Expected value is stored in `artifact-input.sha256`.

The same digest produces this A1 original-artifact identity:

```text
artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17
```

For this vector the media type is intentionally unavailable, so the canonical
extension fallback and notebook path are:

```text
artifacts/artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17.bin
```

This supplies an exact bytes-to-ID-to-path vector without pretending that the
illustrative `.jpg`, `.m4a`, and `.pdf` references in the notebook-shape corpus
have checked-in payloads.

## Normalized Markdown content hash

Candidate public form:

```text
fn-content-v1-sha256:<lowercase-hex>
```

Candidate normalization of a Note's Markdown body is:

1. Decode valid UTF-8. Invalid input must follow the separately approved damage policy; silent replacement is not proposed here.
2. Remove one leading UTF-8 BOM (`EF BB BF`) when present at ingestion.
3. Convert CRLF and bare CR line endings to LF.
4. Preserve the Unicode code-point sequence and its UTF-8 bytes exactly. Do not apply NFC, NFD, NFKC, NFKD, case folding, confusable mapping, or any other Unicode normalization.
5. Preserve all other characters and whitespace, including leading whitespace, internal blank lines, and trailing spaces.
6. Replace any run of final LF characters with exactly one final LF; add one if absent.
7. Encode the result as UTF-8.

For a complete Note file, the one empty line after the closing `---` delimiter
separates frontmatter from the body and is not part of this hash input.

The SHA-256 byte input is the concatenation of:

1. ASCII bytes for `fieldnotes-content-v1`;
2. one NUL byte (`00`);
3. the normalized Markdown body bytes.

`normalized-body-input.md` is already UTF-8 and LF and has exactly one final LF. Its characters happen to use precomposed forms, but that is incidental; the bytes are not normalized by this algorithm. Its body bytes are:

```text
# Grüezi\n\nCafé and Grüße.\n
```

The complete hashed input is therefore conceptually:

```text
fieldnotes-content-v1\0# Grüezi\n\nCafé and Grüße.\n
```

Complete hexadecimal input, including the domain prefix and NUL separator:

```text
66 69 65 6c 64 6e 6f 74 65 73 2d 63 6f 6e 74 65 6e 74 2d 76 31 00 23 20 47 72 c3 bc 65 7a 69 0a 0a 43 61 66 c3 a9 20 61 6e 64 20 47 72 c3 bc c3 9f 65 2e 0a
```

The NUL separator is a byte, not the two printable characters `\` and `0`. Expected value is stored in `normalized-body-input.sha256`.

### Unicode preservation decision

Unicode normalization is deliberately outside the content-hash algorithm.
Canonically equivalent but byte-distinct Unicode sequences therefore produce
different hashes. This preserves exact Note-body evidence and avoids changing
Extraction span locations during hashing. The `0.1.8` capability gate defines
whether text evidence offsets count UTF-8 bytes, Unicode scalar values, or
another explicit unit; hashing does not remap those offsets through Unicode
normalization.

## Semantic conflict hash

Candidate internal/public diagnostic form:

```text
fn-record-v1-sha256:<lowercase-hex>
```

This proposal hashes the concatenation of:

1. ASCII bytes for `fieldnotes-record-v1`;
2. one NUL byte (`00`);
3. the exact bytes of the canonical serialized semantic record.

The in-band prefix prevents these bytes from being confused with another SHA-256 domain even if an external label is dropped.

`semantic-record-source.md` is an illustrative current Note containing semantic fields and merge/collection bookkeeping. `semantic-record-canonical.md` is the exact candidate hash input after exclusions and canonical serialization.

For this vector, canonical serialization means:

- UTF-8 and LF, preserving the Unicode code-point sequence and UTF-8 bytes without NFC, NFKC, or any other Unicode normalization;
- opening and closing `---` frontmatter delimiters;
- retained property names in ascending ASCII byte order;
- the approved canonical scalar/list YAML spelling shown by the fixture;
- datetime values converted by instant to canonical UTC `+00:00` spelling;
- exactly one blank line between frontmatter and Markdown body;
- the normalized Markdown body with exactly one final LF.

The following are excluded before serialization because they would make the same current upstream state differ by collection or merge history:

- `id` (Fieldnotes Note identity);
- `instance_id` and `field_id` (first/surviving producer provenance);
- `collected_by` (merged producer provenance);
- `captured_at` (local durable-capture instant);
- `source_version` (used separately only by the connector-approved freshness rule);
- `content_hash` (a computed value with a different hash domain);
- `entities` and `related` (rebuildable graph-projection links);
- filename and path;
- merge conflict envelope/status and temporary reconciliation bookkeeping, once those representations are approved;
- serialization-only comments, YAML aliases/tags, or formatting, which the public canonical format does not preserve.

The portable `source_scope` and `source_identity`, event time, source-semantic properties, and normalized body remain included. They distinguish different source objects and different current states.

Public Note frontmatter retains its meaningful source/client-local offset. UTC
conversion occurs only in this semantic-comparison encoding, so byte-distinct
offset spellings for the same instant do not create a false conflict.

The domain prefix and separator begin with these bytes:

```text
66 69 65 6c 64 6e 6f 74 65 73 2d 72 65 63 6f 72 64 2d 76 31 00
```

Expected value is stored in `semantic-record-canonical.sha256`.

### Semantic-hash decisions proposed for A1 approval

- `id` is excluded so independently collected Notes for the same portable
  source object can compare semantically.
- Retained keys use ascending ASCII order and the A1 canonical scalar emitter.
- Rebuildable `entities` and `related` projection links are excluded even when
  present on the public Note.
- `source_version` is excluded from equality and used separately only through
  the connector-approved ordering rule. Local capture time and producer order
  never establish freshness.

## Verification commands

The expected files were calculated with the system `shasum -a 256` over the exact fixture bytes. The normalized-content calculation streamed the ASCII domain prefix and NUL byte immediately before the body; it did not create an intermediate file.

These proposed vectors remain review-only until A1 approves normalization,
exclusions, property ordering, and the exact canonical serializer. IG1 adds
paired BOM/CRLF/lone-CR/final-LF and exclusion vectors before compatibility is
declared; the A1 algorithm above already fixes their required result.
