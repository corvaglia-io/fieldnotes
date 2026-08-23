# Security and privacy

**Status:** Proposed v0.1 security model; protected IPC and installation details
are approval-gated

Fieldnotes collects sensitive work context from external systems into a local
notebook. Its security goal is to avoid creating unnecessary credential and
execution risk while making the limits of local file storage explicit.

Fieldnotes is not a compliance archive, data-loss-prevention system, malware
sandbox, or secure-erasure tool.

## Assets to protect

The security model distinguishes:

- **credentials:** refresh tokens, access tokens, passwords, API keys, client
  secrets, session cookies, signed URLs, and credential-provider output;
- **sensitive content:** messages, contacts, recordings, documents, identities,
  source scopes, graph relationships, and derived observations;
- **integrity state:** instance and Field IDs, source keys, current Notes,
  artifacts, checkpoints, identity overrides, and proposal review decisions;
- **executables and dependencies:** the Fieldnotes binary, trusted Field child
  processes, renderers, optional model assets, and their supply chain.

Sensitive content is not automatically a secret in the credential sense, but
it still requires clear storage, sharing, archive, and deletion behavior.

## Trust model

Fieldnotes assumes the local operating-system account and notebook owner are
trusted to read the notebook. Source data, downloaded artifacts, imported
files, merge inputs, archive members, and protocol frames are untrusted data.

First-party or explicitly approved Field executables are trusted code. The
child-process boundary isolates dependencies and failures; it is not a sandbox
that can safely contain a malicious connector. A Field granted a credential can
use that credential and can read whatever the operating system permits.

v0.1 must not silently execute a conventional `PATH` match or hand credentials
to an executable merely because it returns a plausible manifest. Exact
discovery, path pinning, installation provenance, upgrade, and trust-confirmation
UX require approval before external Field distribution is enabled.

## Credential storage

Configuration contains a named credential-profile reference, never a secret:

```yaml
credential_profile: microsoft_acme
```

Default providers are:

- macOS Keychain;
- Windows Credential Manager or DPAPI-backed secure storage;
- Linux Secret Service through the available desktop keyring.

If the default provider is unavailable, Fieldnotes explains the problem and
requires an explicit supported alternative. It never silently falls back to a
plaintext notebook or configuration file.

The roadmap includes an explicit environment-variable provider for CI and
headless use in `0.1.3`. That provider reads a named value into core memory; it
does not imply that secrets are copied into a child process environment,
configuration, cursor, or diagnostic.

External-command and local-file credential providers are not implicit v0.1
capabilities. If later approved, an external command is arbitrary code execution
and a local file requires explicit warnings and restrictive permission checks.

## Authentication and protected Field IPC

`fieldnotes fields auth <field_id>` performs the interactive authorization-code
flow with PKCE on an ephemeral loopback redirect. Long-lived credentials go
directly to the selected credential provider. Short-lived access material
remains in memory for the minimum useful time.

**Device-code flow is implemented nowhere.** It is a phishing vector — a user
can be walked into approving an attacker-generated code on a genuine login page,
with nothing distinguishing the two attempts — and it is blocked by Conditional
Access in tenants that require a compliant device (observed returning
`AADSTS53003` even on a compliant device). A Field declaring it is refused with
that explanation rather than accommodated.

As implemented in `0.1.3`:

- **core owns refresh.** The refresh token is stored by the credential provider
  and never leaves the Fieldnotes process. A Field never sees one, and a
  manifest declaring `refresh_owner: field` is refused.
- **a Field receives only an access token**, minted immediately before its run
  and expiring on its own.
- **the protected channel is a per-run, path-based endpoint core creates, names
  in the collection request, serves for the length of one run, and destroys when
  the run ends.** On Unix it is a Unix-domain socket, created inside a directory
  whose `0700` mode is set at creation, with the socket itself `0600`. A
  connection must present that run's `grant_id`; core refuses an expired grant,
  refuses a purpose it cannot serve, and refuses a scope outside the grant. The
  endpoint is removed on every exit path, including a refusal or a panic.
  Windows is the same design as `windows_named_pipe`; what is missing is the
  server end, which belongs with the rest of the per-platform child-process
  handling in the Field-protocol host crate. Until it lands there, a run that
  would need the channel on that platform is refused rather than authenticated
  some other way.

**Two of A2's four channel kinds cannot be built here at all.**
`inherited_fd` and `duplicated_handle` both require turning a raw descriptor or
handle into an I/O object, which requires `unsafe`; `unsafe_code = "forbid"` is
set workspace-wide and a crate cannot locally override a `forbid`. An approved
protocol therefore names a mechanism this project's own lint policy prohibits
implementing. This is recorded rather than worked around: core neither
implements nor ever emits those two kinds, and every first-party Field refuses
them on its side, so the contradiction cannot turn into a run-time failure on a
user's machine. It needs an A2 amendment narrowing the admitted kinds to the
path-based two.
- **a credential failure fails the run before anything is spawned**, before any
  staging directory exists, and never advances a cursor.
