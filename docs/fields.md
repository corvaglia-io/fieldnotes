# Fields

**Status:** Product boundaries are normative for the 0.1 line. Exact Field type
names, executable names, capability manifests, configuration keys, and wire
schemas are proposed until their approval milestones pass.

## Purpose

A **Field** is a stable producer of Notes. External Fields are independently
configured, read-only source adapters. The reserved `self` Field records text,
files, and voice recordings supplied directly by the user.

Fields collect current working context. They do not create a permanent event
history and do not write back to their sources. A connected source remains
authoritative and may be refetched when it is still available.

The selected 0.1 Field set is:

1. `self`;
2. local reference material;
3. Outlook Mail;
4. Outlook Calendar;
5. Outlook Contacts;
6. Microsoft Teams;
7. Jira.

GitHub is postponed until after the 0.1 line.

## Ownership boundary

An external Field owns:

- source authentication-flow integration;
- bounded, incremental reads;
- pagination, throttling, retries, and source-specific backfill behavior;
- stable source identity and timestamps;
- mapping reliable source values into shared or prefixed properties;
- source-declared identity anchors;
- reporting unavailable, truncated, damaged, or skipped content.

Fieldnotes core owns:

- the only durable writes to the notebook;
- global record IDs and filenames;
- validation and canonical serialization;
- portable source-key reconciliation;
- atomic Note and artifact replacement;
- cursor commits after preceding changes are durable;
- artifact hashing and byte deduplication;
- deterministic entity and relationship derivation;
- credential-leak prevention.

A Field must never write notebook files or call a source write API. Process
isolation separates vendor dependencies from core; it is not a security
sandbox for untrusted executables.

## Field identity

Each configured Field has a stable `field_id`. The ID is unique within one
Fieldnotes instance and immutable after the Field has produced Notes. The
producer key is:

```text
(instance_id, field_id)
```

That key records who collected a Note. It is not the portable identity of the
upstream object.

### Proposed ID grammar

The proposed external ID form is:

```text
<registered-field-stem>_<user-label>
```

Both parts use lowercase ASCII letters, digits, and underscores. `self` is the
only reserved single-part ID. Labels should be short and stable, such as
`work`, `private`, or `acme`.

The following names are a proposal for fixture and CLI review, not yet an
approved public registry:

| Field | Proposed configured ID | Proposed property prefix |
|---|---|---|
| built-in self | `self` | shared properties only |
| local reference | `local_work` | `local_` |
| Outlook Mail | `outlook_mail_work` | `outlook_mail_` |
| Outlook Calendar | `outlook_calendar_work` | `outlook_calendar_` |
| Outlook Contacts | `outlook_contacts_work` | `outlook_contacts_` |
| Microsoft Teams | `teams_work` | `teams_` |
| Jira | `jira_acme` | `jira_` |

The final stems and prefixes must be approved with the property registry and
golden notebook fixtures. Renaming a display name, account address, tenant
name, or machine must not rename an established Field ID.

## Portable source identity and reconciliation

Every external Note for which the source supplies stable identity contains:

```yaml
source_scope: "microsoft-graph:tenant/8d82..."
source_identity: "mail-message/AAMk..."
```

The exact portable source-object key is:

```text
(source_scope, source_identity)
```

`source_scope` is a connector-namespaced, non-secret authority, tenant,
account, or installation scope. `source_identity` is stable within that scope
and includes an object-kind namespace when the source does not guarantee that
raw IDs are unique across kinds.

Reconciliation follows these rules:

- the same portable source key denotes the same upstream object across
  Fieldnotes instances;
- matching current content collapses to one current Note while all known
  producer provenance is retained;
- demonstrably newer source state may atomically replace older state under the
  same Note ID;
- ambiguous divergent state becomes a visible conflict;
- a content hash, display name, timestamp, subject, or similarity score never
  substitutes for the portable source key or discards distinct context;
- content-hash equality may deduplicate artifact bytes without collapsing
  Notes from different source objects.

Fieldnotes represents current collected state, not an append-only history
ledger. It does not retain every prior form of an edited source object. It
removes a Note for a source deletion only after an authoritative tombstone or
an authoritative complete-snapshot reconciliation. Absence from a time window,
filtered query, partial page set, unavailable history, or failed sync is not a
deletion signal.

Losing a cursor may require a refetch or bounded backfill followed by the same
portable-key reconciliation. A Field that cannot refetch must report that
limitation; Fieldnotes cannot promise recovery the source does not provide.

## Shared and source-specific properties

A Field uses shared Fieldnotes properties when their meaning matches exactly,
for example `from`, `participants`, `occurred_at`, `source_url`, and
`attachments`. A source-only concept uses the Field's registered prefix.

```yaml
from: alice@example.com
participants:
  - alice@example.com
  - joe@example.net
teams_chat_id: "19:abc..."
teams_message_type: message
```

A Field must not introduce arbitrary unprefixed properties. Every property is
flat, has one stable scalar or list type across the notebook, and is bounded.
Vendor response objects and secrets never enter frontmatter.

## Configuration

Field configuration is private application state under `.fieldnotes/fields/`.
It is not constrained by the flat public-frontmatter format, but it must not
contain credential values.

A proposed configuration shape is:

```yaml
id: outlook_mail_work
driver: outlook-mail
display_name: Work mail
account: joe@example.net
credential_profile: microsoft_work
```

Configuration may contain non-secret source scope, collection bounds, feature
switches, and references to named credentials. Stable IDs and source-scope
settings that affect reconciliation require an explicit migration rather than
a silent rewrite.

