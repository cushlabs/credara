# Credara

**Credara** is a decentralized, peer-to-peer substrate for **cross-institutional patient
identity provenance** and **portable authorization** in US healthcare.

Institutions run peers that form a vetted-but-uncoordinated network. A directed acyclic
graph (DAG) of signed events records two co-primary things: **who a patient is** —
identity continuity across institutions — and **what they have authorized** — portable,
revocable, verifiable-at-point-of-use authorization. The graph replicates asynchronously
via gossip and anti-entropy. **FHIR R4** is the integration surface. There is admission
control (a vetted trust framework, modeled on DirectTrust) but **no runtime coordinator**:
once admitted, peers operate directly with one another.

Credara is complementary infrastructure. It does **not** replace institutional Master
Patient Indexes (MPIs), EHRs, or QHIN-mediated exchange. It fills a gap those systems
leave open: cross-institutional identity with cryptographic provenance, plus persistent,
revocable authorization that stays verifiable after data has moved — without a central
authority or vendor lock-in.

> The name *Credara* shares the Latin *cred-* root ("to believe / to trust") — fitting for an
> identity-provenance system.

## Status

> **Spec finalized at v1.0 · software pre-launch — all milestones implemented; multi-peer
> testbed green; hardening in progress. Open for testers.** The
> [technical specification](docs/credara-technical-spec.md) (Sections 1–13 + appendices, ~90
> pages) is **finalized at v1.0.0** — complete and authoritative — while the implementation
> hardens toward a matching software release. All ten build milestones
> (M0–M9) are implemented and verified, the default workspace builds and tests green, the
> opt-in libp2p adapter compiles and lints clean against the pinned version, and the
> **multi-peer testbed under kind passes its gossip-convergence and anti-entropy-repair
> scenarios end-to-end** — two real peers, real libp2p transport, real gossipsub mesh, real
> manifest exchange, all running as Helm-installed StatefulSets in separate namespaces. The
> first end-to-end gossip propagation measured at **2 ms** at peer-b after publication at
> peer-a, well inside the §4.7 Bound 1 commitment (~1–2 s).
>
> This is still **pre-launch software**: not yet deployed to a real network and not yet
> independently security-reviewed. Do not use it with real PHI.
>
> **New testers start here:** [`docs/HOW_TO_TEST.md`](docs/HOW_TO_TEST.md) walks through
> prerequisites, the in-process conformance suite (`anchor creda`), the multi-peer testbed
> (`make -C testbed up && make -C testbed smoke`), the anti-entropy scenario
> (`make -C testbed ae-repair`), what success looks like, and how to file what you find. The
> testbed itself has its own deeper guide under [`testbed/README.md`](testbed/README.md).
> New-tester friction is the bug we most want to hear about during hardening.
>
> **New contributors start here:** [`docs/STATUS.md`](docs/STATUS.md) is the authoritative map of
> what is real vs. demo vs. not-yet-implemented vs. tracked-open-question — read it before building
> on any surface. The persona apps under `clients/` are **demonstration code**, not production.