- **a collection run never authorizes interactively.** It refreshes silently or
  refuses with an instruction, so a scheduled run cannot open a browser.

The out-of-the-box OAuth client ID is a shared first-party public client, which
makes reads attributable to *that application* in the tenant's sign-in logs
rather than to Fieldnotes; it is overridable per Field, and a deployment should
override it. See `docs/cli.md`'s `0.1.3` section.

Fields receive only the credential material and scope needed for their run.
Secrets must never be passed in command-line arguments, normal environment
inheritance, notebook files, protocol output, process titles, cursor state, or
logs.

The exact protected channel may use a dedicated anonymous pipe, inherited
handle, or another operating-system-appropriate mechanism. It must be separate
from normal diagnostics and have explicit ownership/lifetime behavior. The
precise cross-platform channel, auth messages, refresh ownership, and memory
cleanup rules are part of the Field-protocol approval gate for `0.1.1` and the
Microsoft authentication gate for `0.1.3`; this document does not invent them.

Best-effort memory clearing reduces accidental retention but is not presented
as a guarantee against a privileged debugger, core dump, compromised process,
or malicious trusted Field.

## Process containment

Core imposes bounded collection runs:

- protocol frame, record, and total-output limits;
- stderr capture and display limits;
- timeouts, cancellation, and child termination behavior;
- sanitized environment inheritance;
- no connector-supplied filesystem destinations;
- deterministic handling of malformed UTF-8, invalid JSON, and unexpected
  protocol ordering.

Fields write no notebook files. Core validates every property, path, ID,
datetime, source key, artifact reference, and diagnostic before persistence or
display. A non-zero exit cannot advance a checkpoint beyond durable records.

Resource-limit values, cancellation grace periods, environment allowlists, and
stderr retention are approval-gated with the exact protocol schemas.

## Filesystem safety

Core derives notebook paths from validated IDs, Field IDs, types, UTC filename
timestamps, and artifact hashes. Source names and archive member names are
display metadata, not path components.

Writes use restrictive temporary files on the destination filesystem followed
by verified atomic installation. Implementations must account for symlinks,
hard links, case-insensitive collisions, reserved Windows names, path length,
and check/use races. Existing unexpected targets are conflicts, not overwrite
permission.

Imports and merges must not follow paths outside the approved source root by
accident. Archive extraction rejects parent traversal, absolute paths, unsafe
links, device files, duplicate normalized paths, and decompression bombs.

Notebook timestamps use explicit-offset RFC 3339 frontmatter. Note filenames
use the same instant rendered in UTC. Parsing a timezone-less value or trusting
a filename timestamp over validated frontmatter is not permitted.

## Untrusted content and artifacts

Markdown bodies may contain source-controlled text, links, HTML-looking
content, or code fences. Fieldnotes preserves evidence but does not execute it.
It should avoid emitting generated constructs that cause automatic network
loads or script execution in ordinary viewers where a safe literal rendering
is possible.

Artifacts are never executed or given active-content trust. Renderers and
parsers use input, output, nesting, page, decompression, memory, and time limits.
Macros and embedded scripts are not enabled. Unsupported or damaged material is
reported visibly instead of handed to an arbitrary external program without
explicit user action.

Original bytes remain separate from renditions so a compromised or lossy
renderer cannot silently replace source evidence.

## Logging, diagnostics, and redaction

Normal diagnostics contain structured metadata needed to act on a failure, not
full source payloads or HTTP traces. Connectors must classify and sanitize
diagnostics before emission, and core applies a second redaction layer before
display or persistence.

Redaction covers, at minimum:

- authorization and cookie headers;
- token, password, secret, code, and signature fields;
- credentials embedded in URLs or error strings;
- protected IPC material;
- credential-like values in pagination errors and cursors.

Redaction is defense in depth, not permission to log secrets first. Debug modes
remain secret-free by default. Any explicit sensitive-payload capture must name
its destination, permissions, retention, and risk and is outside the normal
v0.1 diagnostic contract until approved.

Tests use unique secret canaries and verify their absence from argv, inherited
environment, stdout, stderr, logs, diagnostics, cursors, Notes, artifacts,
archives, handback packages, and crash-recovery files.

## Credential handling is an internal boundary, not content scanning

Credential protection is entirely about how Fieldnotes handles credentials it
holds, never about scanning collected evidence for secret-looking text.
Enforcement is by design and by release-gate scanning of Fieldnotes' own
output:

- the `CredentialProvider` abstraction and OS keychain integrations (`0.1.3`);
- protected secret delivery to Field processes rather than command-line
  arguments;
- diagnostic and log redaction, described above;
- release gates R3 and R9 scanning argv, logs, diagnostics, cursors, Notes,
  and artifacts for credential leakage before release.

