# Command-line interface

**Status:** Proposed command surface for approval. Product boundaries and
release ownership are normative; exact command names, argument spelling, output
schemas, and exit codes are not final until their release contract is approved.

## Design rules

The executable is `fieldnotes`. Commands use literal, unsurprising verbs. The
CLI adds collection, validation, explanation, and lifecycle capabilities; it
does not gate ordinary access to notebook files.

Across the 0.1 line:

- human-readable output is the default;
- automation-oriented commands provide a stable machine-readable form,
  proposed as `--format json`;
- mutating operations identify the target notebook explicitly or use a clearly
  discovered notebook root;
- destructive operations require explicit targets and conservative defaults;
- diagnostics go to standard error when standard output is machine-readable;
- credentials never appear in arguments, normal output, logs, or notebook
  files;
- `sync` and all `fields` commands are read-only with respect to source
  systems;
- no command delivers a handback package or applies a downstream change.

Global options such as `--notebook <path>`, `--format <human|json>`, verbosity,
and color control are proposed and require a common CLI decision before their
spelling is treated as stable.

## Release map

| Release | Proposed command areas |
|---|---|
| `0.1.0` | `init`, `note`, `status`, `inspect`, `validate` |
| `0.1.1` | `fields`, `sync` |
| `0.1.2` | `rebuild`, `graph`, `entities`, `explain`, `gaps`, `merge` |
| `0.1.3` | `fields auth` and Outlook Mail operation |
| `0.1.4` | Calendar and Contacts through existing Field commands |
| `0.1.5` | Teams through existing Field commands |
| `0.1.6` | Jira through existing Field commands |
| `0.1.7` | `archive`, `prune`, `proposals`, package preparation |
| `0.1.8` | `enhancement` |
| `0.1.9` | reference completion and release hardening; no new area required |

## 0.1.0 — local notebook kernel

### Initialize a notebook

```text
fieldnotes init [path]
```

Creates the notebook directories, non-secret configuration, built-in `self`
Field, and stable instance ID. It must refuse to overwrite an incompatible or
unexpected existing tree.

### Create a user Note

```text
fieldnotes note <text>
fieldnotes note --at <datetime> <text>
fieldnotes note --file <path>
fieldnotes note --voice <audio-path>
```

Text defaults to a `self` text Note. File and voice inputs are copied into
content-addressed artifact storage before the Note references them. A voice
import remains playable without enhancement. Direct microphone capture is not
part of the 0.1 contract.

The exact flags for title, Note type, standard-input content, and occurrence
time remain proposed.

### Inspect local state

```text
fieldnotes status
fieldnotes inspect <record-id-or-path>
fieldnotes validate [path]
```

`status` summarizes notebook health without requiring a remote source.
`inspect` renders one known record and its provenance. `validate` checks the
public notebook contract and reports flatness, property-type, filename,
reference, and collision errors without rewriting files by default.

`validate` is proposed CLI naming; validation behavior is required even if it
ships under another command arrangement.

## 0.1.1 — Field protocol and local Field

### Manage Field configuration

```text
fieldnotes fields list
fieldnotes fields add <type> <label>
fieldnotes fields status [field_id]
fieldnotes fields remove <field_id>
```

The proposed local setup is:

```text
fieldnotes fields add local work --path <directory>
```

`fields add` validates the registered type, constructs a stable Field ID, and
stores non-secret configuration. Exact type tokens and configuration flags must
match the approved Field registry and capability manifests.

`fields remove` must not mutate the source. Whether it disables configuration
or removes it while retaining Notes and operational state needs an approved,
non-destructive command contract. It must not silently delete collected Notes,
artifacts, or credentials.

### Collect current source state

```text
fieldnotes sync [field_id]
                [--snapshot [--scope <scope>]]
                [--max-artifact-bytes <bytes>]
                [--media-type <type/subtype> ...]
                [--run-seconds <seconds>] [--idle-seconds <seconds>]
```

With a Field ID, `sync` runs one bounded Field. Without one, it runs every
enabled configured Field in ascending Field ID order; one Field's failure never
abandons the others. It consumes records, checkpoints, and diagnostics and
commits a cursor only after preceding notebook changes are durable.

