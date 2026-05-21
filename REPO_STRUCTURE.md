# Creda — Repository Structure

This document defines the target layout of the Creda repository and which part of
the technical specification governs each component. It is the map the build follows
through milestones **M0–M9** (see `docs/COWORK_BUILD_GUIDE.md`). The authoritative
source of truth for all technical decisions is `docs/creda-technical-spec.md`; where
this document and the spec disagree, the spec wins.

## Guiding principle

Creda is **assembled, not reinvented** (spec Appendix C). The Rust crates and the
Java bridge below contain only the genuinely-new healthcare-domain layer
(~8,000–15,000 lines) on top of mature libraries: libp2p, HAPI FHIR, RocksDB/libgit2,
the `pqcrypto` family, ciborium, blake3, SPIRE, cert-manager, and others. If a
component starts reimplementing a gossip protocol, a DHT, a FHIR server, or a crypto
primitive, that is a mistake — assemble it instead.

**Cross-cutting requirements** that constrain how milestones are built are tracked in
`docs/DESIGN_QUEUE.md`. Notably, **every container runs non-root** in all environments
(DQ-1) — a hard requirement, not a per-deployment default. Deployment automation for an
existing cluster lives in `deploy/ansible/` (DQ-2); the local multi-peer test bed lives in
`testbed/` (DQ-3).

## Top-level layout

```
creda/
├── Cargo.toml                 # Rust workspace root. Members are added per milestone
│                              #   (empty at M0 so CI is green on the placeholder build).
├── rust-toolchain.toml        # Pinned Rust toolchain for reproducible builds.
├── LICENSE                    # Apache License 2.0 (open source is a spec precondition, §12.2.2).
├── README.md                  # Project overview, status, and quickstart.
├── REPO_STRUCTURE.md          # This file.
├── CONTRIBUTING.md            # Spec-first, conformance-driven, DCO sign-off.
├── CODE_OF_CONDUCT.md         # Contributor Covenant.
├── SECURITY.md                # Private vulnerability disclosure (healthcare infrastructure).
├── .gitignore                 # Rust / Java / Node / OS-editor cruft.
├── Makefile                   # Docker-only task runner: make test / fmt / clippy / ci (DQ-5).
├── anchor                     # `anchor creda` — settle the workspace into a known-good state
│                              #   (full suite, single-threaded; = make anchor / test JOBS=1).
│
├── .devcontainer/             # Reproducible dev environment — Docker is the only host
│   ├── devcontainer.json      #   prerequisite (DQ-5). VS Code / Codespaces config.
│   └── Dockerfile             #   Dev/build image (NOT shipped); base switchable to Hummingbird.
│
├── .github/
│   └── workflows/
│       ├── ci-rust.yml        # fmt, clippy, test, release build for the Rust workspace.
│       ├── ci-java.yml        # Gradle build + test for the FHIR Bridge (M7).
│       ├── ci-conformance.yml # Runs the Conformance Suite (grows per milestone, M9).
│       └── ci-docs.yml        # Renders the spec and validates internal links.
│
├── docs/
│   ├── creda-technical-spec.md   # AUTHORITATIVE specification, Sections 1–13 + appendices.
│   ├── creda-technical-spec.pdf  # Rendered spec for non-technical reviewers.
│   ├── COWORK_CONTEXT.md         # Context & decision history (the "why").
│   ├── COWORK_BUILD_GUIDE.md     # Milestone-by-milestone build instructions.
│   ├── DESIGN_QUEUE.md           # Queued design reqs (DQ-1..DQ-5: non-root, ansible, testbed,
│   │                             #   Hummingbird images, Docker-only dev env).
│   └── DEVELOPMENT.md            # Docker-only developer workflow (DQ-5).
│
├── crates/                    # The Rust workspace members (Core, Export Gate, Verifier).
│   ├── creda-events/          # M1 — event node schema, 10 event types, canonical CBOR,
│   │                          #   blake3 hashing, UUIDv7, algorithm-agile signatures.
│   │                          #   Spec §3 (Identity Model), §4 (Portable Authorization),
│   │                          #   §5 (Data Structures). Pure data + crypto, no I/O.
│   ├── creda-store/           # M2 — `Store` trait + RocksDB impl; libgit2 scaffolded
│   │                          #   behind the trait. Secondary indexes (§5.2.5).
│   │                          #   Spec §5.2, §7.3, Appendix C.1/C.3. TODO(open-question-13.1).
│   ├── creda-graph/           # M3 — subgraph materialization, effective-identity
│   │                          #   projection, 7-step authorization evaluation, confidence
│   │                          #   scoring (Fellegi-Sunter, ported). Spec §5.2.4, §4.6, §5.3.
│   ├── creda-net/             # M4 — rust-libp2p stack (gossipsub bucketed topics,
│   │                          #   Kademlia DHT, Noise), anti-entropy, snapshots.
│   │                          #   Spec §6, §7. TODO(open-question-13.3) DHT query-privacy.
│   ├── creda-core/            # M5 — composes M1–M4 into a peer daemon; gRPC API (tonic),
│   │                          #   tokio runtime, CLI, hierarchical config. Spec §10.1.
│   ├── creda-export-gate/     # M6 — source-side dual-control enforcement; validates a
│   │                          #   Portable Authorization Artifact, emits ExportReceipt.
│   │                          #   Spec §4.5, §10.2. Reuses creda-graph authorization eval.
│   └── creda-verifier/        # M6 — relying-side SDK/runtime; validates authorization +
│                              #   identity continuity + provenance, incl. offline.
│                              #   Spec §4.5, §10.3. TODO(open-question-13.4.3) stale-state.
│
├── bridge/                    # M7 — HAPI FHIR Bridge (Java/Kotlin, Gradle). Plain Server
│                              #   mode, custom resource providers + operations, US Core
│                              #   profiles. TRANSLATOR, NOT REASONER. Spec §8, §10.4.
│
├── conformance/               # M9 — conformance suite + synthetic data generator
│                              #   (deterministic seed, test-data tagging/filtering).
│                              #   Spec §11.4, §11.
│
├── deploy/                    # M8 — deployment packaging. Same image + same Helm chart
│   │                          #   must run on laptop, on-prem, and cloud (config only).
│   ├── docker/                #   Multi-stage Dockerfiles on Fedora Hummingbird distroless
│   │                          #   NONROOT base (FIPS by default, DQ-4) per binary.
│   ├── compose/               #   Docker Compose for laptop dev.
│   ├── helm/creda/            #   Helm chart: StatefulSet, Services, ConfigMap, RBAC,
│   │                          #   NetworkPolicy, PDB, CronJobs; non-root securityContext (DQ-1).
│   └── ansible/               #   Deploy onto an existing cluster: cert-manager + SPIRE +
│                              #   Helm release, idempotent (DQ-2). Spec §10.5, §10.6, §7.4, §11.
│
├── testbed/                   # Local multi-peer test bed (DQ-3); same scenarios, two paths.
│   ├── compose/               #   Fast multi-peer bring-up (Docker Compose).
│   ├── kind/                  #   Production-fidelity: real Helm chart on kind/k3d, non-root.
│   └── scenarios/             #   Shared, runner-agnostic scenario library (also used by M9).
│
└── tools/                     # Dev utilities and scripts.
```

