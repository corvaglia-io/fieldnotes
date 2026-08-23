# ADR 0011: Neutralize illustrative personal identifiers before first publication

- **Status:** Accepted on 2026-08-23
- **Date:** 2026-08-23

## Context

This repository is about to be published to a public remote for the first
time. Until now it was a private working tree, and its illustrative material
had grown out of the owner's own environment rather than from a deliberately
fictional cast. Four categories of value were affected:

1. **Illustrative absolute paths.** Every illustrative filesystem path was
   rooted in the owner's real home directory on their own machine: the
   `root_path` and `artifact_staging_dir` values throughout the A2 protocol
   transcripts, a `fieldnotes-store` profile doc comment, the absolute-path
   rejection case in the `fieldnotes-field-protocol` artifact-handle grammar,
   and two test literals.
2. **The notebook-owner persona.** The person the A1 notebook corpus, the A2
   transcripts, the fixture Field, and the prose docs addressed and named —
   recipient addresses, identity anchors, an event organizer, a Note-body
   salutation, an instance friendly name — was the owner, under the owner's
   own given name and a mailbox-shaped address built from it.
3. **An illustrative Field label.** One of the labels used throughout the
   examples (`teams_<label>`, `outlook_<label>`, `outlook_mail_<label>`,
   `jira_<label>`, `twenty_<label>`, `microsoft_<label>`) abbreviated the
   owner's company.
4. **Two deliberately realistic home-directory paths.** Two tests — the Field
   SDK's `scope::derive` and the `local` Field's `scope::compute` — used a
   path containing the owner's real account name, precisely to prove that the
   derived `local-root:<sha256>` scope does not leak a home-directory user
   name. This was the one place a real identifier was load-bearing for the
   test's own point.

None of these is contract-bearing. Every one is an *illustrative value*: a
path, an address, a display name, or a Field label chosen to make an example
readable. The contract is the grammar, property names, types, ordering, list
semantics, limits, and rejection codes around them.

The complication is that the A1 notebook corpus and the A2 protocol corpus
are approved, frozen material, and some of these identifiers sit inside Note
bodies, which are content-hashed, and inside the retained portion of the
semantic-record encoding. Changing an illustrative address therefore changes
real approved bytes and invalidates real approved digests, which is why this
needs an ADR rather than a quiet edit.

Alternatives considered and rejected:

1. **Publish as-is.** Rejected: the owner's directive is that nothing
   identifying them reaches the remote, and a public repository is exactly the
   wrong place to discover that an illustrative fixture was autobiographical.
2. **Neutralize only the paths and leave the persona.** Rejected: the persona
   address and the company-derived Field label are the same class of leak as
   the path, and a partial pass would leave the corpus internally inconsistent
   — a real personal address sitting next to a scrubbed path.
3. **Rewrite the corpus wholesale with a richer fictional cast.** Rejected: it
   would change coverage. Each fixture pins one condition — a
   quoting-sensitive scalar, a single-member list, damage properties, a
   `+00:00` value, UTC date boundaries crossed in each direction, a same-ID
   divergent pair — and a rewrite is a chance to lose one of them silently.
   Substituting identifiers one for one cannot.
4. **Keep the real home-directory path in the two scope tests, because the
   test is precisely about not leaking it.** Rejected: the test's point is
   that *a* realistic home-directory user name does not survive the
   derivation. Any realistic user name proves it. The real one adds nothing
   except the leak.
5. **Record the old values in this ADR for traceability.** Rejected for the
   same reason as alternative 1: an amendment note that quotes what it removed
   publishes it anyway. The recomputed digests below are the auditable record,
   and they are auditable without naming anybody: each one reproduces from the
   fixture bytes now in the tree.

## Decision

Substitute illustrative identifiers, and nothing else, throughout the
committed working tree. The values now in the tree are:

