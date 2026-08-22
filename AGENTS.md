# Fieldnotes agent instructions

These instructions apply to the entire repository.

## Product invariants

- Treat the notebook as disposable working material, not an immutable ledger.
- Preserve attribution and stable source identity, but do not add event-sourcing or revision history unless an approved decision changes the product.
- Source-backed Notes represent current collected state and may be replaced or removed during reconciliation.
- Keep source systems read-only. Fieldnotes prepares material for handback; it does not perform destination writes.
- Keep Notes useful without enhancement. Extractions and Observations are optional, separate, and rebuildable.
- Keep frontmatter flat: scalars and one-dimensional scalar lists only.
- Never write credentials or tokens into notebooks, diagnostics, fixtures, or process arguments.
- Cross-instance deduplication must use portable source scope and source identity while retaining producer provenance.

## Coordination

- The root coordinator owns workspace manifests, shared schemas, protocol versions, vocabulary, and integration gates.
- Do not change an approved shared contract from a feature branch or connector workstream. Propose an ADR and route it through the coordinator.
- Keep connector changes inside their assigned Field and shared connector libraries unless the task explicitly includes a core change.
- Prefer recorded, sanitized fixtures over live-account dependencies in ordinary tests.
- Do not begin feature implementation until the repository-layout and notebook-contract approval gates in `docs/roadmap.md` are complete.

## Engineering quality

- Add tests with behavior changes.
- Make filesystem operations replay-safe and crash-aware.
- Keep deterministic output ordered and inject clocks/ID generators in tests.
- Treat external content and Field process output as untrusted input.
- Keep the default build free of model downloads and GPU requirements.

