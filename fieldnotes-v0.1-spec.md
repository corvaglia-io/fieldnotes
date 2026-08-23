# Fieldnotes v0.1 Product and Technical Specification

**Status:** Draft specification for v0.1  
**Product name:** Fieldnotes  
**Tagline:** Notes from where the work actually happens.  
**Primary implementation:** Rust  

This document consolidates the product and technical decisions made for Fieldnotes v0.1. It is intentionally opinionated about the core boundaries and intentionally conservative about areas that have not yet earned a complex design.

The words **must**, **should**, and **may** indicate required, recommended, and optional behavior for v0.1.

## 1. Product definition

Fieldnotes is a local-first context collector for humans and AI agents.

A working day already leaves traces across email, calendars, Teams, ticket systems, documents, calls, repositories, contacts, CRM systems, and other tools. Those systems are the **fields** where work happens. Fieldnotes reads from configured Fields and lets the user add material directly. Both paths produce coherent file-based **Notes**. Fieldnotes can then build a deterministic graph and, when its optional built-in enhancement is enabled, attach evidence-backed **Extractions** and **Observations**.

The output is a plain Markdown corpus with sparse YAML frontmatter. It is designed to be useful to:

- a human browsing the notebook in a text editor or Obsidian;
- ordinary filesystem and frontmatter-aware command-line tools;
- an AI agent using the notebook as grounded context;
- a downstream workflow that turns evidence into proposals or writes to systems of record.

Fieldnotes is not the final system of record. It is a notebook between the places where work happens and the systems where durable decisions belong.

```text
FIELDS
  ↓
source records
  ↓
deterministic normalization
  ↓
NOTES ← typed, imported, or recorded by the user
  ↓
optional EXTRACTIONS
  ↓
optional OBSERVATIONS
  ↓
identities + entities + relationships
  ↓
local Markdown notebook
  ↓
human or external AI processing
  ↓
CRM / ticketing / tasks / knowledge / time tracking / other systems
```

Fieldnotes stops before the final write.

## 2. Product philosophy and naming

### 2.1 Why “Fieldnotes”

The name combines three useful ideas:

- **Fields** are the source systems where work actually happens.
- **Notes** are readable records of what was observed there.
- A field notebook is working material: capture first, process later, keep only what deserves to become permanent.

The software and CLI use the single word `fieldnotes`, rather than “Field Notes.” The latter is associated with the physical notebook brand; the single word also reads naturally as a package and executable name.

The metaphor is useful, but the CLI must remain literal and unsurprising. Commands such as `fieldnotes sync`, `fieldnotes status`, and `fieldnotes fields add` are preferable to clever metaphorical verbs.

### 2.2 Constitutional boundary

> Fieldnotes preserves evidence and may form explainable observations. It does not make unsupported judgments or act on behalf of the user.

In practical terms:

- Fieldnotes reads source systems; it does not write back to them.
- Notes remain canonical; graphs and indexes are derived.
- Deterministic behavior is the default.
- Optional built-in inference is restricted to evidence-backed Extractions and Observations stored separately from Notes.
- Fieldnotes does not host arbitrary model providers or user-supplied inference pipelines.
- A human or external AI decides what the evidence means and whether another system should be changed.

### 2.3 Evidence before interpretation

Fieldnotes should say:

> This signature contains “Head of Operations” and “Example AG.”

It should not silently turn that into:

> This is a strategically important senior customer contact.

The first statement is recoverable from source evidence. The second contains business and psychological judgment that belongs to a consumer of the notebook.

## 3. Goals for v0.1

Fieldnotes v0.1 should:

1. Collect incrementally from independently configured, read-only external Fields and accept direct user Notes through the built-in `self` Field.
2. Produce portable Markdown notes whose meaning survives without Fieldnotes installed.
3. Make provenance, deduplication, copying, and multi-instance merging reliable.
4. Build the best practical identity and relationship graph using deterministic source data.
5. Allow narrow, optional built-in Extractions and Observations without making inference part of the core Note contract.
6. Remain pleasant to browse in Obsidian, especially through Properties and Bases.
7. Remain efficient for AI agents and generic command-line tools to search and parse.
8. Produce explainable entity, relationship, and proposal material without becoming a CRM.
9. Run as a native Rust application on mainstream desktop and server platforms.
10. Keep every cache, index, and graph reconstruction disposable.

## 4. Design principles

### 4.1 Local first

Fieldnotes must remain useful:

- without a Fieldnotes account;
- without a Fieldnotes server;
- without a required daemon;
- without a proprietary database;
- without Internet access after collection;
- after the Fieldnotes executable has been removed.

Collection from remote sources naturally requires their network access. Storage, inspection, search, and downstream use do not require a Fieldnotes service.

### 4.2 Disposable by design

A notebook follows this lifecycle:

```text
collect → inspect → interpret → process downstream → prune or discard
```

Deleting a completed notebook is not data corruption; it is a supported end state. Long-lived facts should be promoted to the appropriate CRM, ticketing system, knowledge base, task manager, time tracker, or other system of record by the user or an external agent.

Fieldnotes should support retention workflows such as archiving or pruning, but it must never imply that its notebook is the only durable copy of important business state.

### 4.3 Files are canonical

Markdown notes and their referenced artifacts are the canonical notebook representation.

Indexes, search databases, graph stores, and caches may be used for performance, but they must be rebuildable from notebook files and configuration. A cache must never contain the only copy of a claim or relationship.

### 4.4 Vocabulary, not schemas

Fieldnotes defines a small shared vocabulary with stable meanings and property types. It does not require every note type to satisfy a rigid schema.

Missing properties are omitted. They are not padded with `null` values. A note contains only the properties Fieldnotes actually recorded.

### 4.5 Flatness is a feature

YAML frontmatter values must be scalars or one-dimensional lists of scalars. Nested mappings, arrays of objects, and connector-defined object trees are forbidden in notebook frontmatter.

Complex explanations belong in Markdown bodies or in separately addressable evidence notes, not nested YAML.

### 4.6 Useful duplication is allowed

Timestamp, field ID, note type, and note ID appear in both the filename and frontmatter. This small duplication makes each file usable through directory listings, Obsidian, generic YAML tools, and AI context without special filename parsing.

### 4.7 Deterministic quality is a core product goal

Inference-disabled mode is not a degraded demo. It is the default product.

Fieldnotes should make aggressive use of reliable data that sources already provide:

- source object IDs;
- tenant and account IDs;
- email addresses and normalized phone numbers;
- user IDs, account IDs, and handles;
- thread, reply, and conversation metadata;
- participants and organizer fields;
- domains and configured aliases;
- explicit organization or contact links;
- timestamps and source URLs;
- content hashes and attachment relationships.

The graph should be useful before any model is installed or enabled.

### 4.8 The CLI adds capability; it does not gatekeep files

The user must be able to inspect, copy, search, and delete notebook files with ordinary tools. The CLI exists for collection, validation, explanation, graph reconstruction, and lifecycle operations—not to make the corpus proprietary.

## 5. Non-goals

Fieldnotes v0.1 is not:

