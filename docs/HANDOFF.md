# Working-State Handoff (UAT hardening sessions)

Last updated: 2026-06-12 (session 5). This file is the session-to-session memory: what is real,
what is still fixture, what comes next, and how to validate. Update it at the end of every working
session — a fresh session (human or agent) should be productive from this file + the spec alone.

## What landed (session 5, 2026-06-12) — release-readiness + closed synthetic pilot

- **Direction**: accelerate to a contributor release AND deploy a closed pilot where peers
  propagate **synthetic, non-PHI** events. Release bar = "honest + gated" (no silent fakes).
- **`docs/STATUS.md`** (authoritative real/demo/stub/open-question map) + **`docs/PILOT.md`**
  (closed-synthetic-pilot go-live runbook, hard "SYNTHETIC ONLY / NO PHI" guardrail, go/no-go).
  Root README + clients/README point to STATUS; clients fenced as DEMONSTRATION code.
- **Silent fake removed**: `PatientResourceProvider.read` → `NotImplementedOperationException`
  (was a hollow Patient); placeholder helper deleted.
- **Synthetic-only guardrail (Core)**: `CredaConfig.synthetic_only` (env `CREDA_SYNTHETIC_ONLY`,
  Helm `config.syntheticOnly`). When on: `create_event` auto-tags `test_data`; `ingest_event`
  rejects any untagged event. New `Signer::create_test_event`; tests in `tests/engine.rs`. Helm
  configmap + values wired. **Uncompiled here** — `make grpc && anchor creda` must pass.
- **#1 visible demo-data marking** (DemoData chip) shipped session 4; FE audit table is below.

✅ **Build gate GREEN (2026-06-12, end of session 5).** `make bridge` (no warnings — the
`CredaCoreClient` val-init fix landed), `make grpc`/`anchor creda` (Core + all crates), clients
(`tsc -b && vite build`), and BOTH multi-peer scenarios pass: gossip smoke (0 ms) and ae-repair
(3 events, max AE 17.5 s / 75 s budget; the namespace-race `wait --for=delete` fix held). The full
sessions 3–5 stack compiles and the multi-peer network works. The earlier "uncompiled debt" is
cleared. Note: clients use **pnpm** (`pnpm typecheck`/`pnpm build`), not npm/npx.

**Also landed:** `docs/dht-query-privacy.md` — design note + cost model + closure plan for the
§13.3 DHT query-privacy security item (OPRF+relay near-term; PIR/PSI endgame; hard gate before
real PHI; fine for the synthetic pilot). Two cosmetic follow-ups still open from session 3:
`encodeContest` legacy shape, `$creda-amend` demo tokenization.

## What landed (session 3, 2026-06-12)

