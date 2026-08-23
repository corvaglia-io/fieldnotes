# Test fixtures

This directory will contain sanitized, deterministic fixtures approved at the contract gates:

- `notebooks/`: byte-exact public notebook examples approved at A1;
- `protocol/`: JSON Schema transcripts and process-failure cases approved at A2;
- `hashes/`: canonical artifact, normalized-content, and semantic-record vectors approved at A1.

No live credentials, proprietary account data, or unsanitized vendor payloads may be committed here.

Illustrative people, addresses, machine paths, and Field labels in these fixtures are fictional. The notebook-owner persona is `sam` / `sam@example.net`; the other participants are `alice@example.com` (Alice Müller), `bob@example.net` (Bob Rossi), and `former.colleague@example.com`; illustrative absolute paths live under `/home/user/`. See [ADR 0011](../../docs/decisions/0011-neutralize-illustrative-personal-identifiers.md) for why, and for the digests it recomputed.

