# Creda — Closed Synthetic Pilot Runbook

Stand up a **real, multi-peer, propagating Creda network** across hosts/organizations using
**synthetic, non-PHI events only**. This is the sanctioned bridge between the single-host kind
testbed and a real deployment: real peers, real libp2p gossip + anti-entropy, real signing and
admission control — but **zero regulated data**, so no HIPAA/BAA/IRB exposure.

> ## 🚫 HARD GUARDRAIL — SYNTHETIC ONLY
> This pilot must carry **only synthetic, `test_data`-tagged events** (§11.4). **No real PHI,
> no real patient identifiers, ever.** The codebase is explicitly pre-launch and **not
> independently security-reviewed** (README, SECURITY.md). The moment real patient data is on
> the wire you are in regulated territory this system is not yet cleared for — see `docs/STATUS.md`
> and the spec §13 open questions (notably §13.3 DHT query-privacy, which leaks lookup patterns).
> Treat "someone added a real record" as a P0 incident: stop the network, wipe stores, rotate.

## 0. Readiness gate (do not deploy until ALL pass)

- [ ] `make grpc && anchor creda && make bridge` — compile + tests green. **The published images
      are the session 3–4 code that has not yet been compiled; this gate is non-negotiable.**
- [ ] `(cd clients && pnpm install && pnpm typecheck)` clean (pnpm, not npm/npx) — if shipping the
      demo clients (they're 🧪 example only,
      see clients/README).
- [ ] Multi-peer testbed green: `make -C testbed up && make -C testbed smoke && make -C testbed ae-repair`.
- [ ] Production images **built from the green tree and pushed**:
      `ghcr.io/cushlabs/creda-core` and `…/creda-bridge` (digests pinned in values).
- [ ] You have read `docs/STATUS.md` and accept the 🚧/❓ list (libp2p-verify, DHT privacy,
      uncalibrated confidence, unvalidated revocation Bounds 2/3, capacity unknown).

## 1. Network & admission design (decide before any deploy)

- **Participants**: the closed set of orgs/peers in the pilot. Creda is a *vetted* network —
  inbound replication refuses everything until the **participant registry** is populated (§3.6,
  §10.7).
- **Per peer**: a unique Ed25519 **institutional signing key** (32 raw bytes). The `institution_id`
  derives from it; if it changes, other peers stop trusting that peer's events. **Persist it.**
- **Registry**: a ConfigMap holding **every admitted peer's public key**, one `<algorithm> <hex>`
  entry per participant. Every peer mounts the same registry. Distributing/rotating it is manual
  for the pilot (App C is the open question) — do it out of band, e.g. collect pubkeys, assemble
  the registry, ship to all peers.
- **Topology**: pick 1–2 **bootstrap peers** with stable, externally-reachable libp2p addresses;
  everyone else lists them in `bootstrapPeers`.
- **Buckets**: in production set `config.subscribedBuckets` to the buckets each peer covers.
  **Do NOT use `subscribeAllBuckets: true`** (testbed-only; heavy).

## 2. Per-participant deploy

Each participant, on their own cluster/host:

```sh
# restricted Pod Security Standard (DQ-1 — chart is non-root throughout)
kubectl create namespace creda && \
kubectl label namespace creda pod-security.kubernetes.io/enforce=restricted

# institutional signing key (PERSIST/back this up; HSM/KMS preferred per §10.1.4)
head -c 32 /dev/urandom > signing.key
kubectl -n creda create secret generic creda-signing-key --from-file=signing.key

# participant registry (every admitted peer's pubkey; derive each with the peer-driver:
#   docker run --rm -v "$PWD":/k peer-driver:testbed derive-pubkey --secret-file /k/signing.key )
kubectl -n creda create configmap creda-participants --from-file=participants/
```

`values-pilot.yaml` (per peer — bootstrap peer shown; joiners add `bootstrapPeers`):

```yaml
image:
  core:   { repository: ghcr.io/cushlabs/creda-core,   tag: "<pinned-digest-or-version>" }
  bridge: { repository: ghcr.io/cushlabs/creda-bridge, tag: "<pinned-digest-or-version>" }
signingKey:        { secretName: creda-signing-key }
participantRegistry: { configMapName: creda-participants }
config:
  defaultPosture: treatment-presumed     # or deny-by-default (§9.3.2)
  subscribedBuckets: [ <buckets this peer covers> ]   # NOT subscribeAllBuckets
  bootstrapPeers: [ ]                      # joiners: ["/ip4/<bootstrap-ip>/tcp/4001/p2p/<peer-id>"]
libp2pService: { type: LoadBalancer }      # or NodePort on-prem
bridge: { enabled: true }
```

