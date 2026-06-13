# Creda — Implementation Status

**Read this before contributing.** Creda is pre-launch software (see the README banner: not
deployed to a real network, not independently security-reviewed, **do not use with real PHI**).
This file is the single authoritative map of *what is real vs. what is not*, so nothing in the
tree silently misleads you. Where this file and the code disagree, that is a bug — file it.

The companion `docs/HANDOFF.md` is the rolling working-session log; this file is the durable,
contributor-facing summary. The authoritative *design* is `docs/creda-technical-spec.md`; tracked
unknowns live in its **§13 Open Questions**.

## Legend

| Mark | Meaning |
|---|---|
| ✅ **Real** | Implemented and tested; behaves as specified. |
| 🧪 **Demo/example** | Illustrative only (the persona UIs). Clearly fenced; not production code. |
| 🚧 **Stub (loud)** | Not implemented — **fails loudly** (errors / `NotImplementedOperationException` / 404). Never returns fake data. |
| ❓ **Open question** | Deliberately unresolved; tracked in spec §13 / `TODO(open-question-*)`. Not a bug. |

Principle: **no silent fakes.** A surface is either real, a fenced demo, or it fails loudly. If
you find code returning plausible-but-fabricated data as if it were real, that is the highest-
priority class of bug here.

## Substrate (Rust workspace `crates/`) — ✅ builds + tests green (`anchor creda`)

| Crate | State | Notes |
|---|---|---|
| `creda-events` | ✅ | Event model, 10 event types, canonical CBOR, Blake3, UUIDv7, algorithm-agile signatures. |
| `creda-store` | ✅ | `Store` trait + RocksDB + in-memory; secondary indexes. libgit2 substrate is ❓ (§13.1). |
| `creda-graph` | ✅ | Subgraph materialize, **effective-identity projection** (confidence-weighted, attestation-amplified, disputed-flagged), 7-step authorization eval, link-chain defense. Confidence *weights* are ❓ calibration (§5.3.2). |
| `creda-core` | ✅ | Engine + gRPC (`creda.proto`): CreateEvent, GetEvent, GetSubgraphEvents, GetEffectiveIdentity (structured), MatchByTokens, EvaluateAuthorization, GetMetrics. |
| `creda-export-gate`, `creda-verifier` | ✅ | Dual-control enforcement. Verifier stale-state policy is ❓ (§13.4.3). |
| `creda-net` | ✅ logic / ⚠️ version-pinned | Pure replication logic green; the rust-libp2p adapter is marked `TODO(libp2p-verify)` (constructor/event-shape pinning against the libp2p version) and DHT query-privacy is ❓ (§13.3). |

## FHIR Bridge (`bridge/`, Kotlin/HAPI) — partial

| Surface | State | Notes |
|---|---|---|
| `$creda-authorize` / `-revoke` / `-export` / `-verify` | ✅ | Patient-typed plain-provider ops; F0 CBOR mappers + golden tests. `-verify` calls Core's `EvaluateAuthorization`. |
| `Consent?patient=` search | ✅ | Authorization read-back. |
| `Organization` search | ✅ | Network-wide institution discovery — distinct grant audiences store-wide (Core `ListInstitutions`). Backs the patient share datalist. Name-only (institutions are fingerprints here, not directory entries). |
| `Task` create/search/`$creda-resolve-request` | ✅ (pilot) | Off-chain access-request inbox (hybrid workflow, §4.3.4). Ephemeral in-Bridge state — not a DAG event, not persisted, single-Bridge delivery. Cross-peer delivery is a real-PHI design item. |
| `$creda-provenance` | ✅ | Bundle of CredaProvenance over `GetSubgraphEvents`. |
| `$creda-effective-identity` | ✅ | Per-field projection (value/confidence/supporting/disputed). |
| `$creda-attest` | ✅ | Attests the real events in `references` (targets = parents); per-patient root-stub only as the no-reference fallback. |
| `$creda-amend` | ✅ (DOB-only) | Tokenization is demo-shaped — production needs the real tokenizer (HANDOFF follow-up). |
| `$creda-contest` | ✅ wire / ⚠️ | Works, but `encodeContest` emits the legacy `{Other:text}`; reconcile to `ContestReason {code, detail?}` (HANDOFF follow-up). |
| `Patient/read` (CredaPatient) | 🚧 | **Throws `NotImplementedOperationException`** — was returning a hollow Patient. CredaPatient projection is §8.2.2 pending; cleartext is intentionally not at the Bridge (§9.2). |
| `$creda-link` / `-tombstone` / `-disambiguate` / `-self-verify` / `$match` / `$export`, Subscription, Bulk Data, CapabilityStatement IG customization | 🚧 | Documented as not-yet-implemented (§8.2.5–8.2.14); not registered → 404 if called. |

