# Credara — Implementation Status

**Read this before contributing.** Credara is pre-launch software (see the README banner: not
deployed to a real network, not independently security-reviewed, **do not use with real PHI**).
This file is the single authoritative map of *what is real vs. what is not*, so nothing in the
tree silently misleads you. Where this file and the code disagree, that is a bug — file it.

This file is the durable, contributor-facing summary. The authoritative *design* is
`docs/credara-technical-spec.md`; tracked unknowns live in its **§13 Open Questions**.

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
| `creda-events` | ✅ | Event model, 11 event types, canonical CBOR, Blake3, UUIDv7, algorithm-agile signatures. Tombstoned-husk reduction (§3.4.6): scrubs `Assert`/`Amend` demographics to empty and voids the content hash, keeping the envelope. |
| `creda-store` | ✅ | `Store` trait + RocksDB (the production substrate) + in-memory; secondary indexes. §13.1.1 **resolved → RocksDB**; the libgit2 alternative was retired — its immutable content-addressed objects fight the §3.4.6 scrub, and libgit2 lacks the reftable backend for millions of subgraph refs (`docs/storage-substrate.md`). |
| `creda-graph` | ✅ | Subgraph materialize, **effective-identity projection** (confidence-weighted, attestation-amplified, disputed-flagged), 7-step authorization eval, link-chain defense. Confidence *weights* are **bootstrap priors** with a defined per-deployment calibration methodology — validated, auditable (`docs/matching-calibration.md`, §5.3.2). |
| `creda-core` | ✅ | Engine + gRPC (`creda.proto`): CreateEvent, GetEvent, GetSubgraphEvents, GetEffectiveIdentity (structured), MatchByTokens, EvaluateAuthorization, GetMetrics, ListInstitutions, GetSubgraphIdentity (§8.2.2). Applying a `Tombstone` **scrubs its targets' stored PII** to husks (§3.4.6) on both create and ingest — enforced for out-of-order (tombstone-before-target) delivery, idempotent against a re-received original, and re-applied on boot via `CredaCore::open()`. Health server (§10.5.3): `/livez`, `/readyz`, and `/metrics` — a **real** Prometheus exporter (`crate::metrics`) of operational gauges (build/up/ready/process-start, event + institution counts). The §11.2.1 golden-signal counters/histograms (gRPC/FHIR/gossip/AE traffic, latency, errors) are the next request-path instrumentation slice — tracked, not emitted as fabricated zeros. |
| `creda-export-gate`, `creda-verifier` | ✅ | Dual-control enforcement. The Verifier's stale-state policy (§13.4.3) is now **per use type**: `StalenessPolicy` classifies each request (pre-export / sensitive read / research-AI / routine, most-protective first) from the query's `use_mode`/`purpose`/data-categories and applies that class's threshold — advisory, with the relying institution keeping override authority. Recommended defaults (5 min / 1 h / 12 h / 24 h) are bootstrap values refined per deployment with pilot data (`docs/staleness-policy.md`); §13.4.3 resolved as structure + defaults (numbers operational). |
| `creda-net` | ✅ (DHT privacy ❓) | Pure replication logic green with **cross-peer wire-contract golden vectors** (DHT key / bucket / topic + gossip-batch envelope — exact-value pins so routing can't silently drift). The rust-libp2p adapter **compiles + clippy-cleanly against the pinned rust-libp2p 0.56**, guarded on every push by `ci-rust`'s `libp2p-adapter` job (the old `TODO(libp2p-verify)` gap is closed); live multi-peer convergence/AE tests run in the testbed. The peer's libp2p identity is a **stable, persistent transport key** loaded from a mounted Secret (`libp2p_key_path` / `CREDA_LIBP2P_KEY_PATH`; ephemeral with a loud warning if unset), so the `PeerId` is stable across restarts instead of churning the DHT routing tables and bootstrap on every cycle. It is a dedicated transport credential, **not** the institution signing key (which never leaves the signer, so HSM/KMS-backed signers work). *Which institution* operates a peer is an application-layer question (UDAP, §9.2), built with the cross-institution transport (tracked below). DHT query-privacy (§13.3) is a **documented, deliberately-gated** item, not an open unknown — full leak model, cost model, mitigation menu, and an OPRF+relay → PIR/PSI roadmap in `docs/dht-query-privacy.md`; a hard gate before real PHI, harmless on synthetic data (so it gates real-PHI, not the first install). |

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
| `$creda-amend` | ✅ (DOB-only) | Tokenization is demo-shaped — production needs the real tokenizer. |
| `$creda-contest` | ✅ | Emits canonical `ContestReason {code, detail?}` (§3.4.3, kebab code). Cross-language golden vector pins Rust ↔ cbor2 ↔ bridge; clients send a real reason code (clinician link-confirm/DOB, steward). |
| `Patient/read` (CredaPatient) | ✅ | US Core Patient projection (§8.2.2): the three `mustSupport` extensions (subgraph identifier / root set / last-modified, from Core's new `GetSubgraphIdentity`), MRN identifiers, and **real gender**; name/DOB **masked** (`data-absent-reason`) since cleartext stays off the Bridge (§9.2). The unmasked fetch is `$creda-cleartext` (next row). |
| `$creda-cleartext` (§9.2) | ✅ (gate + SPI; P2P transport pending) | The consent-gated fetch of the cleartext name/DOB/address that `Patient/read` masks. Runs Core `EvaluateAuthorization` against the requester's fingerprint+purpose+useMode (**403** with no covering grant), then delegates to a `CleartextProvider` **SPI** the institution implements against its own EHR/MPI — Credara never stores cleartext. No provider bean ⇒ **501**; provider holding no record for the patient ⇒ **404**; never a fabricated demographic. The cross-institution **Bridge↔Bridge P2P transport** (requester's bridge → originating bridge over libp2p Noise) is the one remaining dependency — tracked, not stubbed; the operation itself is production-real for an in-cluster/direct call. |
| `AuditEvent?patient=` (disclosure ledger) | ✅ | The on-chain disclosure half of audit (§4.3.3, §8.2.4): the patient's `ExportReceipt` events projected as FHIR `AuditEvent` (`AuditEventMapper`, FAST `$record-disclosure` shape), newest-first, each tied to the patient. Reads the real DAG via `GetSubgraphEvents` — empty until real `$creda-export` events exist (no fabricated ledger). Now a registered resource provider (was the HAPI-0289 empty stub). |
| Read-side access audit (interceptor) | ✅ | The "who **queried** which subgraph" half (§8.2.4, §9.1.6): a HAPI `@Interceptor` on `SERVER_PROCESSING_COMPLETED_NORMALLY` emits an `AccessAuditRecord` to an `AccessAuditSink` SPI. Default sink writes a structured audit log (SIEM-forwarded); institutions register a SIEM sink. No fabricated principal — UDAP/SMART identity binding is wired with the auth layer (tracked). Stored separately from the DAG, per §8.2.4. |
| `CapabilityStatement` (`/metadata`, §8.2.12) | ✅ | HAPI's auto-generated statement, annotated by a `SERVER_CAPABILITY_STATEMENT_GENERATED` interceptor with the Credara IG (`implementationGuide`) and per-resource profiles (Patient→CredaPatient, Provenance→CredaProvenance, Consent→CredaAuthorization). The `$creda-*` operations + `_creda-token` search param are advertised from the providers' annotations. |
| `Patient/$match` (FHIR) | ✅ | Scored identity matching: blocks on Core `MatchByTokens`, then **scores each candidate by real per-field token agreement** (`PatientMatcher`) against its effective identity, returning a searchset Bundle of CredaPatients with `search.score` + the FHIR `match-grade` extension, best first (honors `count` / `onlyCertainMatches`). The query carries **tokens**, never cleartext (§9.2): name in `name`, other fields as `…/match-token/<field>` identifiers. Scoring is real **Fellegi–Sunter** agreement (per-field `log2(m/u)` → LLR → grade), not fabricated; weights/thresholds are **bootstrap priors** with a defined per-deployment calibration methodology (`docs/matching-calibration.md`, §5.3.2) — loadable, frequency-ready. |
| `$creda-tombstone` (§3.4.6) | ✅ | Right-to-be-forgotten as a **real scrub**, not a projection trick. Records a signed `Tombstone` over the events in `references`; Core then **physically reduces those targets to husks** in the store — demographics gone and no longer findable by token (the demographic-token index is rebuilt), while the structural envelope + the signed tombstone remain for audit. It **deliberately breaks the target's content signature** (§3.4.6): the tombstone is the integrity anchor, and the husk is intentionally not content-verified (`verify_content_hash` → `None`, never a mismatch). The scrub is enforced on local create **and** on ingest — a tombstone arriving before its target husks it on arrival, a re-received original can't un-scrub it, and `CredaCore::open()` re-applies stored tombstones on boot (durable across restarts, self-healing). Multi-peer propagation gossips the signed `Tombstone` (each peer scrubs locally); a husk is a local artifact, never a wire object — if ever served it self-rejects on the receiver's signature check, so PII can't propagate. Bridge op + golden-vector CBOR encoder; Core integrity tests gate it (PII gone, unindexed, un-resurrectable, recovered on boot). The §3.4.6 content-signature tradeoff has a **governance review (§13.1.2)** packaged in `docs/tombstone-integrity-review.md` — recommended posture documented (keep the signed action-attestation; default to retaining no content digest; offer an off-by-default counsel-approved keyed-HMAC attestation), **open** pending sign-off from privacy counsel / security architects / HL7 Security. |
| `$creda-link` / `-disambiguate` / `-self-verify` / `$export`, Subscription, Bulk Data | 🚧 | Documented as not-yet-implemented (§8.2.5–8.2.14). Not registered → 404 if called. |

## Persona clients (`clients/`) — 🧪 DEMO / EXAMPLE + manual E2E harness

The five SPA personas (patient, clinician, prior-auth, steward, audit) are **demonstration
clients** *and* the project's **manual end-to-end test harness** (`docs/E2E.md`) — not production
software. Run against a real bridge they exercise the full client→FHIR→bridge→gRPC→Core→DAG→gossip
path. A `DEMO DATA` chip on a surface means it isn't a valid E2E test yet (a coverage gap). They run in two modes (`VITE_FHIR_BASE`): a **mock bridge**
(in-memory fixtures; global "MOCK BRIDGE" chip) and **real** (against a live peer). In real mode,
any surface still backed by fixtures shows an amber **`DEMO DATA`** chip so it cannot be mistaken
for live data. Current real-vs-fixture state (drive every row to ✅):

| App | Real against the bridge | Still fixture (chip-marked) |
|---|---|---|
| patient | grants list, share, revoke, token resolution, activity feed (event-sourced from `$creda-provenance`: grants/revocations/export receipts) | — |
| clinician | consent badge, DAG, DOB conflict challenge, **link-confirm challenge**, Attest/Contest resolution, legal name, **address**, **per-institution MRNs**, action log (event-sourced), request-access (off-chain Task → on-chain grant, §4.3.4) | headline confidence score; sex; worklist membership; stale challenge (can't be synthesized — needs real elapsed time) |
| prior-auth | one attest write | orders queue; **decision (should call `$creda-verify`)** |
| steward | one contest write | queue/cases/link-chain viz |
| audit | — | entire ledger/KPIs/report — the real **bridge** surface now exists (`AuditEvent?patient=` disclosure ledger + access-audit interceptor); wiring this client to it is the remaining (demo) step |

The **steward** and **audit** personas are slated to merge into one **Peer Operator Console** (operator
view: metrics + fleet-wide events + disclosures + stewardship + compliance). Mockup in
`clients/mockups/peer-operator-console-mockup.html`; wiring + E2E tracked below.

## Tracked unfinished work (not bugs)

- **Peer Operator Console — consolidate + wire up (+ E2E)** — a single operator/admin UI that merges
  today's **steward** and **audit** personas with peer-health metrics. "This peer's full store" event
  scope. **Cleartext authority is by trust boundary, not by role** (corrected from an earlier
  break-glass-everywhere framing): a **first-party operator** (institution workforce) sees its **own
  institution's** demographics directly from the MPI — same covered entity / BAA / source a clinician
  reads, resolved through the `CleartextProvider`, logged like any read; **cross-institution** cleartext
  is consent-gated via `$creda-cleartext` (the peer only holds tokenized provenance for other
  institutions); and a **delegated / managed operator** (a third party running the peer on the
  institution's behalf — a business associate, not workforce) is **break-glass** for all cleartext, with
  the `CleartextProvider` ideally kept on the institution side. The console therefore carries an
  operator-trust mode (first-party ⇄ delegated). Mockup:
  `clients/mockups/peer-operator-console-mockup.html` (synthetic, no backend). Wiring still to complete:
  **Overview** → `/metrics`; **Disclosures** → `AuditEvent` (generalize the per-patient ledger to
  peer-wide); **Stewardship** → the contest path that's already live, plus the rest of the queue on real
  Links/contests; **Events** → needs a **new fleet-wide Core `list events` RPC + bridge surface** (today
  everything is patient/subgraph-scoped); **cleartext** → own-institution resolves directly (audited),
  cross-institution + delegated-operator route to the consent-gated `$creda-cleartext`, writing
  requester+reason to the access-audit log. **Needs E2E coverage**: add the console to the manual harness
  (`docs/E2E.md`) and the planned automated smoke, asserting each section's real effect in Core (no
  fixture passes). Until wired it is a 🧪 mockup, not a live surface.
- **Golden-signal metric instrumentation (§11.2.1)** — `/metrics` exports real operational gauges
  today; the labeled traffic/latency/error counters and histograms need request-path hooks (a tonic
  tower layer in Core, the Bridge's HAPI interceptor, gossip/AE hooks). Tracked, not faked — absent
  series are absent, never zero-valued placeholders.
- **Cleartext P2P transport (§9.2)** — `$creda-cleartext` (consent gate + `CleartextProvider` SPI) is
  production-real for a direct/in-cluster call. The cross-institution leg — routing a requester bridge's
  call to the **originating** bridge over libp2p Noise — is the one remaining dependency. Tracked, not
  stubbed; until it lands, cleartext fetch is same-institution only.
- **Institutional peer authentication (UDAP application layer, §9.2)** — *which institution*
  operates a peer is established by mutual UDAP cert auth on connect/request (the §9.2 "UDAP cert
  auth at the application layer"), not by the libp2p transport key. This is one unit with its only
  consumer, the cross-institution, consent-gated `$creda-cleartext` Bridge↔Bridge fetch — a verifier
  with nothing calling it would itself be unfinished — so it is built complete *together with* that
  transport, against the partner institution's UDAP PKI, rather than as a standalone layer. It binds
  to the stable persistent transport identity (creda-net row).
- **DHT query-privacy (§13.3)** — **thoroughly analyzed and deliberately gated**, not an
  unexamined unknown. The full design note (`docs/dht-query-privacy.md`) covers: the leak model
  (Kademlia lookups expose `(querier, key)` to path nodes; deterministic per-epoch tokens enable
  offline precompute + correlation; the record also maps token→holders); what already mitigates it
  (no cleartext on the wire, salted+rotating tokens, and an **admission-controlled, non-open** DHT,
  so the residual adversary is only a *curious admitted institution*, not an anonymous Sybil); a
  **cost model** (bucket-coarsening's transfer cost and its anonymity set are the same number,
  `P/B` — cheap-but-weak early, strong-but-expensive at scale); a mitigation menu; and a
  **recommended roadmap** — OPRF-blinded exact-token lookups + relay near-term, bucket-coarsening
  opportunistically for sensitive lookups, PIR/PSI as the §9.5 endgame. The OPRF/PIR crypto **must
  be cryptographer-reviewed, not hand-rolled**, and wants pilot measurement (lookup fan-out / bucket
  occupancy) first — so this is a **hard gate before real PHI** yet **harmless on synthetic data**
  (no real person behind a synthetic token): it gates real-PHI deployment, not the first install.
  Safe near-term increments that need no new crypto: lookup measurement instrumentation, and
  audit + rate-limiting of lookups (defense-in-depth over admission + the immutable DAG).
- **Spec §13 Open Questions** — the canonical list (DHT privacy, revocation bounds 2/3, R4→R5, FAST
  Consent F1–F5, etc.). Storage substrate (§13.1.1) is now **resolved → RocksDB**
  (`docs/storage-substrate.md`). Match/confidence calibration (§5.3.2) is **resolved as a methodology**
  (`docs/matching-calibration.md`); only the per-deployment calibrated numbers (which need real data)
  remain. The tombstone content-integrity tradeoff (§13.1.2) is **packaged for governance review**
  (`docs/tombstone-integrity-review.md`) — recommended posture documented, open pending sign-off.
- **`TODO(open-question-*)`** / **`TODO(bridge-verify)`** — in-code, each referencing the above.
  These are sign-posted, intentional. (`TODO(libp2p-verify)` is resolved — the adapter compiles +
  clippy-cleanly against libp2p 0.56 and CI's `libp2p-adapter` job keeps it that way.)

## Release gates (what "green" means)

1. `make grpc && anchor creda && make bridge` compile + test clean.
2. `(cd clients && pnpm install && pnpm typecheck)` clean (pnpm — not npm/npx).
3. Multi-peer testbed scenarios pass (`make -C testbed up && smoke && ae-repair && revocation-latency && partition-rejoin && rogue-link && rolling-upgrade`).
4. No **silent fakes**: every not-yet-real surface is 🚧 (loud) or 🧪 (chip-marked demo).
5. (Planned) integration smoke drives each client interaction against a real bridge
   and asserts a real effect; CI grep gate rejects untracked `TODO`/`FIXME`/fixture leakage.