- **DOB resolution rebuilt on Core's effective identity (§5.2.4/§5.3).** UAT found that resolving
  James Whitfield's DOB recorded "no change": the photo-ID option wrote an Attest (which doesn't
  mutate demographics) and the other option was a **self-referential no-op Amend** — the client
  was *reasoning about identity itself* (violating §8.3.2). Fixed by exposing Core's projection
  and consuming it:
  - **Core**: `GetEffectiveIdentity` now returns a STRUCTURED reply — `repeated EffectiveIdentityField
    {key, disputed, values[]}` / `EffectiveIdentityValue {value, confidence_bp, supporting[]}` —
    built from the existing `project()` output (confidence-weighted, attestation-amplified,
    disputed-flagged). Was a debug string. New `field_key_name()` kebab helper in grpc.rs.
  - **Bridge**: `CredaCoreClient.effectiveIdentity()` + a Patient-typed `$creda-effective-identity`
    operation returning a Parameters (field → key/disputed/value parts; each value → token +
    confidence + `support` ids).
  - **Client**: transport `effectiveIdentity()`; `project.ts` now derives the DOB field + challenge
    from Core's projection (NOT client reasoning). Displayed DOB = Core's top-confidence value;
    the challenge's options Attest the chosen value's **supporting Assert** (real target via the
    `supporting` ids) — so affirming a value raises its confidence on re-projection. The no-op Amend
    is gone from the DOB path. Mock bridge mirrors the projection for p1/p2.
  - **`$creda-attest` bridge fix (the actual persistence bug).** The op had been *ignoring*
    `references` and always attesting a throwaway per-patient root-Assert stub — so DOB attestations
    landed in a detached subgraph and Core's effective identity for the real patient never saw them
    ("Attest not persisting"). `PatientResourceProvider.attest` now attests the real events named in
    `references` (targets = parents, so it lands in the patient's subgraph); `attestReferences()`
    regex-extracts UUIDs from any encoding (proper repeated params, `Provenance/<uuid>`, or the
    legacy JSON-stringified array). Root-stub kept only as the no-reference fallback. Client `attest`
    now sends clean repeated `references` params instead of a stringified array.
  - **Behavioral note**: resolution is by *confidence/attestation*, not deletion — `disputed` stays
    true while both Asserts exist (honest: institutions still disagree). The clinician now sees a
    real, persisted effect (effective DOB + confidence shift); the Attest survives refresh and
    `make -C testbed reset` restores the baseline conflict. See follow-ups for full conflict-clearing.

## Front-end interaction audit (session 4, 2026-06-12) — running checklist

Status: ✅ real (service, correct) · ⚠️ partial (some surfaces real) · ❌ fake/broken (fixture,
local-state, or service call with ignored/fixture params). Goal: drive every row to ✅.

**Patient** — ✅ resolve(token), who-has-access(`listAuthorizations`), share(`authorize`),
revoke(`revoke`). ⚠️ activity feed: first entry hardcoded + rest local optimistic, never read from
real ExportReceipts.

**Clinician** — ✅ consent badge(`listAuthorizations`), DAG(`readSubgraph`), DOB field+challenge
(`effectiveIdentity`), DOB→Attest(real Assert, after the s3 bridge fix), DOB→Contest(real Link).
❌ challenge "resolved" state is LOCAL (`resolveChallenge` map; never re-reads Core); action log
LOCAL (`appendAction`, dies on refresh); link/stale challenges target FIXTURE event ids; request-access
= toast only. ⚠️ worklist = the 4 fixtures (no real "list patients"); presentation fields
(name/address/MRNs/confidence) fixture.

**Prior-auth** — ❌ orders queue fixture; submit→`attest` passes non-UUID refs
(`'patient-subgraph-head'`, cpt code) → root-stub fallback; **decision reads `o.decision` fixture,
should call `$creda-verify`** (Core implements it). 

**Steward** — ❌ queue/case/link-chain fixture; resolve→`contest` uses FIXTURE link ids; local state.

**Audit** — ❌ zero bridge calls; ledger/KPIs/report 100% `AUDIT_EVENTS` fixture.

### Three systemic root causes (the whack-a-mole engine)
1. **Silent fixture fallback** (`try{bridge}catch{return fixture}`, render-fixture-then-enrich) — fake
   impersonates real; only payload inspection reveals it. → **#1 fix: visible demo-data marking (DemoData chip), no silent swallow.**
2. **Local optimistic state instead of read-after-write** — a no-op write and a real write look identical.
3. **Fixture ids leak into service calls** — handlers pass `e1`/`p1`/`'patient-subgraph-head'` as real targets.

### Remediation plan
- **#1 (LANDED session 4)**: `shared/components/DemoData.tsx` chip (renders only in REAL mode;
  mock mode keeps the global MOCK BRIDGE chip). Placed on: audit ledger header, prior-auth Decision
  card ("Demo decision — not from $creda-verify"), steward queue, clinician worklist rows + detail
  header (per-patient `demo` flag), and clinician demographics ("Name/MRNs demo" even when DOB is
  live). Clinician silent `catch`→fixture and no-token-match now set `demo:true` (PatientProjection
  gained the field; `enrichWithSubgraph` sets it false only when it overlays real data). Patient
  activity feed's fabricated "export receipt" seed entry removed (now starts empty, seeds from real
  grants). tsc clean. NOTE: clients-only change — no Rust/Kotlin, but still needs the clients image
  rebuild to see it in UAT.
- **#2**: read-after-write for identity/consent/resolution (re-read, never optimistic).
- **#3**: one integration smoke that drives every interaction against the real bridge and asserts a
  real effect (event count ↑, effective identity shift, consent/provenance appears) — the build-failing
  test that catches fake/broken interactions wholesale.
- Then wire the ❌ surfaces: prior-auth `$creda-verify` (highest value), audit ledger, steward queue.

### Release-readiness program (directive: accelerate to a contributor release; bar = "honest + gated", not "finish everything")

Disposition for every not-yet-real surface: ✅ real, 🧪 fenced demo, 🚧 loud stub, or ❓ tracked
open-question — **no silent fakes**. Landed this session:
- **`docs/STATUS.md`** — authoritative contributor-facing map (real/demo/stub/open-question per
  component + FHIR surface + persona apps + release gates). Root README + clients/README point to it.
- **Silent fake removed**: `PatientResourceProvider.read` now throws `NotImplementedOperationException`
  (was returning a hollow Patient); placeholder-id helper deleted. (Kotlin — not compiled here.)
- **Clients fenced** as DEMONSTRATION code (clients/README banner + STATUS + root README).

Remaining for the release bar (next sessions):
1. **Compile/verify** the uncompiled Rust/Kotlin from sessions 3–4 (`make grpc && anchor creda &&
   make bridge`) — effective-identity, attest fix, `read`→loud. THIS IS THE FIRST THING NEXT SESSION.
2. **#3 integration smoke + CI grep gate** — smoke drives each client interaction against a real
   bridge and asserts a real effect; grep gate fails CI on untracked `TODO`/`FIXME` or fixture
   leakage into real paths. (Needs the testbed; the build-failing safety net.)
