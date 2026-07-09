# Scenario: storage-class

Verifies that a Credara peer's RocksDB store **survives a peer restart on a given storage class**
(§10.6.8): the PersistentVolume re-attaches to the recreated pod and RocksDB reopens the store with
no committed events lost.

`make storage-class` — or `STORAGE_CLASS=<class> make storage-class` to test a specific class.

## Scope — read this first

§10.6.8's core worry is **fsync durability under power loss**: a storage class that acks `fsync`
before the write is truly on durable media can corrupt RocksDB on a power cut or node failure. That
failure mode **cannot be reproduced in kind** — deleting a pod does not wipe the PV's disk cache the
way a power cut does, so the data is still there regardless of the class's fsync honesty. Verifying
real fsync behavior needs actual hardware and a tool like `diskchecker.pl`, which §10.6.8 points
operators to directly.

What this scenario *does* validate, faithfully, is the other half: **PV persistence + RocksDB reopen
across a restart** on the class under test. That is the "survives a peer restart" claim in the
tested-matrix — and it is exactly what breaks if persistence is misconfigured, the PVC fails to bind,
or the data directory is backed by ephemeral storage.

kind ships a single provisioner (rancher `local-path`), so the default run tests the cluster-default
class. On a cluster that provisions the real §10.6.8 matrix — AWS gp3, OpenEBS LocalPV, Longhorn —
run the identical assertions against each with `STORAGE_CLASS=<class> make storage-class`. Same
scenario, real storage; the testbed and the on-prem/cloud closure share it.

## What it does

A single seed peer (no mesh — this is about local durability, not gossip):

1. **Install** the peer with persistence on the class under test, and assert the `data-peer-0` PVC
   actually reached **Bound** (an unbindable class fails loudly here).
2. **Write** two marker events and confirm both are visible before the restart.
3. **Restart** — `kubectl delete pod peer-0` (default grace, so the daemon closes RocksDB cleanly).
   The StatefulSet recreates `peer-0`, re-binding the **same** `data-peer-0` PVC.
4. **Assert**:

| # | Assertion | Proves |
|---|---|---|
| 1 | the recreated `peer-0` has a new pod UID and reaches Ready | a genuine restart that came back healthy — not the same pod, not a stuck one |
| 2 | both markers are present after the restart | the store persisted on this class — the PVC re-attached and RocksDB reopened with the data intact |

## What success looks like

```
==> storage class under test: <cluster-default>
==> installing a single seed peer (persistence on <cluster-default>)
==> PVC data-peer-0: phase=Bound class=standard
==> injecting marker events
    markers = 0190... 0190...
    both markers committed and visible before the restart
==> restarting the peer (delete pod peer-0; the StatefulSet recreates it, re-binding data-peer-0)
==> waiting for the recreated peer-0 to reach Ready (180s budget)
==> peer-0 restarted: UID before=1f2e... after=8c3d... (Ready)
==> asserting both markers survived the restart (RocksDB reopened from the re-attached PVC)
PASS: storage-class (peer restarted on <cluster-default>; PVC re-attached; RocksDB reopened with both markers intact — §10.6.8)
```

## Reading a failure — which layer it points at

- **"PVC data-peer-0 is not Bound."** The class can't provision, or the name is wrong. On kind this
  should be `standard` (local-path). On a real cluster, confirm the `STORAGE_CLASS` you passed
  exists (`kubectl get storageclass`) and has a working provisioner.
- **"peer-0 did not come back Ready."** The recreated pod didn't reach `/readyz` — not storage
  specific unless it's crash-looping on the data directory. Check `KEEP_NAMESPACES=1 make
  storage-class` then the pod logs; a RocksDB open error on the re-attached volume is the storage
  signal.
- **A marker is missing after the restart.** The store did **not** persist across the restart on this
  class — the durability failure this scenario exists to catch. Either the PVC didn't re-attach
  (check it's the same `data-peer-0`, not a fresh one) or the class did not durably retain the write.
  On kind's `local-path` this should never happen; on a real class it's an actionable finding.

## Relationship to the other scenarios

`rolling-upgrade` also relies on the PVC surviving a pod rotation, but as a side effect of a graceful
Helm roll. This scenario isolates the storage guarantee itself, parametrizes it over the storage
class, and asserts the PVC bound before trusting the result — so it's the one to run when qualifying
a new storage class for a deployment, per §10.6.8's tested-matrix guidance.
