# How to Get Started Testing Credara

Welcome. This is the on-ramp for new testers. If you can install Docker and run
`make`, you can test Credara — there is no Rust, JDK, or Kubernetes toolchain to set
up by hand.

> **Synthetic data only.** Credara is pre-launch healthcare infrastructure. Never use
> it with real PHI, real credentials, or real institutional keys. All testing uses
> the synthetic data generator (M9) with `test-data` tagging. See
> [`SECURITY.md`](../SECURITY.md) and the security note in
> [`CONTRIBUTING.md`](../CONTRIBUTING.md).

## What you need

1. **Podman or Docker** — Podman (rootless; `podman machine init` on macOS/Windows) or
   Docker Desktop/Engine. The `make` targets shell out to `docker`, which Podman provides as a
   compatible CLI. Give the engine **6–8 GB of memory** (Podman: `podman machine set --memory
   8192`; Docker: Settings → Resources). RocksDB builds from source and the OOM killer will
   take down `cc1plus` on a default 2 GB allocation.
2. **Git**, to clone the repo.
3. For the multi-peer testbed only: **kind ≥ 0.23**, **kubectl ≥ 1.30**,
   **helm ≥ 3.14**. Everything else runs inside containers.

You do **not** need a local Rust toolchain, a JDK, Gradle, or `protoc`.

## Clone and orient

```sh
git clone https://github.com/cushlabs/credara.git
cd credara
```

The two documents to read first:

- [`README.md`](../README.md) — what Credara is and the architectural thesis.
- [`docs/DEVELOPMENT.md`](DEVELOPMENT.md) — how the dev container works and the
  full `make` target list.

The spec ([`docs/credara-technical-spec.md`](credara-technical-spec.md)) is
authoritative but ~90 pages. You do not need to read it to start testing — pull it
up when a test fails and you need to understand which invariant was violated.

## The three test paths

Credara has three complementary test surfaces. Run the first two unconditionally; run the
third when you are touching anything user-facing or want to do user-acceptance testing
against the persona clients.

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
make grpc            # opt-in gRPC server (feature `grpc`; needs protoc inside the container)
make libp2p          # compile + lint the shipped feature set (gRPC + libp2p) — green in CI
make libp2p-adapter  # compile + clippy the rust-libp2p adapter against its pinned version (CI parity)
make bridge          # build the HAPI FHIR Bridge (Java/Kotlin) in a Gradle + JDK container
make ci              # fmt-check + clippy + test + grpc + libp2p + libp2p-adapter — everything CI gates on
```

The rust-libp2p adapter (the `creda-net` `libp2p` feature) used to be the one surface never
built in CI. It is now compiled and clippy-linted in CI on every push (the `ci-rust` →
`libp2p-adapter` job), closing the old `TODO(libp2p-verify)` gap — so these targets should be
**green** from a clean clone.

If your test report references `creda-events`, `creda-store`, `creda-graph`,
`creda-core`, or `creda-conformance`, this is the path that exercised it.

### Path 2 — Multi-peer testbed (real libp2p, two peers in kind)

> The canonical scenario catalog — what each asserts, its spec section, and green/planned status —
> is [`docs/E2E.md`](E2E.md). This section is the quickstart.

The in-process suite cannot exercise the real libp2p adapter — gossipsub mesh,
Kademlia DHT, anti-entropy over the wire — because all of that requires more than
one process. The testbed brings up a kind cluster with two Credara peers and runs
scenarios against it.

```sh
cd testbed
make up          # create kind cluster + build & load Core / Bridge / peer-driver images
make smoke       # gossip-convergence scenario: inject at peer A, observe at peer B (≤5s budget)
make ae-repair   # anti-entropy repair scenario (~75s; late-joining peer catches up via AE)
make revocation-latency  # Grant + Revocation; measures revocation propagation vs §4.7 Bound 1
make partition-rejoin    # real node-level partition; both sides write; reconcile via AE on heal
make rogue-link  # rogue peer's Link-fused Grant denied (manual) / admitted (crosswalk), §4.6 step 5.5
make rolling-upgrade     # helm upgrade rolls a peer; data persists; rejoin + catch-up, §10.6.7
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