- a CRM, personal CRM, sales pipeline, or contact master;
- a task manager, time tracker, ticketing system, or knowledge base;
- a bidirectional integration or automation platform;
- an inference orchestration framework;
- a prompt-management or model-provider abstraction;
- a hosted synchronization service;
- an immutable archive or compliance record;
- an event bus or general-purpose ETL framework;
- a graph database product;
- a new query language;
- a system that judges sentiment, relationship strength, customer health, priority, seniority, personality, or intent;
- a guarantee that every old notebook will be migrated forever as the vocabulary evolves.

Fieldnotes may expose evidence from which another tool makes those judgments. It does not make them itself.

## 6. Core product model

### 6.1 Field

A **Field** is a stable producer namespace for Notes. An external Field is a configured, read-only source connector; the reserved built-in `self` Field represents material supplied directly by the user.

Examples include:

- Microsoft Teams;
- Outlook Mail or Calendar;
- Gmail or Google Calendar;
- Jira or another ticket system;
- GitHub;
- Slack;
- local files;
- call logs;
- Outlook Contacts, Twenty, HubSpot, or another contact/CRM system;
- the built-in `self` Field for typed, imported, and voice Notes.

For external Fields, the source remains authoritative. A Field authenticates, reads incrementally, and emits source records plus the mapping needed to create Notes. The built-in `self` Field has no external source: the user's text, selected file, or recording is the source evidence.

### 6.2 Note

A **Note** is the canonical durable unit Fieldnotes writes. It is either collected through a Field or created directly by the user.

Each Note:

- represents one primary type;
- has a globally unique Fieldnotes Note ID;
- identifies the producing Fieldnotes instance and Field;
- preserves source identity and timestamps where available;
- contains sparse, flat properties;
- has a human- and AI-readable Markdown body;
- may link to artifacts, entities, other Notes, Extractions, or Observations.

Cross-source activity is represented by relationships among Notes, not by forcing one Note to carry several primary types.

Source records are transient implementation inputs. They may be noisy, duplicated, incomplete, or wrapped in vendor-specific formats. A Field maps them into Notes; the notebook need not persist each vendor response as a separate public artifact.

### 6.3 Extraction

An **Extraction** is optional inference-assisted structured evidence recovered directly from one Note. It must cite that Note and, for text or audio, the exact source span or time range where practical. It may recover a literal sign-off, language, role, organization name, document reference, or transcript segment. It may not invent missing facts.

An Extraction is stored separately and never rewrites the Note it enhances.

### 6.4 Observation

An **Observation** is an optional inference-assisted interpretation or synthesis supported by one or more Notes or Extractions. Examples include a usual sign-off across repeated interactions, a likely organization affiliation, or an explainable pattern of language use.

An Observation must preserve its evidence, generator version, confidence or ambiguity where relevant, and enough explanation for a human to review it. It is disposable and rebuildable. It never becomes part of the canonical Note merely because a model produced it.

### 6.5 Identity

An **identity** is a matchable handle observed for a person, organization, account, or artifact.

Examples:

```text
email:alice@example.com
phone:+41441234567
microsoft-graph-user:83a7...
entra-user:8d82...
github-user-id:123456
github-login:alice-example
domain:example.com
twenty-person-id:83472
```

Identity namespaces must be explicit. Source-local IDs are qualified by source context and must never be treated as globally unique merely because their string values match.

### 6.6 Entity

An **entity** is Fieldnotes' current, evidence-backed belief that several identities refer to one real-world thing. Initial entity types should remain broad: `person`, `organization`, and `artifact` are sufficient until actual usage requires more.

Entity files are accumulated working notes, not authoritative contact records. They may contain names, observed identities, channels, first/last seen dates, affiliations, and communication conventions supported by notes.

### 6.7 Relationship

A **relationship** is an evidence-backed connection between notes, identities, entities, or artifacts.

Examples:

- a person sent a message to another person;
- a person attended a meeting;
- a person appears affiliated with an organization;
- a message replies to another message;
- a note mentions a ticket or document;
- two source records are duplicates by content hash;
- several notes form a reconstructed thread or activity cluster.

Relationships are derived. The notes and source evidence remain canonical.

### 6.8 Proposal

A **proposal** is a vendor-neutral, human-readable document describing a possible downstream change.

For example, Fieldnotes may propose that a person entity's organization and role be recorded in a contact system. Fieldnotes does not perform that update. A human or external AI may review the evidence and translate the proposal into the target system's API or UI.

The core conceptual flow is therefore:

```text
Field/User → Note → Extraction/Observation → Entity/Relationship → Proposal → external action
```

Only the stages through Proposal belong to Fieldnotes.

## 7. Implementation and platform targets

### 7.1 Rust implementation

The Fieldnotes core and first-party Fields should be implemented in Rust.

Reasons include:

- predictable native distribution without a language runtime;
- strong cross-platform filesystem and process support;
- good control over streaming, hashing, parsing, and incremental collection;
- a type system suitable for enforcing the small set of stable notebook invariants;
- an easy path to a single CLI executable per target;
- acceptable performance on large directories without requiring a daemon.

Internal Rust types may be strict even though the user-facing notebook format is intentionally sparse. Strict implementation types should protect invariants such as IDs, timestamps, property types, provenance, and frontmatter flatness rather than expand into a public schema hierarchy.

### 7.2 Supported v0.1 targets

Release artifacts should target 64-bit Intel/AMD and ARM64 where the operating system supports them:

| Operating system | x64 | ARM64 |
|---|---:|---:|
| Windows | Supported | Supported |
| Linux | Supported | Supported |
| macOS | Supported | Supported |

Expected Rust target triples include:

```text
x86_64-pc-windows-msvc
aarch64-pc-windows-msvc
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
x86_64-apple-darwin
aarch64-apple-darwin
```

The precise Linux libc baseline and whether a musl build is also published remain open, but both Linux architectures are product requirements.

### 7.3 Runtime shape

The v0.1 core should be a command-line application with no required daemon, server, account, or canonical database.

Fields run as short-lived child processes. Optional enhancement may use an additional built-in process or library, but it must not change the notebook contract.

## 8. Field identity and configuration

### 8.1 Stable Field IDs

Configuring a Field returns a stable, human-readable ID:

```text
<field-type>_<label>
```

Examples:

```text
teams_work
teams_niivo
outlook_wxs
outlook_private
jira_wxs
twenty_wxs
contacts_private
self
```

Rules:

- `self` is the reserved built-in Field ID for Notes created directly by the user.
- External Field IDs use the two-segment form below.
- The left segment identifies the connector type and determines its property prefix.
- The right segment is a short user-selected label for the configured source/account.
- IDs should use lowercase ASCII letters, digits, and underscores.
- A Field ID must be unique within one Fieldnotes instance.
- A Field ID is immutable once notes have been produced.
- A Field ID is not globally unique across separate Fieldnotes instances.
- The globally meaningful producer identity is `(instance_id, field_id)`.

Display names, account addresses, tenant names, and machine names may change without changing the Field ID.

### 8.2 Example Field configuration

Configuration is private application state and is not constrained by the flat notebook-frontmatter rule.

```yaml
id: teams_work
driver: teams
account: joe@example.net
tenant: Example AG
credential_profile: microsoft_work
```

Secrets must not appear in this file.

The built-in user Field requires no credential and may be represented as:

```yaml
id: self
driver: builtin-self
```

### 8.3 Connector property prefix

