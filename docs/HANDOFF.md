# Working-State Handoff (UAT hardening sessions)

Last updated: 2026-06-08. This file is the session-to-session memory: what is real, what is
still fixture, what comes next, and how to validate. Update it at the end of every working
session — a fresh session (human or agent) should be productive from this file + the spec alone.

## What landed (June 2026 sessions)

- **F0 (§8.5.6)**: authorization FHIR↔CBOR mappers (Grant/Revocation/ExportReceipt), the four
  `$creda-*` authorization operations as a HAPI *plain provider* on Patient, golden-vector CBOR
  tests (`bridge/src/test/...`). Wire rule that bites: `Uuid` = 16-byte bstr; `Vec<u8>` = CBOR
  array of ints (ciborium has no bstr special-case).
- **Read path**: `GetSubgraphEvents` RPC (proto + grpc.rs + test); `Consent?patient={id}` search;
  `$creda-provenance` (Bundle of CredaProvenance); `$creda-amend` (DOB-only). Graph fix:
  `Subgraph::materialize` now follows the parent→child index past absent entry nodes.
- **Clients**: transport translates ALL real FHIR ↔ UI shapes (consentToAuthorization,
  provenanceFromFhir) — components never see raw FHIR. Patient app fully real; clinician consent
  badge live. Patients resolve by token (`tok:demo:*`), never hardcoded ids.
- **Testbed**: `make -C testbed reset` (wipe PVC + reseed, ~90s, no cluster cycling); `seed-demo`
  driver subcommand (Maria Gonzalez linked pair + Mercy grant; James Whitfield conflicting-DOB
  pair); UAT peer gRPC now `tcp://0.0.0.0:50051` (bridge dials loopback; seed Jobs reachable);
  cachebusts on core/bridge/driver images (podman stale-COPY defense); `wait-ready` context-pinned.

## Real vs fixture (client audit, 2026-06-08)

| App | Real | Still fixture |
|---|---|---|
| patient | grants/share/revoke/read-back, token resolution | initial activity entry; seed.ts is dead code |
| clinician | attest/contest writes, consent badge | worklist, demographics, DAG view, challenges (incl. Whitfield DOB targets), action log, request-access |
| audit | — | everything (218-line fixture ledger) |
| prior-auth | one attest | orders queue, decisions (should call `$creda-verify` — Core implements it) |
| steward | one contest | console/queue (289-line fixture) |

## Next work, in priority order

1. **Clinician read rewiring**: project PatientDetailPage/WorklistPage from `bridge.readSubgraph()`
   (works end-to-end now) — demographics, DAG, DOB-conflict challenges with REAL Assert targets;
   wire the Amend branch to `bridge.amend` (machinery done). This unlocks Whitfield persistence.
2. **Audit ledger**: real grants/revocations/receipts via Consent search + type-filtered provenance.
3. **Prior-auth**: call `$creda-verify` for real decisions.
4. **Steward queue**; **patient activity feed** from real events.
5. **F1 (§8.5.6)**: FASTConsent-conformant projection on the existing Consent search.

## Validation cycle (run each session, before building)

1. `make grpc && anchor creda && make bridge` + `tsc --noEmit` in clients/ — compile truth.
2. `make -C testbed ui-up-real && make -C testbed reset` — behavioral baseline.
3. Drift check: grep clients for fixture imports (see audit table) and confirm the spec/reference
   architecture (`docs/creda-technical-spec.md` §8.5, `design/creda-reference-architecture.html`)
   still match the code. **Known drift to remediate**: the reference architecture HTML predates
   GetSubgraphEvents, the plain-provider operation layout, TCP gRPC mode, and the seed/reset
   lifecycle — it needs a validation pass and update.
4. Record findings here (remediate doc or code, whichever is wrong; reinforce what held).
