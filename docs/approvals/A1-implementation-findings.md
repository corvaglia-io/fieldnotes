# A1 implementation findings from IG1

**Status:** Resolved. The coordinator ruled on all five findings on
2026-08-22; the rulings are recorded in
[ADR 0006](../decisions/0006-a1-implementation-rulings.md) and applied to
[A1](A1-notebook-contract.md), [notebook format](../notebook-format.md), and
[security](../security.md).
**Scope:** Contradictions and under-specifications found while implementing the
approved A1 notebook contract

IG1 implemented the approved A1 contract as domain types, canonical
serialization, hashes, and an executable conformance suite. The suite passes:
every valid corpus file round-trips byte-for-byte, every invalid fixture is
rejected with its documented conceptual error, and every hash vector reproduces.

Implementation followed the frozen fixture bytes wherever contract prose and
corpus bytes could both be satisfied. The items below are cases where they
could not, or where A1 left a rule open. Each was recorded rather than decided
at the time, because changing an approved shared contract requires a
coordinator ruling and an explicit migration proposal.

## 1. Derived-record type length contradicts the frozen corpus

A1 section 10 says derived record types "use the approved lowercase type
grammar", and the only type grammar in section 4 is `[a-z][a-z0-9_]{0,31}`,
which bounds a type at 32 bytes. The frozen fixture type
`organization_affiliation_candidate` is 34 bytes.

IG1 reads section 4's bound as specific to the eleven primary Note types and
implements non-Note record types as `[a-z][a-z0-9_]*` bounded at 63 bytes, the
property-name limit. If the 32-byte bound was intended to apply to every record
type, the frozen Observation fixture violates the contract it was approved
alongside, and one of the two must change.

**Ruling (ADR 0006 §1):** the 32-byte grammar applies only to the eleven
primary Note types. Non-Note record types use `[a-z][a-z0-9_]{0,62}` (63
bytes), matching the property-name bound, exactly as IG1 implemented.
Renaming the fixture to fit 32 bytes was rejected. A1 sections 4 and 10 are
amended accordingly. No approved byte changes; no format version bump.

## 2. Two distinct key-ordering rules

The public canonical form orders the structural keys first, then remaining keys
in ascending ASCII order. The frozen semantic-record vector orders every
retained key in ascending ASCII order with no structural exception, so `type`
sorts among the ordinary keys.

Both are implemented as the fixtures require. A1 section 5 describes only the
public rule and does not call out that the semantic encoding differs, so the
difference currently lives in fixture bytes rather than contract prose.

**Ruling (ADR 0006 §2):** the semantic encoding has two differences from the
public emitter — UTC datetime normalization and unconditional ascending-key
order with no structural-first exception — exactly as IG1 implemented. A1
section 8 is amended to state both, and to state that the semantic encoding
is a hash input and never a publishable notebook record. No approved byte
changes; no format version bump.

## 3. Secret detection is under-specified

The negative corpus defines exactly one secret case: an `access_token=` bearing
canary in a registered `source_url` property. IG1 detects that pattern
case-insensitively in frontmatter text values.

Two boundaries remain open. Markdown bodies are not scanned, and no general
secret-pattern policy exists, so a credential pasted into body evidence is not
rejected. Widening detection is a policy decision with false-rejection risk for
legitimate collected evidence, so it needs its own registry or gate rather than
an implementation guess.

**Ruling (ADR 0006 §3): withdrawn.** Fieldnotes performs no secret or
password scanning of notebook content. The `security.secret_detected`
rejection and its negative fixture are withdrawn; entropy-based detection was
also considered and rejected because it would false-positive on legitimately
high-entropy values such as `content_hash`, artifact IDs, and UUIDs.
Credential handling remains an internal concern (`CredentialProvider`,
protected Field delivery, diagnostic/log redaction, release-gate scanning of
Fieldnotes' own output), documented in [security](../security.md). A future,
optional PII-span-detection capability is recorded as an unapproved candidate
for the `0.1.8` enhancement gate in [the roadmap](../roadmap.md), pointing at
text a user may choose to mask without ever altering the Note.

## 4. Connector-prefixed property typing has no registry entry

Registered properties take their type from the property registry. A
connector-prefixed property has no registry entry, so IG1 infers its type from
canonical spelling: double-quoted values are text, and plain values matching a
datetime, date, boolean, or number shape take that type. A plain
timezone-less datetime-shaped token is rejected, which is safe because
canonical text of that shape must be quoted.

Separately, A1 does not bind a property prefix to the record's own `field_id`,
and no fixture exercises the combination, so any registered prefix is currently
accepted on any record.

**Ruling (ADR 0006 §4):** a Note's prefixed properties must belong to its own
`field_id`'s registered stem, plus unprefixed shared registry properties;
derived and projection records may carry any registered prefix; the built-in
`self` Field contributes no property prefix, exactly as A1 already required
for one-part Field IDs, and does not gain a `self_` prefix. This adds an
enforcement rule; A1 section 4 is amended. Spelling-based type inference
remains the interim rule for connector-prefixed property typing; declaring
and enforcing prefixed-property types from each Field's `describe` manifest
is deferred to gate A2. Not urgent for `0.1.0`, since only `self` ships and
carries no prefix.

## 5. A documentation example is not canonical

The `collected_by` example in [notebook format](../notebook-format.md) uses
double-quoted list members, while the frozen `semantic-record-source.md` fixture
uses plain style. Plain is what the A1 quoting rule produces for those values,
so the prose example is non-canonical and should be corrected to match the
approved bytes.

**Ruling (ADR 0006 §5):** corrected. The `collected_by` example now uses plain
style, matching the approved quoting rule and the frozen fixture. No approved
byte changes.

## Defects found and fixed in IG1

These were implementation bugs rather than contract questions, and are fixed:

- `Datetime::parse` sliced at fixed byte offsets before proving them to be char
  boundaries, so one adversarial record file with multibyte text in a datetime
  property panicked the parser.
- The parser stripped one separator LF without rejecting further blank lines, so
  extra blank lines became body content, the emitter produced a third LF, and
  records with identical evidence received different `fn-record-v1`
  fingerprints.
