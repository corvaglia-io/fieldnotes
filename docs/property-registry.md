# Property and record-type registry

**Status:** Proposed for approval milestone A1  
**Compatibility:** Names, meanings, and types are not frozen until approved with exact Markdown fixtures.

## Rules

- A property name has one meaning and one type everywhere in a notebook.
- Allowed types are text, finite number, boolean, date, offset-bearing datetime, and homogeneous one-dimensional lists of those scalar types.
- Missing values are omitted. Lists remain lists even with one member.
- Source-specific properties use the registered Field prefix.
- Datetimes use RFC 3339 with an explicit numeric offset. Filename timestamps use UTC.
- Frontmatter never contains secrets, nested objects, arrays of objects, arbitrary tags, or unbounded vendor payloads.

## Required Note properties

| Property | Type | Meaning |
|---|---|---|
| `id` | text | Global Fieldnotes Note ID |
| `instance_id` | text | Instance that first or currently-surviving producer metadata identifies |
| `field_id` | text | Field paired with `instance_id` |
| `type` | text | One primary Note type |
| `occurred_at` | datetime | Best source/user event instant with explicit offset |

External Notes also require `source_scope` and `source_identity` whenever a stable portable source key exists.

## Shared Note properties

| Property | Type | Meaning |
|---|---|---|
| `captured_at` | datetime | When Fieldnotes durably wrote the current Note form |
| `started_at` | datetime | Start of an interval |
| `ended_at` | datetime | End of an interval |
| `duration_seconds` | number | Duration in seconds |
| `source_scope` | text | Connector-namespaced portable source authority/account scope |
| `source_identity` | text | Stable object identity within `source_scope` |
| `source_parent_id` | text | Source-local parent object identity |
| `source_url` | text | Canonical source URL when available |
| `source_version` | text | Reliable opaque source version used only when the source supplies one |
| `collected_by` | list[text] | All known `<instance_id>/<field_id>` producers, including the primary pair, when more than one exists |
| `content_hash` | text | Versioned normalized-content hash; not source identity |
| `from` | text | Sender or caller identity |
| `to` | list[text] | Direct recipients |
| `cc` | list[text] | Carbon-copy recipients |
| `bcc` | list[text] | Observable blind-copy recipients when legitimately available |
| `organizer` | text | Event or meeting organizer identity |
| `participants` | list[text] | People involved regardless of source-specific role |
| `subject` | text | Message-like subject |
| `title` | text | Human-facing artifact or event title |
| `thread_id` | text | Normalized or source-provided thread identity |
| `conversation_id` | text | Conversation identity |
| `reply_to` | text | Related Note ID or qualified source object reference |
| `related` | list[text] | Related Fieldnotes record IDs |
| `attachments` | list[text] | Role-ordered attachment artifact IDs; every member also appears in `artifacts` |
| `artifacts` | list[text] | Sorted artifact IDs carried by the Note |
| `audio_duration_seconds` | number | Duration of an audio artifact |
| `audio_media_type` | text | Media type of an audio artifact |
| `identities` | list[text] | Namespaced identity anchors present in the Note |
| `entities` | list[text] | Resolved Fieldnotes entity IDs |
| `damaged` | boolean | Known content loss or corruption |
| `truncated` | boolean | Source or rendered content is incomplete |
| `lost_characters` | number | Measurable detected character loss |

`source_version` and `collected_by` arise from current-state reconciliation and
cross-instance exact deduplication. When `collected_by` is present it is
self-contained; readers do not union an omitted primary pair.

For `fn-record-v1` semantic comparison, `id`, `instance_id`, `field_id`,
`captured_at`, `collected_by`, `source_version`, and `content_hash` are
bookkeeping and are excluded. `entities` and `related` are rebuildable
projection links and are also excluded. Other registered Note properties are
source-semantic unless a later registry revision explicitly classifies them;
connectors cannot create ad hoc exclusions.

## Proposed Note types

| Type | Intended use |
|---|---|
| `text` | User-authored general Note |
| `message` | Chat or message-system item |
| `mail` | Mail message where distinct mail semantics are useful |
| `meeting` | Meeting record |
| `call` | Call record without an imported voice recording as its primary identity |
| `ticket` | Issue or ticket-system item |
| `document` | Text-bearing document record |
| `file` | Generic imported or collected file |
| `contact` | Source contact record |
| `event` | Calendar or other time-bounded event |
| `voice` | User-supplied playable voice recording |

A1 must explicitly decide whether `mail` remains distinct from `message`, whether Calendar uses `event` or `meeting`, and whether a file with a rendered document is primarily `file` or `document`. Source nuances never create ad hoc primary types; they use prefixed properties.

## Proposed record ID prefixes

| Prefix | Record kind |
|---|---|
| `fn_` | Fieldnotes instance |
| `note_` | Note |
| `artifact_sha256_` | Immutable original artifact bytes followed by their 64-hex SHA-256 digest |
| `ext_` | Extraction |
| `obs_` | Observation |
| `ent_` | Entity |
| `rel_` | Relationship |
| `prop_` | Proposal |
| `pkg_` | Handback package |
| `conf_` | Reconciliation conflict bundle |

