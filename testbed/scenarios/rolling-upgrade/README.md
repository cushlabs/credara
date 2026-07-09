# Scenario: rolling-upgrade

Exercises the **within-institution maintenance path** (§10.6.7): a Credara peer is upgraded via a
rolling `helm upgrade` while the rest of the network keeps serving, and the rolled peer rejoins and
reconverges with no lost events. This is the one scenario that drives the real StatefulSet
`RollingUpdate` + Helm upgrade path end to end.

`make rolling-upgrade`

## What §10.6.7 promises

Credara is designed to be upgraded without coordinated network downtime. A Credara StatefulSet uses
the default `RollingUpdate` strategy with `OrderedReady`: replicas roll one at a time, each gated on
`/readyz` returning 200 (which is not until the peer has re-bootstrapped, §10.5.3). The rolled peer
re-joins the mesh via the bootstrap flow (§11.1.2) and catches up anything it missed via anti-entropy
(§6.1.8) or, if it was gone longer than the snapshot interval, snapshot bootstrap (§6.2.5). Its
persistent volume re-attaches, so its own data is not lost across the rotation.

## How the roll is triggered

The testbed pins a fixed image tag, so we can't roll by bumping the image. Instead the upgrade
changes a benign config value (`snapshotIntervalSecs`), which rewrites the ConfigMap, which changes
the pod template's `checksum/config` annotation, which drives the **identical** `RollingUpdate` a
real image or chart bump would. The mechanic under test — StatefulSet rolls the pod, `/readyz` gates
it, the PVC re-attaches, the peer rejoins — is exactly the production one.

## What it does

Two peers, meshed (peer-b bootstraps to peer-a). **peer-a is the rest of the network** (a separate
release, untouched by the upgrade); **peer-b is the institution being upgraded**.

1. **Baseline** — inject at peer-a, observe at peer-b: the mesh is live before we touch it.
2. **Pre-roll marker** — write an event to peer-b that must survive the rotation.
3. **Capture identity** — record peer-b's StatefulSet `updateRevision` and pod UID.
4. **Upgrade** — `helm upgrade` peer-b with the config change (no `--wait`), so the roll begins.
5. **Serve during the roll** — inject at peer-a *while peer-b is rolling*. The write succeeding is
   the "no coordinated downtime" guarantee: the rest of the network never stopped serving.
6. **Wait for the roll** — `kubectl rollout status` + `/readyz`, then assert:

| # | Assertion | Proves |
|---|---|---|
| 1 | StatefulSet `updateRevision` changed **and** pod UID changed | the upgrade genuinely rolled peer-b-0 — a new revision and a new pod, not a no-op re-apply |
| 2 | the pre-roll marker is still present at peer-b | data survived the rotation — the PVC re-attached to the rolled pod (§10.6.3) |
| 3 | the during-roll write to peer-a succeeded | the rest of the network kept serving through peer-b's roll (§10.6.7) |
| 4 | the during-roll write reaches peer-b within budget | the rolled peer rejoined and caught up — no event lost across the upgrade (§11.1.2, §6.1.8) |

Assertion 4 typically rides an anti-entropy round: the write lands at peer-a while peer-b is down, so
peer-b misses the live gossip and catches it up after reconnecting. The budget accommodates a full
AE round (as in `partition-rejoin`) rather than assuming instant delivery.

## What success looks like

```
==> baseline: injecting at peer-a, observing at peer-b (mesh must be live pre-upgrade)
    baseline gossip works (0190... present at peer-b)
==> injecting a pre-roll marker at peer-b (must persist across the rotation)
==> peer-b before upgrade: revision=peer-6d4c... podUID=1f2e...
==> helm upgrade peer-b (config change → RollingUpdate; the roll begins)
==> injecting at peer-a DURING peer-b's roll (the network must keep serving)
    peer-a accepted a write during the roll: 0190...
==> waiting for the RollingUpdate to complete (/readyz-gated, 180s budget)
==> peer-b after upgrade:  revision=peer-7a9b... podUID=8c3d...
    confirmed: new revision + new pod (the RollingUpdate replaced peer-b-0)
    confirmed: 0190... still present at peer-b after the roll
    confirmed: 0190... present at peer-b (no event lost across the upgrade)
PASS: rolling-upgrade (...§10.6.7)
```

## Prerequisite

Uses only the `inject` and `observe` peer-driver subcommands (no new driver code), but needs the
`peer-driver:testbed` image present:

```
make up          # or: make images
make rolling-upgrade
```

## Reading a failure — which layer it points at

- **"StatefulSet revision did not change" / "pod UID unchanged."** The config change didn't roll the
  pod. Confirm `snapshotIntervalSecs` is actually rendered into the ConfigMap (it feeds
  `CREDA_SNAPSHOT_INTERVAL_SECS`) and that the pod template carries the `checksum/config` annotation —
  without it, a config-only change won't trigger a `RollingUpdate`.
- **Pre-roll marker missing after the roll.** Data did not survive the rotation — the PVC did not
  re-attach, or persistence is disabled. Check `persistence.enabled` and that the StatefulSet has a
  bound `volumeClaimTemplate` PVC. This overlaps with `storage-class`.
- **During-roll write never reaches peer-b (assertion 4 times out).** The rolled peer didn't rejoin
  or anti-entropy didn't run. Cross-check `make ae-repair` and `make partition-rejoin`: if those also
  fail, the fault is rejoin/AE itself, not the upgrade path; if only this fails, suspect the
  post-roll bootstrap re-dial (the upgrade must retain `config.bootstrapPeers`).
- **Baseline fails.** The mesh never formed — not upgrade-specific. Cross-check `make smoke`.
- **Driver Job errors with an unrecognized subcommand.** Stale `peer-driver:testbed` image — rebuild.

## Relationship to the other scenarios

`ae-repair` and `partition-rejoin` prove catch-up over a *network* fault (late join, partition heal).
This scenario adds catch-up over a *lifecycle* fault: the pod is rotated by a real `helm upgrade`,
its PVC re-attaches, `/readyz` gates the rejoin, and the rest of the network is proven to keep serving
throughout — the maintenance path an operator actually runs, not a simulated network event.

## Not yet covered: multi-replica no-interruption

§10.6.7's multi-replica case (a 2+ replica institution rolling one replica at a time with *zero*
interruption, PDB `minAvailable: 1` holding) is not exercised here: a single StatefulSet's replicas
share one ConfigMap, so the per-ordinal bootstrap wiring the intra-institution mesh needs isn't in
the chart yet. This scenario covers the single-replica path the spec documents (brief window, rejoin,
catch-up). The multi-replica variant is a follow-up once the chart grows per-ordinal bootstrap (or
mesh mDNS).