Every Field type owns a stable prefix. When a source concept maps cleanly to the shared Fieldnotes vocabulary, the Field uses the shared property. Otherwise it preserves the value under its prefix.

```yaml
from: alice@example.com
participants:
  - alice@example.com
  - joe@example.net
teams_chat_id: "19:abc..."
teams_message_type: message
teams_importance: normal
```

Fields must not create arbitrary unprefixed property names. A source-specific property may later be promoted into the shared vocabulary, but that is a deliberate Fieldnotes change.

## 9. Minimal executable/process contract for Fields

### 9.1 Purpose

External Fields are process-isolated, read-only source adapters. The process boundary keeps the core independent from vendor SDKs and authentication libraries and permits Fields to evolve separately without turning the notebook format into a plugin object model.

The built-in `self` Field is implemented by Fieldnotes core and does not use this process contract. It performs no external write and exists only to give typed, imported, and recorded user Notes the same provenance model as collected Notes.

The contract should be small enough to implement in any language, although first-party Fields use Rust.

### 9.2 Discovery

A Field executable is referenced explicitly in configuration or discovered by a conventional name such as:

```text
fieldnotes-field-teams
fieldnotes-field-jira
```

`describe` must return a machine-readable manifest containing at least:

```json
{
  "protocol": 1,
  "type": "teams",
  "property_prefix": "teams",
  "capabilities": ["message", "meeting"],
  "auth": "microsoft-oauth"
}
```

The manifest describes executable compatibility and capabilities. It does not define a nested notebook schema.

### 9.3 Collection operation

The core starts the Field for a bounded collection run and passes one JSON request on standard input. The request includes:

- protocol version;
- configured `field_id`;
- opaque prior cursor, if any;
- requested time window, if any;
- non-secret connector configuration;
- an opaque credential reference or a protected credential channel.

The Field writes newline-delimited JSON events to standard output. Event kinds are intentionally few:

- `record`: one collected source record mapped toward a note;
- `checkpoint`: an opaque cursor safe for the core to commit after preceding records are durable;
- `diagnostic`: structured warning or health information.

Logs go to standard error. Secrets must never appear in either stream.

A conceptual exchange is:

```json
{"protocol":1,"operation":"collect","field_id":"teams_work","cursor":"opaque"}
{"event":"record","source_identity":"msg-123","type":"message","occurred_at":"2026-08-22T09:36:14Z","properties":{"from":"alice@example.com","teams_chat_id":"19:abc"},"body":"Alice asked about the migration."}
{"event":"checkpoint","cursor":"next-opaque"}
```

The process contract may use nested JSON because it is an internal transport. The emitted notebook frontmatter must still be flattened and validated by Fieldnotes core.

### 9.4 Ownership boundaries

The Field is responsible for:

- source authentication flow integration;
- incremental source access;
- source pagination and rate-limit handling;
- preserving source IDs and timestamps;
- mapping source values to shared or prefixed properties;
- returning enough raw/normalized content to render a useful note;
- declaring identity anchors supplied by the source;
- reporting damage, truncation, or skipped content.

The core is responsible for:

- assigning global note IDs;
- validating property names and types;
- enforcing flat frontmatter;
- writing files atomically;
- hashing and deduplicating content;
- committing cursors only after durable writes;
- resolving identities and deriving graphs;
- rendering explainability and proposal files;
- preventing credentials from entering the notebook.

### 9.5 Failure behavior

- A non-zero exit indicates that the run did not complete normally.
- Records durably written before the last committed checkpoint may remain.
- The core must not advance a cursor past uncommitted notes.
- Repeating a run must be safe: source identity and content hashes prevent duplicate notebook records.
- Diagnostics must be actionable but must not contain secrets or full sensitive payloads unless explicitly requested by the user.

The exact wire schema and exit-code table are open questions for v0.1; the boundaries above are not.

## 10. Credentials and authentication

### 10.1 Credential abstraction

Configuration refers to a named credential profile. It never embeds a refresh token, client secret, password, API key, or session cookie.

```yaml
credential_profile: microsoft_work
```

The core resolves that profile through a `CredentialProvider` abstraction and supplies credentials to a Field without placing secrets in command-line arguments, notebook files, cursors, logs, or process listings.

### 10.2 Default provider by operating system

The default is the operating system's secure credential store:

- macOS: Keychain;
- Windows: Credential Manager/DPAPI-backed storage;
- Linux: Secret Service through the available desktop keyring.

If the default secure store is unavailable, Fieldnotes must explain the problem and ask the user to choose a supported alternative. It must not silently fall back to plaintext storage.

### 10.3 Optional providers

The abstraction may also support:

- environment variables for CI, containers, or temporary sessions;
- an external command that prints a secret to standard output, for systems such as 1Password, Bitwarden, or Vault;
- a local credential file only as an explicit, strongly warned fallback with restrictive permissions.

The exact provider set shipped in v0.1 is an open question. OS keychain/default behavior is not.

### 10.4 Authentication flow

`fieldnotes fields auth <field_id>` initiates the source-appropriate flow, such as browser OAuth or a device code. Long-lived refresh credentials are stored in the selected provider. Short-lived access tokens should remain in memory and be refreshed as needed.

Fields should receive secrets over standard input, an inherited handle, or another protected IPC mechanism. They must never receive them as command-line arguments.

## 11. Note file contract

### 11.1 Filename

Every note filename follows:

```text
<utc-timestamp>_<field-id>_<type>_<note-id>.md
```

Example:

```text
20260822T093614Z_teams_work_message_note_01K3M7K6CZQ7F6PX4V8G0XWJ83.md
```

The timestamp is UTC and filename-safe on Windows, Linux, and macOS. It is derived from `occurred_at`, so lexicographic sorting produces a global timeline and the time the Note happened acts as its primary human-facing locator.

Components:

- `utc-timestamp`: `YYYYMMDDTHHMMSSZ`, using the best event time available;
- `field-id`: the configured stable ID such as `teams_work`;
- `type`: one lowercase primary note type;
- `note-id`: a globally unique, stable Fieldnotes ID.

The Note ID may be time-sortable and may encode the initial `occurred_at` value, but the timestamp is never the complete technical identity. Several Notes may occur in one second, imported material may arrive later, and a user may correct the time. Correcting `occurred_at` renames the file but must not change the Note ID.

The filename must not contain subjects, participant names, customer names, or other mutable/sluggified content.

### 11.2 Primary type

A note has exactly one primary `type`. The initial vocabulary should remain small, for example:

```text
text
message
mail
meeting
call
ticket
document
file
contact
event
voice
```

A source-specific nuance belongs in a prefixed property. Several notes that represent one piece of work are connected by derived relationships or an activity cluster.

### 11.3 Required frontmatter

Every note contains these five structural properties:

```yaml
---
id: note_01K3M7K6CZQ7F6PX4V8G0XWJ83
instance_id: fn_01K3M5AWZ2MN7PX1JY9M6EA54F
field_id: teams_work
type: message
occurred_at: 2026-08-22T09:36:14
---
```

By Fieldnotes convention, frontmatter datetimes are UTC. The filename retains an explicit `Z`. The final serialization choice for preserving both explicit timezone semantics and native Obsidian Date & time behavior is listed as an open question.

`source_identity` should also be present whenever an external source supplies a stable identity. User-created Notes from the `self` Field may omit it.

