# Multi-Agent Delivery and Integration Plan

**Status:** Draft for approval  
**Purpose:** Coordinate a horde of bounded implementation agents while keeping the notebook and Field contracts coherent

## Operating principle

Parallelism is useful only after shared contracts are explicit. Agents may investigate risks in parallel before approval, but they must not independently invent durable IDs, properties, Markdown shapes, protocol events, credential flows, or deduplication rules.

The coordinator owns the critical path and integration order. Specialist agents own bounded work packages with declared files, inputs, outputs, tests, and stop conditions. One agent writes a shared file at a time.

## Roles

### Coordinator/integrator

The coordinator is accountable for the delivered product rather than merely dispatching tasks. Responsibilities are:

- maintain the acceptance-criteria traceability matrix and current critical path;
- prepare the A0, A1, and A2 user approval packages;
- freeze approved contracts and announce their versions;
- assign one owner per file or subsystem for each wave;
- prevent overlapping edits and resolve integration order before agents begin;
- give every agent the relevant specification sections, approved fixtures, dependencies, and explicit non-goals;
- review agent results, run integration tests, and reject work that passes only isolated tests;
- serialize changes to workspace manifests, shared registries, protocol schemas, and golden fixtures;
- record decisions and migrations rather than allowing undocumented convention drift;
- maintain a risk register for Microsoft permissions, credentials, cross-platform behavior, artifact parsing, enhancement packaging, and live-account test access;
- report blockers that require user authority instead of letting agents assume product choices;
- declare each release gate passed only from reproducible evidence.

The coordinator must not ask connector agents to compensate for an unstable core contract. If several agents discover the same contract defect, pause their work, revise the contract centrally, rerun its approval gate when necessary, and then resume from a new version.

### Contract steward

Owns proposed durable contracts until approval:

- IDs, filenames, datetimes, Note types, property registry, prefixes;
- exact Markdown/YAML fixtures;
- Field protocol JSON Schemas and examples;
- normalization and content-hash versioning;
- current-state reconciliation and exact-dedup rules.

After approval, this role reviews changes but connector agents do not edit these files directly.

### Workstream owners

Each workstream owner implements one bounded subsystem and its tests. An owner may spawn short-lived subagents for research or fixture generation, but remains responsible for integrating their output and respecting file ownership.

### Verification agent

An independent agent reviews gate evidence, attempts failure cases, and checks specification traceability. It does not rewrite the implementation while reviewing it; findings return to the owning agent or coordinator.

## Workstreams

| ID | Workstream | Primary outputs | Depends on | Must not own |
|---|---|---|---|---|
| W0 | Scaffold and build | workspace tree, manifests, CI skeleton, toolchain policy | A0 approval | product contracts |
| W1 | Notebook core | IDs, parser/validator, renderer, atomic writer, `self`, artifacts | A1 approval, W0 | vendor mapping |
| W2 | Field protocol | schemas, child-process host, SDK, fake Field, conformance kit | A2 approval, W0 | live connector behavior |
| W3 | CLI/config | commands, config loading, stable human/JSON output | W1, W2 as applicable | core invariants |
| W4 | Local Field | local/NDJSON collection, cursor, current-state fixtures | W2, W1 writer API | protocol changes |
| W5 | Graph and merge | scan/rebuild, identities, entities, relationships, portable-source dedup, exact merge/conflicts | W1/A1 | connector auth |
| W6 | Credentials | provider abstraction, OS stores, env provider, protected IPC, redaction | W2 | connector mapping |
| W7 | Microsoft shared | Graph client, OAuth/token refresh, pagination, throttling, fixtures | W6, W2 | notebook writer |
| W8 | Outlook Mail | mail mapping, attachments, conversations, identities | W7, W2 | shared registry changes |
| W9 | Calendar/Contacts | two distinct Field mappings and conformance suites | W7, W5 identity rules | Teams/Jira |
| W10 | Teams | permission preflight, messages/replies/meetings, partial-data diagnostics | W7, Outlook/Calendar evidence | unsupported history workarounds |
| W11 | Jira | auth/client, issues/comments/mapping, conformance fixtures | W6, W2 | Microsoft code |
| W12 | Lifecycle/artifacts | renderers, damage reporting, archive/prune/reference safety | W1, W5 | enhancement |
| W13 | Proposals/handback/Obsidian | proposal boundary, reviewed-state policy, package preparation, Bases, notebook guidance | W5, A1 | write-back APIs |
| W14 | Enhancement | pinned engine package, Extraction/Observation validators and evaluation | W1, W5, W12 | canonical Note mutation or BYO models |
| W15 | Platform/release | six targets, packaging, security/license/performance tests, docs closure | all release workstreams | feature invention |

## Dependency shape

```text
A0 ─→ W0 ─→ A1 ─┬─→ W1 ─→ W3 ─→ W5 ─→ W13
                 │    └──────────→ W12 ─→ W14
                 └─→ A2 ─→ W2 ─→ W4
                            └─→ W6 ─→ W7 ─→ W8 ─→ W9 ─→ W10
                                 └────────────────→ W11

W1, W2, W4…W14 ─→ W15
```

