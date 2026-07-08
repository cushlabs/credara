# Credara — End-to-End Testing

Credara's end-to-end surface has two complementary halves, both driving the **real** path (no
`MockTransport`). They complement the in-process conformance suite (`anchor creda`) — the
definitive functional green, but a single process:

- **Automated multi-peer scenarios** (`testbed/`) — real libp2p across two-plus peers in kind:
  gossip, anti-entropy, the authorization plane, partition tolerance. Fast and scriptable.
- **Manual persona harness** (`clients/`) — the five persona UIs driven against a real bridge, the
  way external clients will. Hands-on; also validates the UI contract.

## Automated multi-peer scenarios (testbed)

The multi-process counterpart to `anchor creda`: real gossipsub / Kademlia / anti-entropy across
peers in a kind cluster, which no single process can exercise. Run from `testbed/` (Docker or
Podman + kind + kubectl + helm; no host Rust or JDK). `make up` once to build the images and create
the cluster, then a scenario, then `make down`. Per-scenario detail lives in `testbed/README.md`
and each `testbed/scenarios/<name>/README.md`.

| Scenario | Asserts | Spec | Run | Status |
|---|---|---|---|---|
| gossip-convergence | an event at peer A reaches peer B within Bound 1 | §6.1.4, §4.7 | `make smoke` | ✅ |
| anti-entropy-repair | a late-joining peer catches up via the periodic AE round | §6.1.8 | `make ae-repair` | ✅ |
| revocation-latency | a Revocation propagates and *takes effect* at the other peer within Bound 1 (validated on arrival, §4.6 step 2) | §4.3.2, §4.7 | `make revocation-latency` | ✅ |
| partition-rejoin | a real node-level partition; both sides stay available; the divergent DAGs reconcile via AE on heal | §6.1.7, §6.1.8 | `make partition-rejoin` | ✅ |
| ui-smoke | each persona's primary flow (Playwright in-cluster, mock bridge) | §8 | `make ui-smoke` | ✅ |
| rolling-upgrade | Helm peer rotation with no convergence loss | §10.6.7 | — | 🚧 planned |
| storage-class | each tested storage class survives a peer restart | §10.6.8 | — | 🚧 planned |
| rogue-link | multi-peer link-chain defense (rogue-Link rejection) | §4.6 step 5.5 | — | 🚧 planned |

Release gate: `make -C testbed up && smoke && ae-repair && revocation-latency && partition-rejoin`.

Notes:

- Latency scenarios report a real number where possible — `revocation-latency` times inject→observe
  in one process to avoid the inter-Job scheduling gap that would otherwise read ~0.
- Reconciliation-paced scenarios (`anti-entropy-repair`, `partition-rejoin`) run slower (~75–120 s):
  they wait for the anti-entropy interval, not gossip.
- The in-cluster primitive is the peer-driver (`testbed/tools/peer-driver`): `inject`,
  `inject-grant`, `inject-revoke`, `time-revocation`, `observe`, `check-absent`, `derive-pubkey`,
  `seed-demo`.

## Manual persona harness (persona clients)

The five persona clients are the project's **manual** end-to-end harness: driven against a **real**
bridge they exercise the full path — client → FHIR → bridge → gRPC → Core → DAG → gossip — exactly
the way external clients will. Run this pass *before* opening the network to external clients. The
automated equivalent is the planned integration smoke; this is the hands-on version that also
validates the UI contract.

## Golden rule

**A `DEMO DATA` chip means "this surface tests nothing yet."** It is connected to a real bridge
but still rendering fixtures/local state, so a green-looking screen there proves nothing. Every
chip is a **coverage gap** to close before that interaction counts as a passing E2E test. A valid
E2E pass = the interaction (1) issues a real bridge call and (2) the effect is confirmed in Core,
not just reflected optimistically in the UI.

## Setup

1. **Build gate green** (`make grpc && anchor creda && make bridge`, `(cd clients && pnpm typecheck)`, testbed
   scenarios). You cannot E2E-test against a backend that doesn't build.
