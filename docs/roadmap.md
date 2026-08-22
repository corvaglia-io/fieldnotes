# Fieldnotes 0.1 Release Roadmap

**Status:** Normative v0.1 delivery plan; contract details remain approval-gated  
**Scope:** The complete v0.1 product and technical specification, delivered as a sequence of coherent `0.1.x` releases

## Delivery model

Fieldnotes will not attempt the entire specification in one integration event. The `0.1.x` line is a release train: every release has a narrow product promise, can be exercised end to end, and leaves the notebook in a format later releases can continue to read.

The first three milestones are explicit user approval gates. Implementation must not silently decide these contracts because they shape every connector and every public notebook file.

1. Approve the repository and folder scaffold.
2. Approve IDs, note types, property names and types, filenames, datetime serialization, and exact Markdown fixtures.
3. Approve the Field process protocol and its exact JSON schemas.

A0 unlocks workspace generation. A1 unlocks the notebook/core work that consumes the public file contract. A2 separately unlocks local and live Field implementations; it may be prepared in parallel with A1 but cannot silently define A1 record vocabulary. A release is complete only after its integration gate passes; completion of isolated component code is not enough.

## Invariants across the release line

These rules apply to every `0.1.x` release:

- Notes and retained artifacts are the canonical representation. Caches, indexes, entities, relationships, Extractions, and Observations are disposable unless a document explicitly identifies user-maintained state.
- Fieldnotes represents the current collected state, not an append-only history ledger. When a source object changes, the matching Note is reconciled and atomically rewritten under the same Note ID. Fieldnotes does not retain source revisions merely because it observed them during earlier syncs.
- Deleting a refetchable notebook or cache is a supported lifecycle action. Fieldnotes must not imply that it is the system of record.
- Collection is read-only. No Field writes to its source system.
- Sync is repeatable. A checkpoint is committed only after all preceding Notes and artifacts are durable.
- Cross-instance deduplication is exact. Copies with the same global Fieldnotes Note ID are the same Note. Independently collected Notes with the same portable `(source_scope, source_identity)` are also the same upstream object: matching current content collapses to one current Note and unions producer provenance. Demonstrably newer source state may replace older state. Ambiguous divergent state becomes a visible conflict. A content hash, display name, timestamp, or other similarity never discards one context.
- A Note is removed because its source disappeared only when the Field receives an authoritative deletion tombstone or completes an authoritative snapshot reconciliation. Absence from a partial window, page, filtered query, or failed sync is never evidence of deletion. Fieldnotes does not retain a synthetic revision or tombstone Note as a history ledger.
- Same-note conflicts and contradictory source metadata are preserved and explained, never resolved by last-writer-wins.
- Notebook frontmatter remains flat, sparse, typed, prefixed where source-specific, and free of credentials.
- Enhancement remains optional and separate. No release before the enhancement milestone may require a model, model download, GPU, or network access for notebook use.
- GitHub is not an initial Field in the 0.1 line. It can be considered after the required Fields and the common connector conformance suite are complete.

## Approval milestone A0: repository scaffold

**Status:** Approved and implemented on 2026-08-22.

The approved tree, crate responsibility table, dependency rules, toolchain policy, alternatives, and verification evidence live in the [A0 repository-scaffold package](approvals/A0-repository-scaffold.md). The core is the only notebook writer; the protocol crate contains no vendor logic; Microsoft Fields share transport code without sharing user-facing property prefixes or importing host credential-store adapters.

**Approval evidence:** proposed tree, crate responsibility table, dependency direction, supported-toolchain policy, and a minimal build/test command.

## Approval milestone A1: public notebook contract

**Status:** Approved on 2026-08-22; IG1 implementation in progress.

Approve exact, byte-visible fixtures before multiple agents generate notebook content. The fixture set must cover:

- instance metadata;
- a `self` text Note;
- a file Note with a content-addressed artifact;
- a voice Note;
- an external message or mail Note;
- a calendar event;
- a contact;
- a ticket;
- a damaged or truncated Note;
- an Extraction, Observation, entity, relationship, and proposal;
- a same-ID merge conflict.

The approval includes:

- ID family and prefixes;
- filename grammar;
- RFC 3339 datetime serialization with an explicit numeric offset, plus UTC filename timestamps;
- initial Note type vocabulary;
- shared property names and stable types;
- connector prefixes;
- list serialization and Markdown body conventions;
- content-hash algorithm and normalization versioning;
- rules for atomically updating the current form of a source object without creating a revision history;
- authoritative source-deletion behavior and protection against treating partial results as deletion.

**Approval evidence:** checked-in candidate Markdown/YAML fixtures and hash
vectors sufficient to review the byte contract. IG1 then turns them into an
executable conformance suite, expands boundary coverage, and proves the parser
and validator accept normative fixtures and reject invalid ones.

## Approval milestone A2: Field protocol

**Status:** Package prepared and ready for review; not approved. See the
[A2 Field protocol package](approvals/A2-field-protocol.md).

Approve the process boundary before implementing live Fields. The approval includes exact JSON Schemas for:

- `describe` manifest;
- collection request;
- `record` event;
- `checkpoint` event;
- `diagnostic` event;
- protocol-version negotiation;
- exit codes and partial-failure behavior;
- credential references and protected secret delivery;
- portable exact-source identity `(source_scope, source_identity)` for
  collapsing independently collected copies of the same upstream object while
  retaining producer provenance;
- separately declared person/account identity anchors that may relate graph
  entities but never substitute for the exact-source key.

The chosen record envelope must be explicit about whether content is a normalized source envelope or a nearly rendered Note candidate. In either case, core remains the final validator and sole durable writer.

**Approval evidence:** schemas, example transcripts, a fake Field, and conformance tests for resumption, duplicate records, malformed output, process failure, log redaction, and crash boundaries around checkpoints.

## Release train

### 0.1.0 — Local notebook kernel

**Product promise:** A user can create and inspect a portable Fieldnotes notebook without any external account.

Deliver:

- `fieldnotes init`, `note`, `status`, and `inspect`;
- stable instance and record IDs;
- the built-in `self` Field;
- text Notes, copied file imports, and common voice-file imports;
- content-addressed artifacts and strong hashes;
- flat-frontmatter validation, filename generation, and atomic writes;
- human-readable and stable JSON CLI output where useful;
- exact fixtures approved at A1.

Independently testable by initializing a temporary notebook, adding text/file/voice Notes, validating the files with generic YAML and Markdown tools, replaying the same import, and opening the result without Fieldnotes.

**Release gate R0:** golden fixtures are stable; interrupted writes leave no valid-looking partial Note; repeated imports reuse artifacts; no secret-like CLI input is persisted accidentally; macOS, Linux, and Windows CI exercise path and rename behavior.

### 0.1.1 — Field protocol and local Field

**Product promise:** Fieldnotes can collect incrementally through the external process contract without requiring a remote service.

Deliver:

- approved A2 protocol and a reusable Field SDK crate;
- `fields add/list/status/remove` and `sync [field_id]`;
- a `local` Field that collects from a bounded directory or NDJSON fixture source;
- durable cursors and checkpoint recovery;
- source-identity reconciliation that updates current state under a stable Note ID;
- authoritative tombstone/snapshot deletion handling without revision or tombstone Notes;
- diagnostic and credential-redaction infrastructure;
- a connector conformance kit used by every later Field;
- an explicit re-collection operation (ADR 0007) that finds Notes carrying
  `skipped_attachments` and asks the owning Field to recollect exactly those
  known source objects via `collect_request.recollect_targets`, re-evaluating
  their attachments against the currently effective retention policy; this is
  distinct from `sync`'s ordinary cursor-forward incremental pass, which never
  revisits settled objects.

The local Field is both useful and the executable reference implementation. It must exercise the same child-process boundary as live Fields.

**Release gate R1:** kill-and-resume tests at every checkpoint boundary; duplicate-source, changed-source, authoritative-deletion, and partial-result tests; malformed child output containment; cursor does not advance past a failed durable write; exact-current-state rewrite does not create history Notes.

### 0.1.2 — Deterministic graph, rebuild, and merge

**Product promise:** A local or copied notebook can be rebuilt into an explainable identity and relationship graph without inference.

Deliver:

- `rebuild`, `graph rebuild`, `entities list/show/candidates`, `explain`, and `gaps`;
- deterministic identity normalization and namespace/scoping rules;
- person, organization, and artifact entities;
- explicit and matched relationships with cited Note evidence;
- thread, participant, first/last-seen, interaction-count, and duplicate-artifact derivation;
- `merge <path>` plus direct-copy reconciliation checks;
- exact same-Note-ID and portable-source-key cross-instance duplicate handling, producer-union metadata, and unresolved conflict preservation;
- disposable graph/index storage rebuilt from canonical files and non-secret configuration.

**Release gate R2:** deleting all caches and derived graph files reproduces the same semantic graph; copying two fixture notebooks together loses no provenance; identical Note IDs deduplicate exactly; divergent content for one Note ID produces a preserved conflict; content-hash equality alone never removes a Note.

### 0.1.3 — Authentication and Outlook Mail

**Product promise:** A user can authenticate a read-only Microsoft account and incrementally collect useful mail Notes.

Deliver:

- `fields auth`;
- the `CredentialProvider` abstraction;
- default Keychain, Credential Manager/DPAPI, and Secret Service integrations;
- an explicit environment-variable provider for CI and headless use;
- protected secret delivery to Fields, never command-line arguments;
- shared Microsoft Graph transport/authentication crate;
- Outlook Mail mapping for messages, participants, conversations, source URLs, attachments, and identity anchors;
- pagination, throttling, token refresh, resumable checkpoints, and sanitized fixtures.

**Release gate R3:** auth-to-sync works on all supported OS families; revoked and expired credentials fail actionably; recorded-fixture tests cover pagination and throttling; source updates reconcile current state; scans find no credentials in argv, logs, diagnostics, cursors, Notes, or artifacts.

### 0.1.4 — Calendar and Contacts

**Product promise:** Microsoft calendar activity and contact records enrich the same deterministic graph while remaining distinct, prefixed Fields.

Deliver:

- separate `calendar` and `contacts` Field manifests, IDs, prefixes, cursors, and mappings;
- shared Microsoft transport/auth code without a combined notebook schema;
- event organizer, participant, interval, recurrence-instance, and source identity handling;
- contact names, addresses, phones, organizations, and source IDs as explicit read-only evidence;
- deterministic cross-Field identity resolution with source scope retained.

**Release gate R4:** one account can configure the Fields independently; disabling/removing one Field does not affect Notes produced by the others; contact evidence improves identity resolution without silently merging name-only candidates; recurring-event fixtures do not duplicate instances.

### 0.1.5 — Microsoft Teams

**Product promise:** Authorized tenants can collect Teams messages and meeting-related evidence through the same notebook contract.

Deliver:

- Teams authentication/permission diagnostics, including admin-consent limitations;
- chats, channel messages, replies, participants, conversations, meeting references, and attachments within the approved API scope;
- explicit truncation or damage reporting when history or content is unavailable;
- source-driven thread reconstruction and links to related calendar Notes;
- throttling, pagination, and refetch/current-state behavior.

Teams scope must follow what supported Microsoft APIs can reliably and lawfully read. Unsupported history must be reported, not simulated.

**Release gate R5:** tenant-permission preflight is actionable; chat/channel/reply fixtures pass the common conformance suite; unavailable or partial content is visibly marked; calendar/Teams overlap remains distinct Notes connected by exact source evidence.

### 0.1.6 — Jira

**Product promise:** Jira issues and their activity can be collected as read-only ticket evidence and connected to people and referenced artifacts.

Deliver:

- Jira authentication and Field configuration;
- issue, comment, participant, project, status, priority-as-explicit-source-value, reference, and source URL mappings;
- cloud pagination/rate-limit/retry behavior and current-state issue reconciliation;
- stable `jira_` properties for source-specific concepts;
- deterministic references between tickets and Notes from other Fields.

**Release gate R6:** Jira passes the common conformance kit; issue edits update the same current-state Note rather than producing a history ledger; explicit source priority is preserved without Fieldnotes assigning priority; cross-Field ticket references remain explainable.

### 0.1.7 — Artifacts, lifecycle, proposals, and Obsidian polish

**Product promise:** A collected notebook can be browsed, processed downstream, prepared as a handback package, safely archived or pruned, and used to review vendor-neutral proposals.

Deliver:

