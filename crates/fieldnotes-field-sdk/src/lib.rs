//! A reusable SDK for writing a Fieldnotes Field: the small, sibling child
//! process that speaks the approved A2 process protocol
//! (`fieldnotes_field_protocol`) over its own standard input and output.
//!
//! # What this crate is
//!
//! Everything here is protocol-boundary bookkeeping that any conforming
//! Field needs, regardless of what source system it maps:
//!
//! - [`percent`]: byte-safe percent-encoding for embedding a path- or
//!   identifier-shaped string inside a cursor, without ever splitting a
//!   multi-byte UTF-8 character.
//! - [`stage`]: hashing local bytes once and staging them under a handle for
//!   core to verify, plus the `a<seq>`-style handle-naming convention that
//!   pairs a staged artifact with the record that references it.
//! - [`truncate`]: byte-bounded, UTF-8-boundary-safe text truncation, for
//!   fitting a record's body or a diagnostic's message under its declared
//!   byte bound without panicking or splitting a character.
//! - [`scope`]: deriving a stable, non-secret portable scope from a local
//!   identifier via SHA-256, so a scope never embeds anything
//!   user-identifying verbatim.
//! - [`emit`]: a frame emitter that self-polices against one run's declared
//!   diagnostic, record, and cursor-size limits, and stops writing after the
//!   first frame-write failure -- while still admitting the raw, unchecked
//!   writes a conformance counterparty needs to misbehave on purpose.
//! - [`dispatch`]: the argv-parsing and request-frame-reading boilerplate
//!   every Field's `main` repeats: selecting the one v1 operation token, and
//!   reading and matching the one request frame each operation expects.
//!
//! It was extracted from `fields/fieldnotes-field-local` once that Field was
//! built end to end against the approved A2 protocol and had to invent all
//! five, rather than designed speculatively before any real Field existed.
//! See `docs/decisions/0009-field-sdk-extraction.md`.
//!
//! # What this crate deliberately is not
//!
//! It contains **no vendor logic**: nothing here knows about a filesystem, a
//! mailbox, a calendar, or any other specific source. Its only dependency in
//! this workspace is `fieldnotes-field-protocol`, for the wire contract
//! itself. It never depends on `fieldnotes-format`, `fieldnotes-store`, or
//! `fieldnotes-app`: a Field does no notebook byte work, and pulling the
//! canonical serializer into every Field binary is exactly what the A2
//! normalized-envelope decision exists to prevent.
//!
//! It also does not make misbehavior impossible to express. [`emit::Emitter`]
//! offers raw, unchecked writes alongside its self-policing ones, because
//! `fields/fieldnotes-field-fixture` -- a deliberately misbehaving
//! conformance counterparty -- needs to emit malformed, oversized, and
//! hostile output on purpose so that core's rejection of it is actually
//! exercised.

pub mod dispatch;
pub mod emit;
pub mod hex;
pub mod percent;
pub mod scope;
pub mod stage;
pub mod truncate;