2. **Peer up in synthetic-only mode** (`config.syntheticOnly: true`) — every write is auto-tagged
   `test_data`, so the harness is provably non-PHI. Locally: `make -C testbed ui-up-real` then
   `make -C testbed reset` for a clean seeded baseline.
3. **Clients in REAL mode** pointed at the bridge: `cd testbed && UAT=1 make ui-forward` →
   http://localhost:5173. (If you see the global "MOCK BRIDGE" chip, you're in mock mode — fix
   `VITE_FHIR_BASE` before testing.)
4. **Two confirmation windows open** so you verify the *real* effect, not the UI's claim:
   - Core log: `kubectl --context kind-creda-testbed -n creda-uat logs peer-0 -c creda-core -f`
   - Event count: `kubectl … port-forward svc/peer-fhir 8080:8080` then watch a surface re-read,
     or `… -c creda-core` metrics. A write should make the count rise and the event appear.
5. **Reset between runs** (`make -C testbed reset`) to return to the seeded baseline — the DAG is
   append-forward, so "start over" = wipe + reseed, not undo.

## Pass — verify each step's REAL effect, not just the screen

### Patient (`/patient`)
- [ ] Page load → "who has access" lists the seeded Mercy grant (real `Consent?patient=` read).
- [ ] Share with an institution → grant appears; **refresh the page → it persists** (read-back),
      and the Core log shows a new signed `AuthorizationGrant`.
- [ ] Revoke it → moves to "stopped"; **refresh → still stopped**; Core log shows a signed
      `AuthorizationRevocation`.
- [ ] Activity feed → now event-sourced from `$creda-provenance` (the real DAG): each grant,
      revocation, and export receipt is its own entry and **survives a page reload**. Share then
      revoke an institution → the feed shows *both* the grant and the revocation, not just the
      revoke. (Export-receipt "access" rows appear only once real `$creda-export` events exist.)

### Clinician (`/clinician`)
- [ ] Open the seeded patient (James) → DAG renders from the real subgraph; DOB field + conflict
      come from Core's effective identity.
- [ ] Resolve the DOB ("1971-08-04 is correct") → Core log shows a signed `Attest` on the real
      supporting Assert; **re-open the patient → that value's confidence is higher** (real
      re-projection, persists across refresh; `reset` restores the conflict).
- [ ] Consent badge reflects the patient app's grant/revoke (both read the same DAG).
- [ ] Legal name → from Core's effective identity (title-cased). Action log → event-sourced from
      the DAG (Attest/Contest/Amend), survives refresh.
- [ ] Request access → sends an off-chain FHIR Task (§4.3.4); it appears in the **patient app's**
      "requests for access," and Approve there writes a real on-chain grant the clinician then sees.
- [ ] Address + per-institution MRNs → live from Core's effective identity (MRNs are a non-disputed
      identifier set; the issuing institution travels in the MRN payload).
