# Creda Testbed (DQ-3)

Local multi-peer test bed for Creda. Brings up a kind cluster, installs two or more Creda peers
via Helm, and runs scenarios that exercise the gossip mesh, anti-entropy, DHT discovery, and
authorization-revocation propagation against the spec's bounded-latency commitments (§4.7).

This is the local closure of DQ-3 — kind-based, single-machine, fast iteration. The same
scenarios will run against on-prem and cloud k8s for the broader DQ-3 closure once the local bed
is green.

## Layout

```
testbed/
├── README.md              — this file
├── Makefile               — top-level targets: up, smoke, down, clean
├── kind/
│   ├── cluster.yaml       — kind cluster spec (1 control-plane, 2 workers)
│   ├── up.sh              — create cluster
│   └── down.sh            — delete cluster
├── images/
│   ├── core.Dockerfile        — testbed Core image (creda-dev:local builder + Fedora minimal)
│   ├── bridge.Dockerfile      — testbed Bridge image (Gradle JDK builder + Temurin JRE)
│   ├── peer-driver.Dockerfile — peer-driver image (built in creda-dev:local)
│   └── build-and-load.sh      — docker build all three + kind load into the cluster
├── helm/
│   ├── values-peer-a.yaml — seed peer (no bootstrap)
│   └── values-peer-b.yaml — peer-b uses peer-a as bootstrap (populated at scenario runtime)
├── scripts/
│   ├── peer-multiaddr.sh  — extract a peer's libp2p multiaddr (for bootstrap wiring)
│   └── wait-ready.sh      — block until peer pods are Ready
├── tools/
│   └── peer-driver/       — small Rust binary; inject + observe + derive-pubkey. Built only
│                            inside the dev image; runs in-cluster as a kubectl Job.
└── scenarios/
    └── gossip-convergence/ — the smoke test
```

## Requirements

- Docker (the only host toolchain requirement)
- kind ≥ 0.23
- kubectl ≥ 1.30
- helm ≥ 3.14

No host Rust, JDK, or Gradle — every build runs inside `creda-dev:local`, and the peer-driver
runs as a Kubernetes Job inside the kind cluster (matching the eventual operator deployment
model).

## Scripts and the executable bit

Every script invocation goes through `bash <path>` rather than executing the script directly
(both in the Makefile and inside scenario scripts that call helper scripts). This means a fresh
clone of the repo can run `make smoke` without any `chmod +x` step — git only preserves the
executable bit for files that had it set in the commit that introduced them, and we don't want
new testers to hit a permission-denied error before the scenario starts. If you add a new
script, follow the same pattern: `$(BASH) $(REPO_ROOT)/path/to/script.sh` in the Makefile and
`bash "$TESTBED/path/to/script.sh"` inside scenario scripts.

If you want direct `./script.sh` invocation to work too (handy for ad-hoc debugging), set the
bit in git with `git update-index --chmod=+x <path>` once and commit. The Makefile path doesn't
require it.

## Quickstart

```
cd testbed
make up        # create kind cluster + build & load Core/Bridge images
make smoke     # run the gossip-convergence scenario
make down      # tear down the cluster
```

`make smoke` is non-destructive — it brings up two peers in their own namespaces, runs the
scenario, and tears down the peers but leaves the cluster running for the next run.

### User-acceptance testing the front-end clients

To drive the persona UIs in a browser (clinician / prior-auth / steward / patient / audit),
run these one per terminal (or one at a time — each block is paste-safe in both bash and
zsh):

```sh
cd testbed
make up
```

Once, if the cluster + images aren't already built. Then bring the clients up — mock-mode
FHIR fixtures, persistent namespace `creda-ui`:

```sh
cd testbed
make ui-up
```

In a second terminal, port-forward the in-cluster Service to your laptop. This is
blocking; Ctrl-C kills the forwarder only and the UI keeps running:

```sh
cd testbed
make ui-forward
```

Tear the UAT namespace down when you're done:

```sh
cd testbed
make ui-down
```

`make ui-up` is idempotent — re-running it upgrades the chart in place rather than
reinstalling. The UAT install lives in its own `creda-ui` namespace and is **not** affected by
`make ui-smoke`, which uses the ephemeral `creda-ui-smoke` namespace and cleans up after
itself. UAT and ui-smoke can run side-by-side without interference.

## Why this exists

The unit and integration tests live in `crates/` and `conformance/` and run against
`MockTransport`. The testbed is where the **real libp2p adapter** runs — across two peers, with
real gossipsub mesh, real Kademlia DHT, real request-response, real anti-entropy. None of that
can be exercised inside a single process.