### 11.4 Frontmatter value rules

Allowed values:

- text;
- number;
- checkbox/boolean;
- date or datetime;
- one-dimensional lists of those scalar types.

Forbidden values:

- nested objects/maps;
- arrays of objects;
- arbitrary YAML tags;
- binary payloads;
- secrets;
- unbounded source responses.

Lists remain lists even when they contain one item:

```yaml
participants:
  - alice@example.com
```

They must not sometimes be emitted as scalars:

```yaml
participants: alice@example.com
```

### 11.5 Stable property types

Obsidian assigns a property type by property name across a vault. Therefore, whenever a Fieldnotes property appears, it must retain the same meaning and type in every note.

An initial shared vocabulary is:

| Property | Type | Meaning |
|---|---|---|
| `id` | text | Global Fieldnotes record ID |
| `instance_id` | text | Producing Fieldnotes instance |
| `field_id` | text | Configured Field ID |
| `type` | text | Primary record type |
| `occurred_at` | datetime | Best source event time, normalized to UTC |
| `captured_at` | datetime | Time Fieldnotes durably recorded the note |
| `started_at` | datetime | Start of an interval |
| `ended_at` | datetime | End of an interval |
| `duration_seconds` | number | Duration in seconds |
| `source_identity` | text | Stable opaque identity within the source Field |
| `source_parent_id` | text | Source-local parent object ID |
| `source_url` | text | Canonical source URL, when available |
| `content_hash` | text | Hash of normalized content or referenced artifact |
| `from` | text | Sender/caller identity |
| `to` | list[text] | Direct recipients |
| `cc` | list[text] | Carbon-copy recipients |
| `bcc` | list[text] | Blind-copy recipients when legitimately observable |
| `organizer` | text | Meeting/event organizer identity |
| `participants` | list[text] | All people involved, regardless of source-specific roles |
| `subject` | text | Message-like subject |
| `title` | text | Human-facing artifact/event title |
| `thread_id` | text | Normalized or source-provided thread identifier |
| `conversation_id` | text | Conversation identifier |
| `reply_to` | text | Related note or source message ID |
| `related` | list[text] | Related Fieldnotes record IDs |
| `attachments` | list[text] | Attachment/artifact IDs or relative links |
| `artifacts` | list[text] | Artifact IDs or relative links carried by this Note |
| `audio_duration_seconds` | number | Duration of a voice/audio artifact |
| `audio_media_type` | text | Media type of a voice/audio artifact |
| `identities` | list[text] | Namespaced identity anchors present in the record |
| `entities` | list[text] | Resolved Fieldnotes entity IDs |
| `damaged` | checkbox | Content loss or corruption was detected |
| `truncated` | checkbox | Source or rendered content is incomplete |
| `lost_characters` | number | Detected number of lost characters, when measurable |

This table is vocabulary, not a requirement that every note contain every property. The final v0.1 registry may add or remove entries after implementation experience.

### 11.6 Field-specific properties

Source-specific keys use the connector prefix:

```yaml
teams_chat_id: "19:abc..."
teams_message_type: message
jira_key: ACME-142
jira_status: In Progress
outlook_conversation_index: "..."
```

The same scalar/list and stable-type rules apply. Connector-specific nested YAML is not permitted.

### 11.7 Markdown body

The body should be coherent without requiring the YAML to be decoded. It may contain:

- normalized source text;
- source attribution and local display time;
- separated current content and quoted history;
- readable links with vendor wrappers removed where safe;
- references to deduplicated attachments;
- warnings about truncation or rendering damage;
- concise evidence sections.

The body should not be replaced by a model-generated summary when original or deterministically normalized source content can be preserved.

### 11.8 Example note

```markdown
---
id: note_01K3M7K6CZQ7F6PX4V8G0XWJ83
instance_id: fn_01K3M5AWZ2MN7PX1JY9M6EA54F
field_id: teams_work
type: message
occurred_at: 2026-08-22T09:36:14
captured_at: 2026-08-22T09:38:02
source_identity: "174531..."
source_url: https://teams.microsoft.com/l/message/...
from: alice@example.com
participants:
  - alice@example.com
  - joe@example.net
subject: Migration Thursday
thread_id: "19:abc..."
content_hash: sha256:6f4d...
identities:
  - email:alice@example.com
  - email:joe@example.net
teams_chat_id: "19:abc..."
teams_message_type: message
---

# Migration Thursday

Alice asked whether the migration can happen on Thursday.

Bob will provide the DNS credentials.
```

## 12. User-created Notes, voice, and artifacts

### 12.1 The built-in `self` Field

A user may create a Note without an external connector. Fieldnotes records it through the reserved `self` Field so provenance remains uniform rather than introducing a special class of unowned files.

A typed Note uses `type: text` unless the user chooses a more specific shared type. The body is the user's own text. The user may supply `occurred_at`; otherwise Fieldnotes uses the creation time. `captured_at` records when the Note was durably written.

The minimal CLI direction is:

```text
fieldnotes note "Remember to ask Alice about the rollout"
fieldnotes note --at "2026-08-22T16:00:00Z" "Customer called about licensing"
fieldnotes note --file ./meeting-photo.jpg
fieldnotes note --voice ./recording.m4a
```

Imported material is copied or content-addressed according to artifact policy. Fieldnotes must not leave the only copy at an ephemeral input path.

### 12.2 Voice Notes

Voice is a first-class Note type, not merely an attachment that becomes useful only after transcription.

A voice Note:

- uses `field_id: self` and `type: voice`;
- uses the start of the recording, or a user-supplied time, as `occurred_at`;
- references the original audio as a content-addressed artifact;
- records duration and media type when available;
- may have a short user-authored title or body;
- remains playable and attributable when inference is disabled.

Example:

```markdown
---
id: note_01K3MA8Q...
instance_id: fn_01K3M5AWZ2MN7PX1JY9M6EA54F
field_id: self
type: voice
occurred_at: 2026-08-22T14:25:11
captured_at: 2026-08-22T14:26:42
audio_duration_seconds: 84
audio_media_type: audio/mp4
artifacts:
  - artifact_sha256_6f4d...
---

# Voice note

Recorded after the customer call.
```

When optional built-in enhancement is enabled, speech-to-text may create an Extraction that cites the voice Note and preserves audio time ranges. Language, named entities, references, and a transcript are derived evidence; they never replace or mutate the original voice Note.

v0.1 must support importing common audio recordings as voice Notes. Whether the CLI also records directly from the microphone is an open implementation question; the Note and artifact contract must support both paths.

### 12.3 Attachments and artifacts

Attachments should be deduplicated by a strong content hash. The same document received through eight emails should be stored or rendered once and referenced from all eight notes.

Where practical, Fieldnotes should render text-bearing formats into readable Markdown while retaining enough provenance to trace back to the source attachment. Binary retention policy may be configurable.

Rendering must report known damage. For example, if PDF font or encoding issues cause character loss, Fieldnotes should mark the artifact or note as damaged and include a measurable count when available rather than presenting corrupted text as trustworthy.

Vendor URL wrappers, such as safe-link redirects, may be normalized to the underlying URL when the transformation is deterministic and the original URL remains recoverable.

## 13. Multi-instance provenance and merging

### 13.1 Instance identity

Every installation creates a stable global `instance_id` on initialization:

