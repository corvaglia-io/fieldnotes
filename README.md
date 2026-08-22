# Fieldnotes

> Notes from where the work actually happens.

Fieldnotes is a local-first context collector for humans and AI agents. It reads configured, read-only **Fields** such as mail, calendars, contacts, Teams, and Jira, and turns their current source material into a portable Markdown notebook. Users can add text, files, and voice recordings through the built-in `self` Field.

The notebook is a disposable working set, not a system of record or an evidence ledger. It is intended to be collected, inspected, enriched, processed, and handed back to durable systems. If a source-backed notebook is lost, Fieldnotes can collect it again when the source and connector still support the required refetch or backfill.

## Status

Fieldnotes is at the v0.1 notebook-contract gate. The approved Rust workspace scaffold compiles, but there is no usable CLI yet.

The repository layout is approved and generated. Record schemas, naming conventions, and normative Markdown examples are reviewed at A1; the exact Field protocol is reviewed separately at A2. The [v0.1 roadmap](docs/roadmap.md) delivers the product incrementally through independently testable releases, with release closure at `0.1.9`.

## Product boundary

Fieldnotes:

- collects current material from read-only Fields;
- writes readable Markdown Notes with flat YAML frontmatter;
- deduplicates source objects across machines without losing producer provenance;
- builds disposable deterministic identity and relationship projections;
- may add rebuildable Extractions and Observations through its bounded built-in enhancement;
- prepares reviewable material for handback to systems of record.

Fieldnotes does not write to source or destination systems, become a CRM, preserve an immutable history, or host arbitrary inference providers.

## Planned v0.1 Fields

- `self`: typed Notes plus file and voice import;
- `local`: reference external Field and deterministic fixture source;
- Outlook Mail;
- Outlook Calendar;
- Outlook Contacts;
- Microsoft Teams;
- Jira.

GitHub is intentionally postponed until after the v0.1 line.

## Design in one minute

```text
Fields and user input
        |
        v
current source Notes + artifacts
        |
        +----> deterministic identities, entities, and relationships
        |
        +----> optional Extractions and Observations
        |
        v
review, processing, and handback package preparation
        |
        v
human or external AI updates durable systems
```

The current A1 proposal uses UTC filenames so a directory listing is a global timeline, while frontmatter timestamps retain an explicit RFC 3339 offset so local/source time remains visible without losing the underlying instant.

## Documentation

- [Product definition](docs/product.md)
- [v0.1 scope](docs/v0.1-scope.md)
- [Architecture proposal](docs/architecture.md)
- [Notebook format and naming proposal](docs/notebook-format.md)
- [Fields and protocol](docs/fields.md)
- [Property and type registry proposal](docs/property-registry.md)
- [Identity and graph](docs/identity-and-graph.md)
- [Security and operations](docs/security.md)
- [Optional enhancement](docs/enhancement.md)
- [Handback packages](docs/handback.md)
- [Release roadmap](docs/roadmap.md)
- [Multi-agent delivery model](docs/agent-coordination.md)
- [Approval gates](docs/approvals/README.md)
- [Architecture decisions](docs/decisions/README.md)
- [Original consolidated specification](fieldnotes-v0.1-spec.md)

The original specification remains in the repository as design input and historical context. Focused documents under `docs/` become normative as their approval gates are completed.

## Development

The implementation is a Rust 2024 workspace producing native command-line applications. The approved crate and Field layout has been generated with placeholder-only crates and binaries. A1 unlocks notebook/core implementation; A2 separately unlocks local and live Field implementation.

The first implementation gate will establish formatting, linting, tests, exact Markdown fixtures, protocol conformance fixtures, and crash/replay behavior before remote connector development fans out.

## License

No license has been selected yet. Until one is added, the repository is not offered under an open-source license.
