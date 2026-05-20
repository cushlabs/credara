# testbed — Local multi-peer test bed (DQ-3)

Simulates a small Creda network (2–3+ peers) and verifies production-like behavior. Two
paths run the **same scenario library** (`scenarios/`):

- `compose/` — fast, lightweight multi-peer bring-up for everyday iteration.
- `kind/`    — production fidelity: peers run as pods from the **real Helm chart** on a
  local kind/k3d cluster, exercising non-root securityContexts (DQ-1), Services,
  NetworkPolicy, and CronJobs as production would.

Scenarios to support (grow M4 → M9): gossip convergence within window; anti-entropy repair
of a desynced peer; snapshot bootstrap of a new peer; partition + rejoin; dual-control
(Export Gate refuse/permit + Verifier decision); revocation propagation within the Bound-1
window (§4.7). **Synthetic data only** (M9 generator); results asserted automatically.

Relationship to M9 conformance: they share the synthetic generator and scenario library.
The test bed is the interactive/local runner; conformance is the CI gate.

See `docs/DESIGN_QUEUE.md` DQ-3. Bootstrapped at M4, full fidelity by M8/M9.
