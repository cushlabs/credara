# Scenario: Gossip Convergence

The smoke test for the testbed. Inject an event at peer A; observe it at peer B; assert that the
total propagation latency is within Bound 1 (§4.7 — typically 1-2 seconds across the network,
~5s budget for kind on a developer laptop).

## What it exercises

End-to-end, in one scenario:

- libp2p TCP transport between two pods in the kind cluster
- Noise + yamux + identify handshakes
- Bootstrap-peer wiring (peer B uses peer A as its only bootstrap peer)
- gossipsub mesh formation
- The publish-on-create hook (`Replicator::publish_event`) in peer A's daemon
- The inbound pump (gossipsub message → `Replicator::ingest_batch`) in peer B's daemon
- Signature verification at ingest, against the participant registry that holds peer A's pubkey
- Storage commit at peer B (RocksDB)
- `GetEvent` gRPC visibility

If this fails, the failure mode tells you which layer broke. The scenario reports the latency
distribution across N injects so you can spot tail latency too.

## Running

```
cd testbed
make smoke
```

Or directly:

```
testbed/scenarios/gossip-convergence/run.sh creda-testbed
```

## Execution model

The peer-driver runs as a **Kubernetes Job inside the kind cluster** — not as a host binary. It
talks to its target peer via in-cluster DNS (`peer-0.peer-headless:50051`) and prints its result
(event id or latency in ms) on stdout, which the scenario reads via `kubectl logs job/<name>`.
This matches the eventual operator deployment model: same execution path for local kind, on-prem
k8s, and cloud k8s. The only host prerequisite is Docker (plus kind/kubectl/helm to drive the
cluster).

## What success looks like

```
==> generating Ed25519 keypairs
==> peer-a multiaddr: /ip4/10.244.1.5/tcp/4001/p2p/12D3KooW...
==> installing peer-b (bootstrap → peer-a)
==> giving the mesh a moment to form (3s)
==> injecting event at peer-a
    event-id = 01923b4f-deef-7e9a-9b76-fafe5d2b71c1
==> observing event at peer-b (budget 5000 ms)
    converged in 612 ms
PASS: gossip convergence smoke test (612ms)
```

## Prerequisites

Before this can pass green you need:

1. **kind cluster up** (`make up`).
2. **Three testbed images built and loaded into the kind cluster** — `creda-core:testbed`,
   `creda-bridge:testbed`, `peer-driver:testbed`. `make up` handles all three.
3. **Two Ed25519 keypairs pre-generated.** The scenario script writes 32 bytes of urandom per
   peer, then derives each public key by invoking `peer-driver derive-pubkey` via `docker run`
   against the host-mounted key file — no host Rust toolchain. Public keys go into a shared
   ConfigMap mounted at both peers' participant registries; private keys go into per-peer
   Secrets.
4. **Peer B's Helm values reference peer A's multiaddr in `config.bootstrapPeers`.** The scenario
   extracts peer A's multiaddr from its container logs after peer A is Ready, then installs
   peer B with that value via `--set-string config.bootstrapPeers[0]=...`.

## What this scenario does NOT yet exercise

- **mDNS or random-walk discovery.** Peer B only finds peer A via the bootstrap config. mDNS is
  not in the libp2p behavior currently.
- **Anti-entropy.** The smoke test injects one event and waits for gossip; AE-repair is its own
  scenario (`anti-entropy-repair/`, not yet implemented).
- **Authorization.** The injected event is a synthetic Assert; no Grant/Revoke logic runs.

## Known follow-ups

- **Pubkey extraction** — the scenario currently derives peer A's pubkey from the locally
  generated keypair (which it also passes into peer A's Secret). A cleaner path is to expose the
  pubkey on a small `/peer-id` endpoint and let peer B discover it. Today: pre-share.
- **Three-peer scenario** — same scaffold should handle peer-c. Add when AE-repair lands.
- **CI integration** — once the smoke test passes locally, hook it into `ci-conformance.yml`
  behind a `testbed-kind` matrix entry. The runner needs Docker-in-Docker for kind.
- **Operator track** — the in-cluster Job execution model is the seed for the §10.6.6 Kubernetes
  Operator. Once we have three or four scenarios running this way, factoring the orchestration
  out of bash and into an Operator CRD (e.g. `kind: CredaTestScenario`) is straightforward.