The proposed logical-record value portion is lowercase UUIDv7. Immutable
original artifacts deliberately use their exact byte hash instead; a future
mutable document or attachment-occurrence record would require a separate
approved logical kind.

## Derived-record properties

| Property | Type | Meaning |
|---|---|---|
| `generated_at` | datetime | Time a derived current projection was produced |
| `generator_version` | text | Versioned deterministic or enhancement generator contract |
| `source_note_id` | text | Single Note enhanced by an Extraction |
| `evidence_spans` | list[text] | Validated normalized-text offsets or audio time ranges |
| `supported_by` | list[text] | Note and/or Extraction IDs supporting an Observation |
| `confidence` | number | Bounded generator confidence where meaningful |
| `subject_entity_id` | text | Entity that an Observation describes |
| `from_entity_id` | text | Relationship source entity |
| `to_entity_id` | text | Relationship target entity |
| `first_seen` | datetime | Earliest supporting current evidence instant |
| `last_seen` | datetime | Latest supporting current evidence instant |
| `channels` | list[text] | Deterministically observed source channels |
| `evidence_count` | number | Reproducible count of supporting current records |
| `evidence` | list[text] | Bounded supporting record IDs |
| `binding_status` | text | Entity-proposal binding state; one of `bound`, `unresolved`, `ambiguous` |
| `entity_id` | text | Proposal subject entity |
| `subject_identity` | text | Stable normalized identity anchor used to rebind a proposal subject after graph rebuild |
| `target_field_id` | text | Optional read-only destination/source Field hint |
| `target_source_id` | text | Optional existing target object hint |
| `status` | text | Public proposal-state projection; initial values `proposed`, `accepted`, `rejected`, `superseded` |
| `detected_at` | datetime | Time a current reconciliation conflict was detected |
| `candidate_fingerprints` | list[text] | Sorted semantic-record fingerprints in a conflict bundle |
| `involved_note_ids` | list[text] | Note IDs involved in a conflict |
| `producer_references` | list[text] | Sorted `<instance_id>/<field_id>` producers involved in a conflict |
| `source_identities` | list[text] | Qualified source identities involved in a conflict |
| `source_scopes` | list[text] | Portable source scopes involved in a conflict |

Extraction- and Observation-specific values such as `language`, `address_form`, `salutation`, `signoff`, `preferred_language`, `preferred_address_form`, and `usual_signoff` require their own approved capability fixtures before entering the stable registry.

The A1 proposal makes `status` a readable public projection backed by durable
private proposal intent. The `0.1.7` lifecycle gate still decides the exact
private-state file and command transition contract so rebuild cannot erase a
human decision.

For an entity-targeting proposal, `binding_status: bound` requires
`entity_id`; `unresolved` or `ambiguous` prohibits it. `subject_identity`
remains required in all three states and is the stable rebinding key.

## Proposed derived types

The A1 envelope proposal reserves `person`, `organization`, and `artifact` entities,
`person_person` relationships, `same_note_id` conflicts, `entity_update`
proposals, and `handback_package` manifests. Extraction and Observation types
remain capability-gated for `0.1.8`; candidate fixtures demonstrate their flat
envelopes without approving every enhancement property or generator.

## Proposed list semantics

Canonical serialization needs to know whether list order carries meaning.

- Sort and deduplicate set-like lists: `collected_by`, `identities`, `entities`,
  `related`, `artifacts`, `channels`, `supported_by`,
  `candidate_fingerprints`, `involved_note_ids`, `producer_references`,
  `source_identities`, and `source_scopes`.
- Preserve source/role order for `to`, `cc`, `bcc`, `participants`, and
  `attachments`.
- Preserve coordinate or generator-declared order for `evidence_spans` and
  `evidence`; their producing rule must make that order deterministic.

New list properties must declare one of these semantics with their registry
entry.

## Proposed Field prefixes

| Field | Prefix |
|---|---|
| local reference | `local_` |
| Outlook Mail | `outlook_mail_` |
| Outlook Calendar | `outlook_calendar_` |
| Outlook Contacts | `outlook_contacts_` |
| Microsoft Teams | `teams_` |
| Jira | `jira_` |

The prefix belongs to the registered Field type, not the user's configured label. A Field may use a shared property only when the source value has exactly the registered meaning and type.

## A1 approval evidence

A1 freezes this registry together with representative exact-byte fixtures for
`self` text/file/voice Notes, local material, mail, Calendar, Contacts, Teams,
Jira, damaged content, derived envelopes, handback packages, and conflicts.
The corpus README marks later-gate semantics that remain illustrative. IG1
parser tests must accept the normative fixtures, add the missing primary-type
templates, and reject nested, mistyped, unprefixed, timezone-less,
duplicate-key, and secret-bearing examples.
