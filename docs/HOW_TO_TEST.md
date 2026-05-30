# How to Get Started Testing Creda

Welcome. This is the on-ramp for new testers. If you can install Docker and run
`make`, you can test Creda — there is no Rust, JDK, or Kubernetes toolchain to set
up by hand.

> **Synthetic data only.** Creda is pre-launch healthcare infrastructure. Never use
> it with real PHI, real credentials, or real institutional keys. All testing uses
> the synthetic data generator (M9) with `test-data` tagging. See
> [`SECURITY.md`](../SECURITY.md) and the security note in
> [`CONTRIBUTING.md`](../CONTRIBUTING.md).

## What you need

1. **Docker** — Docker Desktop on macOS/Windows, or Docker Engine on Linux. Give
   it **6–8 GB of memory** (Settings → Resources). RocksDB builds from source and
   the OOM killer will take down `cc1plus` on a default 2 GB allocation.
2. **Git**, to clone the repo.
3. For the multi-peer testbed only: **kind ≥ 0.23**, **kubectl ≥ 1.30**,
   **helm ≥ 3.14**. Everything else runs inside containers.

You do **not** need a local Rust toolchain, a JDK, Gradle, or `protoc`.

## Clone and orient

```sh
git clone <repo-url> creda
cd creda
```

The two documents to read first:

- [`README.md`](../README.md) — what Creda is and the architectural thesis.
- [`docs/DEVELOPMENT.md`](DEVELOPMENT.md) — how the dev container works and the
  full `make` target list.

The spec ([`docs/creda-technical-spec.md`](creda-technical-spec.md)) is
authoritative but ~81 pages. You do not need to read it to start testing — pull it
up when a test fails and you need to understand which invariant was violated.

## The two test paths

Creda has two complementary test surfaces. Run both.

### Path 1 — In-process conformance (start here)

The default workspace test suite. This is M1–M6 + M9 plus the replication core,
exercised against `MockTransport` in a single process. Fast, deterministic, and
the one to trust for a definitive green.

```sh
anchor creda     # rolled-up one-line nextest summary across all crates
# or
make test        # equivalent; full workspace tests with PQC on
make test-fast   # Ed25519 only — skips the pqcrypto C build, much faster
```

What success looks like: a single `Summary: N tests run: N passed` line (plus a
separate doctest line). If `anchor creda` is green, the spine is healthy.

Optional adjacent targets:

```sh
make grpc        # opt-in gRPC server (feature `grpc`; needs protoc inside the container)
make libp2p      # compile-check the shipped feature set — libp2p is still being reconciled
make bridge      # build the HAPI FHIR Bridge (Java/Kotlin) in a Gradle + JDK container
make ci          # fmt-check + clippy + test — everything CI gates on
```

If your test report references `creda-events`, `creda-store`, `creda-graph`,
`creda-core`, or `creda-conformance`, this is the path that exercised it.

### Path 2 — Multi-peer testbed (real libp2p, two peers in kind)

The in-process suite cannot exercise the real libp2p adapter — gossipsub mesh,
Kademlia DHT, anti-entropy over the wire — because all of that requires more than
one process. The testbed brings up a kind cluster with two Creda peers and runs
scenarios against it.

```sh
cd testbed
make up          # create kind cluster + build & load Core / Bridge / peer-driver images
make smoke       # gossip-convergence scenario: inject at peer A, observe at peer B (≤5s budget)
make ae-repair   # anti-entropy repair scenario (~75s; late-joining peer catches up via AE)
make down        # tear down the cluster
```

`make smoke` is non-destructive — it brings up two peers in their own namespaces,
runs the scenario, and tears down the peers but leaves the cluster running.

What success looks like (from `make smoke`):

```
==> injecting event at peer-a
    event-id = 01923b4f-deef-7e9a-9b76-fafe5d2b71c1
==> observing event at peer-b (budget 5000 ms)
    converged in 612 ms
PASS: gossip convergence smoke test (612ms)
```

Read [`testbed/README.md`](../testbed/README.md) and
[`testbed/scenarios/gossip-convergence/README.md`](../testbed/scenarios/gossip-convergence/README.md)
before opening a bug against the testbed — the second one lists which layer each
failure mode points at.

## A reasonable first session

1. Clone the repo and confirm Docker has enough memory.
2. Run `anchor creda`. Confirm green.
3. `cd testbed && make up && make smoke`. Confirm green.
4. Read whichever scenario you just ran and try changing something — bump
   `AE_INTERVAL_SECS`, inject more events, drop a peer mid-run. The point of the
   testbed is to be poked at.
5. `make ae-repair` for the anti-entropy backstop.
6. `make down` when you are done.

## When something fails

Before filing an issue, capture:

- **Exact command** you ran and the **commit SHA** (`git rev-parse HEAD`).
- **Full output** of the failing command — `anchor creda` and the testbed scripts
  both print enough to bisect which layer broke.
- **`docker info`** memory line, if it might be the OOM-killer case below.
- For testbed failures: `kubectl logs` for both peer pods and the peer-driver Job
  (`kubectl logs job/<name>` — namespaces are per-peer).

Common first-time hiccups:

- **`Killed signal terminated program cc1plus` / `librocksdb-sys` build fails.**
  Docker is out of memory. Raise it to 6–8 GB, or use `make test JOBS=1`, or
  `make test-fast` to skip RocksDB entirely while iterating. See
  [`docs/DEVELOPMENT.md`](DEVELOPMENT.md#troubleshooting).
- **`make libp2p` red.** Expected. The libp2p adapter is the one quarantined
  surface still being reconciled against its pinned version. Do not file a bug
  unless the failure is new and reproducible from a clean clone.
- **Bridge fails to build.** Try `make bridge-stock` (Debian fallback image) and
  note in the bug which path failed.

## Filing what you find

- **Functional bugs and test failures** → open an issue. One issue per failure
  mode, with the capture list above. If the failure relates to a spec invariant,
  cite the spec section (`§4.7 Bound 1`, `§6.1.8`, etc.).
- **Security findings** → **do not open a public issue.** Route privately per
  [`SECURITY.md`](../SECURITY.md).
- **Documentation gaps, unclear errors, broken onboarding** → also an issue, tag
  `docs`. New-tester friction is the bug we most want to hear about right now.

## Where to go next

- [`docs/DEVELOPMENT.md`](DEVELOPMENT.md) — every `make` target and the dev image.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — what a PR has to look like if you
  want to land a fix alongside the bug report.
- [`REPO_STRUCTURE.md`](../REPO_STRUCTURE.md) — where everything lives, plus the
  M0–M9 build order.
- [`testbed/README.md`](../testbed/README.md) — the multi-peer bed and the
  scenario catalog, including the planned scenarios (`partition-rejoin`,
  `revocation-latency`, `rolling-upgrade`, `storage-class`, `rogue-link`) that
  testers can help bring up.

Thank you for testing. Pre-launch hardening is exactly the phase where outside
eyes catch the things the maintainers have stopped being able to see.
