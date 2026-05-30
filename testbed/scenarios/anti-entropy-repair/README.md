# Scenario: Anti-Entropy Repair

The second testbed scenario. Proves that the periodic anti-entropy peer-exchange round (§6.1.8,
the backstop for gossip's best-effort delivery) actually catches a late-joining peer up to a set
of events it missed.

## Why this exists

Gossip is best-effort. Events published before a peer subscribes are not retained for later
delivery — gossipsub fans out to current mesh members only. The anti-entropy round is the spec's
durability backstop: each peer periodically asks a sample of its connected peers for their
manifest (the set of UUIDs they hold), computes the delta against its own store, and requests
the missing events. This scenario exercises that path end to end against real peers in kind.

## What it exercises

End to end, in one scenario:

- libp2p `request-response` for the `PeerRequest::Manifest` exchange (the wire protocol we
  added to the libp2p adapter this milestone)
- `EventSource::all_event_ids()` on the serving side (peer-a returning its manifest)
- `Replicator::run_anti_entropy_round` driving the reconcile-and-fetch cycle
- The 30-second scheduler interval + fan-out 3 spawned by the daemon (`grpc.rs`)
- Signature verification at ingest on peer-b — every AE-delivered event re-checks against the
  participant registry, just like gossip
- Storage commit at peer-b

If this passes, the spec's "Bound 2 — anti-entropy catch-up" commitment (§4.7) is mechanically
real for the simple two-peer case. Multi-peer fan-out behavior and large-delta efficiency are
later scenarios.

## Execution model

Same in-cluster Job pattern as `gossip-convergence`. No host toolchain other than Docker +
kubectl + helm + kind.

```
cd testbed
make ae-repair       # or: scenarios/anti-entropy-repair/run.sh creda-testbed
```

## Approach

The clean reliable trigger here is **temporal**, not network-level: inject events at peer-a
*before* peer-b exists. Those events were never gossiped to peer-b (peer-b wasn't subscribed,
the mesh didn't include it), so the only way they can reach peer-b is via AE.

Sequence:

1. Stand up peer-a alone.
2. Inject 3 events at peer-a, collect their event ids.
3. Verify all 3 are visible at peer-a (sanity check — the publish-on-create + store path works).
4. Stand up peer-b with peer-a as its bootstrap peer.
5. Wait for peer-b's `/readyz`.
6. Observe each event id at peer-b with a budget of 75 seconds — enough for the first AE round
   to fire (30s AE interval, first useful round 30s after daemon startup) plus the manifest
   exchange round-trip.
7. Report per-event AE latencies. The first event's latency dominates (it triggers the AE
   round); the rest should be near-instant once the round completes.

The scenario passes if every injected event arrives at peer-b within the budget. It fails if
any event does not arrive (which would indicate a real bug in the AE wire path or scheduler).

## What it does NOT exercise

- **Multi-peer fan-out.** Today's scenario is two-peer. The scheduler picks up to `AE_FANOUT`
  peers per round; verifying that the right peers are picked under a larger network is a
  follow-up.
- **Partition + heal.** A separate `partition-rejoin` scenario will use NetworkPolicy or
  iptables to actively partition mid-test, then heal. That tests the catch-up-after-network-
  outage case rather than the catch-up-after-late-join case.
- **Large deltas.** Three events is enough to prove the wire protocol works. Stress-testing AE
  against a peer that missed thousands of events is a load-test scenario, not a smoke test.

## Known follow-ups

- Tighten the per-test budget when AE_INTERVAL_SECS is made configurable (currently hard-coded
  at 30s in `grpc.rs`). For the testbed we want to be able to run it at 5s to keep scenarios
  fast.
- Add a 3-peer variant where peer-c joins later and exchanges manifests with both peer-a and
  peer-b. Demonstrates the fan-out path.
- Verify the receiving peer's `creda_events_admitted_by_signature_algorithm_total` counter
  (when we wire metrics from §11.6.5) increments under AE the same way it does under gossip.