```yaml
instance_id: fn_01K3M5AWZ2MN7PX1JY9M6EA54F
created_at: 2026-08-22T11:24:00
name: joes-macbook
```

The friendly name may change. The ID does not.

### 13.2 Merge scenario

A user may run separate instances on work and private systems, email or copy notebooks to themselves, and combine all notes in one directory.

For example:

```text
Work Mac       fn_A → teams_work, outlook_wxs
Private Mac    fn_B → gmail_private, calendar_private
Laptop         fn_C → teams_work
```

Both `fn_A` and `fn_C` may legitimately contain a Field named `teams_work`. Their producer identities remain distinct because the true provenance key is `(instance_id, field_id)`.

### 13.3 Identity layers used during merge

Fieldnotes distinguishes:

1. **Note identity:** the globally unique `id` prevents filename and record collisions.
2. **Producer identity:** `(instance_id, field_id)` identifies the configured source producer.
3. **Source identity:** `(instance_id, field_id, source_identity)` identifies the source object as collected by that producer.
4. **Content identity:** `content_hash` detects identical material reached through different source objects or Fields.
5. **Entity identity:** namespaced identities and evidence allow entity graphs from several notebooks to be reconciled.

### 13.4 Merge rules

- Same note ID means the same Fieldnotes note; conflicting content is an error requiring preservation and explanation.
- Same producer and source identity is a duplicate source object and should be reconciled deterministically.
- Same content hash means the content is identical, not necessarily that the notes have the same context.
- Different IDs and source identities are preserved even when their content hashes match.
- Conflicting metadata is never silently discarded.
- Original `instance_id`, `field_id`, source identity, and content hashes survive copying and merging.
- Derived indexes and graphs are rebuilt after a merge.
- Directly copying note directories together should get most of the way to a valid merge.

### 13.5 Entity graphs across instances

Two instances may independently create entity IDs for the same person. Entity IDs alone are not sufficient to merge those entities.

Strong shared identities—such as the same normalized email address or a source-provided global user ID—may produce an automatic deterministic merge. Weak evidence such as matching display names should create a merge candidate, not a silent union.

The merged graph must retain evidence showing why identities or entities were joined.

## 14. Graph model and entity resolution

### 14.1 Notes are canonical; the graph is derived

The graph can be deleted and regenerated. An edge without recoverable note or configuration evidence must not be treated as durable truth.

Fieldnotes does not require a graph database. The implementation may use in-memory structures or a disposable local index for performance.

### 14.2 Evidence classes

Graph links should distinguish at least four origins:

- **explicit:** the source directly states the relationship;
- **matched:** deterministic rules establish the relationship;
- **extracted:** an optional built-in Extraction found explicit source evidence;
- **observed:** an optional built-in Observation synthesized cited evidence into a reviewable claim.

An optional extractor may produce a candidate string such as `Example AG`. That string does not automatically become an existing organization entity. A deterministic resolver must match it to a known name, alias, domain, or source identity; otherwise it remains unresolved evidence.

### 14.3 Identity strength

Fields should declare which source values are useful identity anchors and their matching scope. Example guidance:

```text
Entra/Graph object ID       exact within declared tenant/source
verified email address      strong
normalized phone number     strong
source account ID           exact within its source namespace
stable username             medium and namespace-scoped
display name                weak
```

The declaration belongs to connector metadata or mapping configuration, not repeated nested structures in every note.

### 14.4 Entity notes

A derived person note can remain flat:

```markdown
---
id: ent_01K3N1...
type: person
names:
  - Alice Müller
identities:
  - email:alice@example.com
  - entra-user:8d82...
seen_in:
  - teams_work
  - outlook_wxs
first_seen: 2026-06-18T08:22:00
last_seen: 2026-08-22T09:36:14
languages_observed:
  - de
  - en
preferred_language: de
preferred_address_form: du
usual_signoff: Liebe Grüsse
---

# Alice Müller

Preferred language inferred from 38 observed interactions:

- German: 34
- English: 4

German interactions consistently use “du.”

See the evidence references below for the underlying notes.
```

This is an interaction profile backed by prior behavior. It is not a CRM profile and must not contain unsupported judgments. Properties such as preferred language, address form, and usual sign-off are materialized from cited Observation records; they are rebuildable projections, not mutations of any canonical Note.

### 14.5 What Fieldnotes may derive

The deterministic graph and optional Observations are related but not identical.

Appropriate deterministic graph facts include:

- exact or configured identity resolution;
- source-provided organization affiliation;
- thread reconstruction from explicit reply and conversation identifiers;
- duplicate artifact detection;
- relationship edges with counts and source Notes;
- first seen and last seen;
- channel presence and interaction counts;
- document/reference graphs;
- unresolved identity or affiliation gaps.

Appropriate inference-assisted Observations include:

- organization affiliation supported by signatures and other extracted evidence;
- uncertain thread reconstruction where source metadata is damaged;
- likely cross-source activity clusters;
- observed and usual communication conventions;
- identity or affiliation candidates whose evidence is too weak for deterministic resolution.

Inappropriate graph facts or Observations include:

- `relationship_strength: strong`;
- `sentiment: negative`;
- `customer_health: at_risk`;
- `priority: high` unless that value is explicitly supplied by the source;
- `important_contact: true`;
- subjective personality or communication-style labels;
- a claim that an entity is a lead, qualified contact, customer owner, or strategic account unless merely preserving that explicit source-system fact.

## 15. Extractions and Observations

Extractions and Observations are the optional inference-assisted layers that enhance Notes. They are separate, derived files. They never modify the canonical Note.

```text
Note ──→ Extraction
  └────→ Observation
          ↑
     other Notes and Extractions
```

Both layers are disposable and rebuildable. Deleting `extractions/` and `observations/` must leave a complete, useful deterministic notebook.

### 15.1 Extraction definition

An **Extraction** recovers structured evidence already present in one Note but not supplied as reliable source metadata.

The extracted value must be traceable to the exact Note and, where applicable, an exact text span or audio time range. It must not invent or enrich the source.

Examples:

- **voice transcript:** words tied to audio time ranges;
- **contact details:** name, role, organization, phone, email, address, or URL explicitly present in a signature or document;
- **signature boundary:** distinguish the current sender signature from message content, quoted history, and disclaimers;
- **salutation/sign-off:** preserve literal values such as `Hallo Alice` or `Liebe Grüsse`;
- **language/address form:** record `language: de` and literal `address_form: du` for one interaction;
- **document reference:** invoice, purchase-order, contract, project, ticket, or case references;
- **role/organization:** explicit job title, department, and organization names;
- **location:** a literal place or address stated in the Note.

Syntax-led deterministic normalization should handle source metadata, email addresses, phone numbers, URLs, hashes, and timestamps before optional inference is considered. Those deterministic values may live directly on the Note and do not need to be called Extractions.

### 15.2 Extraction file contract

An Extraction is a flat Markdown file that cites exactly one source Note.

```markdown
---
id: ext_01K3X1...
type: interaction_language
source_note_id: note_01K3M7...
generated_at: 2026-08-22T10:02:14
generator_version: fieldnotes-interaction-1
language: de
address_form: du
salutation: Hallo Alice
signoff: Liebe Grüsse
evidence_spans:
  - text:0-11
  - text:482-496
---

# Interaction extraction

The literal values above were recovered from the cited Note. The source spans
refer to the deterministically normalized Note body.
```