## Persona clients (`clients/`) — 🧪 DEMO / EXAMPLE + manual E2E harness

The five SPA personas (patient, clinician, prior-auth, steward, audit) are **demonstration
clients** *and* the project's **manual end-to-end test harness** (`docs/E2E.md`) — not production
software. Run against a real bridge they exercise the full client→FHIR→bridge→gRPC→Core→DAG→gossip
path. A `DEMO DATA` chip on a surface means it isn't a valid E2E test yet (a coverage gap). They run in two modes (`VITE_FHIR_BASE`): a **mock bridge**
(in-memory fixtures; global "MOCK BRIDGE" chip) and **real** (against a live peer). In real mode,
any surface still backed by fixtures shows an amber **`DEMO DATA`** chip so it cannot be mistaken
for live data. Current real-vs-fixture state (drive every row to ✅; see HANDOFF for detail):

| App | Real against the bridge | Still fixture (chip-marked) |
|---|---|---|
| patient | grants list, share, revoke, token resolution, activity feed (event-sourced from `$creda-provenance`: grants/revocations/export receipts) | — |
| clinician | consent badge, DAG, DOB conflict challenge, **link-confirm challenge**, Attest/Contest resolution, legal name, **address**, **per-institution MRNs**, action log (event-sourced), request-access (off-chain Task → on-chain grant, §4.3.4) | headline confidence score; sex; worklist membership; stale challenge (can't be synthesized — needs real elapsed time) |
| prior-auth | one attest write | orders queue; **decision (should call `$creda-verify`)** |
| steward | one contest write | queue/cases/link-chain viz |
| audit | — | entire ledger/KPIs/report |

## Tracked unfinished work (not bugs)

- **DHT query-privacy (§13.3)** — security-relevant; closure plan + cost model in
  `docs/dht-query-privacy.md` (hard gate before real-PHI; fine for the synthetic pilot).
- **Spec §13 Open Questions** — the canonical list (confidence calibration, DHT privacy, revocation
  bounds 2/3, storage substrate, R4→R5, FAST Consent F1–F5, etc.).
- **`TODO(open-question-*)`** / **`TODO(libp2p-verify)`** / **`TODO(bridge-verify)`** — in-code,
  each referencing the above. These are sign-posted, intentional.
- **`docs/HANDOFF.md`** — the prioritized next-work queue + the front-end de-fixturing plan
  (#1 visible-demo marking ✅ landed; #2 read-after-write; #3 integration smoke + CI gate).

## Release gates (what "green" means)

1. `make grpc && anchor creda && make bridge` compile + test clean.
2. `(cd clients && pnpm install && pnpm typecheck)` clean (pnpm — not npm/npx).
3. Multi-peer testbed scenarios pass (`make -C testbed up && smoke && ae-repair`).
4. No **silent fakes**: every not-yet-real surface is 🚧 (loud) or 🧪 (chip-marked demo).
5. (Planned, HANDOFF #3) integration smoke drives each client interaction against a real bridge
   and asserts a real effect; CI grep gate rejects untracked `TODO`/`FIXME`/fixture leakage.