```sh
helm upgrade --install -n creda peer deploy/helm/creda -f values-pilot.yaml --wait
```

## 3. Cross-host libp2p reachability (⚠️ the main unknown)

The code has only run multi-peer in **single-host kind**. Real cross-host gossip is new territory:

- Each peer needs an **externally reachable** libp2p endpoint (LoadBalancer/NodePort + firewall
  open on the libp2p port, default 4001).
- Bootstrap peers need a **stable** address + known peer id; joiners put the full multiaddr
  (`/ip4/<ip>/tcp/4001/p2p/<peer-id>`) in `bootstrapPeers`.
- Confirm peers actually dial: each peer logs its libp2p peer id at startup
  (`creda-net: local libp2p peer id: …`); check connectivity before expecting propagation.
- Expect to debug NAT/advertised-address issues here. This is where a pilot earns its keep.

## 4. Seed synthetic events (test_data-tagged ONLY)

- Use the **M9 synthetic generator** (`conformance/src/generator.rs`) / `create_test_data`
  (§11.4) so every event carries the `test_data` tag — provably non-clinical, filtered from
  clinical FHIR responses, but it still propagates like a real event.
- ⚠️ The local `testbed seed-demo` driver uses **untagged** `create_event` — fine for kind, **not
  for the pilot**. Seed the pilot with tagged events (generator, or a tagged seed job).
- **Synthetic-only guardrail (IMPLEMENTED).** Set `config.syntheticOnly: true` in the Helm values
  (env `CREDA_SYNTHETIC_ONLY=true`) on **every** peer. With it on, a peer auto-tags every event it
  creates as `test_data` AND refuses to ingest any event lacking that tag — so a misconfigured
  client physically cannot put PHI on the network, and untagged events cannot propagate in. This
  turns "synthetic only" from a policy into an enforced invariant. **Pending the build gate (§0):
  this is the session-5 Rust change, not yet compiled — `make grpc && anchor creda` must pass first.**

## 5. Validate propagation

- Inject a tagged event at peer A; confirm it appears at peer B (gossip, ~1–2s) and that a
  late-joining peer catches up via anti-entropy (the `smoke` / `ae-repair` behaviors, now
  cross-host).
- Watch `/metrics` (port 9090) event counts converge across peers; `/readyz` green everywhere.

## 6. Operate

- Health/metrics: `/livez` `/readyz` `/metrics` on `:9090` (kubelet probes wired).
- Snapshots: the snapshot CronJob runs every 6h (`snapshotCronJob`); retention per §7.5.
- Logs: `kubectl -n creda logs <peer>-0 -c creda-core` / `-c hapi-bridge`.

## 7. Rollback / teardown

- The DAG is append-forward — "reset" = stop peers, **wipe the data PVC**, redeploy, reseed
  (mirror of `make -C testbed reset`). For the pilot: `helm uninstall`, delete the data PVCs,
  reinstall.
- Incident (suspected PHI / bad actor): stop the network, wipe all stores, rotate signing keys
  and the registry, investigate before restarting.

## 8. Known limitations carried into the pilot (accept explicitly)

- **No independent security review** (SECURITY.md).
- **DHT query-privacy unresolved** (§13.3) — lookups reveal interest patterns; acceptable for a
  closed synthetic pilot, not for PHI.
- **libp2p adapter version-pinning** unverified at scale (`TODO(libp2p-verify)`).
- **Confidence weights uncalibrated** (§5.3.2); **revocation Bounds 2/3 unvalidated** (§4.7).
- **Capacity unknown** (§13.7.2) — 50 GB / resource defaults are guesses; monitor.
- **Synthetic-only is now an enforced invariant** (`config.syntheticOnly`) — but only once the
  session-5 Core change is compiled and shipped in the images (build gate §0). Until then it's a
  policy.

## Go / no-go checklist

- [ ] Readiness gate (§0) fully green
- [ ] Images built from the green tree + pushed, digests pinned
- [ ] Signing keys generated + backed up; participant registry assembled + distributed to all peers
- [ ] Bootstrap topology + cross-host libp2p reachability confirmed (peers dial each other)
- [ ] `config.syntheticOnly: true` on **every** peer (enforced guardrail), shipped in the built images
- [ ] Propagation validated across ≥2 real hosts (gossip + anti-entropy)
- [ ] Monitoring + snapshot + rollback verified
- [ ] Every operator briefed: **synthetic only, no PHI**