[![ci-rust](https://github.com/cushlabs/credara/actions/workflows/ci-rust.yml/badge.svg?branch=main)](https://github.com/cushlabs/credara/actions/workflows/ci-rust.yml)
[![ci-java](https://github.com/cushlabs/credara/actions/workflows/ci-java.yml/badge.svg?branch=main)](https://github.com/cushlabs/credara/actions/workflows/ci-java.yml)
[![ci-conformance](https://github.com/cushlabs/credara/actions/workflows/ci-conformance.yml/badge.svg?branch=main)](https://github.com/cushlabs/credara/actions/workflows/ci-conformance.yml)
[![ci-docs](https://github.com/cushlabs/credara/actions/workflows/ci-docs.yml/badge.svg?branch=main)](https://github.com/cushlabs/credara/actions/workflows/ci-docs.yml)
[![gitleaks](https://github.com/cushlabs/credara/actions/workflows/gitleaks.yml/badge.svg?branch=main)](https://github.com/cushlabs/credara/actions/workflows/gitleaks.yml)

Beyond the milestones, the in-daemon **gRPC serve socket** (Bridge ↔ Core over a Unix
domain socket or TCP), the **libp2p transport ↔ engine replication** path (signed-event
ingest with mandatory signature verification, gossip publish, anti-entropy peer-exchange,
DHT provider correlation), and the **HTTP health endpoint** (`/livez`, `/readyz`,
`/metrics` on port 9090 for kubelet probes per §10.5.3) are all wired and exercised by the
testbed. The link-chain authorization defense (§4.6 step 5.5 + §5.3.5 method ceilings) ships
with three conformance scenarios pinning the boundary between rogue-Link attacks and
legitimate first-encounter Links. The DHT key derivation switched to **SHA-512** so the
routing-key primitive is FIPS-validated under OpenSSL's FIPS module without any future
migration.

## Architectural thesis

- **Verification, not mediation.** Credara verifies identity and authorization claims; it
  does not sit in the data path or broker transactions. There is no central node that
  sees PHI.
- **Provenance by structure.** Every assertion is a signed event with parent references.
  The DAG *is* the audit trail — tamper-evident by construction.
- **Append-forward, content-mutable by exception.** History is structurally append-only;
  the right to be forgotten is honored by *tombstoning* (scrubbing PII content while
  preserving graph topology), distinct from authorization revocation.
- **Portable authorization is co-primary.** A grant is a signed, scoped, detachable
  artifact that travels with data references and is re-verifiable at any point of use —
  enforced by **dual control**: an Export Gate at the source and a Verifier at the
  relying party, neither able to unilaterally circumvent authorization.
- **Standards over invention.** The system is assembled from mature components (libp2p,
  HAPI FHIR, RocksDB/libgit2, the `pqcrypto` family, SPIRE, cert-manager, TEFCA
  tokenization). Only the healthcare-domain layer is new code. See spec **Appendix C**.

## What peers exchange

Three distinct kinds of exchange flow between Credara peers. They are worth separating because
they carry different things — and because none of them ever carries cleartext PHI.

**1. Event gossip (the main flow).** When an institution creates an event — a new identity
assertion, a link, an authorization grant, a revocation, a tombstone — its peer pushes that
event to a handful of neighboring peers, who push it onward, spreading it across the network in
roughly log(N) rounds (about 1–2 seconds network-wide). What is on the wire is a *batch* of
event nodes serialized in canonical CBOR, inside an encrypted Noise channel. Each batch carries:

- the sender's peer ID,
- a batch sequence number (for deduplication), and
- the serialized events themselves.

Batches flush every 100 ms or when they reach 64 events, whichever comes first — amortizing
encryption and framing overhead and cutting message count 10–50× versus sending events one at a
time. Each event node inside a batch contains its UUIDv7 (the address), the event type, parent
UUIDs (the edges of the graph), the payload, the originating institution's signature,
timestamps, and a logical clock. Critically, the payload carries **tokenized demographics, never
cleartext PHI** — cleartext patient data never traverses the gossip network by design. Receiving
peers verify the signature, store the event, and re-gossip it; they ignore any event UUID they
have already seen.

**2. Anti-entropy (the backstop).** Gossip is best-effort, so peers periodically reconcile. Two
peers holding the same patient's subgraph exchange a Merkle root computed over their *sorted set
of event UUIDs* — deliberately not over event contents, so tombstoning (which mutates content)
never makes two peers with the same event set diverge. If the roots match, they are in sync and
nothing more is sent. If they differ, they exchange the UUID sets each holds, identify the
delta, and transfer only the missing event nodes. This is how anything gossip dropped — during a
partition, an outage, or message loss — eventually gets caught.

**3. DHT routing and targeted pulls (discovery).** To find who holds a given patient's events,
peers exchange DHT records that map a *tokenized* subgraph key to peer IDs — again, tokens, not
demographics. Once a peer learns which peers hold a subgraph, it makes a targeted
point-to-point request for the actual event nodes. A **Portable Authorization Artifact** (a
signed `AuthorizationGrant` in CBOR) can travel this way too — detached and handed to a relying
party so it can verify authorization locally.

**What never crosses the wire.** Cleartext PHI. Demographics are tokenized before anything is
gossiped, the DHT only ever sees tokenized keys, and clinical payloads never enter the trust
graph at all. Everything that moves between peers is one of: a signed CBOR event node (with
tokenized content), a set of UUIDs and Merkle roots for reconciliation, or DHT routing records —
all inside encrypted, mutually-authenticated Noise channels.

The three exchanges, in sequence:

```mermaid
sequenceDiagram
    autonumber
    participant EHR as Institution / EHR
    participant A as Peer A
    participant B as Peer B
    participant C as Peer C
    participant DHT as Kademlia DHT

    Note over EHR,DHT: 1 · Event gossip — the main flow (spreads in ~log N rounds, ~1–2 s)
    EHR->>A: create event — Assert / Link / Grant / Revoke / Tombstone
    Note right of A: peer batches events,<br/>flushing at 64 events or 100 ms
    A->>B: GossipBatch over Noise — sender ID, seq no., CBOR event nodes
    A->>C: GossipBatch (same)
    Note over B: verify signature → store → dedup by UUID
    B->>C: re-gossip the batch
    C-->>B: UUID already seen — ignored

    Note over A,C: 2 · Anti-entropy — the backstop (periodic reconcile)
    B->>C: Merkle root over the sorted event-UUID set
    alt roots match
        C-->>B: in sync — nothing more sent
    else roots differ
        C->>B: its UUID set (to compute the delta)
        B->>C: only the missing event nodes
    end

    Note over A,DHT: 3 · DHT routing and targeted pull — discovery
    C->>DHT: look up tokenized subgraph key
    DHT-->>C: peer IDs that hold the subgraph
    C->>A: request events — point to point
    A-->>C: CBOR event nodes · or a detached AuthorizationGrant

    Note over EHR,DHT: On the wire: tokenized CBOR events · UUIDs and Merkle roots · DHT routing records — all inside encrypted, mutually-authenticated Noise. Never: cleartext PHI.
```

## Technology at a glance

| Layer | Choice |
|---|---|
| Core / Export Gate / Verifier | **Rust** |
| FHIR Bridge | **Java/Kotlin** — HAPI FHIR, Plain Server mode (not JPA) |
| FHIR version | **R4** (R5 deferred — open question 13.6.1) |
| Networking | **libp2p** — gossipsub, Kademlia DHT, Noise transport |
| Storage | `Store` trait — **RocksDB** impl first, **libgit2** scaffolded (open question 13.1) |
| Serialization | **Canonical CBOR** (ciborium, RFC 8949 deterministic encoding) |
| Hashing | **Blake3** (content integrity, Merkle roots) · **SHA-512** (DHT routing key, FIPS-validated) |
| Node IDs | **UUIDv7** |
| Signatures | **Algorithm-agile** — Ed25519 default; ML-DSA-65 (FIPS 204) and SLH-DSA (FIPS 205) for PQC; hybrid mode |
| Identity | **UDAP** (institutional) + **SPIFFE/SPIRE** (workload), cert-manager rotation |
| Deployment | **Kubernetes-native** — **Helm** chart is the primary production target; **Ansible** for cluster automation (cert-manager + SPIRE + Helm release); **Podman** or **Docker Compose** for local dev; Operator deferred |
| License | **Apache 2.0** |

## Repository layout

See [`REPO_STRUCTURE.md`](REPO_STRUCTURE.md) for the full map. In brief: the Rust
workspace lives in `crates/`, the FHIR Bridge in `bridge/`, deployment artifacts in
`deploy/`, the conformance suite and synthetic-data generator in `conformance/`, and all
specification documents in `docs/`.

## Build milestones

The build proceeds in strict dependency order:

| Milestone | Component | Spec sections | Status |
|---|---|---|---|
| M0 | Repo init + CI | §12.2.2 | Done |
| M1 | Event model (`creda-events`) | §3, §4, §5 | Implemented · tests green |
| M2 | Storage (`creda-store`) | §5.2, §7.3, App. C | Implemented · tests green (incl. RocksDB) |
| M3 | Graph / computation (`creda-graph`) | §5.2.4, §4.6, §5.3, §4.6 step 5.5 | Implemented · tests green (incl. link-chain) |
| M4 | Networking (`creda-net`) | §6, §7 | Pure logic green; libp2p adapter green under `make libp2p` |
| M5 | Credara Core (`creda-core`) | §10.1 | Implemented · tests green |
| M6 | Export Gate + Verifier | §4.5, §10.2, §10.3 | Implemented · tests green |
| M7 | FHIR Bridge (`bridge/`) | §8, §10.4 | Builds green; FHIR↔CBOR mappers are stubs |
| M8 | Deployment (`deploy/`) | §10.5, §10.6, §11 | **Verified on kind** · gossip-convergence + AE-repair scenarios green |
| M9 | Conformance + synthetic data (`conformance/`) | §11.4 | Implemented · tests green (incl. rogue-Link scenarios) |
| DQ-3 | Multi-peer testbed | §11.4, §10.6 | Two scenarios green on kind; partition/revocation/upgrade/storage-class scenarios in progress |

Verified by component: the default workspace (M1–M6, M9, plus the replication core) builds
and tests green via `anchor creda` (or `make test`); the opt-in **gRPC** server via `make
grpc`; the **FHIR Bridge** via `make bridge`. The shipped **libp2p** feature set
(`make libp2p`) builds and lints clean against the pinned libp2p version. End-to-end
multi-peer deployment is exercised in the test bed under `testbed/`:
`make -C testbed up && make -C testbed smoke` brings up two real peers in kind, injects an
event at peer-a via a Kubernetes Job, and observes it at peer-b through real gossipsub —
typically sub-10 ms convergence on a developer laptop. `make -C testbed ae-repair` proves
the §6.1.8 anti-entropy backstop by joining peer-b after the events were already published
at peer-a and watching the periodic manifest exchange heal the gap. Partition, revocation
latency, rolling upgrade, storage class, and in-cluster rogue-Link scenarios use the same
scaffold and land in subsequent passes.

## Building

The only host prerequisite is a container engine — **Podman** or **Docker**: every task runs
inside the dev container, so no one installs a Rust toolchain, protoc, or a JDK by hand. The
`docker`-invoking targets run unchanged under Podman's Docker-compatible CLI (see
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)).

```sh
anchor creda           # build + test the whole default workspace, one rolled-up summary (= make anchor)
make grpc              # build + lint + test the opt-in gRPC server (feature `grpc`; needs protoc)
make libp2p            # compile + lint the shipped feature set (gRPC + libp2p)
make bridge            # build the HAPI FHIR Bridge (Java/Kotlin) in a Gradle + JDK container

# Multi-peer testbed on kind (Podman or Docker + kind + kubectl + helm; no host Rust):
make -C testbed up        # create kind cluster + build & load all 3 testbed images
make -C testbed smoke     # gossip-convergence scenario — typically <10 ms end-to-end
make -C testbed ae-repair # anti-entropy-repair scenario — late-join healed via AE round
make -C testbed down      # tear down the cluster
```

The default build is intentionally free of the heavy, version-volatile dependencies
(libp2p, tonic/protoc, the JVM bridge): those live behind features and separate targets so
`anchor creda` stays fast and always green. With a local toolchain the workspace also builds
the ordinary way (`cargo build --workspace` / `cargo test --workspace`). Local multi-peer
development uses Podman or Docker Compose under `deploy/compose/`. The multi-peer test bed under
`testbed/` runs entirely in-cluster — the peer-driver is a Rust binary packaged as an image
and invoked as a Kubernetes Job — so no host Rust toolchain is needed even for the
end-to-end scenarios.

## Security and data handling

This is healthcare infrastructure. **Never commit secrets, credentials, or real PHI** —
all testing uses the synthetic data generator (M9) with `test-data` tagging so synthetic
events are provably invisible to clinical FHIR queries. Vulnerability reports route to a
private channel; see [`SECURITY.md`](SECURITY.md). The security model (UDAP + SPIFFE dual
credential, mandatory signature verification on replication, authorization enforcement at
the responding peer, and dual-control) is load-bearing — see spec §9.

## Contributing

Contributions are welcome under the spec-first, conformance-driven model described in
[`CONTRIBUTING.md`](CONTRIBUTING.md). Read the relevant specification section before
writing code for a component, and cite it in your commits.

## License

Licensed under the [Apache License 2.0](LICENSE). Open source is a precondition of the
design (spec §12.2.2): it enables independent security review, standards-body acceptance,
and freedom from vendor lock-in.
