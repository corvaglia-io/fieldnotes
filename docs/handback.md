# Handback packages

**Status:** Planned for `0.1.7`; package schema requires approval before implementation.

## Purpose

Fieldnotes exists between the places where work happens and the systems where processed work belongs. Collection is only the first half of that loop. After a human or AI reviews a working notebook, Fieldnotes should help prepare selected material for **handback** to a durable system such as a second brain, knowledge base, CRM, ticket system, task manager, or time tracker.

A handback package is a portable, reviewable preparation bundle. It is not an API request and Fieldnotes does not submit it to the destination.

## Boundary

A package may gather:

- selected Notes and artifact references;
- relevant entities and relationships;
- optional Extractions and Observations;
- human or AI-authored proposals;
- destination hints supplied by the user;
- a manifest that lets an external consumer verify what was included.

An external human or AI translates that package into the destination's concepts and decides whether to write anything. Destination credentials and write APIs remain outside Fieldnotes.

## Lifecycle

```text
collect -> inspect -> process -> prepare package -> hand back externally -> discard
```

Packages are working output and may be recreated. Fieldnotes does not need an immutable record proving that a package was delivered or applied. A destination system remains authoritative for accepted changes.

## v0.1 delivery

The earlier v0.1 increments first make proposals and selected records easy to gather deterministically. `0.1.7` then introduces an explicit package command and portable manifest after the following are approved:

- selection semantics and dependency closure;
- whether artifacts are copied or referenced;
- treatment of sensitive material;
- size and retention limits;
- destination hints without vendor payload schemas;
- a clear distinction between package preparation and delivery.

Until that contract is approved, ordinary directories and Markdown documents remain a valid manual handback format. Package delivery, destination authentication, and destination writes remain outside the v0.1 contract.