| Category | Value now used |
|---|---|
| Illustrative absolute paths | rooted at `/home/user/` |
| Notebook-owner persona | `sam` / `Sam` / `sam@example.net` |
| The company-derived Field label | `acme` |
| The two scope tests' realistic home directory | `/home/samkeller/reference-library` |

Constraints this substitution respected:

- **Path shape is preserved.** A staging path still reads as a staging path:
  `/home/user/notebook/.fieldnotes/cache/staging/run-1a4c9f2e`. A configured
  local root still reads as a configured local root:
  `/home/user/reference-library`. The hostile artifact handle is still an
  absolute path, `/home/user/.ssh/id_ed25519`, so the handle grammar still
  rejects the same shape for the same reason.
- **One identity, used consistently.** `sam` / `sam@example.net` everywhere
  the owner persona appeared. The pre-existing fictional cast —
  `alice@example.com` / `Alice Müller`, `bob@example.net` / `Bob Rossi`,
  `former.colleague@example.com` — is untouched.
- **`acme`, not `work`.** The corpus already contains `teams_work` and
  `outlook_mail_work`. Collapsing a second label onto `work` could silently
  merge two distinct Field IDs, or deduplicate two entries inside a
  `collected_by` list, and the corpus would still validate while having lost
  the distinction it was demonstrating. `acme` was already in use as a Jira
  label (`jira_acme`) and composes without collision everywhere else.
- **The substituted persona sorts where the old one sorted.**
  `fieldnotes-graph` orders derived entities by primary anchor, and
  `crates/fieldnotes-graph/tests/corpus.rs` asserts the resulting order.
  `alice@example.com` < `bob@example.net` < `sam@example.net` holds exactly as
  the previous ordering did, so that assertion is unchanged rather than
  restated.
- **The two scope tests keep their intent and their assertions.** Each still
  derives a scope from a realistic home-directory path and still asserts both
  that the scope does not contain the path's user-name segment and that it
  carries the `local-root:` prefix. Only the segment named in the path and in
  the assertion changed.

No property name, record type, grammar, limit, rejection code, list
semantics, property ordering, filename form, or fixture coverage property
changed. No Note's filename inputs changed, so no filename changed. The
invalid notebook corpus and the `valid: false` protocol frames are rejected
with exactly the same errors, each still isolating its single error: the
hostile-handle frame still fails `record-event.schema.json` at
`artifacts[0].handle` against the same single-segment handle pattern, because
an absolute path was substituted for an absolute path.

## Consequences

### Hashes recomputed

The persona address appears inside content-hashed Note bodies and inside the
retained portion of the semantic-record encoding, so two hash domains had to
be recomputed. Neither was computed by hand: each value was produced by the
algorithm's own implementation and confirmed by the conformance suite, which
recomputes every vector from the fixture bytes on every test run.

1. **`fn-content-v1-sha256` on five Notes in
   `tests/fixtures/notebooks/proposed-v1/notes/`** — the five whose Markdown
   bodies addressed or named the persona:

   | Note | New `content_hash` |
   |---|---|
   | `…_mail_note_01a0287d-acc0-…-000000000005` | `65cfccffb83e395f4d3ac4d4127bfe16989ad2d517a2ab366088daa1a6cdcf9e` |
   | `…_event_note_01a02880-6be0-…-000000000006` | `62d9407dc6467b74cce88aead09886f75b1501c2d916443fb9edead2df450c80` |
   | `…_mail_note_01a028c6-eac0-…-00000000000f` | `7fd3d1ea8da3c5a0f42251c5a76f8d18a05bca5600290a1364556bc02bbab5ef` |
   | `…_call_note_01a029c7-43c0-…-00000000000d` | `95e01b9f0f4a7040a6de4e89403b72ed160efd136471193d7b7de3d8bca2c2dc` |
   | `…_meeting_note_01a02b9a-2f00-…-00000000000c` | `c965177cf7c5fab043471c9cc0816b3fd2918ab59dc41038b9b29b427a2ffc5d` |

   The corpus's other fourteen embedded content hashes recompute unchanged,
   because their bodies never named the persona.