Fieldnotes performs **no secret or password scanning of notebook content**,
and never rejects a Note or other collected evidence for containing
secret-looking text. A credential appearing in a Note body was put there by a
person or upstream system, not by Fieldnotes; rejecting it would discard real
evidence, be unfixable by the user (who cannot edit the upstream mail), and
permanently break sync for that source — one colleague pasting an API key
into an email must not brick a mail Field. Entropy-based detection is also
rejected as a mechanism: `content_hash` values, artifact IDs, and UUIDs are
legitimately high-entropy and would false-positive under an entropy
heuristic, rejecting the approved fixtures themselves.

The masking use case this might otherwise motivate is served, if ever
approved, by a future, optional PII-detection capability at the `0.1.8`
enhancement gate, modelled as an Extraction: evidence-backed spans over exact
normalized-body offsets that point at text a user may choose to mask, never
altering the Note. Any such capability must remain optional, outside the
default build, and require no model download, GPU, or network by default. Its
schema is not approved; see [ADR 0006](decisions/0006-a1-implementation-rulings.md).

## Source identity and privacy

`source_scope` must be portable and stable across instances but non-secret. It
may still reveal a tenant, account, project, or repository identity and is
therefore sensitive notebook metadata. Connectors should prefer stable opaque
upstream identifiers over human account names when both provide the required
scope.

Exact cross-instance deduplication uses `(source_scope, source_identity)`.
Producer provenance uses `(instance_id, field_id)`. Neither content hashes nor
display names are promoted to identity merely to avoid duplicates.

An authoritative source update atomically replaces the current Note for that
source key. An authoritative deletion may remove it. Fieldnotes does not retain
a hidden revision ledger or tombstone that would defeat an expected deletion.
Backups, copied notebooks, archives, and filesystem snapshots may still retain
earlier content and must be managed separately.

## Network behavior

The default local notebook workflow performs no required network request after
collection. Network access occurs only for an explicit capability such as:

- source authentication and collection;
- an explicitly approved update or installation check;
- installing the optional pinned enhancement assets in `0.1.8`.

Vendor clients validate TLS normally, follow redirects conservatively, and do
not forward authorization across an authority change. Proxy, custom CA,
certificate-pinning, and offline installation behavior require product approval
and platform tests rather than undocumented environment magic.

## Merge, archive, and handback

Merge inputs are untrusted notebooks. Fieldnotes validates them before copying,
never silently overwrites same-ID divergence, and collapses independently
collected Notes only on an exact portable source key under the approved current-
state rule. A matching content hash alone does not discard context.

Archive and handback packaging are preparation operations, not network
delivery. Their approved manifests must be secret-free, checksum-covered, and
closed over required references. Destination credentials and executable vendor
payloads remain outside Fieldnotes.

Encryption-at-rest for notebooks or archives is not implicitly supplied by
Fieldnotes. Users may rely on operating-system volume protection. Any native
archive encryption and key-management design is an `0.1.7` approval gate.

## Deletion limits

Prune, source deletion, and ordinary filesystem removal are logical deletion.
Fieldnotes does not guarantee secure erase from SSDs, snapshots, backups,
archives, synchronized folders, recipient machines, or systems of record.

The notebook has no required source-history ledger or tombstone store. This
reduces hidden retention but means refetch can restore source objects still
available upstream.

## Software supply chain and releases

Dependencies, first-party Fields, renderers, and optional model assets require
license and vulnerability inventory. Release artifacts carry checksums and
approved signing/provenance metadata. Model downloads are explicit, pinned,
integrity-checked, and never triggered by inference-disabled commands.

Reproducible metadata, installer/update trust, Field discovery, SBOM shape,
signing keys, and response policy close at the `0.1.9` release gate.

## Release alignment

- **0.1.0:** safe local files, strict format validation, atomic writes, and
  secret-free local diagnostics.
- **0.1.1:** bounded Field process behavior, protocol redaction, and offline
  conformance with the local Field.
- **0.1.2:** hostile merge validation, exact source-key dedup, and safe rebuild.
- **0.1.3:** OS credential providers, protected Field delivery, Microsoft auth,
  and secret-leak gates.
- **0.1.4:** independent Calendar/Contacts least-privilege scopes and identity
  handling.
- **0.1.5:** Teams permissions/admin-consent diagnostics and partial-content
  visibility.
- **0.1.6:** Jira authentication, remote limits, and secret-safe diagnostics.
- **0.1.7:** hardened artifact rendering, archive/prune/handback validation, and
  any approved archive encryption.
- **0.1.8:** pinned model assets, evidence validation, resource limits, and no
  bring-your-own provider surface.
- **0.1.9:** security review, supply-chain inventory, release signing metadata,
  soak tests, and traceability closure.

## Approval gates

Approval is required before freezing:

- Field executable discovery, trust confirmation, and update behavior;
- protected credential IPC and refresh ownership;
- protocol limits, cancellation, and debug diagnostic behavior;
- headless/CI provider UX and any non-OS credential provider;
- renderer isolation and resource budgets;
- archive encryption and key management;
- model asset distribution, integrity, licensing, and resource envelope;
- release signing, SBOM, and vulnerability-response policy.