For voice, span references use time ranges such as `audio:12.400-18.250`. A transcript may live in the Extraction body with timecodes. The original audio remains in the source Note's artifact reference.

The precise filename convention for Extractions is open for v0.1. It must include the Extraction ID and remain collision-safe; chronological ordering is less important than it is for Notes.

### 15.3 Observation definition

An **Observation** is an inference-assisted interpretation or synthesis supported by one or more Notes or Extractions.

The evidence does not need to contain the conclusion as one exact string, but a reviewer must be able to follow the conclusion from the cited material.

Examples:

- repeated signatures support a usual sign-off for one contact;
- a signature plus other identity evidence supports a likely organization affiliation;
- several damaged message threads likely belong to one reconstructed conversation;
- Notes with common participants, references, and nearby timestamps likely form an activity cluster;
- German appears in 34 of 38 extracted interactions and is therefore the usual or preferred interaction language;
- every German exchange with a contact uses `du`, supporting `preferred_address_form: du`;
- recurring references across Notes suggest that two source identities represent the same person.

Purely deterministic results such as exact-email identity matching, first/last seen timestamps, duplicate hashes, and interaction counts remain graph facts. They do not require an Observation merely to fit the product vocabulary.

### 15.4 Observation file contract

An Observation is a flat Markdown file with explicit evidence and generator provenance.

```markdown
---
id: obs_01K3X9...
type: interaction_pattern
subject_entity_id: ent_01K3N1...
generated_at: 2026-08-22T10:04:31
generator_version: fieldnotes-observer-1
supported_by:
  - ext_01K3X1...
  - ext_01K3X2...
  - ext_01K3X3...
preferred_language: de
preferred_address_form: du
usual_signoff: Liebe Grüsse
confidence: 0.94
---

# Interaction observation

Across 38 cited interaction Extractions:

- German: 34
- English: 4
- all German interactions used `du`
- 21 of 23 messages sent by the user ended with `Liebe Grüsse`

This supports the properties above. It does not establish the contact's private
preference beyond the observed interaction history.
```

`usual_signoff` is preferable to `preferred_signoff` because repeated behavior is directly observable while personal preference may not be.

`preferred_language` and `preferred_address_form` may be used only when the evidence counts and Observation rule are explicit. Literal linguistic forms such as `du`, `Sie`, `tu`, `vous`, and `Lei` are more useful than a lossy formal/informal abstraction.

### 15.5 Capability boundary

An Extraction may recover explicit source content. An Observation may connect and interpret that evidence. Neither may make unsupported psychological or business judgments.

> Extraction requires source presence. Observation requires cited, reviewable evidence.

Anything beyond that belongs to a downstream consumer.

### 15.6 Derived-file invariants

The scalar/list, stable-type, no-secrets, and no-nested-frontmatter rules apply to Extractions and Observations as well as Notes.

Shared derived properties include:

| Property | Type | Meaning |
|---|---|---|
| `id` | text | Global Extraction or Observation ID |
| `type` | text | Derived record type |
| `source_note_id` | text | The single Note enhanced by an Extraction |
| `generated_at` | datetime | Time the derived file was produced |
| `generator_version` | text | Versioned built-in extraction or observation contract |
| `evidence_spans` | list[text] | Text offsets or audio time ranges in the source Note |
| `supported_by` | list[text] | Note and/or Extraction IDs supporting an Observation |
| `confidence` | number | Bounded generator confidence where meaningful |

An Extraction ID uses the `ext_` prefix and an Observation ID uses `obs_`. These prefixes are part of the human-readable vocabulary, not a substitute for globally unique IDs.

## 16. Optional built-in enhancement

### 16.1 Default: disabled

A fresh installation performs no local model inference. Running:

```bash
fieldnotes sync
```

must work without downloading model weights, requiring a GPU, or introducing hidden probabilistic behavior. Notes, deterministic identities, exact relationships, hashes, and graph facts remain useful without Extractions or Observations.

### 16.2 Narrow purpose

Optional built-in inference is exposed as **enhancement**, consisting only of Extractions and Observations.

It may:

- transcribe a voice Note with timecode evidence;
- recover explicit values and source spans;
- connect repeated extracted evidence into a reviewable pattern;
- produce bounded summaries of the evidence cited by an Observation.

It may not:

- guess missing facts;
- label sentiment, relationship strength, customer health, importance, personality, or intent;
- decide customer attribution through subjective reasoning;
- generate or apply CRM updates without cited evidence and a separate proposal;
- alter a canonical Note;
- create an Extraction or Observation unsupported by source material.

### 16.3 No bring-your-own inference

Fieldnotes v0.1 provides no configuration such as:

```text
provider = openai
provider = anthropic
provider = ollama
endpoint = ...
model = ...
prompt = ...
```

There is no public model-provider, prompt, or extractor plugin surface.

If enhancement is enabled, Fieldnotes uses a tested built-in engine and model paired with the Fieldnotes release. Users who want other models should build an external processing pipeline on top of the generated notebook.

### 16.4 Validation

Model output is candidate evidence, never truth.

For extractable text, the preferred contract is source spans:

```json
{
  "name": {"start": 0, "end": 12},
  "role": {"start": 13, "end": 31},
  "organization": {"start": 32, "end": 42}
}
```

For voice, the equivalent evidence is a validated start/end time within the referenced audio artifact. Fieldnotes rejects nonexistent text spans, impossible time ranges, missing Note references, and invalid property types.

An Observation must cite existing Notes or Extractions and include an evidence explanation. Structured-output constraints may improve reliability but are not the safety boundary; deterministic post-generation validation is mandatory.

### 16.5 Reproducibility

Extraction and Observation provenance must identify their generator contract and version. Changing the built-in engine is a Fieldnotes implementation change that may require rebuilding both derived layers.

Identical Notes and identical Fieldnotes/generator versions should produce stable results to the degree practical. Where probabilistic output differs, the versioned evidence files make that difference visible rather than silently changing Notes.

The exact model, speech-to-text component, packaging, download behavior, and multilingual benchmark are open questions for v0.1.

## 17. Explainability and evidence rules

### 17.1 Every derived claim must explain itself

A derived fact or edge must provide:

- the claim;
- its origin class (`explicit`, `matched`, `extracted`, or `observed`);
- the note IDs or configuration rules used as evidence;
- the rule or generator identifier and version;
- enough human-readable detail to reproduce the conclusion;
- unresolved ambiguity or conflicting evidence.

### 17.2 Source-span rule for extraction

When a value came from free text, Fieldnotes should preserve the exact source value and source-span location, or audio time range, where practical. If the span cannot be validated, the extraction is rejected or clearly marked unresolved.

### 17.3 Aggregation rule for observations

Aggregated observations must include relevant counts and time ranges. For example:

```text
Preferred language: German
Evidence: 34 German and 4 English interactions between 2026-06-18 and 2026-08-22.
```

An edge such as `Alice ↔ Bob` should state the notes or aggregate counts supporting it rather than a subjective strength label.

### 17.4 Conflicts are preserved

Fieldnotes must not hide contradictory roles, organizations, names, or identifiers. It may choose a current or usual value only through a documented rule while retaining the competing evidence and time ranges.

