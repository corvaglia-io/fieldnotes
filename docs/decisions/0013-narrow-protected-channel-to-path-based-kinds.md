# ADR 0013: Narrow the protected credential channel to path-based kinds

- **Status:** Accepted on 2026-08-23
- **Date:** 2026-08-23

## Context

[A2](../approvals/A2-field-protocol.md) section 12 admits four protected
channel kinds for delivering credential material to a Field:
`inherited_fd`, `duplicated_handle`, `unix_socket_path`, and
`windows_named_pipe`. Turning a raw file descriptor or a duplicated OS
handle into something a Rust program can read from or write to requires
`unsafe` code — there is no safe standard-library constructor for either
operation. This workspace sets `unsafe_code = "forbid"` in
`[workspace.lints.rust]` in the root `Cargo.toml`, and every crate in the
workspace inherits it via `[lints] workspace = true` with no per-crate
override: Rust's lint system lets a crate raise a workspace lint's level but
not lower a `forbid`. A2's own approved text names a mechanism this
project's own approved lint policy prohibits building anywhere in the
workspace — a contradiction between two approved documents, not an
implementation shortfall in one crate.

This was not a single implementer's guess. It was reached independently,
during four separate implementation efforts, before any of them coordinated
with the others:

- core's protected-channel server,
  `crates/fieldnotes-app/src/credentials/channel.rs`, implements
  `unix_socket_path` and refuses to implement `inherited_fd` or
  `duplicated_handle` at all, recording the same reasoning in its module
  documentation;
- `fields/fieldnotes-field-outlook-mail/src/credential.rs`,
  `fields/fieldnotes-field-outlook-calendar/src/credential.rs`, and
  `fields/fieldnotes-field-outlook-contacts/src/credential.rs` — the three
  shipped Microsoft Fields — each implement the client end of
  `unix_socket_path` and `windows_named_pipe` with plain
  `std::os::unix::net::UnixStream::connect` and
  `std::fs::OpenOptions::open`, and each refuses `inherited_fd` and
  `duplicated_handle` with an actionable `CredentialError::UnsupportedChannel`
  rather than attempting them.

All four converged on the same two-kind, path-based subset without being
told to. `docs/security.md`'s "Authentication and protected Field IPC"
section already recorded the contradiction and said outright that it "needs
an A2 amendment narrowing the admitted kinds to the path-based two." The
project owner has approved making that amendment, recorded here with the
reasoning, the rejected alternatives, and the one consequence that remains
outstanding.

## Decision

### 1. Admit only `unix_socket_path` and `windows_named_pipe`

A2 section 12's admitted channel kinds are narrowed from four to two. Both
remaining kinds name a filesystem path in the collection request rather than
a number or handle value threaded through process creation, and both are
sufficient — not merely available — for what the channel actually needs to
do:

- the exchange the channel carries is one request and one response per
  grant (`credential_request` then `credential_response`), which a
  connect-then-read-write over a path-named endpoint serves exactly as well
  as a pre-opened descriptor would;
- both platform families open their end with a safe standard-library call
  and nothing else: `std::os::unix::net::UnixStream::connect` on Unix, and
  — the detail that makes this work at all on Windows — the *client* end of
  a Windows named pipe is just a file, openable with
  `std::fs::OpenOptions::open` against its `\\.\pipe\...` path, with no
  `unsafe` and no platform crate;
- both preserve the property A2 section 12 actually cares about: credential
  material crosses on a channel separate from the Field's standard input,
  output, and error, named in the request by reference, and never by value
  in argv, the environment, or `config`. A path is exactly as much "by
  reference" as a descriptor number is — neither is the secret itself — and
  nothing about being path-shaped instead of descriptor-shaped weakens that
  separation.

### 2. What is lost, and where the security burden moves

Descriptor inheritance and handle duplication share one property neither
path-based kind has: the channel never touches the filesystem. A descriptor
inherited across `fork`/`exec`, or a handle duplicated into a child's
inheritable handle table, exists only in process and kernel state — there is
no directory entry a second local process could stat, list, or race against.