The diagram is logical, not a command to serialize all work. After A1 and A2, W1 and W2 can proceed in parallel. W5 can begin against approved fixtures before live connectors are ready. W11 can run in parallel with later Microsoft Fields after credentials and protocol are stable.

## Ownership rules

1. Every task brief declares its writable paths. Unlisted files are read-only to that agent.
2. The coordinator is the sole writer for root workspace manifests, shared property registries, protocol schemas, golden fixtures, release notes, and the acceptance matrix unless it explicitly delegates temporary ownership.
3. One writer owns a connector directory during an active task. Shared Microsoft code and individual Field mappings have separate owners.
4. Connector agents consume the core writer API; they never write notebook files directly and never weaken validation to accommodate vendor payloads.
5. Connector agents cannot add unprefixed properties. They propose registry additions to the contract steward with type, semantics, examples, and collision analysis.
6. Derived graph/enhancement agents cannot mutate canonical Notes. Their tests must compare Notes before and after rebuild byte for byte where no current-source sync occurred.
7. Agents preserve unrelated workspace changes. They do not reset, delete, or reformat files outside their assignment.
8. Test fixtures must be sanitized, deterministic, reviewable, and free of credentials or proprietary live data.
9. Generated dependencies, model assets, and large binary fixtures require coordinator approval before entering the repository.
10. A completed agent reports changed files, test commands/results, assumptions, unresolved risks, and the next dependency it unblocks.

## Waves of work

### Wave 0 — Discovery and approval preparation

Run bounded research agents in parallel for:

- Rust workspace/crate alternatives;
- Obsidian datetime and property fixture behavior;
- Field protocol failure/checkpoint cases;
- Microsoft Graph permissions and test-tenant requirements;
- cross-platform credential-store constraints;
- enhancement model/licensing/CPU feasibility.

These agents produce decision inputs, not implementation. The coordinator then presents A0, A1, and A2 in order. A later approval package may be prepared while an earlier one is under review, but no durable contract implementation proceeds past its unapproved gate.

### Wave 1 — Kernel and protocol

Parallel assignments:

- W1 notebook core and `self` Field;
- W2 protocol host, SDK, fake Field, and conformance harness;
- W3 CLI/config skeleton against stable interfaces;
- verification agent for golden fixtures, atomicity, and malicious protocol cases.

Integration order: core types and validation, atomic writer, CLI surface, protocol host, fake Field. The local Field begins only after that vertical path passes.

### Wave 2 — Offline vertical slice and deterministic graph

Parallel assignments:

- W4 local Field and checkpoint/reconciliation fixtures;
- W5 graph/rebuild/merge using approved notebooks;
- W6 credential abstraction and OS-specific spikes;
- platform agent for Windows path/atomic-rename tests.

Integration order: local Field through the real child-process host, then graph scan/rebuild, then merge. Credentials may land independently because the local Field does not need them.

### Wave 3 — First live Field

Parallel assignments:

- W7 Microsoft shared auth/client;
- W8 Outlook Mail mapper and recorded fixtures;
- verification agent for secret leakage, rate limits, pagination, source updates, and checkpoint resumption;
- release agent for the three-OS credential test matrix.

Outlook is not integrated until the same executable passes the generic conformance kit and its vendor-specific fixture tests.

### Wave 4 — Microsoft breadth

Parallel assignments:

- Calendar owner;
- Contacts owner;
- Teams permission/API spike while Calendar and Contacts implement;
- graph owner extends only approved deterministic relationships needed by these Fields.

Calendar and Contacts integrate separately. Teams implementation starts after its spike confirms an approved, testable API/permission scope. Missing access is a diagnostic requirement, not permission to scrape or infer unavailable content.

### Wave 5 — Jira and product lifecycle

Parallel assignments:

- W11 Jira;
- W12 artifact render/lifecycle;
- W13 proposals/Obsidian;
- independent merge/prune safety verification.

Jira is the proof that protocol, credentials, property prefixes, and graph evidence are not Microsoft-specific. Any Jira-driven core redesign is treated as a contract review, not hidden in the connector.

### Wave 6 — Optional enhancement

Parallel assignments after deterministic mode is release-quality:

- speech-to-text packaging and timecode validation;
- text Extraction/span validation;
- Observation aggregation/evidence validation;
- evaluation, licensing, and platform packaging.

The coordinator integrates validators before any model runner. Candidate model output is rejected unless it satisfies deterministic evidence contracts. Enhancement never blocks the usability of earlier deterministic releases.

### Wave 7 — Release closure

Parallel assignments:

- six-target packaging;
- security, dependency, and license audit;
- large-corpus/performance and interruption soak tests;
- documentation and acceptance-traceability review;
- independent end-to-end release-candidate verification.

No new feature work begins in this wave without reopening scope explicitly.

## Approval and integration gates

### User approvals