- deterministic text rendering for the approved initial artifact formats, with originals retained according to policy;
- damage/truncation metrics and safe-link normalization where deterministic;
- `archive` and conservative `prune` behavior with shared-artifact reference checks;
- proposal generation/listing and a rebuild-safe boundary for user-reviewed proposal state;
- a vendor-neutral handback-package command and manifest that gathers an approved selection and its required references without calling a destination API;
- default `fieldnotes.base` views;
- complete notebook README conventions for humans and AI agents.

**Release gate R7:** shared artifacts are not pruned while referenced; archives round-trip; corrupted fixture formats surface damage; accepted/rejected proposal state survives graph rebuild by the approved mechanism; a package fixture is complete, portable, secret-free, and contains no executable vendor payload; Obsidian views preserve property types.

### 0.1.8 — Optional built-in enhancement

**Product promise:** A user may explicitly install and enable a pinned built-in engine that creates evidence-backed Extractions and Observations without changing canonical Notes.

Deliver:

- `enhancement status/enable/disable/rebuild`;
- explicit model asset installation or package flow;
- voice transcription with validated time ranges;
- approved text Extractions with exact normalized-body spans;
- bounded Observations with evidence IDs, counts, time ranges, ambiguity, confidence where meaningful, and generator versions;
- deterministic rejection of nonexistent spans, invalid times, missing evidence, wrong property types, and unsupported judgments;
- no provider, endpoint, prompt, or bring-your-own-model surface.

A candidate, not-yet-schema-approved optional capability for this gate: a
PII/maskable-span Extraction that detects likely-sensitive text and points at
it with evidence-backed spans over exact normalized-body offsets, so a user
may choose to mask it, without ever altering the Note. Like every enhancement
capability it must remain optional, outside the default build, and require no
model download, GPU, or network by default — relevant because tools such as
Microsoft Presidio pull Python and spaCy model assets. It is recorded as a
candidate, not approved, in
[ADR 0006](decisions/0006-a1-implementation-rulings.md).

**Release gate R8:** default installation performs no inference or model download; deleting `extractions/` and `observations/` leaves Notes byte-for-byte unchanged; rebuilding uses pinned generator contracts; evaluation fixtures meet approved evidence-precision, language, CPU/memory, packaging, and licensing thresholds.

### 0.1.9 — Full v0.1 hardening and release closure

**Product promise:** The complete v0.1 specification is supported as a native, documented, reproducible release across the promised platforms.

Deliver:

- builds for Windows, Linux, and macOS on x64 and ARM64;
- selected Linux libc baseline and any approved musl artifacts;
- installer/package, upgrade, and Field discovery behavior;
- security review, dependency/license inventory, and reproducible release metadata;
- large-corpus, merge, interruption, rebuild, and connector soak testing;
- completed CLI reference, notebook contract, Field authoring, troubleshooting, security, and architecture documentation;
- a traceability matrix from specification acceptance criteria to tests and release evidence.

**Release gate R9:** all specification acceptance criteria are mapped to passing tests or an explicitly approved scope correction; six release targets build; credential and secret-leak tests pass; notebooks remain useful offline and without Fieldnotes installed; a release candidate survives end-to-end testing with every required Field.

## Field order and rationale

The initial Field order is intentional:

1. `self` proves the canonical notebook without a process or account.
2. `local` proves the external process contract, cursors, and reconciliation offline.
3. Outlook Mail proves authentication, a live API, attachments, messages, and strong identity anchors.
4. Calendar and Contacts reuse Microsoft transport while testing intervals and CRM/contact evidence.
5. Teams follows only after Microsoft auth and Graph behavior are stable; it carries the greatest permissions and availability risk.
6. Jira proves that the contract is not Microsoft-specific.

GitHub is postponed until the required initial set is complete. Adding it later should require only a new conforming Field, fixtures, and property-registry review—not a core or protocol redesign.

## Release and scope controls

- A connector may narrow its supported source objects, but the limitation must be documented and surfaced by `describe`/status diagnostics.
- No release may introduce unapproved shared property names opportunistically. New shared names require registry review and fixtures.
- A release may be delayed by its gate without blocking already completed releases from remaining useful.
- Experimental work should occur behind fixtures or feature flags; experimental files must not enter the canonical notebook contract.
- If implementation evidence invalidates an approved contract, return to the relevant approval milestone with an explicit migration and compatibility proposal.