Closing DQ-3 means: every commit can produce green smoke locally in under five minutes, and the
testbed runs the same scenarios CI runs against on-prem and cloud k8s.

## Notes

- **Images**: the testbed builds its own Core + Bridge + peer-driver images via
  `testbed/images/*.Dockerfile`. Core and peer-driver use `creda-dev:local` as builder + Fedora
  minimal as runtime; Bridge uses Gradle+JDK21 as builder + Eclipse Temurin JRE as runtime. The
  production Dockerfiles under `deploy/docker/` target Hummingbird FIPS images (registry path
  `registry.access.redhat.com/hi/`, DQ-4); the testbed substitutes public bases for fast local
  iteration. Production parity (DQ-6) is preserved.
- **In-cluster execution**: peers expose gRPC TCP on `:50051` via the headless Service (only
  rendered when `config.grpcSocket` starts with `tcp://`). The peer-driver Jobs talk to peers
  using stable in-cluster DNS — `peer-0.peer-headless:50051` from inside a peer's namespace. No
  port-forward, no host networking, no Mac-vs-Linux branching.
- libp2p bootstrap peer wiring is required for two peers to find each other. The smoke-test
  scenario extracts peer-a's libp2p multiaddr after it's Ready, then installs peer-b with that
  multiaddr in `config.bootstrapPeers`.
- See `scenarios/gossip-convergence/README.md` for what the smoke test asserts and how to read
  the output.

## Scenarios

The canonical end-to-end overview — this catalog plus the manual persona harness, with
per-scenario status — is [`docs/E2E.md`](../docs/E2E.md).

- `gossip-convergence/` — single event injected at peer A, observed at peer B within Bound 1
  (~2s normal). `make smoke`.
- `anti-entropy-repair/` — peer-a publishes events before peer-b exists; peer-b joins later;
  events arrive at peer-b only via the periodic anti-entropy round (§6.1.8). `make ae-repair`.
- `revocation-latency/` — both peers meshed; a Grant is replicated to peer-b, then an
  `AuthorizationRevocation` is injected at peer-a and its propagation to peer-b measured against
  §4.7 Bound 1. Because peer-b holds the Grant first, the revocation is validated on arrival
  (§4.6 step 2) — a revocation that has taken effect, not just an event that landed.
  `make revocation-latency`.
- `partition-rejoin/` — a real node-level iptables partition between the two peers; both keep
  accepting writes (§6.1.7); isolation is asserted; then heal and the divergent DAGs reconcile via
  anti-entropy (§6.1.8). `make partition-rejoin`.
- `ui-smoke/` — deploys the persona front-end clients (`clients/`) into the cluster and runs
  Playwright e2e specs as an in-cluster Job. Asserts each persona's primary flow against a
  mock FHIR bridge; rebases onto a real bridge once the M7 `TODO(bridge-verify)` stubs land.
  `make ui-smoke`.
- `rogue-link/` — a rogue peer gossips a self-issued Grant fused onto the responder's patient by a
  Link it controls; the deny-by-default responder's `EvaluateAuthorization` denies the Grant reached
  through a ceiling-capped `manual` Link and admits the one reached through a trusted
  `insurance-crosswalk` Link (§4.6 step 5.5, §5.3.5). `make rogue-link`.
- `rolling-upgrade/` — a `helm upgrade` rolls peer-b's pod (StatefulSet RollingUpdate, /readyz-gated);
  asserts the roll advanced to a new revision and replaced the pod, pre-roll data survived the
  rotation (PVC re-attach), peer-a kept serving throughout, and the rolled peer rejoined and caught up
  with no lost events (§10.6.7). `make rolling-upgrade`.
- `storage-class/` — a single peer on the storage class under test writes events, its pod is deleted
  and recreated (same PVC re-attaches), and the events must still be present — PV persistence +
  RocksDB reopen (§10.6.8). Default tests kind's `local-path`; `STORAGE_CLASS=<class>` targets a real
  matrix class (gp3, Longhorn, OpenEBS) on a richer cluster. `make storage-class`.

## Relationship to the M9 conformance suite

M9 (`crates/creda-conformance`) is the single-process conformance suite — runs under
`anchor creda` against `MockTransport`. The testbed is the multi-process counterpart: it brings
up real peers in kind and exercises the same invariants over real wire. The two suites share
the synthetic-data generator and the test-data tagging.

This testbed corresponds to spec §10.5.4 (conformance test suite tooling) and §11.4 (integration
testing in production).