`--snapshot` requests a scope reconciliation instead of a cursor-forward pass,
and is the only mode in which a Field's completeness claim can authorize
removing Notes it did not report. `--scope` names the scope reconciled;
without it, the scope is inferred from the single distinct `source_scope` the
Field's existing Notes carry, and the run is refused when there is not exactly
one. A Field computes its own scope value at run time and its manifest declares
only that value's shape, so the notebook is the only place core can learn it
before a run (see [A2 implementation findings](approvals/A2-implementation-findings.md)).

`--max-artifact-bytes` and `--media-type` are the two artifact retention
settings. Each resolves through flag, then the profile's `artifact_max_bytes` /
`artifact_media_types` setting, then the approved protocol default (25 MiB, and
the [ADR 0007](decisions/0007-attachment-retention-policy.md) include set). The
byte threshold may be configured in either direction between the product minimum
and the frozen 512 MiB ceiling; crossing the ceiling is refused. An attachment
excluded by either setting stays at its source, and its stable reference is
recorded in the Note's `skipped_attachments`.

Sync reconciles by the exact portable source key:

```text
(source_scope, source_identity)
```

An updated source object replaces its current Note atomically under the same
Note ID. It does not create a revision ledger. A deletion removes current
collected state only when the Field provides an authoritative tombstone or
complete-snapshot reconciliation. Partial or failed collection never implies
deletion.

Content hashes may reuse artifact bytes but never collapse distinct source
objects. Every sync is read-only with respect to its source.

Automation output is the schema-tagged `fieldnotes.sync.v1` object, one report
per Field, distinguishing created, updated, unchanged, removed (by tombstone and
by snapshot separately), renamed, artifacts stored and reused, attachments
skipped, damaged, truncated, and durable-write failures, plus the committed
cursor and its coverage, every withheld checkpoint and why, the deletion
authorization or every reason it was refused, every redacted diagnostic, any
reached conflict boundary, the rejection code when a run failed, and how the
child process ended. A run that leaves any Field short of `complete` exits 3
while still reporting every Field, because durable work committed before a
failure stands.

Each run also records its outcome to the reserved
`.fieldnotes/state/sync/<field_id>.status.json`, whose reader tolerates members
it does not interpret.

## 0.1.2 — graph, rebuild, and merge

### Rebuild derived state

```text
fieldnotes rebuild
fieldnotes graph rebuild
```

These names may be consolidated during CLI approval. Rebuild recreates
disposable indexes and deterministic projections from public notebook files,
non-secret configuration, and versioned rules. It does not refetch Fields and
does not rewrite source systems.

### Inspect entities and explanations

```text
fieldnotes entities list
fieldnotes entities show <id-or-identity>
fieldnotes entities candidates
fieldnotes explain <derived-id>
fieldnotes gaps
```

These commands expose exact identities, merge candidates, relationship
evidence, conflicts, and unresolved gaps. Display-name similarity must remain a
candidate rather than an automatic union.

### Merge notebook material

```text
fieldnotes merge <path>
```

Merge preserves producer provenance while reconciling exact Note IDs and exact
portable source keys. Same-ID divergent files and ambiguous portable-source
state become visible conflicts; last-writer-wins is forbidden. A content hash
alone never removes a Note.

The exact dry-run, conflict-report, and source/destination flags require
approval before merge performs writes. A non-mutating preview should be the
default or be clearly available.

## 0.1.3 — authentication and Outlook Mail

### Authenticate a Field

**Implemented.**

```text
fieldnotes fields auth <field_id> [--no-browser]
```

The command runs the OAuth 2.0 authorization-code flow with PKCE in the user's
browser, on an ephemeral loopback redirect, and stores the resulting refresh
token in the platform credential store. Nothing is entered as an argument, and
nothing credential-shaped is printed.

**Device-code flow is implemented nowhere in Fieldnotes.** It is a phishing
vector, and it is blocked by Conditional Access in tenants that require a
compliant device (observed returning `AADSTS53003` even on a compliant one). A
Field whose manifest declares `oauth_device_code` is refused with that
explanation rather than accommodated.

Core keeps the refresh token and never gives it to a Field. Before each
collection run it mints a short-lived access token and delivers only that, on
the protected channel A2 section 12 defines, separate from the Field's standard
input, output, and error.

The setup sequence is:

```text
fieldnotes fields add outlook_mail work \
  --executable /usr/local/bin/fieldnotes-field-outlook-mail \
  --config credential_profile=work
fieldnotes fields auth outlook_mail_work
fieldnotes sync outlook_mail_work
```

#### Credential configuration keys

A Field that authenticates is configured by **reference, never by value**,
through the same `--config` map as any other non-secret setting. Only the first
key is required; every other has a default and every one is overridable.

| Key | Default | Meaning |
|---|---|---|
| `credential_profile` | *(required)* | The non-secret name the credential is stored under. A name, not a secret. |
| `credential_provider` | `keychain` | `keychain` (macOS Keychain, Windows Credential Manager, Linux Secret Service) or `environment`. |
| `credential_env_var` | `FIELDNOTES_CREDENTIAL_<PROFILE>` | The variable the `environment` provider reads. |
| `oauth_client_id` | the shared first-party client below | The OAuth public client ID. |
| `oauth_tenant_id` | `organizations` | The authority tenant segment. `organizations` resolves to the signed-in user's home tenant. |
| `oauth_authority` | `https://login.microsoftonline.com` | The authority base URL. Must be `https`. |
| `oauth_redirect_path` | *(none)* | The loopback redirect path. Empty by default: an authorization server waives the port when matching a loopback redirect URI but compares the path exactly, and a public-client registration conventionally names no path. Set one only if yours does. |

The `environment` provider is explicit, opt-in, and **read-only**: it reads a
refresh token a person or a CI system placed in the named variable, into core's
memory only, and never into a child process's environment. `fields auth` refuses
for it rather than appearing to have saved something.

#### The default client ID is shared, and that is visible in your tenant

The out-of-the-box `oauth_client_id` default is
`14d82eec-204b-4c2f-b7e8-296a70dab67e`, the **Microsoft Graph PowerShell**
first-party public client. It is the default because it works in a tenant with
no app registration and no administrator involvement, which is what makes a beta
usable.

It has a real cost: **reads are attributed to that application in the tenant's
sign-in logs**, so an administrator auditing access sees "Microsoft Graph
PowerShell" rather than "Fieldnotes", and a Conditional Access policy scoped to
that application applies to Fieldnotes' reads. `fields auth` says so on every
run that uses it. A deployment should register its own application and set
`--config oauth_client_id=<guid>`.

#### Scopes come from the Field, not from the command

Core requests exactly the least-privilege **resource** scopes the Field's own
manifest declares, plus three of its own that grant access to nothing:

| Scope | Why core adds it |
|---|---|
| `offline_access` | So a refresh token is issued at all. |
| `openid` | So the authorization server says **which account** signed in, by returning an ID token. |
| `profile` | So that ID token carries the human-recognizable `preferred_username` claim. |

The last two are requested **for identification, not for access**. Neither grants
access to any data and neither needs administrative consent. `email` is
deliberately not requested: `profile` already yields a recognizable name.

Core never requests a broader resource set, and the protected channel refuses a
Field that later asks for a scope outside its grant. `fields auth` runs
`describe` first to read those scopes, which is safe because A2 gives a describe
run no credential grant, no cursor, and no staging directory.

#### `fields auth` tells you which account you signed in as

A browser silently reuses whatever sign-in session is already open, so
authenticating several Fields in several flows can store credentials for several
different people without saying so. `fields auth` therefore reports an `account`
line, and `fieldnotes.fields_auth.v1` gained `credential_account`:

```text
Authenticated Field outlook_mail_work
  profile     work
  stored in   keychain
  account     mailbox.owner@example.test
  ...
```

The account is read from the ID token the `openid` scope makes the server return
and is recorded in the Field's non-secret configuration file. It is a **label for
you to confirm, never an authorization input** — nothing in Fieldnotes grants or
denies anything on it. The ID token it came from is never logged, never
persisted, and never printed; see `docs/security.md`.

If the sign-in returns no account claim, the account is reported as `unknown`
rather than guessed, and the credential is still stored.

#### Fields signed in as different accounts are called out

When more than one Field in a notebook has a recorded account and they do not all
agree, `fields auth`, `fields status`, and `sync` all print a prominent warning
naming every account and the Fields it belongs to:

```text
WARNING  this notebook's Fields are authenticated as different accounts:
           mailbox.owner@example.test  outlook_calendar_work, outlook_mail_work
           tenant.admin@example.test   outlook_contacts_work
         This is legitimate if you meant to collect a shared or delegated mailbox
         alongside your own. If you did not, a browser reused an existing sign-in
         session during `fields auth`, and one of these Fields is authenticated as the
         wrong person.
         Sign that account out, then run `fieldnotes fields auth <field_id>` again.
```

It is a **warning, not a refusal**: collecting a shared or delegated mailbox
alongside your own legitimately means two accounts, so Fieldnotes names what it
found and leaves the judgement to you. Exit codes are unaffected. All three
commands gained a `credential_account_mismatch` member in JSON — `{accounts:
[{account, field_ids}], advice, remedy}` — `null` when the accounts agree or
fewer than two are recorded.

The check is notebook-wide even when you name one Field, because "your Fields are
signed in as different people" is true of the notebook whichever Field you asked
about.

#### A collection run never opens a browser

`sync` refreshes silently or fails with an instruction. A missing, expired, or
revoked credential fails the run **before** any child process is spawned and
before any staging directory is created, and never advances a cursor. Setting
`FIELDNOTES_NON_INTERACTIVE` makes `fields auth` itself refuse with the same
instruction rather than opening a window on an unattended machine.

Auth never grants Fieldnotes write behavior. Outlook collection cannot send,
move, delete, flag, categorize, or otherwise mutate mail.

### See whether a Field is authenticated

`fieldnotes fields status` reports a `credential` line, and
`fieldnotes.fields_status.v1` gained `credential_profile`,
`credential_provider`, and `credential_state`. The state is one of
`not_required`, `not_configured`, `stored`, `absent`, or `unavailable`, read
from the credential store without attempting a sync, without a network call,
and without starting a process.

It also reports an `account` line for every Field that names a credential
profile, and `fieldnotes.fields_status.v1` gained `credential_account` (`null`
when unknown) plus the notebook-wide `credential_account_mismatch`. Because the
account lives in the Field's configuration rather than in sync state, it is
answerable **before the first sync**. A credential stored before Fieldnotes
recorded accounts reports:

```text
    account            unknown; run `fieldnotes fields auth outlook_mail_work` to record it
```

`fieldnotes.sync.v1` gained a `credential` member, `null` for a Field that needs
none: the profile, the provider, the granted scopes, and how many requests the
protected channel answered and refused. It carries no material, and neither does
the `credential` object recorded in
`.fieldnotes/state/sync/<field_id>.status.json`.

`fieldnotes.sync.v1` also gained `credential_account`
(`{account, previous_account, changed_since_last_sync}`, `null` for a Field that
names no credential profile) and the notebook-wide
`credential_account_mismatch`. The per-Field block is reported on **every** run
of an authenticating Field, including one that refused before spawning anything —
which is exactly the case that motivated it: a collection that fails because the
account it authenticated as has no mailbox should say, in the same report, which
account that was. `previous_account` is set when the credential was
re-authenticated as somebody else since this Field's last successful sync, which
is reported prominently and, for the same reason as the mismatch warning, still
not refused. The status file records `credential_account` so the next run can
make that comparison; it is a name, never material.

## 0.1.4 — Outlook Calendar and Contacts

Calendar and Contacts reuse the approved Field-management, authentication, and
sync commands rather than adding top-level verbs.

```text
fieldnotes fields add outlook-calendar work --account sam@example.net
fieldnotes fields add outlook-contacts work --account sam@example.net
fieldnotes fields auth outlook_calendar_work
fieldnotes fields auth outlook_contacts_work
fieldnotes sync outlook_calendar_work
fieldnotes sync outlook_contacts_work
```

Type tokens, resulting IDs, and flags are proposed. The Fields remain
independently configurable even when they share a Microsoft credential profile.
No command creates or changes an event, invitation, attendance response, or
contact.

## 0.1.5 — Microsoft Teams

Teams also uses the common Field surface:

```text
fieldnotes fields add teams work --account sam@example.net
fieldnotes fields auth teams_work
fieldnotes fields status teams_work
fieldnotes sync teams_work
```

