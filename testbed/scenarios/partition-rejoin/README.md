# Scenario: partition-rejoin

Exercises **partition tolerance** (§6.1.7) and the **anti-entropy backstop** (§6.1.8) over the real
libp2p mesh: a sustained network partition between two peers, both sides continuing to accept
writes, and the divergent DAGs reconciling on rejoin.

`make partition-rejoin`

## What it does

Two peers, meshed (peer-b bootstraps to peer-a):

1. **Baseline** — inject at peer-a, observe at peer-b. The mesh must be live *before* we cut it, or
   the isolation assertion below would be meaningless (peers that were never connected trivially
   "don't share" events).
2. **Partition** — drop all traffic between the two peer **pod IPs** at the kind node level
   (`iptables -I FORWARD ... -j DROP` inside each node container, both directions). This is
   CNI-agnostic: it does not rely on kindnet's partial NetworkPolicy enforcement, adds no cluster
   load (unlike installing Calico), and only touches the two peer IPs — the driver Jobs still reach
   each peer's gRPC.
3. **Both sides keep working** — inject an event at peer-a *and* at peer-b. Both writes succeed,
   which is the point: a partitioned peer stays available (§6.1.7).
4. **Isolation** — after a settle window, assert peer-a's event is **absent** at peer-b and
   peer-b's event is **absent** at peer-a (`check-absent`). The partition is real.
5. **Heal** — remove the iptables rules.
6. **Reconcile** — assert each side's partition-time event reaches the other within
   `RECONCILE_BUDGET_MS`. Peers do **not** re-gossip old events, so this rides the periodic
   anti-entropy round (§6.1.8) once the libp2p connection re-establishes — the same backstop
   `ae-repair` tests, here across a heal rather than a late join.

## What success looks like

```
==> baseline: injecting at peer-a and observing at peer-b (mesh must be live pre-partition)
    baseline gossip works (peer-b saw 0190... in 480 ms)
==> PARTITION: dropping all traffic between peer-a (10.244.1.4) and peer-b (10.244.2.5) on the kind nodes
==> injecting on BOTH sides during the partition
    peer-a wrote 0190...
    peer-b wrote 0190...
==> settling 8s, then asserting the partition held (neither event crossed)
    isolation confirmed: peer-a's event absent at peer-b, and vice versa
==> HEAL: removing the partition rules
==> waiting for the DAGs to reconcile via anti-entropy (budget 120000 ms)
    peer-a's event reached peer-b (31200 ms after heal-observe start)
    peer-b's event reached peer-a (240 ms after heal-observe start)
PASS: partition-rejoin (...§6.1.7/§6.1.8)
```

The first reconcile latency reflects the anti-entropy interval (the AE round after reconnect); the
second is quick once that round has run.

## Prerequisite

Uses the `check-absent` peer-driver subcommand added alongside this scenario. Rebuild the image:

```
make up          # or: make images
make partition-rejoin
```

## Reading a failure — which layer it points at

- **Isolation fails (an event crosses during the partition).** The node-level DROP didn't take
  effect — this is the experimental part of the scenario. Inspect the rules mid-run
  (`KEEP_NAMESPACES=1 make partition-rejoin`, then `docker exec <cluster>-worker iptables -L FORWARD -n`);
  if the DROP isn't there or the node uses a different iptables backend, that's the bug. A leaked
  event is a real FAIL, never a silent pass.
- **Reconcile times out after heal.** The peers didn't reconnect, or anti-entropy didn't run.
  Cross-check `make ae-repair`: if that also fails, the fault is AE itself, not the rejoin; if only
  this fails, suspect the post-partition libp2p reconnect (bootstrap re-dial / DHT re-discovery).
- **Baseline fails.** The mesh never formed — not partition-specific. Cross-check `make smoke`.
- **Driver Job errors with an unrecognized subcommand.** Stale `peer-driver:testbed` image —
  rebuild (`make up`).
- **`docker exec <node> iptables` permission/So-such-file error.** The node container's iptables
  path or backend differs; try `iptables-legacy`, or capture the exact error for a fix.

## Partition mechanism note

kind's default CNI (kindnet) does not reliably enforce NetworkPolicy, so a `NetworkPolicy`-based
partition would be silently ignored (the cluster config flags this). The alternatives are Calico
(a full CNI swap — more cluster resources, affects every scenario) or node-level iptables (this
scenario). iptables was chosen because it is surgical, CNI-agnostic, and adds no cluster load — the
last matters on a resource-constrained local engine.

## Relationship to the in-process suite

The single-process conformance suite tests partition *logic* against `MockTransport` (divergent
event sets reconciling). This scenario adds what a single process cannot: a **real** network
partition between two libp2p peers, both staying available, and reconciliation over the wire after
heal.