- [ ] Link-confirm challenge → derived from a real uncontested, un-attested Link (James). Confirm
      writes a real Attest on the Link; "wrongly merged" writes a real **Contest carrying a
      ContestReason `{code, detail}`** (code `distinct-patients`; the DOB challenge's "Neither /
      unsure" uses `demographic-conflict`). Verify in the Core log that the signed `Contest`'s
      reason is the structured `{code, detail}` (not the legacy `{Other:text}`), and **re-open the
      patient → the contested Link is severed**: the two records no longer merge in the effective
      identity (§5.2.4 step 4). `reset` restores the un-contested link.
- [ ] ❌ Still fixture (coverage gaps): headline confidence score, sex, worklist membership. The
      stale-assert challenge is intentionally absent in real mode — a time-decayed assert can't be
      seeded (Core stamps wall-clock at creation), so it's not faked.

### Prior-auth (`/prior-auth`)
- [ ] Submit a bundle → Core log shows a signed `Attest`.
- [ ] ❌ **Decision card is chip-marked** — it's a fixture, NOT `$creda-verify`. The decision is
      not a real authorization evaluation yet. Highest-value gap to close first.

### Steward (`/steward`)
- [ ] ❌ Queue/cases are fixtures; resolve actions target fixture ids. Whole persona is a gap.

### Audit (`/audit`)
- [ ] ❌ The audit *client* is still a fixture (zero bridge calls) — it does not reflect the
      disclosures you just created. But the real **bridge** surface now exists: the disclosure ledger
      `AuditEvent?patient=` (on-chain ExportReceipts) and the read-side access-audit interceptor
      (see the Bridge API spot-checks below). Wiring this persona to `AuditEvent?patient=` is the
      remaining (demo) step.

### Bridge API spot-checks (curl)

The persona UIs render demographics (name/DOB/address/MRNs) from **`$creda-effective-identity`**,
de-tokenized client-side — so they don't call `Patient/read`. `Patient/read` is the *standards-facing*
CredaPatient resource (for external FHIR consumers / QHINs), checked here directly:

- [ ] `GET /Patient/{subgraph-entry-uuid}` → a **CredaPatient** (§8.2.2): `meta.profile` = CredaPatient;
      the subgraph-identifier / root-set / last-modified extensions present; an MRN identifier and a
      stable subgraph `identifier`; **gender** populated; **name and birthDate masked** (each carries a
      `data-absent-reason: masked` extension and no real value — never a fabricated demographic). A
      bad (non-UUID) id → 400; an unknown id with no events → 404.
- [ ] `POST /Patient/{id}/$creda-cleartext` (params: `requester` fingerprint hex, `purpose`, `useMode`,
      optional repeated `field` = `name`/`birthDate`/`address`) → the unmasked complement to `Patient/read`,
      consent-gated (§9.2). With **no covering grant** → `403`; with a grant but **no `CleartextProvider`
      configured** → `501` (the pilot default — cleartext is institution-supplied via the SPI, never
      Credara-held, so an un-integrated bridge fails loudly rather than faking it); a bad (non-UUID) id → 400.
      A wired provider returns a Patient with **real** name/DOB/address (past the gate, so *not* masked).
      The cross-institution P2P leg (requester's bridge → originating bridge) is tracked separately; this
      checks the operation + gate + SPI directly.
- [ ] `GET /AuditEvent?patient={subgraph-entry-uuid}` → the **disclosure ledger** (§8.2.4): the
      patient's `ExportReceipt` events as FHIR `AuditEvent` (ATNA Export type, source + recipient
      agents, governing `Consent` entity, the patient as an entity), **newest first**. Empty until you
      run `$creda-export` (honest — no fabricated ledger); after an export, that disclosure appears and
      **survives a reload** (it's read from the DAG, not buffered). A bad (non-UUID) id → 400.
- [ ] Read-side access audit: make any read/search above, then check the bridge's audit log for an
      `access op=… resourceType=… path=… ` line (logger `health.creda.bridge.audit.access`). This is
      the "who queried which subgraph" stream — separate from the on-chain disclosure ledger, and
      SIEM-forwarded in deployment. (A custom `AccessAuditSink` bean redirects it to a SIEM.)
- [ ] Reconcile with the UIs: the readable name/DOB the **clinician** shows are de-tokenized
      `$creda-effective-identity` values, *readable only because demo tokens embed their display form*
      (`tok:demo:1971-08-04`). In production those tokens are opaque and real cleartext comes from the
      consent-gated `$creda-cleartext` fetch (§9.2, now implemented — see the bullet above) — the same
      path this masked CredaPatient points at. So "the UI shows the name" and "CredaPatient masks the
      name" are the demo and production ends of one privacy model, not a contradiction.

## Coverage gaps to close (each = "make this a real test")

In priority order (also the de-fixturing backlog in STATUS):
1. **Prior-auth decision → `$creda-verify`** (Core already implements `EvaluateAuthorization`).
2. **Audit ledger** — the bridge surface is now real (`AuditEvent?patient=` disclosure ledger +
   access-audit interceptor); the remaining step is **wiring the audit client** to it (replace the
   fixture ledger with an `AuditEvent?patient=` read).
3. **Clinician** action-log / request-access → real events + read-after-write.
4. **Steward** queue → real Links + contest on real ids.
5. **Patient** activity feed → real ExportReceipt stream.

When a row is chip-free **and** its effect is confirmed in Core, it's a passing E2E test. Mirror
each in the automated smoke (#3) so external-client traffic can't silently regress it.
