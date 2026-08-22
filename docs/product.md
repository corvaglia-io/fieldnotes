# Fieldnotes product

## Product promise

Fieldnotes is a local-first context collector for humans and AI agents. It reads
the places where work happens, accepts material directly from the user, and
turns both into a coherent, portable Markdown notebook.

The notebook helps a person or an external AI understand recent work, process
it, and hand durable outcomes back to the systems where they belong. Fieldnotes
stops before that final write. It is working material between source systems and
systems of record, not another system of record itself.

```text
Fields and user input
        |
        v
current working notes and artifacts
        |
        v
deterministic identities, entities, and relationships
        |
        v
optional evidence-backed enhancement
        |
        v
human or external AI processing
        |
        v
durable handback to the appropriate external system
```

## The working-notebook model

A working day leaves useful traces in mail, calendars, chats, contacts,
tickets, documents, calls, and repositories. Fieldnotes calls these source
systems **Fields**. Each configured Field is a stable, read-only producer of
**Notes**. The reserved `self` Field records notes and artifacts supplied
directly by the user.

Notes are readable snapshots of useful source state. They preserve enough
identity, time, content, and provenance to remain understandable after copying
or merging notebooks. They are not immutable events, a compliance archive, or
an evidence ledger. When a connected source remains available, its current
state can be fetched again; the source remains authoritative.

The notebook itself is intentionally disposable. A normal lifecycle is:

```text
collect -> inspect -> process -> hand back -> prune, archive, or discard
```

This does not make careful storage unimportant. While a notebook exists,
Fieldnotes writes it safely, prevents accidental duplication, keeps credentials
out of it, and makes derived state rebuildable. It means only that Fieldnotes
does not promise permanent historical retention or treat deletion as data
corruption.

User-created material deserves explicit treatment. Fieldnotes copies imported
artifacts into the notebook rather than depending on an ephemeral input path,
but the user remains responsible for promoting anything that must be retained
permanently before discarding the notebook.

## What the notebook is for

The notebook is designed to work without a Fieldnotes service or executable.
Its Markdown, flat YAML properties, and referenced artifacts support:

- a person browsing in a text editor or Obsidian;
- ordinary filesystem, search, and frontmatter-aware command-line tools;
- an AI agent grounding work in source-derived context;
- a downstream workflow that prepares or applies changes in a CRM, ticket
  system, task manager, knowledge base, time tracker, or another system of
  record.

Processing and handback are central to the product loop even though the write
itself is outside Fieldnotes. Fieldnotes should make it easy to gather the
relevant notes, entities, relationships, conflicts, and source references into
a reviewable package for a person or external AI. The v0.1 line defines a
portable preparation package and manifest. Destination APIs, delivery,
approval workflow, and execution remain outside that contract.

Human-readable, vendor-neutral proposals may be one useful form of prepared
material. They must not be confused with an executable vendor payload or an
authorization for Fieldnotes to update another system.

## Product principles

### Local first

Collection may require access to a remote Field. Storage, inspection, search,
merge, and deterministic rebuild do not require a Fieldnotes account, hosted
service, or required daemon.

### Files remain useful on their own

Markdown notes and retained artifacts are the portable notebook
representation. Configuration and disposable indexes may help reconstruct
derived views, but a reader should not need a proprietary database to
understand a Note.

### Current state over permanent history

For external Fields, Fieldnotes captures useful current-state source material
and can refresh it. v0.1 does not promise an append-only history of every source
edit or deletion. Where a refresh replaces prior source state, the behavior
must be deterministic and visible; it must not create uncontrolled duplicate
Notes.

### Deterministic by default

Inference-disabled mode is the default product, not a degraded demo. Source
IDs, timestamps, participants, reply metadata, identities, hashes, and explicit
links should produce a useful notebook and graph before enhancement is enabled.

### Evidence before interpretation

Optional enhancement may recover explicit source content and form reviewable
observations from cited material. It does not make unsupported psychological or
business judgments, silently rewrite Notes, or act on behalf of the user.

### Portable, sparse vocabulary

Fieldnotes defines a small set of stable property names and types rather than a
large schema. Frontmatter stays flat, missing values are omitted, and
source-specific properties are namespaced by their Field type.

### Explain derived material

Entities, relationships, Extractions, Observations, and prepared proposals must
remain traceable to their source Notes or deterministic rules. Caches and graph
indexes never hold the only copy of a derived claim that the product exposes to
the user.

### Read-only Fields

Fields authenticate and collect incrementally. They do not write back. A human
or external AI decides what should become durable elsewhere and uses the target
system's own interface or integration to do it.

## Core concepts

- **Field:** a configured, read-only source producer; `self` is the built-in
  producer for direct user material.
- **Note:** the primary readable file produced from a Field or user input.
- **Artifact:** retained binary or rendered material referenced by Notes.
- **Identity:** a namespaced handle such as an email address or source account
  ID.
- **Entity:** a derived, evidence-backed grouping of identities representing a
  person, organization, or artifact.
- **Relationship:** a derived connection supported by source metadata, matching
  rules, Extractions, or Observations.
- **Extraction:** optional enhancement that recovers explicit content from one
  Note and cites a text span or audio time range where practical.
- **Observation:** optional enhancement that synthesizes cited Notes or
  Extractions into a reviewable claim.
- **Proposal:** readable preparation for a possible downstream change; it is not
  an external write request or vendor API contract.

## Product boundary

Fieldnotes is not:

- a CRM, contact master, task manager, ticket system, or knowledge base;
- an immutable archive, audit trail, compliance store, or evidence ledger;
- a bidirectional integration or automation platform;
- a hosted synchronization service or account system;
- an arbitrary inference-provider, prompt, or pipeline framework;
- a product that labels sentiment, importance, customer health, relationship
  strength, personality, or intent;
- a guarantee that source material remains available after it disappears from
  the authoritative source or after the working notebook is discarded.

## Product success

Fieldnotes succeeds when users can quickly collect recent working context,
understand it with ordinary tools, process it with a person or AI agent, and
confidently promote the durable result to the right system. It should then be
safe and unsurprising to prune or discard the temporary notebook and repeat the
cycle.
