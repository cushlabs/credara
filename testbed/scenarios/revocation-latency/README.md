# Scenario: revocation-latency

Exercises the **authorization plane** over the real libp2p mesh: an `AuthorizationRevocation`
(§4.3.2) must propagate to — and take effect at — a second peer within the §4.7 Bound-1 (gossip)
budget. This is the multi-peer counterpart to the in-process revocation tests, and it validates a
claim the spec otherwise only asserts (`docs/STATUS.md` flags the §4.7 latency bounds as
pilot-measurable rather than proven).

`make revocation-latency`

## What it does

Two peers, meshed from the start (peer-b bootstraps to peer-a):

1. Inject a subject **Assert** at peer-a (the subgraph entry-point).
2. Inject an **AuthorizationGrant** over that subject at peer-a.
3. **Confirm the Grant has replicated to peer-b.** This is the load-bearing step: because peer-b
   holds the Grant *before* the revocation arrives, the revocation — whose parent is the Grant —
   is **validated on arrival** (§4.6 step 2). A revocation only counts once its parents are
   present locally, so this makes the measurement a revocation that has *taken effect*, not merely
   an opaque event that landed.
4. In **one** peer-driver process — so no inter-Job scheduling gap can hide the latency — inject
   an **AuthorizationRevocation** targeting the Grant at peer-a and poll peer-b for it, timing t0
   (the injecting RPC) → t1 (peer-b first sees it). That window is the true cross-peer propagation
   latency. The Job runs in peer-a's namespace and reaches peer-b over cross-namespace DNS
   (`peer-0.peer-headless.creda-peer-b:50051`).
5. Assert the measured latency is within `REVOKE_BUDGET_MS` (5 s; Bound 1 is ~1–2 s).

The Grant is already replicated and the gossip mesh is warm by step 4, so the measurement reflects
near-steady-state gossip latency rather than mesh-formation cost. (An earlier cut injected and
observed in *separate* Jobs; the kubectl scheduling gap between them was longer than the gossip, so
the observed latency was always ~0 — the single-process command is what makes the number real.)

## What success looks like

```
==> injecting subject Assert at peer-a
    subject = 0190....
==> injecting AuthorizationGrant at peer-a
    grant   = 0190....
==> confirming the Grant has replicated to peer-b (so the revocation validates on arrival)
    grant present at peer-b (412 ms)
==> injecting the revocation at peer-a and timing its propagation to peer-b (budget 5000 ms, §4.7 Bound 1)
PASS: revocation-latency (revocation propagated + validated at peer-b in 214 ms; budget 5000 ms, §4.7 Bound 1)
```

## Prerequisite

This scenario uses two peer-driver subcommands added alongside it — `inject-grant` and
`inject-revoke`. Rebuild the testbed images so they exist in `peer-driver:testbed`:

```
make up          # or: make images
make revocation-latency
```

If you skip the rebuild, the driver Job fails with an "unrecognized subcommand" error and the
scenario aborts before injecting the Grant.

## Reading a failure — which layer it points at

- **Fails at "confirming the Grant has replicated to peer-b" (step 3).** The Grant itself did not
  gossip a→b — this is a plain gossip/replication problem, not revocation-specific. Cross-check
  with `make smoke` (gossip-convergence): if that also fails, the fault is in the mesh
  (gossipsub graft, bootstrap wiring, participant-registry admission), not the authorization
  layer.
- **Grant replicates but the revocation misses the budget (step 5).** The authorization *event*
  is gossiping too slowly. Same wire path as any other event, so suspect Bound-1 regression
  (batch flush interval, mesh degree) before anything authorization-specific.
- **Driver Job fails immediately with an unrecognized-subcommand / arg error.** Stale
  `peer-driver:testbed` image — rebuild with `make up` (see Prerequisite).
- **Peers never reach Ready.** Not this scenario — an install/image problem. The failure trap
  dumps `describe` + `creda-core` logs for both namespaces; read those first.

## Relationship to the in-process suite

`crates/creda-graph` and `crates/creda-conformance` already test revocation *semantics*
(`grant_revoked`, the §4.6 step-2 validated-parents rule) against `MockTransport` in a single
process. This scenario adds the missing dimension those cannot exercise: the revocation crossing
**real gossip between two processes** within a bounded time. The two are complementary — semantics
in-process, propagation here.