## How the structure fills in over time

At **M0** the crate and component directories exist with a placeholder `README.md`
that names the milestone and governing spec section, but they are **not yet registered
as Cargo workspace members** and contain no code — this keeps `cargo build`/`cargo test`
green on the empty workspace. Each subsequent milestone:

1. Reads the cited spec section in full.
2. Fills in the corresponding directory with real code + tests.
3. Registers any new Rust crate in the root `Cargo.toml` `members` list.
4. Extends the relevant CI workflow and the conformance suite.

## Dependency order (do not parallelize the spine)

`creda-events → creda-store → creda-graph → creda-net → creda-core` is a strict
dependency chain (M1→M5). `creda-export-gate` / `creda-verifier` (M6) and `bridge`
(M7) proceed once `creda-core` is stable. `deploy` (M8) and `conformance` (M9) come
last. See the build-order table in `docs/COWORK_BUILD_GUIDE.md` §4.

## Open questions are scaffolded, never silently resolved

Where the spec marks a decision unresolved (§13), the relevant directory carries a
clearly-marked `TODO(open-question-13.x)` at the scaffolded interface and a tracked
issue, rather than a quietly-chosen permanent answer. The currently-open items that
affect the build are storage substrate (13.1), the disambiguation question-selection
algorithm (13.2.x), pairwise vs. deterministic subject identifier, DHT query-privacy
(13.3 / §8.5), and revocation latency bounds 2 & 3 plus Export Gate integration and
Verifier stale-state policy (13.4.x).