2. **`fn-record-v1-sha256` for the semantic-record vector**
   (`tests/fixtures/hashes/proposed-v1/semantic-record-canonical.sha256`) —
   the vector's `participants` list carries the persona address, and
   `participants` is *retained* by the semantic encoding, so the fingerprint
   is now
   `fn-record-v1-sha256:dfa17619cfa47d5a6b6736f0c99526a6b935a4d35dc596c7fdb8a7a72823d846`.
   The canonical encoding (`semantic-record-canonical.md`) still reproduces
   byte for byte from `semantic-record-source.md`, and the exclusion rule
   demonstrated itself along the way: the same source record's `field_id` and
   `collected_by` carried the substituted Field label, both are excluded
   before serialization, and neither moved the fingerprint by one byte.

3. **Both `fn-record-v1-sha256` candidate fingerprints in the frozen conflict
   bundle** `tests/fixtures/notebooks/proposed-v1/conflicts/conf_01a02905-…-000000000001/conflict.md`,
   because both candidates' `to` lists carry the persona address:
   `34179e54a18180b6ba011fe21c129028d2cbe8b04944a7e53fdeb5f6e296a2ba` and
   `673055d893d0aa9e25a4abc19aa1a3a32fc59b5ab5f531e8ad00c69fff6665e1`.
   **Candidate ordering did not change**: ascending fingerprint still puts the
   18:00 body in `candidate_1.md` and the 19:00 body in `candidate_2.md`, so
   no candidate file was renumbered and the bundle's own prose —
   "`candidate_1.md` has the lexicographically smaller semantic-record
   fingerprint" — remains correct.

### Hashes deliberately *not* recomputed

- The two artifact byte vectors (`artifact-input.bin`,
  `artifact-input-binary.bin`) and their digests, artifact IDs, and derived
  paths: no identifier appears in either payload, so the README's exact byte
  and hexadecimal listings stay literally true.
- The `fn-content-v1` normalized-body vector (`normalized-body-input.md` and
  its `.sha256`): its body is `# Grüezi\n\nCafé and Grüße.\n`, which names
  nobody.
- The `content_hash` on both conflict candidates and on both `same-id`
  divergent copies: the persona address sits in their frontmatter, not their
  body, and `fn-content-v1` hashes the body alone. That one frontmatter
  substitution moved the semantic fingerprint but not the content hash is the
  two hash domains behaving exactly as specified.
- Every `local-root:` scope in the fixtures. They are all
  `local-root:reference-library-v1` (a configured root *id*) or
  `local-root:one` — never a `local-root:<sha256-of-canonical-root-path>`
  derivation. No fixture scope derives from a path this ADR changed, so none
  needed recomputation. The two tests that *do* derive a scope from a path
  assert only that the path does not survive the hash, which is still exactly
  what they assert.

### Downstream assertions updated

`crates/fieldnotes-graph/tests/corpus.rs` derives entities from the notebook
corpus and asserts the derived primary anchors literally. Its expected anchor
list now names the substituted persona address. The assertion is otherwise
untouched: same three entities, same order, same accompanying claim that
"entities are ordered by primary anchor, and only the contact record's anchors
join". No other assertion in the workspace needed more than the same
one-for-one substitution.

### Contract effect

None. This changes approved bytes without changing any approved rule. A reader
who validated the old corpus against the contract prose and a reader who
validates the current corpus against the same prose reach identical
conclusions. Because approved bytes did change, it is recorded as an amendment
on both [A1](../approvals/A1-notebook-contract.md#approved-amendments) and
[A2](../approvals/A2-field-protocol.md#approved-amendments).

Anyone holding a copy of the pre-publication digests must take the values
above instead. That affects nobody outside this repository, which is the point
of doing this before the first push rather than after.
