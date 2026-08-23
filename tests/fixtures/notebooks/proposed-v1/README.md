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

## IG1 corpus expansion

A1 required IG1 to expand the corpus before the `0.1.0` compatibility suite is
complete (see the A1 approval's fixture-evidence list). This batch adds, all
normative at A1:

- `20260822T223000Z_teams_work_meeting_note_01a02b9a-2f00-7000-8000-00000000000c.md` —
  the missing `meeting` primary type (A1 section 5), produced by the `teams_work`
  Field: the meeting itself, not its calendar reservation, is primary. Its
  `occurred_at` (`2026-08-23T01:30:00+03:00`) is a positive explicit offset
  whose local calendar date is a day ahead of its UTC instant
  (`2026-08-22T22:30:00Z`), demonstrating that the filename is computed from
  the UTC instant rather than the local wall clock.
- `20260822T140000Z_teams_work_call_note_01a029c7-43c0-7000-8000-00000000000d.md` —
  the missing `call` primary type: an observed call record, distinct from a
  playable `voice` recording. Its `occurred_at` uses the explicit `+00:00` UTC
  offset required by the fixture-evidence list.
- `20260823T010000Z_local_work_document_note_01a02c26-4260-7000-8000-00000000000e.md` —
  the missing `document` primary type: a text-bearing source document whose
  document identity is primary, distinct from a generic imported `file`. Its
  `occurred_at` (`2026-08-22T20:00:00-05:00`) is a negative explicit offset
  whose UTC instant (`2026-08-23T01:00:00Z`) falls on the day *after* the local
  calendar date, the opposite boundary-crossing direction from the `meeting`
  Note above. It also carries `local_document_date` (a plain `YYYY-MM-DD`
  date scalar, closing the corpus's only missing scalar-form example) and
  `local_document_flag: "true"` (double-quoted text that would otherwise
  resolve as a boolean under the YAML 1.2 Core Schema, alongside the existing
  `source_version: "1745317800000"` quoted-as-text numeric example).

Single-member lists, colon-quoted text, booleans (`damaged`/`truncated`), and
numbers were already demonstrated by the pre-IG1 corpus; only a plain date
scalar and a boolean-shaped text scalar were missing. Damaged/truncated
material was already demonstrated by
`20260822T091500Z_outlook_mail_work_mail_note_01a028c1-6c80-7000-8000-00000000000a.md`
(`damaged`, `truncated`, `lost_characters`), so IG1 added no further fixture
for that requirement.

`tests/fixtures/hashes/proposed-v1/artifact-input-binary.bin` adds the true
arbitrary-byte artifact vector called for by that corpus's own README; see
that file for the exact bytes, digest, and derived artifact ID.

## ADR 0007 corpus expansion

The A1 amendment approving the new shared property `skipped_attachments`
(see [ADR 0007](../../../../docs/decisions/0007-attachment-retention-policy.md)
and the A1 approval's amendment block) required registry review with
fixtures. This batch adds one normative fixture:

- `20260822T092000Z_outlook_mail_work_mail_note_01a028c6-eac0-7000-8000-00000000000f.md` —
  a mail Note with **one retained attachment and one skipped attachment on
  the same Note**, demonstrating that `artifacts`/`attachments` and
  `skipped_attachments` are independent flat lists rather than parallel or
  correlated ones. `notes.txt` is retained and appears in `artifacts` and
  `attachments` under its illustrative `artifact_sha256_333…3` ID;
  `team-standup-recording.mp4` is not retained (video is outside the default
  media-type include set) and its stable connector-namespaced reference
  appears only in `skipped_attachments`. Per-attachment human detail — name,
  approximate size, and why each was (or was not) retained — appears in the
  Markdown body as deterministic evidence rather than in frontmatter, and the
  body's attachment links follow the retention outcome: a relative artifact
  path for the retained file, the source URL for the skipped one.
  `source_url` remains present in frontmatter regardless. Its `content_hash`
  is a verified vector produced by the `fieldnotes-format` crate's own
  `RecordBuilder` and `content_hash_value`, matching the process the corpus's
  own fixture limits section describes below.

## ADR 0011 identifier neutralization

Before this repository's first publication to a public remote,
[ADR 0011](../../../../docs/decisions/0011-neutralize-illustrative-personal-identifiers.md)
substituted the illustrative personal identifiers this corpus had inherited
from the owner's own environment. The notebook-owner persona is now `sam` /
`Sam` / `sam@example.net`, and the illustrative Field label that abbreviated
the owner's company is now `acme`. `alice@example.com` / `Alice Müller`,
`bob@example.net` / `Bob Rossi`, and `former.colleague@example.com` were
already fictional and are untouched.

Nothing else changed. No property name, record type, grammar, property
ordering, list semantics, scalar form, limit, or rejection code changed; no
fixture's structure, coverage, or isolated edge case changed; and because no
Note's filename inputs changed, no filename changed.

Two consequences for the verified vectors this corpus carries:

- the `content_hash` on the five Notes whose Markdown bodies addressed or
  named the persona was recomputed. The corpus's other fourteen embedded
  content hashes recompute unchanged, because their bodies never named the
  persona;
- both semantic-record fingerprints in `conflicts/<conf-id>/conflict.md` were
  recomputed, because both candidates' `to` lists carry the persona address.
  Candidate ordering by ascending fingerprint did **not** change, so neither
  candidate file was renumbered and `candidate_1.md` still holds the
  lexicographically smaller fingerprint. The candidates' own `content_hash`
  values are unchanged: the persona address is in their frontmatter, and
  `fn-content-v1` hashes the body alone.

`crates/fieldnotes-format/tests/conformance_valid.rs` and
`conformance_hashes.rs` recompute every one of these from the fixture bytes
on each test run.

## ADR 0012 corpus expansion

[ADR 0012](../../../../docs/decisions/0012-graph-implementation-rulings.md)
ruled on findings the `fieldnotes-graph` (IG4) implementation surfaced
against this corpus, recorded in
[A1 graph implementation findings](../../../../docs/approvals/A1-graph-implementation-findings.md).
This batch adds one Note and regenerates three derived fixtures, all
normative at A1:

- `20260822T082000Z_outlook_contacts_work_contact_note_01a02891-5bd0-7000-8000-000000000010.md` —
  a `contact` Note for `bob@example.net`, produced by the same
  `outlook_contacts_work` Field as Alice's existing contact record and
  carrying the same registered properties, so the corpus demonstrates the
  contact-record-to-entity-`title` provenance chain for both people rather
  than only one. Its `identities` list carries a single `email:bob@example.net`
  anchor: Bob has never carried a phone anchor anywhere else in the corpus,
  so this fixture does not invent one merely to mirror Alice's two-anchor
  shape. Its `content_hash` is a verified vector produced by
  `fieldnotes-format`'s own `content_hash_value`.
- `20260822T091500Z_outlook_mail_work_mail_note_01a028c1-6c80-7000-8000-00000000000a.md` —
  the damaged/truncated mail Note gains an `identities` property
  (`email:bob@example.net`, `email:sam@example.net`) matching the convention
  every other mail Note in the corpus already follows for the anchors its
  `from`/`to`/`cc` roles carry. This is a frontmatter-only addition: the
  Note's `damaged`, `truncated`, and `lost_characters` properties, and its
  body, are unchanged, so it still exercises exactly the damage/truncation
  condition it was added to pin. `content_hash` covers the normalized body
  only, so this addition does not move it; `fieldnotes-format`'s own
  conformance suite recomputes and confirms the unchanged value on every run.
- `entities/ent_01a028f2-dcc0-7000-8000-000000000001_person.md` (Alice) and
  `entities/ent_01a028f2-dcc0-7000-8000-000000000002_person.md` (Bob), and
  `relationships/rel_01a028f4-b180-7000-8000-000000000001_person_person.md`
  (their edge), are regenerated directly from
  `fieldnotes_graph::derive_graph` run over this corpus with the
  previously-frozen fixtures supplied as prior projections, so the library
  reused their exact projection IDs rather than minting new ones. Every
  other property, and each record's Markdown body, came from the library's
  own `entity_record`/`relationship_record` emitters — nothing here is
  hand-typed. Alice's entity now cites all eight current Notes that carry
  her anchor; Bob's now cites all four that carry his, including his new
  contact record and the damaged mail Note above. Both fixtures now carry
  the same `generated_at` instant, because one derivation call stamps every
  projected record it returns from a single clock read.

These three regenerated files remain a curated pair of entities and their
edge, not the complete projection the full corpus would produce (which also
derives a third `person` entity for the notebook owner, `sam@example.net`,
not checked in here as a fixture) — see
[A1 graph implementation findings, finding 4](../../../../docs/approvals/A1-graph-implementation-findings.md#finding-4-entity-fixtures-reflect-the-pre-ig1-four-note-corpus-not-the-current-fourteen).

## Gate classification

| Corpus area | A1 approval meaning | Later gate work |
|---|---|---|
| `.fieldnotes/instance.yaml` | Normative for the A1 operational instance-metadata exception and exact bytes | IG1 adds parser/write tests |
| Notes, entities, relationships, proposal, and conflict files | Normative for the represented A1 envelope, naming, property ordering, scalar/list form, and exact bytes | IG1 has added the previously omitted `meeting`, `call`, and `document` Note types and UTC-boundary-crossing/`+00:00` datetime cases (see "IG1 corpus expansion" above); the ADR 0007 amendment pass has added the `skipped_attachments` Note (see "ADR 0007 corpus expansion" above); the ADR 0012 amendment pass has added Bob's contact Note, added `identities` to the damaged mail Note, and regenerated the entity/relationship fixtures from the graph library itself (see "ADR 0012 corpus expansion" above) |
| Extraction and Observation | Normative only for the generic A1 derived-record envelope | `0.1.8` approves capability-specific types, evidence units, properties, and generators |
| Package manifest | Normative for `pkg_`, directory/name, and generic flat manifest envelope only | `0.1.7` approves selection, closure, checksums, encryption, and lifecycle semantics |
| Artifact IDs and paths embedded in Notes | Normative syntax examples; the absent payloads and their illustrative IDs are not end-to-end vectors | IG1 adds matching stored payload fixtures; `0.1.7` approves rendition layout |
| Standalone artifact/hash corpus | Normative algorithm vector approved at A1; IG1 has added the true arbitrary-byte binary vector (`artifact-input-binary.bin`) | IG1 adds further normalization boundary vectors as needed |