Removing or disabling a Field must not write to the source. The exact CLI
behavior for retaining its Notes, cursor, and credential reference remains a
command-contract decision; destructive cleanup must be explicit.

## Authentication and credentials

`self` and the ordinary local reference Field require no source credential.
Remote Fields refer to a named credential profile rather than embedding a
token, password, key, cookie, or client secret.

The 0.1 default providers are the operating system's secure stores:

- macOS Keychain;
- Windows Credential Manager with DPAPI-backed storage;
- Linux Secret Service when available.

An explicit environment-variable provider supports CI and approved headless
workflows. Plaintext storage is never a silent fallback.

Long-lived credentials stay in the selected provider. Access tokens should
remain in memory. Credentials reach a Field through an approved protected
channel, never command-line arguments, notebook files, cursor values, normal
diagnostics, or protocol event payloads. Exact auth operations and protected
IPC require protocol approval.

Outlook Mail, Outlook Calendar, Outlook Contacts, and Teams may share Microsoft
transport and authentication implementation. They remain separately configured
Fields with distinct IDs, prefixes, capabilities, cursors, and status.

## Process contract

External Fields run as short-lived child processes for bounded operations. The
proposed protocol has:

- a versioned `describe` operation;
- one collection request containing the configured Field ID, an optional
  opaque cursor, optional collection bounds, and non-secret configuration;
- ordered `record`, `checkpoint`, and `diagnostic` events;
- logs on standard error;
- a non-zero exit for a run that did not complete normally.

Core commits a checkpoint only after every preceding Note and artifact change
is durable. Replaying from the prior checkpoint must be safe.

The exact JSON Schemas, protocol negotiation, executable discovery, artifact
transfer, cancellation behavior, resource limits, exit codes, and manifest
shape remain proposed until approval milestone A2. Connector implementations
must use the shared conformance kit rather than independently interpreting the
protocol.

### Proposed manifest concepts

A manifest needs to state at least:

- protocol version;
- registered Field type and property prefix;
- supported source-object capability slices;
- authentication kind;
- source-identity scope rules;
- available incremental, tombstone, snapshot, and refetch behavior;
- known permission or history limitations.

The capability list documents what a release actually supports. It is not a
claim that a Field covers every object or API offered by its vendor.

## Initial capability slices by release

The object names below describe the intended product slice. Exact manifest
tokens and mapping details remain proposed until their fixtures are approved.

### 0.1.0 — `self`

- typed text Notes;
- copied file imports;
- imported common voice recordings as playable Notes;
- optional user-supplied occurrence time and title/body;
- no external process and no credential.

Voice transcription is not required until 0.1.8.

### 0.1.1 — local reference

- a bounded configured directory or approved NDJSON fixture source;
- source files collected without modification;
- stable relative-path/source identity and current-state reconciliation;
- explicit damage, permission, and unsupported-format diagnostics;
- the reference implementation of the external Field process contract.

The approved manifest must distinguish authoritative snapshots from partial
directory or query results before absence may remove a Note.

### 0.1.3 — Outlook Mail

- messages, senders, recipients, participants, and conversations;
- source URLs and attachments where available;
- stable Microsoft identity anchors;
- pagination, throttling, token refresh, and incremental refetch;
- explicit reporting of unavailable or truncated content.

No mail send, move, delete, flag, category, or other mailbox mutation is in
scope.

### 0.1.4 — Outlook Calendar

- events and recurrence instances;
- organizer and participants;
- start/end intervals and source identity;
- deterministic links to related Notes when exact source evidence exists.

No event creation, response, cancellation, or attendance change is in scope.

### 0.1.4 — Outlook Contacts

- names, email addresses, phone numbers, organizations, and source IDs as
  explicit source evidence;
- independently configured collection and cursor state;
- identity-resolution input without name-only automatic merges.

No contact creation, update, merge, or deletion is in scope.

### 0.1.5 — Microsoft Teams

- chats and approved channel-message slices;
- replies, participants, and conversation identifiers;
- meeting references and attachments available through the approved API scope;
- source-driven thread links and exact links to related Calendar Notes;
- explicit permission, admin-consent, unavailable-history, partial-content,
  throttling, and pagination diagnostics.

Unsupported Teams history must be reported rather than inferred or simulated.

### 0.1.6 — Jira

- issues and comments;
- participants and project identity;
- source status and source priority as explicit values;
- ticket references, source URLs, and current-state issue reconciliation;
- cloud pagination, throttling, retry, and permission diagnostics.

Fieldnotes may preserve an explicit Jira priority. It does not assign priority
or perform issue transitions, edits, comments, or other Jira writes.

## Conformance requirements

Every external first-party Field must pass common fixtures for:

- manifest and protocol-version handling;
- restart and replay at checkpoint boundaries;
- duplicate, changed, and authoritatively deleted source objects;
- partial-result protection against false deletion;
- malformed or oversized output and child-process failure;
- property-prefix and stable-type enforcement;
- portable source-scope stability across two Fieldnotes instances;
- secret canaries in logs, errors, URLs, cursors, and process arguments;
- actionable permission, throttling, truncation, and refetch diagnostics.

The conformance suite protects common boundaries. Each Field also needs
vendor-specific recorded or sanitized fixtures for its approved capability
slice.

## Processing and handback boundary

Collected Fields may feed deterministic graphs, proposals, and a package
prepared for downstream review. Package preparation may gather selected Notes,
artifacts, entities, relationships, conflicts, and source references. It does
not call a destination API, authenticate to a destination for write access, or
claim that anything was delivered or applied.

No Field is a backdoor write connector. Package delivery and every durable
external action remain the responsibility of a human or external AI.

