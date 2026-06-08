# Working-State Handoff (UAT hardening sessions)

Last updated: 2026-06-08 (session 2). This file is the session-to-session memory: what is real,
what is still fixture, what comes next, and how to validate. Update it at the end of every working
session — a fresh session (human or agent) should be productive from this file + the spec alone.

## What landed (session 2, 2026-06-08)

- **Clinician read rewiring (next-work item 1)**. New `clients/src/clinician/project.ts` projects
  the provenance DAG and the DOB-conflict challenge from a live subgraph; `state.tsx` resolves
  each fixture patient by its stable `tok:demo:<family>` token, calls `bridge.readSubgraph()`, and
  `enrichWithSubgraph()` overlays real events + a real-target challenge (fixtures render first, the
  read is purely enriching, unseeded patients keep the fixture). DOB-challenge options now carry
  REAL Core ids: photo-ID DOB → Attest on that Assert, other DOB → **Amend on the conflicting
  Assert** (now wired through `bridge.amend` in PatientDetailPage `onCommit`, previously local-only),
  "neither" → Contest on the real Link. **Whitfield's DOB resolution now persists past a reseed.**
- **Bridge payload projection**. `ProvenanceMapper` now emits an `event-payload` extension
  (`…/StructureDefinition/event-payload`) carrying type-specific fields; `EventPayloadCbor.decodePayloadDetails`
  decodes the externally-tagged payload (Assert vm + demo tokens, Link confidence/method, Attest
  purpose, Amend DOB + reason, Contest reason) defensively via a map-safe `opt()` helper. Client
  `provenanceFromFhir` reads the extension and maps kebab-case codes → UI labels.
- **Reference architecture drift remediated** (see Validation cycle step 3).

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

## Real vs fixture (client audit, 2026-06-08, updated session 2)

| App | Real | Still fixture |
|---|---|---|
| patient | grants/share/revoke/read-back, token resolution | initial activity entry; seed.ts is dead code |
| clinician | attest/contest/**amend** writes, consent badge, **DAG + DOB-conflict challenges projected from `readSubgraph()` with REAL Assert/Link targets** | presentation fields (address, MRNs, confidence, summary); link/stale challenges; action log; request-access |
| audit | — | everything (218-line fixture ledger) |
| prior-auth | one attest | orders queue, decisions (should call `$creda-verify` — Core implements it) |
| steward | one contest | console/queue (289-line fixture) |

## Next work, in priority order

1. ~~**Clinician read rewiring**~~ — DONE (session 2). See "What landed" below.
2. **Audit ledger**: real grants/revocations/receipts via Consent search + type-filtered provenance.
3. **Prior-auth**: call `$creda-verify` for real decisions.
4. **Steward queue**; **patient activity feed** from real events.
5. **F1 (§8.5.6)**: FASTConsent-conformant projection on the existing Consent search.

### Follow-ups opened by session 2

- **Clinician presentation fields are still fixture.** `enrichWithSubgraph` overlays real events +
  the DOB challenge onto the static `PATIENTS`; address/MRNs/confidence/summary stay fixture
  because the seed dataset doesn't model them. Making those real needs a structured Patient
  projection from Core (`GetEffectiveIdentity` returns a *debug* string today — see grpc.rs).
- **Contest payload shape mismatch.** The bridge's `encodeContest` still emits the legacy
  `{"Other": <text>}` while Rust `ContestReason` is `{code, detail?}` (creda-events/payload.rs).
  `decodePayloadDetails` reads BOTH for now; reconcile the encoder to the struct and drop the
  legacy branch.
- **`$creda-amend` DOB tokenization is demo-shaped.** The clinician sends the Assert's original
  token back verbatim (round-trips with the seed). A production amend needs the real tokenizer,
  not the `tok:demo:*` passthrough.

## Validation cycle (run each session, before building)

1. `make grpc && anchor creda && make bridge` + `tsc --noEmit` in clients/ — compile truth.
2. `make -C testbed ui-up-real && make -C testbed reset` — behavioral baseline.
3. Drift check: grep clients for fixture imports (see audit table) and confirm the spec/reference
   architecture (`docs/creda-technical-spec.md` §8.5, `design/creda-reference-architecture.html`)
   still match the code. ~~**Known drift to remediate**: the reference architecture HTML predates
   GetSubgraphEvents, the plain-provider operation layout, TCP gRPC mode, and the seed/reset
   lifecycle.~~ **Remediated (session 2)**: `creda-reference-architecture.html` now documents the
   seven gRPC RPCs incl. GetSubgraphEvents + the parent→child materialize fix (engine/grpcsrv/grpc-if
   nodes), the Patient-typed `$creda-*` plain-provider layout (bridge nodes), `tcp://0.0.0.0:50051`
   testbed gRPC mode (uds/grpcsrv nodes), and the append-forward wipe-and-reseed lifecycle (dag node).
   JS validated with `node --check`.
4. Record findings here (remediate doc or code, whichever is wrong; reinforce what held).

### Session 2 validation findings

- **Clients compile clean**: `tsc --noEmit` and `tsc -b` both pass; ESLint clean on all touched
  files (one *pre-existing* `react-hooks/exhaustive-deps` warning on PatientDetailPage:72 remains,
  untouched). Projection logic was additionally runtime-verified by transpiling `project.ts` and
  exercising `projectEvents` / `projectDobChallenge` / `enrichWithSubgraph` against a synthetic
  Whitfield subgraph (real targets confirmed; no-conflict patients keep their link/stale challenges,
  so the existing e2e specs stay green).
- **NOT run in this environment** (no Docker/JDK/cargo on this host — they require the dev
  container): `make grpc && anchor creda && make bridge`, `tsc`-via-binary (used `node …/tsc.js`
  instead), `make -C testbed ui-up-real && reset`, and the Playwright e2e (browsers not installed,
  and the sandbox can't exec the esbuild/vite binaries). **Next session must run the full
  container-side cycle to confirm the Rust/Kotlin changes compile** — the Kotlin CBOR decoder and
  ProvenanceMapper extension were reviewed against existing API patterns (getType/ContainsKey/AsString)
  but not compiled here.
