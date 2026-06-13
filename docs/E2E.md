# Credara — Manual End-to-End Harness (persona clients)

The five persona clients are the project's **manual end-to-end test harness**: driven against a
**real** bridge they exercise the full path — client → FHIR → bridge → gRPC → Core → DAG → gossip —
exactly the way external clients will. Run this pass *before* opening the network to external
clients. The automated equivalent is the integration smoke (HANDOFF #3); this is the hands-on
version that also validates the UI contract.

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
      writes a real Attest on the Link; "wrongly merged" writes a real Contest.
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
- [ ] ❌ Entire ledger is a fixture (zero bridge calls). Does not reflect the grants/revocations/
      receipts you just created. Whole persona is a gap.

## Coverage gaps to close (each = "make this a real test")

In priority order (also the de-fixturing backlog in HANDOFF/STATUS):
1. **Prior-auth decision → `$creda-verify`** (Core already implements `EvaluateAuthorization`).
2. **Audit ledger** → real grants/revocations/export receipts (Consent search + type-filtered
   provenance).
3. **Clinician** action-log / request-access → real events + read-after-write.
4. **Steward** queue → real Links + contest on real ids.
5. **Patient** activity feed → real ExportReceipt stream.

When a row is chip-free **and** its effect is confirmed in Core, it's a passing E2E test. Mirror
each in the automated smoke (#3) so external-client traffic can't silently regress it.
