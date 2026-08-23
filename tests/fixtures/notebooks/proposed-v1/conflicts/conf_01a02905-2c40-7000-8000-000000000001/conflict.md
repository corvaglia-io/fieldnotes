---
id: conf_01a02905-2c40-7000-8000-000000000001
type: same_note_id
candidate_fingerprints:
  - "fn-record-v1-sha256:34179e54a18180b6ba011fe21c129028d2cbe8b04944a7e53fdeb5f6e296a2ba"
  - "fn-record-v1-sha256:673055d893d0aa9e25a4abc19aa1a3a32fc59b5ab5f531e8ad00c69fff6665e1"
detected_at: 2026-08-22T12:30:00+02:00
involved_note_ids:
  - note_01a028ea-9f60-7000-8000-00000000000b
producer_references:
  - fn_01a02837-2de0-7a2b-8c41-f2481851192a/outlook_mail_work
source_identities:
  - mail-message/AAMkAGI2CONFLICT01
source_scopes:
  - "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001"
---

# Same Note ID conflict

Two validated candidates carry the same global Note ID and portable source key
but have divergent current bodies. Same-ID divergence is always a conflict, so
neither candidate is active while this bundle is unresolved.

`candidate_1.md` has the lexicographically smaller semantic-record fingerprint.
The fingerprints are verified A1 vectors computed from the candidates' exact
canonical semantic records and the `fieldnotes-record-v1` hash domain.
