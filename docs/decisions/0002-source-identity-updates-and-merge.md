# ADR 0002: Portable source identity, updates, and merge

- **Status:** Accepted direction; merge metadata name remains reviewable
- **Date:** 2026-08-22

## Context

`(instance_id, field_id, source_identity)` identifies an object only as seen by
one producer. Two Fieldnotes instances connected to the same upstream account
need to recognize the same source object exactly, without treating content
equality as source identity. At the same time, a merge must retain where each
copy came from.

## Decision

External Notes carry two independent identities:

- portable source-object key: `(source_scope, source_identity)`;
- producer provenance key: `(instance_id, field_id)`.

`source_scope` is connector-namespaced, stable across Fieldnotes instances,
non-secret, and based on upstream authority/account identifiers rather than the
user's Field label. `source_identity` is stable within that scope and includes
an object-kind namespace when required to prevent collisions.

A proposed flat `collected_by: list[text]` property preserves additional
`<instance_id>/<field_id>` producer references after exact merge deduplication.
The original required `instance_id` and `field_id` remain the Note's first or
surviving producer pair.

Within a notebook, a source update reconciles the Note for the same portable
source key in place and preserves the Note ID while that Note remains present.
The replacement is atomic. A source deletion may remove the Note. There is no
required revision or tombstone ledger. Refetching a previously removed object
may create a new Note ID.

On merge:

- matching source key plus matching semantic content collapses to one current
  Note while producer provenance is unioned;
- a reliably newer source version may replace an older one;
- unresolved divergent current content is preserved as a visible conflict;
- matching content without a matching source key does not collapse contextual
  Notes.

## Consequences

- Every external connector must document how it derives stable source scope
  and identity.
- The Field protocol must carry both values for external records.
- Current notebooks can deduplicate exact upstream objects across instances.
- A merge can preserve collection provenance without nested frontmatter.
- Content hashes remain useful for artifact/content reuse but are not identity.
- Conflict handling is required even though revision history is not.

