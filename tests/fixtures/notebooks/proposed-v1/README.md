# Proposed A1 notebook fixtures, version 1

This directory is a byte-exact proposal for the A1 notebook contract. The user
approved A1 on 2026-08-22, so these bytes are now frozen as the approved IG1
implementation target rather than review material. Files use UTF-8, LF line
endings, deterministic property ordering, and exactly one blank line between
frontmatter and Markdown body.

## Canonical property ordering

Note frontmatter begins with `id`, `instance_id`, `field_id`, `type`, and
`occurred_at` in that order. Every remaining Note property follows in ascending
ASCII byte order by key.

Every non-Note public record begins with `id` and `type` in that order. Every
remaining property follows in ascending ASCII byte order by key.

## Naming used by this proposal

- Notes use `<UTC>_<field-id>_<type>_<note-id>.md`.
- Instance metadata uses the A1 operational exception at
  `.fieldnotes/instance.yaml`.
- Other public records use `<record-id>_<type>.md`.
- The handback manifest uses `packages/<package-id>/manifest.md`.
- Conflict bundles use
  `conflicts/<conf-id>/{conflict.md,candidate_1.md,candidate_2.md}`.
- Calendar is represented as `type: event`.
- Outlook Mail remains distinct as `type: mail`.
- Imported generic files use `type: file`.

These choices are intentionally reviewable at A1. Field type stems and
properties use the proposed `local_`, `outlook_mail_`, `outlook_calendar_`,
`outlook_contacts_`, `teams_`, and `jira_` prefixes.

Fixture UUIDv7 timestamps are coherent creation instants: the instance ID
matches `created_at`; Note IDs match their initial durable `captured_at`; and
derived/conflict IDs match `generated_at` or `detected_at`. The invalid corpus
uses a plausible creation instant shortly after its scenario event time.

## Fixture limits

Every Note `content_hash` is a verified vector over the exact normalized body
bytes using the `fieldnotes-content-v1` domain separator and one NUL byte.
Original artifact references use content-addressed
`artifact_sha256_<64hex>` IDs, and Note bodies show the proposed relative path
as inline code rather than a clickable link because payload files are not part
of this notebook-shape corpus. Those three ID literals demonstrate reference
syntax only; the independently reproducible exact-byte artifact vector is in
`../../hashes/proposed-v1/`. Semantic-record conflict fingerprints are
verified vectors whose canonical encoding was approved at A1 and is now
normative.

The package inventory is readable Markdown rather than nested frontmatter. The
package schema and checksums remain a later approval, so the manifest uses only
common derived properties in frontmatter and does not imply delivery.

`conflicts/same-id/left/` and `conflicts/same-id/right/` contain the same Note
ID and filename with divergent current content. Neither side supplies a
reliable source version, so the pair must remain a visible conflict. The
proposed materialized result is under `conflicts/<conf-id>/`; its candidates are
ordered by ascending proposed semantic-record fingerprint.

## Gate classification

| Corpus area | A1 approval meaning | Later gate work |
|---|---|---|
| `.fieldnotes/instance.yaml` | Normative for the A1 operational instance-metadata exception and exact bytes | IG1 adds parser/write tests |
| Notes, entities, relationships, proposal, and conflict files | Normative for the represented A1 envelope, naming, property ordering, scalar/list form, and exact bytes | IG1 adds omitted Note types and boundary cases |
| Extraction and Observation | Normative only for the generic A1 derived-record envelope | `0.1.8` approves capability-specific types, evidence units, properties, and generators |
| Package manifest | Normative for `pkg_`, directory/name, and generic flat manifest envelope only | `0.1.7` approves selection, closure, checksums, encryption, and lifecycle semantics |
| Artifact IDs and paths embedded in Notes | Normative syntax examples; the absent payloads and their illustrative IDs are not end-to-end vectors | IG1 adds matching stored payload fixtures; `0.1.7` approves rendition layout |
| Standalone artifact/hash corpus | Normative algorithm vector approved at A1 | IG1 adds arbitrary binary and normalization boundary vectors |
