# A1 implementation findings from IG1

**Status:** Open questions for the coordinator; no approved contract changed
**Scope:** Contradictions and under-specifications found while implementing the
approved A1 notebook contract

IG1 implemented the approved A1 contract as domain types, canonical
serialization, hashes, and an executable conformance suite. The suite passes:
every valid corpus file round-trips byte-for-byte, every invalid fixture is
rejected with its documented conceptual error, and every hash vector reproduces.

Implementation followed the frozen fixture bytes wherever contract prose and
corpus bytes could both be satisfied. The items below are cases where they
could not, or where A1 left a rule open. Each is recorded rather than decided,
because changing an approved shared contract requires a coordinator ruling and
an explicit migration proposal.

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

## 2. Two distinct key-ordering rules

The public canonical form orders the structural keys first, then remaining keys
in ascending ASCII order. The frozen semantic-record vector orders every
retained key in ascending ASCII order with no structural exception, so `type`
sorts among the ordinary keys.

Both are implemented as the fixtures require. A1 section 5 describes only the
public rule and does not call out that the semantic encoding differs, so the
difference currently lives in fixture bytes rather than contract prose.

## 3. Secret detection is under-specified

The negative corpus defines exactly one secret case: an `access_token=` bearing
canary in a registered `source_url` property. IG1 detects that pattern
case-insensitively in frontmatter text values.

Two boundaries remain open. Markdown bodies are not scanned, and no general
secret-pattern policy exists, so a credential pasted into body evidence is not
rejected. Widening detection is a policy decision with false-rejection risk for
legitimate collected evidence, so it needs its own registry or gate rather than
an implementation guess.

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

## 5. A documentation example is not canonical

The `collected_by` example in [notebook format](../notebook-format.md) uses
double-quoted list members, while the frozen `semantic-record-source.md` fixture
uses plain style. Plain is what the A1 quoting rule produces for those values,
so the prose example is non-canonical and should be corrected to match the
approved bytes.

## Defects found and fixed in IG1

These were implementation bugs rather than contract questions, and are fixed:

- `Datetime::parse` sliced at fixed byte offsets before proving them to be char
  boundaries, so one adversarial record file with multibyte text in a datetime
  property panicked the parser.
- The parser stripped one separator LF without rejecting further blank lines, so
  extra blank lines became body content, the emitter produced a third LF, and
  records with identical evidence received different `fn-record-v1`
  fingerprints.
