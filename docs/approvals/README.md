# Approval gates

Fieldnotes freezes shared contracts before parallel feature implementation.

| Gate | Package | Status | Blocks |
|---|---|---|---|
| A0 | [Repository scaffold](A0-repository-scaffold.md) | Approved and implemented | — |
| A1 | [Notebook schema, naming, registry, and golden Markdown fixtures](A1-notebook-contract.md) | Approved 2026-08-22 | Notebook writer and all mappings |
| A2 | [Field protocol JSON Schemas and conformance transcripts](A2-field-protocol.md) | Approved 2026-08-23 | Local and live Field implementation |
| A3 | [Signals, notes, and type-specific rendering](A3-signals-and-notes.md) | **Ready for review; not approved** | Readable-layer restructure, private signals, the signal-to-note index, Contacts as matching data, and the notebook fixture corpus |

An approval package contains a recommendation, alternatives, consequences, and reviewable examples. Approval is explicit; implementation does not infer it from silence.

A0 through A2 froze contracts before implementation. A3 is the first package proposing to *change* an already-frozen one: it restructures A1's readable layer after a notebook collected from live data showed one record kind cannot be both the machine record and the human-readable artifact. It is pending review, changes nothing until approved, and leaves A2 untouched.

Owner settlements and the implementation handoff live in [A3](A3-signals-and-notes.md) (Settled list + §13). Do not re-read this paragraph as the contract.

Implementing an approved gate can surface contradictions between contract prose and frozen fixture bytes. Those are recorded for a coordinator ruling rather than decided in the implementation: see [A1 implementation findings](A1-implementation-findings.md), [A2 implementation findings](A2-implementation-findings.md), and [A1 graph implementation findings](A1-graph-implementation-findings.md).