### Path 3 — Persona front-end clients (UI smoke + UAT in browser)

`clients/` ports the five mockups in `design/` into real Vite + React + TypeScript apps,
one per persona: **clinician**, **prior-auth**, **steward**, **patient**, **audit**. They
talk to the HAPI FHIR Bridge through a typed client. By default they run in **mock mode**
against fixtures shaped like the bridge response, so you can walk every screen with no
backend. The bridge's core authorization and identity operations are now implemented, so you
can also point the clients at a **real** bridge + peer (the `ui-up-real` path below). A few
surfaces are still fixture-backed and show an amber `DEMO DATA` chip — `docs/STATUS.md` is the
authoritative real-vs-fixture map.

There are two flavours to run, depending on whether you want an automated check or want to
poke at the UI yourself:

**Automated — Playwright in the cluster:**

```sh
cd testbed
make ui-smoke
```

Deploys the clients chart into an ephemeral `creda-ui-smoke` namespace and runs the
Playwright e2e specs as an in-cluster Job, then cleans itself up. The spec asserts each
persona's primary flow (clinician attest, patient revoke / grant, steward link policy,
audit ledger filters, prior-auth CRD → DTR → PAS → Decision). Execution model matches
`gossip-convergence` — no host Node, no host browser, no port-forward; the runner image
(`creda-clients-e2e:testbed`) ships Chromium and runs as a Kubernetes Job. See
[`testbed/scenarios/ui-smoke/README.md`](../testbed/scenarios/ui-smoke/README.md).

**Manual — drive the UI in your own browser:**

```sh
cd testbed
make ui-up
```

Idempotent `helm upgrade --install` into the persistent `creda-ui` namespace. Re-running
it after a code change upgrades the chart in place. Then in a second terminal:

```sh
cd testbed
make ui-forward
```

Blocking — forwards `http://localhost:5173` to the in-cluster Service. Ctrl-C kills the
forwarder only; the deployment keeps running. When you're done:

```sh
cd testbed
make ui-down
```

The UAT install lives in its own `creda-ui` namespace and is **not** affected by
`make ui-smoke` — UAT and the smoke scenario can run side-by-side without interference.
`make ui-up` is idempotent (`helm upgrade --install`), so re-running it after a code change
upgrades the chart in place.

**Manual — drive the UI against a real bridge + peer (end-to-end gossip):**

Mock mode lets you walk the UI end-to-end visually, but no event ever lands on the wire.
For end-to-end verification — clinician's "Attest" button producing a signed event in Core
and publishing it to the libp2p gossip mesh — bring everything up together:

```sh
cd testbed
make ui-up-real
```

Brings up a single Credara peer (Core + Bridge) in namespace `creda-uat`, deploys the
clients chart alongside it, and wires the clients' nginx `/fhir` reverse proxy at the
bridge's in-cluster Service. Same image as mock-mode UAT — the SPA switches modes at
container start based on the `FHIR_BASE` env var the chart passes in.

Open the UI by port-forwarding to the real-mode namespace:

```sh
cd testbed
UAT=1 make ui-forward
```

`UAT=1` picks the real-mode namespace `creda-uat`; default targets the mock-mode namespace
`creda-ui`. Both can coexist.

You'll see the yellow `MOCK BRIDGE · no gossip` chip is gone. Click `1971-08-04 is correct`
on James Whitfield. The toast now reads `Attest recorded — propagating to peers` because
the event actually went on the wire. To confirm at the protocol layer, tail the Core log
in another terminal:

```sh
kubectl -n creda-uat logs peer-0 -c creda-core -f
```

You should see an ingest-level log line for the Attest event the moment the toast appears.
The audit reviewer's ledger is still fixture-backed in this build (it doesn't yet read
from the bridge), so don't expect the new event to show up there — that's the next piece.
The on-wire event is what proves the system is working.

Tear down with `make ui-down-real` when you're done.

Persona URLs after `make ui-forward`:

| URL | What it is |
|---|---|
| `http://localhost:5173/` | Landing — links to all five personas |
| `/clinician` | Treating clinician (clinical view) — worklist, identity review, attest/contest |
| `/prior-auth` | Da Vinci CRD / DTR / PAS prior auth + signed Attest receipt |
| `/steward` | Identity steward (operator view) — queue, link policy, blocked-link cases |
| `/patient` | Patient consent client — grants and revocations |
| `/audit` | Compliance & audit reviewer — read-only ledger + §5.5 link-chain decisions |

What success looks like:

- `make ui-smoke` ends with `PASS: ui-smoke` and a Playwright summary line (e.g.
  `5 specs · 9 passed`).
- `make ui-up` ends with a block that lists the five persona URLs and tells you to run
  `make ui-forward` in a second terminal. The pod under `creda-ui` is Ready in ~5s.

If `make ui-up` fails ("Available: 0/1" or "context deadline exceeded"), the script's
fail-trap automatically dumps `kubectl describe` + container logs for the namespace —
read those before opening a bug.

## A reasonable first session

1. Clone the repo and confirm Docker has enough memory.
2. Run `anchor creda`. Confirm green.
3. `cd testbed && make up && make smoke`. Confirm green.
4. Read whichever scenario you just ran and try changing something — bump
   `AE_INTERVAL_SECS`, inject more events, drop a peer mid-run. The point of the
   testbed is to be poked at.
5. `make ae-repair` for the anti-entropy backstop.
6. `make ui-smoke` to confirm the front-end clients are green too. Then `make ui-up` +
   `make ui-forward` in a second terminal and walk the five personas in your browser —
   nothing here is the spec's authoritative behaviour (the spec lives in the events), but
   the visual + interaction surface is what real users will see, and fresh eyes catch
   things stewards stop noticing.
7. `make ui-down` and `make down` when you are done.

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
- **`make libp2p` / `make libp2p-adapter` red.** No longer expected — the adapter is now
  compiled and clippy-linted in CI, so a clean clone should be green. A *new* red here usually
  means rust-libp2p API drift against the pinned version; that **is** worth a bug, with the
  full compiler/clippy output.
- **Bridge fails to build.** Try `make bridge-stock` (Debian fallback image) and
  note in the bug which path failed.
- **`make ui-up` returns "Available: 0/1" / "context deadline exceeded".** The pod
  scheduled but its readiness probe never passed. The script's fail-trap dumps
  `kubectl describe` + container logs automatically — read those first. The most common
  cause has been the nginx base image refusing to start non-root under restricted PSS;
  the `clients/Dockerfile` now uses `nginxinc/nginx-unprivileged`, but if you see a
  fresh failure mode here, capture the diagnostics block in the bug.
- **`make ui-smoke` Playwright timeout.** The Job's 240s budget is plenty on a healthy
  laptop. A timeout usually means the clients pod didn't come Ready before the spec
  started — check the `Pod` events in the dumped diagnostics.

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
  scenario catalog, including the planned scenario (`storage-class`) that
  testers can help bring up.
- [`clients/README.md`](../clients/README.md) — the five persona front-end clients,
  their FHIR-bridge wiring, and how to swap from mock mode to a real bridge.
- [`clients/mockups/personas.md`](../clients/mockups/personas.md) — what each persona is allowed to see
  and do. Useful as a checklist when walking the UI in UAT — if a persona's UI lets
  you do something `personas.md` says they cannot, that is a bug.

## Coming soon: more end-to-end scenarios

The end-to-end surface is still being built out. Today you get the in-process conformance
suite; the two-peer gossip-convergence, anti-entropy, revocation-latency, partition-rejoin,
rogue-link, and rolling-upgrade testbed scenarios; and the persona UI smoke. Still in active
development — and available soon — are:

- the **real-effect integration smoke** (every client interaction driven against a real
  bridge and asserted to produce a real on-wire event, not just a UI transition);
- the remaining multi-peer testbed scenario: **storage-class**.

These are sign-posted in [`testbed/README.md`](../testbed/README.md) and `docs/STATUS.md`;
testers are welcome to help bring them up. Until they land, treat end-to-end coverage as
partial — the in-process suite is the definitive green.

Thank you for testing. Pre-launch hardening is exactly the phase where outside
eyes catch the things the maintainers have stopped being able to see.