### 17.5 Explainability storage

Canonical Note frontmatter remains small. Inference-derived detail lives in separately addressable files under `extractions/` and `observations/`. Disposable indexes may accelerate access, but the derived Markdown and its evidence references remain inspectable and rebuildable.

## 18. CRM and contact-system boundary

### 18.1 CRM systems are Fields

Outlook Contacts, Twenty, HubSpot, a personal CRM, and similar systems may be connected as read-only Fields. Their records contribute identities and explicit source facts just like any other source.

For example:

```text
Fieldnotes person entity
  ├── email:alice@example.com
  ├── entra-user:8d82...
  ├── twenty-person-id:83472
  └── outlook-contact-id:...
```

Fieldnotes does not need a special customer/contact ontology to read these systems.

### 18.2 No CRM semantics in the core

Fieldnotes entities do not inherently represent leads, accounts, clients, owners, opportunities, customer health, or lifecycle stages. Those meanings are vendor- or organization-specific.

`customer` may be retained as a convenience property in a source-specific or legacy workflow, but it is not a foundational Fieldnotes concept. The universal core is note, identity, entity, relationship, and evidence.

### 18.3 Vendor-neutral proposals

A proposal describes the possible durable fact, target entity, evidence, and optional target record without encoding a vendor API payload.

Example:

```markdown
---
id: prop_01K3P2...
type: entity_update
created_at: 2026-08-22T12:10:00
entity_id: ent_01K3N1...
target_field_id: twenty_wxs
target_source_id: person_83472
status: proposed
evidence:
  - note_01K3M7...
  - note_01K3M8...
---

# Proposed update: Alice Müller

## Existing

- Organization: not recorded
- Role: not recorded

## Proposed

- Organization: Example AG
- Role: Head of Operations

## Evidence

- Outlook signature on 2026-08-21: “Head of Operations | Example AG”
- Email identity: `alice@example.com`
- The `example.com` domain is deterministically associated with Example AG.

## Confidence and conflicts

Both proposed values are directly recoverable from the signature. No conflicting current signature was observed.
```

The example uses only flat frontmatter. Proposed changes stay in readable Markdown rather than a nested vendor-neutral object.

A human or agent can translate the same proposal into Twenty, Outlook Contacts, HubSpot, or another destination. Fieldnotes never calls the destination write API.

## 19. Obsidian and Bases usability

### 19.1 Native property behavior

Different notes in the same folder may contain different properties. Missing properties appear as empty cells in an Obsidian Base, which is exactly the sparse behavior Fieldnotes wants.

The requirement that a property name keep one type across the vault is the reason for the stable property registry and consistent list handling.

### 19.2 Source-agnostic and source-specific views

A Base over `notes/` can display a shared timeline:

```text
occurred_at | type | field_id | subject | participants
```

A Teams-specific view can filter `field_id == "teams_work"` and add `teams_chat_id`. A Jira-specific view can display `jira_key` and `jira_status`. Notes without those properties simply have empty cells.

### 19.3 Optional Base file

Fieldnotes should consider shipping a small `fieldnotes.base` file with useful default views:

- all Notes by time;
- Notes grouped by Field;
- user-authored and voice Notes;
- voice Notes with or without transcript Extractions;
- Extractions grouped by source Note;
- Observations grouped by subject entity;
- unresolved entities or gaps;
- proposals by status;
- damaged or truncated items.

The notebook must remain fully usable without Obsidian. The Base is a convenience, not a dependency.

### 19.4 Generic tool usability

Fieldnotes does not invent a query language.

- Use filesystem ordering for a timeline.
- Use `rg` or equivalent for text search.
- Use a YAML/frontmatter parser for structured filtering.
- Use Obsidian Properties/Bases for human database-like views.
- Let AI agents parse the same flat frontmatter directly.

## 20. User-facing CLI direction

The initial CLI should be small and literal:

```text
fieldnotes init [path]
fieldnotes sync [field_id]
fieldnotes note <text>
fieldnotes note --at <datetime> <text>
fieldnotes note --file <path>
fieldnotes note --voice <audio-path>
fieldnotes status
fieldnotes inspect <note-or-entity-id>
fieldnotes explain <derived-id>
fieldnotes merge <path>
fieldnotes rebuild
fieldnotes archive <period>
fieldnotes prune --older-than <duration>
```

Field management:

```text
fieldnotes fields list
fieldnotes fields add <type> <label>
fieldnotes fields auth <field_id>
fieldnotes fields status [field_id]
fieldnotes fields remove <field_id>
```

Example:

```text
$ fieldnotes fields add teams wxs
Added field: teams_work

$ fieldnotes fields auth teams_work
Authentication stored in the operating-system credential store.

$ fieldnotes sync teams_work
Collected 142 records; wrote 119 notes; reused 23 existing artifacts.
```

Entity and graph inspection may use:

```text
fieldnotes entities list
fieldnotes entities show <id-or-identity>
fieldnotes entities candidates
fieldnotes graph rebuild
fieldnotes gaps
fieldnotes proposals list
```

Optional enhancement may use:

```text
fieldnotes enhancement status
fieldnotes enhancement enable
fieldnotes enhancement disable
fieldnotes enhancement rebuild
```

If model assets are not bundled, enabling enhancement may initiate an explicit install/download step. There is no command for selecting a custom model or provider.

CLI output should be readable by default and offer stable machine-readable output, such as `--format json`, where automation is a clear use case.

## 21. Suggested initial directory layout

```text
Fieldnotes/
├── README.md
├── fieldnotes.base
├── .fieldnotes/
│   ├── instance.yaml
│   ├── config.yaml
│   ├── fields/
│   │   ├── self.yaml
│   │   ├── teams_work.yaml
│   │   ├── outlook_wxs.yaml
│   │   └── twenty_wxs.yaml
│   └── cache/
│       ├── cursors/
│       └── graph/
├── notes/
│   ├── 20260822T093614Z_teams_work_message_note_01K3M7....md
│   ├── 20260822T101803Z_outlook_wxs_mail_note_01K3M8....md
│   ├── 20260822T102741Z_jira_wxs_ticket_note_01K3M9....md
│   └── 20260822T142511Z_self_voice_note_01K3MA....md
├── extractions/
│   ├── ext_01K3X1_interaction_language.md
│   └── ext_01K3X2_voice_transcript.md
├── observations/
│   └── obs_01K3X9_interaction_pattern.md
├── entities/
│   ├── ent_01K3N1_person.md
│   └── ent_01K3N2_organization.md
├── relationships/
│   └── rel_01K3R1_person_person.md
├── proposals/
│   └── prop_01K3P2_entity_update.md
└── artifacts/
    ├── sha256_6f4d....pdf
    ├── sha256_81ae....m4a
    └── sha256_6f4d....md
```

Notes:

- `notes/` is flat in v0.1 so directory ordering is the timeline.
- `extractions/` and `observations/` contain optional inference-assisted enhancements and can be deleted and rebuilt without changing Notes.
- `.fieldnotes/cache/` is disposable and may be deleted at any time.
- `entities/`, `relationships/`, and proposals derived solely from Notes, Extractions, and Observations can be regenerated; user-reviewed proposal state may need separate preservation rules.
- Credentials never live anywhere in this tree.
- `README.md` should explain the notebook conventions to both humans and AI readers.

