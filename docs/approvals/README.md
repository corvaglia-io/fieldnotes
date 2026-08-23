# Approval gates

Fieldnotes freezes shared contracts before parallel feature implementation.

| Gate | Package | Status | Blocks |
|---|---|---|---|
| A0 | [Repository scaffold](A0-repository-scaffold.md) | Approved and implemented | — |
| A1 | [Notebook schema, naming, registry, and golden Markdown fixtures](A1-notebook-contract.md) | Approved 2026-08-22 | Notebook writer and all mappings |
| A2 | [Field protocol JSON Schemas and conformance transcripts](A2-field-protocol.md) | Approved 2026-08-23 | Local and live Field implementation |

An approval package contains a recommendation, alternatives, consequences, and reviewable examples. Approval is explicit; implementation does not infer it from silence.

Implementing an approved gate can surface contradictions between contract prose and frozen fixture bytes. Those are recorded for a coordinator ruling rather than decided in the implementation: see [A1 implementation findings](A1-implementation-findings.md) and [A2 implementation findings](A2-implementation-findings.md).