A path-based endpoint gives up that property. The socket (or, once it can be
served, the named pipe) is a filesystem entry for the entire life of the run.
Losing fd/handle inheritance is a genuine, not merely cosmetic, reduction:
the endpoint is now something that *exists*, which means it is also
something that can be found, and the security argument shifts from "there is
nothing on disk to find" to "the thing on disk is unreadable to anyone but
the intended party, for exactly as long as it needs to exist, and only the
right party is answered." That is a weaker starting position than fd
inheritance would have given, which is exactly why section 3 below makes the
resulting obligations explicit rather than leaving them implied by "it's a
socket, sockets are fine."

### 3. The security obligations this narrowing makes load-bearing

Because the endpoint is now filesystem-visible for the run's duration, three
obligations that would have been incidental under fd inheritance are now
part of the channel's contract, not implementation detail:

1. **The endpoint must be created with permissions restricting it to the
   invoking user.** On Unix this is a `0700` directory holding a `0600`
   socket. Critically, the restrictive mode must be set **at creation**, not
   applied afterward with a `chmod`: `mkdir` followed by `chmod 0700` leaves
   a window, however brief, during which the directory is created with a
   more permissive default mode (`0777` less the umask), and a file
   descriptor another local user opened *during that window* keeps working
   after the permissions are tightened — tightening a mode does not revoke a
   handle already obtained under the looser one. Creating the containing
   directory restrictively from the start closes that window instead of
   arguing that nothing sensitive is in it yet.
2. **The endpoint must be removed when the run ends, including on failure or
   panic.** A path-based endpoint that outlives its run is a stale
   credential-adjacent artifact sitting on disk; a run that never cleans up
   because it crashed is the common case a lifecycle contract has to cover,
   not the exceptional one.
3. **A connecting party must present a per-run grant that core can refuse.**
   The path being reachable is not authorization by itself — anyone who can
   name the path can attempt to connect — so the endpoint alone must never be
   treated as sufficient proof of legitimacy. Core answers only a connection
   that presents this run's exact single-use `grant_id`, refuses an unknown
   or mismatched grant, and refuses a grant past its expiry even if the
   endpoint somehow outlived it.

These three obligations are what actually does the protecting once the
"nothing on disk to find" property is gone. `crates/fieldnotes-app/src/
credentials/channel.rs` already implements exactly this shape — a directory
created with `DirBuilder::mode(0o700)`, a socket restricted to `0600`
immediately after bind, `Drop`-driven removal on every exit path, and
grant-checked, expiry-checked serving — which is further evidence this is
the correct contract to freeze, not a new requirement invented for this ADR.

### 4. The Windows serving gap is real and is recorded as such

Serving a Unix domain socket is achievable in safe Rust today. Serving a
Windows named pipe is not: creating the server end requires `CreateNamedPipe`,
which has no safe standard-library equivalent, so core's Windows-side
listener needs either `unsafe` FFI — forbidden by the same workspace lint
this whole amendment exists to respect — or a dependency that safely
encapsulates that FFI on core's behalf. Neither exists yet in this
workspace. `crates/fieldnotes-app/src/credentials/channel.rs`'s non-Unix
`start` function documents this precisely and fails closed: it returns a
`CredentialFailure::Channel` explaining exactly what is missing, and `sync`
refuses to start a run it cannot authenticate rather than falling back to
some other delivery path.

This is a genuinely open consequence, not a detail this amendment can
absorb: **serving the protected channel on Windows is not yet implemented,
and an authenticating Field on Windows currently refuses cleanly rather than
authenticating some other route.** That is an outstanding item against
release gate R3's requirement that "auth-to-sync works on all supported OS
families" (`docs/roadmap.md`), and it is recorded there and in
`docs/security.md` rather than left implicit in this ADR alone. Narrowing
the admitted channel kinds does not create this gap — a Windows server was
always going to need `CreateNamedPipe` regardless of how many other channel
kinds A2 admitted — but it does mean the two-kind, path-based contract this
ADR freezes cannot yet be fully exercised on every platform it names.

### Alternatives rejected