## 22. Example derived relationship

```markdown
---
id: rel_01K3R1...
type: person_person
from_entity_id: ent_01K3N1...
to_entity_id: ent_01K3N4...
first_seen: 2026-06-18T08:22:00
last_seen: 2026-08-22T09:36:14
channels:
  - teams
  - outlook
evidence_count: 31
evidence:
  - note_01K3M7...
  - note_01K3M8...
---

# Alice Müller ↔ Bob Rossi

Observed evidence:

- 24 messages
- 4 shared meetings
- 3 shared tickets

This edge describes observed interaction. It does not label the relationship as strong, trusted, strategic, or friendly.
```

For large evidence sets, the frontmatter list may contain only a bounded representative set or a separate evidence note, while the full set remains reconstructable from notes. The exact v0.1 representation is open.

## 23. Processing and quality requirements

### 23.1 Atomic and repeatable writes

Note and artifact writes should be atomic. A failed sync must not leave a valid-looking partial note. Re-running collection after interruption must reconcile by source identity and hashes rather than create uncontrolled duplicates.

### 23.2 Normalization must be conservative

Fieldnotes may normalize encodings, line endings, phone numbers, email casing, URLs, and quoted message structure when the transformation is deterministic. The original source identity and enough original content must remain available to explain the result.

### 23.3 Damage must be visible

If Fieldnotes cannot fully collect or render content, it should produce a warning or damaged/truncated note rather than silently present incomplete material as complete.

### 23.4 No secret leakage

The core must scan obvious credential-bearing fields before writing diagnostics or notes. Connector contracts and review tests should explicitly cover tokens in errors, URLs, command arguments, and captured HTTP metadata.

### 23.5 Rebuildability test

A clean cache plus the canonical Notes and non-secret configuration should reproduce the same deterministic entity/relationship graph. Optional Extractions and Observations may be reused from their derived files or regenerated with pinned generator versions.

### 23.6 Mergeability test

A note remains understandable and attributable after it is emailed, copied to another computer, dropped into another notebook, and mixed with notes from other instances.

## 24. Open questions for v0.1

The following areas need implementation evidence or small spikes. They should not be expanded into complex frameworks before that work exists.

1. **Global ID format.** Use ULID-style IDs, UUIDv7, or another representation? The chosen format must be globally unique, filesystem-safe, and preferably sortable. A Note ID may encode its initial `occurred_at` value, but correcting the timestamp must not change the ID.
2. **Datetime serialization.** Should YAML contain explicit `Z` offsets, or should Fieldnotes use Obsidian-native timezone-less datetimes with a documented UTC convention?
3. **Final shared vocabulary.** Which properties belong in the initial registry, and which early product-specific concepts such as `customer` remain connector-prefixed or convenience-only?
4. **Note types.** What is the smallest useful initial set, should `mail` be distinct from `message`, and is `text` the right generic type for a user-authored Note?
5. **Field wire contract.** Final JSON event shapes, protocol negotiation, exit codes, executable discovery, and compatibility rules.
6. **Field output level.** Should a Field emit a normalized source envelope or a nearly rendered note candidate? The core must remain the final validator/writer either way.
7. **Field distribution.** Which Fields ship in the main binary, as sibling executables, or through a separately managed first-party package?
8. **Linux credentials.** Exact Secret Service integration and the explicit fallback experience on headless servers.
9. **Optional credential providers.** Whether environment, external command, and encrypted/local file providers all ship in v0.1.
10. **Built-in enhancement engine.** Extraction, observation, and speech-to-text model choices; supported languages; CPU/memory budget; licensing; packaging; and evidence benchmarks.
11. **Enhancement installation.** Bundle model assets, download only on enable, or distribute a separate optional package?
12. **Derived filenames and retention.** Exact filename conventions and retention behavior for rebuildable Extractions and Observations.
13. **Entity auto-merge policy.** Which strong identities permit automatic merge, how tenant/source scopes work, and how users express overrides or aliases.
14. **Derived graph layout.** Whether relationship notes are materialized individually or generated only for significant/queryable edges.
15. **Proposal lifecycle.** Whether proposals are generated automatically or on demand, and how reviewed/accepted/rejected status survives graph rebuilds.
16. **Artifact retention.** When to retain original binaries, rendered Markdown, or both; maximum sizes; and per-Field policy.
17. **Thread and activity reconstruction.** Initial deterministic rule set and representation of uncertain clusters.
18. **Merge conflicts.** User experience for the rare case where the same global note ID has divergent content.
19. **Field rename/removal.** Safe UX for immutable IDs, historical notes, credentials, and cursors.
20. **Obsidian Base.** Exact default views and compatibility baseline while keeping Obsidian entirely optional.
21. **Linux binary baseline.** Minimum glibc version and whether musl releases are provided alongside GNU targets.
22. **Retention commands.** Exact archive format, safe prune defaults, and handling of artifacts shared by retained Notes.
23. **Voice capture surface.** Which audio formats v0.1 imports by default, whether direct microphone recording ships in the CLI, and what cross-platform playback metadata is required.

## 25. v0.1 acceptance criteria

Fieldnotes v0.1 is coherent when all of the following are true:

- A user can initialize a notebook and receive a stable `instance_id`.
- A user can add and authenticate a read-only Field with a stable ID such as `teams_work`.
- A user can create a typed Note through the built-in `self` Field.
- A user can import a voice recording as a playable, attributable Note without enabling inference.
- Sync works with inference disabled and produces useful Notes and a deterministic graph.
- Every note filename includes UTC time, Field ID, type, and a globally unique note ID.
- Every note has the five required structural properties and only flat scalar/list frontmatter.
- Shared property names retain one meaning and type across the notebook.
- Source-specific properties are prefixed.
- A note can be read usefully in plain text and browsed usefully in Obsidian.
- Duplicate source records and duplicate content are distinguished through source identity and hashes.
- Two notebooks from different instances can be copied together without ID collisions or lost provenance.
- Entity and relationship edges can explain their source evidence.
- CRM/contact sources contribute read-only evidence, while proposed updates remain vendor-neutral documents.
- Optional enhancement, if enabled, uses only the built-in engine, writes separate Extractions and Observations, validates source spans or audio time ranges, and exposes no bring-your-own inference surface.
- Deleting Extractions and Observations leaves canonical Notes unchanged and permits both derived layers to be rebuilt.
- Credentials use the OS secure store by default and never enter notebook files or logs.
- Deleting caches and rebuilding preserves the deterministic graph.
- Deleting or archiving the notebook does not require coordination with a Fieldnotes service.

## 26. Summary

Fieldnotes is deliberately small in concept:

```text
Fields collect.
Users add.
Notes preserve evidence in time.
Extractions recover explicit source content.
Observations interpret cited evidence.
Identities connect appearances.
Entities and relationships form a derived graph.
Proposals hand possible actions to humans or external AI.
```

The notebook is local, readable, portable, mergeable, explainable, and disposable. Rust supplies the native cross-platform implementation. Deterministic processing supplies the default trust model. Optional built-in enhancement may add Extractions and Observations, but it never becomes a general inference platform and never changes the boundary: canonical Notes remain untouched, claims remain evidence-backed, and only the user or their external AI decides whether to act.