3. **#2 read-after-write** in the clients (no optimistic local state for identity/consent/resolution).
4. Sweep remaining 🚧: register-or-explicitly-404 the §8.2.5–8.2.14 ops; reconcile `encodeContest`
   to `{code,detail?}`; decide demo-client disposition long-term.

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
| clinician | attest/contest/amend writes, consent badge, DAG from `readSubgraph()`, **DOB field + conflict challenge from Core's effective identity (`$creda-effective-identity`); resolution Attests the supporting Assert** | other presentation fields (address, MRNs, name, summary); link/stale challenges; action log; request-access |
| audit | — | everything (218-line fixture ledger) |
| prior-auth | one attest | orders queue, decisions (should call `$creda-verify` — Core implements it) |
| steward | one contest | console/queue (289-line fixture) |

## Next work, in priority order

1. ~~**Clinician read rewiring**~~ — DONE (session 2). See "What landed" below.
2. **Audit ledger**: real grants/revocations/receipts via Consent search + type-filtered provenance.
3. **Prior-auth**: call `$creda-verify` for real decisions.
4. **Steward queue**; **patient activity feed** from real events.
5. **F1 (§8.5.6)**: FASTConsent-conformant projection on the existing Consent search.

### Follow-ups opened by session 3

- **Conflict never fully *clears* (by design, for now).** Resolution raises confidence via Attest
  but `disputed` stays true while both DOB Asserts exist. Options to "close" the challenge: (a) a
  confidence-gap threshold in the UI (presentation-only, defensible); (b) amend-to-agree — Amend
  the losing Assert to the chosen value so `value_map` collapses to one (works in the demo since
  one peer signs both Asserts; cross-institution it's blocked by the §3.4.5 originating-institution
  rule, which is the honest real-world constraint). Decide per product intent.
- **`GetEffectiveIdentity` debug field retained.** Reply still carries `effective_identity_debug`
  (field 1) alongside the structured `fields` (field 2); drop the debug string once nothing reads it.

### Follow-ups opened by session 2

- **Other clinician presentation fields are still fixture.** The DOB field is now real (effective
  identity); address/MRNs/name/summary stay fixture because the seed dataset doesn't model them
  and the projection only surfaces what's asserted. Extend the seed + render those fields from the
  same `effectiveIdentity()` call to finish de-fixturing the detail view.
- **Contest payload shape mismatch.** The bridge's `encodeContest` still emits the legacy
  `{"Other": <text>}` while Rust `ContestReason` is `{code, detail?}` (creda-events/payload.rs).
  `decodePayloadDetails` reads BOTH for now; reconcile the encoder to the struct and drop the
  legacy branch.
- **`$creda-amend` DOB tokenization is demo-shaped.** The clinician sends the Assert's original
  token back verbatim (round-trips with the seed). A production amend needs the real tokenizer,
  not the `tok:demo:*` passthrough.

## Validation cycle (run each session, before building)

1. `make grpc && anchor creda && make bridge` + `(cd clients && pnpm install && pnpm typecheck)`
   — compile truth. (Clients use **pnpm**, not npm/npx; `pnpm typecheck` = `tsc --noEmit`, `pnpm build`
   = `tsc -b && vite build`. `npx` is not installed on the maintainer host.)
2. `make -C testbed ui-up-real && make -C testbed reset` — behavioral baseline.
3. Drift check: grep clients for fixture imports (see audit table) and confirm the spec/reference
   architecture (`docs/credara-technical-spec.md` §8.5, `design/credara-reference-architecture.html`)
   still match the code. ~~**Known drift to remediate**: the reference architecture HTML predates
   GetSubgraphEvents, the plain-provider operation layout, TCP gRPC mode, and the seed/reset
   lifecycle.~~ **Remediated (session 2)**: `credara-reference-architecture.html` now documents the
   seven gRPC RPCs incl. GetSubgraphEvents + the parent→child materialize fix (engine/grpcsrv/grpc-if
   nodes), the Patient-typed `$creda-*` plain-provider layout (bridge nodes), `tcp://0.0.0.0:50051`
   testbed gRPC mode (uds/grpcsrv nodes), and the append-forward wipe-and-reseed lifecycle (dag node).
   JS validated with `node --check`.
4. Record findings here (remediate doc or code, whichever is wrong; reinforce what held).

### Session 3 validation findings

- **Clients typecheck clean** (`tsc --noEmit`, exit 0) after the effective-identity rewire.
- **Proto ↔ Rust ↔ Kotlin name-consistent**: `EffectiveIdentityReply.fields` / `field.{key,disputed,values}`
  / `value.{value,confidence,supporting}` line up across prost (snake) and protobuf-java (camel getters:
  `fieldsList`/`valuesList`/`supportingList`). Verified by symbol match, not compiled.
- **NOT compiled in this environment** (no cargo/JDK/Docker): the Core grpc.rs structured reply +
  `field_key_name`, and the Kotlin `effectiveIdentity()` + `$creda-effective-identity` op. **Next
  session MUST run `make grpc && anchor creda && make bridge`** before UAT. Then test the Whitfield
  flow: detail shows DOB 1971-08-04 (top confidence) with the conflict listed; "1971-08-04 is correct"
  writes an Attest on its supporting Assert; re-open → confidence higher, effect persists; `reset`
  restores the conflict.

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