The approved capability manifest and status diagnostics determine the chats,
channels, replies, meeting references, and history that this Field can actually
read. The CLI must expose missing permission, admin-consent, inaccessible
history, throttling, and partial-content conditions rather than implying full
coverage.

No Teams send, reply, reaction, membership, meeting, or channel mutation is in
scope.

## 0.1.6 — Jira

Jira proves the common surface is not Microsoft-specific:

```text
fieldnotes fields add jira acme --site <jira-site>
fieldnotes fields auth jira_acme
fieldnotes fields status jira_acme
fieldnotes sync jira_acme
```

The site flag, auth method, and capability manifest remain proposed. Jira
status and priority may be preserved only as explicit source values. No command
edits, transitions, comments on, assigns, or prioritizes an issue.

GitHub commands and Field types are not part of the 0.1 line.

## 0.1.7 — lifecycle, proposals, and package preparation

### Archive and prune

```text
fieldnotes archive <period>
fieldnotes prune --older-than <duration>
```

Exact selection syntax and archive format require lifecycle approval. Archive
must round-trip, and prune must protect shared artifacts and referenced
dependencies. Both operate on disposable working material; neither contacts a
Fieldnotes service or source system.

Destructive execution must provide a reviewable plan or dry run and require an
explicit confirmation mechanism suitable for both people and automation.

### Review proposals

```text
fieldnotes proposals list
fieldnotes proposals show <proposal-id>
fieldnotes proposals prepare [selection]
fieldnotes proposals review <proposal-id> <status>
```

These names and the review-state vocabulary are proposed. A proposal is
vendor-neutral, human-readable preparation for a possible downstream change.
It is not an executable vendor request. Human review state must survive derived
graph rebuilds through the approved durable-state boundary.

### Prepare a handback package

```text
fieldnotes package prepare [selection] --output <path>
fieldnotes package inspect <path>
```

The package command family and all flags are proposed until selection,
dependency closure, artifact handling, sensitive-material policy, manifest,
size, and retention rules are approved.

Preparation gathers selected Notes and required references into a portable,
reviewable bundle. `inspect` validates or summarizes that bundle. The 0.1 CLI
has no `send`, `deliver`, `push`, or `apply` package command. It does not log in
to a destination, translate the manifest into a vendor payload, or claim a
change was accepted.

## 0.1.8 — optional built-in enhancement

```text
fieldnotes enhancement status
fieldnotes enhancement enable
fieldnotes enhancement disable
fieldnotes enhancement rebuild
```

An explicit engine-asset install or package step may be added after the engine
and distribution decision. There is no command for choosing a provider,
endpoint, prompt, arbitrary model, or custom extractor.

Enabling enhancement is an explicit user choice. Default `sync`, local Note
creation, search, graph rebuild, and package preparation must continue to work
without a model download, GPU, or inference.

Enhancement writes separate, cited Extractions and Observations. Rebuild never
mutates canonical Notes. Invalid source spans, audio ranges, evidence IDs, or
property types are rejected rather than written.

## 0.1.9 — release closure

`0.1.9` completes help text, shell-facing consistency, stable automation
schemas for approved commands, troubleshooting, and platform packaging. It
does not require a new command family.

The release gate must verify every documented command on its supported
platforms and map it to integration tests or an explicitly approved scope
correction.

## Proposed output and exit behavior

Human output should lead with the outcome and include actionable recovery when
something is incomplete. JSON output should be versioned, free of prose-only
parsing requirements, and stable for the commands explicitly approved for
automation.

The final exit-code table requires CLI approval. It should distinguish at least
successful completion, validation or usage failure, incomplete Field
collection, authentication failure, preserved merge conflict, and internal or
protocol failure without exposing credentials.

For a multi-Field sync, `0.1.1` settles this as: one Field's failure makes the
process exit non-zero (3) while preserving every other Field's successful durable
work, and the summary identifies every Field's outcome and which cursor each one
committed. The full exit-code table across the whole command surface still
requires CLI approval.

## Commands deliberately absent

The 0.1 CLI has no command that:

- writes to Outlook, Calendar, Contacts, Teams, Jira, or another source;
- delivers or applies a prepared package;
- selects an arbitrary inference provider, endpoint, prompt, or model;
- creates an immutable source-revision ledger;
- collects from GitHub;
- requires a hosted Fieldnotes account or daemon.