- **Narrow the workspace lint with an audited crate-local exception.**
  Cargo's lint system does allow a crate to raise a workspace lint's level,
  but not to lower a `forbid` back down — so this alternative is not even a
  configuration change, it is a request to weaken the workspace policy
  itself (for example, by dropping `unsafe_code` to `deny` at the workspace
  level so an individual crate could locally `allow` it). Rejected: the
  entire reason `forbid` sits at the workspace level, rather than as a
  per-crate default every crate must remember to opt into, is that the
  credential-handling boundary is exactly the code a reviewer most wants
  guaranteed `unsafe`-free without having to check crate-by-crate. Weakening
  the workspace policy to accommodate two channel kinds would remove that
  guarantee for every crate in the workspace, not just the one that needed
  the exception, in exchange for two mechanisms that a path-based channel
  already covers without it.
- **Take a dependency that encapsulates the unsafe.** A safe wrapper crate
  around descriptor inheritance, handle duplication, or `CreateNamedPipe`
  would let core and the Fields stay free of `unsafe` themselves while still
  offering the fd/handle kinds (or, for Windows serving specifically, the
  named pipe server). This is not rejected as wrong in principle — it is the
  option `crates/fieldnotes-app/src/credentials/channel.rs`'s own module
  documentation names as the eventual path for Windows serving, "either
  `unsafe` FFI (forbidden, as above) or a safe wrapper dependency" — but it
  is rejected *for this amendment* because it does not change what A2 should
  admit today: it would add a new dependency to the trusted core/host
  boundary and to every Field's credential path, with the review burden that
  implies, purely to reach two mechanisms (`inherited_fd`,
  `duplicated_handle`) that offer no capability a path-based channel lacks
  for the one-request/one-response exchange this protocol actually needs.
  Reconsidering a safe wrapper dependency remains open specifically for
  **Windows serving** of `windows_named_pipe`, which this ADR does not
  resolve — see section 4 above — but that is a narrower question than
  whether to keep `inherited_fd` and `duplicated_handle` admitted at all.

## Consequences

- [A2 section 12](../approvals/A2-field-protocol.md#12-credential-references-and-protected-delivery)
  is corrected in place to admit only `unix_socket_path` and
  `windows_named_pipe`, with an amendment entry recording the change and
  this ADR.
- `tests/fixtures/protocol/proposed-v1/schemas/collect-request.schema.json`'s
  `$defs.credentialGrant.channel` definition drops `inherited_fd` and
  `duplicated_handle` from the `kind` enum, drops the now-unused `fd` and
  `handle` properties and their `allOf` branches, and keeps the `path`
  branch as the sole requirement for either remaining kind.
- Five of the sixteen approved transcripts carried a `collect_request` whose
  credential channel was `{"kind": "inherited_fd", "fd": 3}`:
  `04-authoritative-deletion-tombstone.ndjson`,
  `06-diagnostic-with-redaction.ndjson`,
  `10-artifact-transfer-and-dedup.ndjson`,
  `15-attachment-retention-policy.ndjson`, and
  `16-explicit-recollection.ndjson`. Each is updated to a
  `unix_socket_path` channel instead, changing no other frame member,
  sequence number, `valid`, or `expect_reject` value in any transcript.
- `docs/security.md`'s "Authentication and protected Field IPC" section no
  longer describes this contradiction as still needing an amendment, and its
  older, broader sentence naming "a dedicated anonymous pipe, inherited
  handle, or another operating-system-appropriate mechanism" is narrowed to
  match.
- `docs/roadmap.md`'s R3 release gate gains the recorded Windows-serving gap
  from section 4 above.
- `docs/approvals/A2-implementation-findings.md` gains a finding recording
  this contradiction and marks it resolved by this ADR, matching how
  [ADR 0010](0010-property-registry-relocation.md) resolved finding 7 in the
  same document.
- No frame grammar, rejection code, limit, cursor rule, checkpoint rule, or
  any other admitted channel behavior changes. `credential_response.material`
  remains the only secret-bearing member in the protocol, and the channel
  descriptor's flat, `additionalProperties: false` object shape is
  unchanged — only the set of values its `kind` member may take is narrower.