| Gate | Decision | Work blocked until approval |
|---|---|---|
| A0 | repository/folder/crate scaffold | W0 implementation and path ownership |
| A1 | IDs, types, properties, naming, datetime, hashes, exact Markdown fixtures | canonical writer, graph parser, all mappings |
| A2 | Field protocol/schema, credential transport, identity scope, exits/checkpoints | local and live Field implementations |

Each package presents a recommended choice, alternatives, consequences, and exact examples. Silence is not approval.

### Integration gates

| Gate | Required evidence |
|---|---|
| IG1 Notebook | golden fixtures; invalid YAML/property tests; atomic interruption tests; artifact dedup |
| IG2 Protocol | fake/local Fields; schema conformance; failure/checkpoint matrix; stderr/secret isolation |
| IG3 Current state | changed source object keeps stable Note ID; atomic replacement; authoritative deletion only; partial results never delete; no revision/tombstone ledger; deleted cache refetches cleanly |
| IG4 Graph/merge | deterministic rebuild; exact same-Note-ID and portable-source-key dedup; producer provenance union; hash-only non-collapse; conflict preservation |
| IG5 Credentials/live API | three-OS stores; protected IPC; revoked-token behavior; pagination/rate-limit/resume; no secret leakage |
| IG6 Connector | common conformance suite plus connector fixture suite and property-registry review |
| IG7 Lifecycle | archive round-trip; prune reference safety; disposable cache/graph proof; proposal reviewed-state preservation; portable secret-free handback package fixture |
| IG8 Enhancement | default-off/no download; span/time validation; unsupported-judgment rejection; Note immutability; pinned rebuild provenance |
| IG9 Release | six targets; install/upgrade; licenses/security; realistic end-to-end corpus; acceptance traceability |

An integration gate is rerun whenever a shared dependency changes. Passing a connector gate against protocol version 1 does not cover an unreviewed protocol version 2.

## Exact deduplication and current-state test matrix

All core, connector, graph, and merge owners share these cases:

| Scenario | Required result |
|---|---|
| same run emits same scoped source identity twice | one current Note; deterministic diagnostic if payloads conflict |
| later sync emits changed payload for same scoped source identity | same Note ID, atomically updated current content, no revision Note |
| copied notebook contains same Note ID and same bytes | one Note after reconciliation |
| copied notebook contains same Note ID and different bytes | preserve both payloads as an explained conflict; no silent winner |
| two instances independently collect the same portable source-object key and matching current content | keep one current Note and union both producer references |
| two instances collect the same portable source-object key with demonstrably ordered source versions | keep the newer current state and union both producer references |
| two instances collect the same portable source-object key with divergent, unordered state | preserve both inputs as an explained merge conflict until resolved |
| two Notes have the same content hash but different source contexts | preserve both Notes; relate as identical content if useful |
| two records share display name/timestamp/subject | preserve both; similarity is not deduplication evidence |
| an authoritative tombstone or complete snapshot proves source deletion | remove the current Note safely; create no revision or tombstone Note |
| a source object is absent from a partial page/window or failed sync | retain the Note; absence is not deletion evidence |
| cache, graph, Extractions, and Observations are deleted | canonical Notes remain useful; deterministic layers rebuild; remote current state can be refetched when authorized |

## Agent task contract

Every dispatched task should contain:

```text
Objective:
Release/gate:
Writable paths:
Read-only dependencies:
Approved contracts/fixture versions:
Required behavior:
Explicit non-goals:
Required tests and commands:
Expected handoff:
Stop/escalate conditions:
```

Tasks should be sized to produce one reviewable result in one agent turn where practical. Examples of good bounded tasks are “implement checkpoint persistence and its crash matrix” or “map Outlook recipient fields using protocol v1 fixtures.” “Implement Outlook” is too broad for one owner without decomposition.

## Stop and escalation conditions

An agent stops and reports to the coordinator when:

- an approved fixture, schema, or registry cannot express required source evidence;
- completing the task requires editing another owner's active files;
- a source API requires broader permissions, write access, scraping, or an unapproved commercial license;
- test data cannot be sanitized or legally retained;
- an operation would make a cache or derived file the only copy of evidence;
- deduplication would rely on a content hash or heuristic rather than exact identity;
- current-state reconciliation would require creating an implicit revision ledger;
- a credential could enter argv, logs, diagnostics, cursors, fixtures, or the notebook;
- enhancement output cannot cite and validate its evidence;
- platform behavior would violate atomicity, portability, or the approved filename contract.

## Definition of a successful handoff

A workstream handoff is ready for integration when:

- it changes only its declared files;
- formatting, unit, fixture, conformance, and relevant platform tests pass;
- public behavior is documented at the appropriate level;
- errors and diagnostics are actionable and sanitized;
- no canonical or shared contract changed implicitly;
- its report identifies assumptions, live-account coverage, untested conditions, and known risks;
- the coordinator can reproduce the result with listed commands;
- the next dependent workstream can consume a stable interface without reading private implementation details.

The coordinator then runs the release's integration gate and either accepts the handoff, returns focused defects to the owner, or reopens a user approval milestone when the evidence requires a product-level decision.
