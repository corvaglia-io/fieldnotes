# ADR 0001: Current-state notebook and state classes

- **Status:** Accepted for v0.1 documentation
- **Date:** 2026-08-22

## Context

Fieldnotes calls notebook files canonical, while also describing the notebook as
disposable and source-backed. Opaque cursors were previously shown under a
disposable cache, and some generated files can contain human review state. That
mix makes backup, rebuild, and deletion guarantees ambiguous.

## Decision

Fieldnotes v0.1 is a current-state notebook, not an append-only archive.
Canonical means the authoritative public file representation at a point in
time; it does not mean irreplaceable or immutable.

There are four state classes:

1. **Public notebook state:** portable Notes, artifacts, and generated Markdown
   records. Collected Notes may be reconciled, removed, pruned, or refetched.
2. **Durable local intent/configuration:** instance and Field identity, user
   aliases and merge decisions, review state, and retention policy. It is not a
   cache and contains credential references rather than secrets.
3. **Operational synchronization state:** opaque cursors and checkpoint/backoff
   metadata under `.fieldnotes/state/`. Loss triggers refetch/backfill; this
   state is not falsely described as reconstructable.
4. **Disposable caches:** indexes and acceleration stores under
   `.fieldnotes/cache/`, always safe to delete and rebuild.

v0.1 does not require a revision ledger or tombstone history. Human decisions
must be stored separately from rebuildable generated projections.

## Consequences

- A source update can replace the current Note without retaining its old body.
- A source deletion can remove the collected Note.
- Refetch is recovery where the source supports it.
- Losing an opaque cursor may be operationally expensive or incomplete if the
  source cannot backfill; the connector must communicate that limitation.
- Backup guidance can distinguish portable evidence from behavior/review state.
- Rebuilding generated Markdown must not erase human decisions.

