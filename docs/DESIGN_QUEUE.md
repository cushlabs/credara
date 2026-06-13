# Creda — Design Queue

Queued design requirements and refinements that extend the technical specification.
Each item records the requirement, its rationale, which milestone it attaches to, and
acceptance criteria. This file is the local backlog until issue tracking exists on the
remote; when the GitHub repo is created, each open item below should become a tracked
issue.

> The authoritative source of truth remains `creda-technical-spec.md`. Items here refine
> or constrain how a milestone is built; they do not override the spec's architecture.

---

## DQ-1 — Non-root containers (cross-cutting hardening) — REQUIRED

**Requirement.** Every Creda container runs as an unprivileged, non-root user. No
component runs as UID 0, in any environment (laptop, on-prem, cloud). This is a hard
constraint, not a default that can be relaxed per-deployment.

**Rationale.** Healthcare infrastructure handling PHI; minimizing the blast radius of a
container compromise is load-bearing. Aligns with the spec's distroless choice (§10.6)
and zero-trust insider-threat posture (§9). Many hardened k8s environments (restricted
Pod Security Standard, OpenShift) refuse root containers outright, so this is also a
portability requirement.

**Attaches to.** M8 (Deployment), and influences M5–M7 binaries (must not require
privileged ports or root-owned paths).

**How to apply.**
- **Images:** build on the Fedora Hummingbird hardened distroless base images (see DQ-4),
  *nonroot* variants; set a non-root `USER` in every Dockerfile; no `setuid` binaries;
  read-only root filesystem where possible with explicit writable volumes for state.
- **Binaries:** bind only to unprivileged ports (>=1024); never assume write access outside
  declared data/cache volumes; no reliance on root-owned config paths.
- **Helm / k8s:** pod and container `securityContext` set
  `runAsNonRoot: true`, a fixed non-zero `runAsUser`/`runAsGroup`/`fsGroup`,
  `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`,
  `capabilities.drop: ["ALL"]`, and `seccompProfile.type: RuntimeDefault`. Chart must be
  installable under the **restricted** Pod Security Standard.
- **CI:** add a check (M8) that fails if any image runs as root or any Helm template omits
  the non-root securityContext.

**Acceptance criteria.** All images pass a "does not run as root" scan; the Helm chart
deploys cleanly into a namespace labeled `pod-security.kubernetes.io/enforce=restricted`;
no container requests added capabilities or privilege escalation.

---

## DQ-2 — Ansible playbook: deploy onto an existing cluster

**Requirement.** Ship an Ansible playbook that automates installing Creda **onto an
existing Kubernetes cluster**. It does not provision the cluster itself; it assumes a
working cluster and kubeconfig.

**Scope (decided).** The playbook:
1. Validates prerequisites (reachable cluster, Helm present, required API versions).
2. Installs/ensures Creda's cluster dependencies — **cert-manager** (UDAP cert rotation)
   and **SPIRE** (SPIFFE workload identity) — idempotently, pinned to known-good versions.
3. Deploys the Creda **Helm release** with a supplied values file, including the non-root
   securityContext settings from DQ-1.
4. Verifies rollout (pods Ready, liveness/readiness passing) and reports status.

Out of scope (for this item): OS/host provisioning, container-runtime install, and
standing up k8s itself. (If full bare-metal provisioning is wanted later, add a separate
layered play — tracked as a future item, not this one.)

**Rationale.** Operators adopting Creda will commonly already run k8s; "deployable with
little to no oversight" (spec §6) means a one-command, idempotent, repeatable install
against that cluster.

**Attaches to.** M8 (Deployment). Depends on the Helm chart existing.

**Acceptance criteria.** `ansible-playbook deploy.yml -e @cluster-values.yml` against a
clean existing cluster brings up a working Creda peer with cert-manager and SPIRE present;
re-running is idempotent (no changes on second run); the play fails fast with a clear
message if prerequisites are missing.

**Lives in.** `deploy/ansible/`.

---

## DQ-3 — Local multi-peer test bed (Compose + kind/k3d)

**Requirement.** A local test bed that simulates a small Creda network (2–3+ peers) and
verifies the system behaves as it would in production. Two paths, same scenarios:

- **Compose path (`testbed/compose/`)** — fast, lightweight multi-peer bring-up for
  day-to-day development iteration. Optimized for speed and quick log/inspect cycles.
- **kind/k3d path (`testbed/kind/`)** — production-fidelity: peers run as pods from the
  **real Helm chart** on a local k8s cluster, exercising the non-root securityContexts
  (DQ-1), Services, NetworkPolicy, and CronJobs exactly as production would.

**Scenarios the test bed must support.** Bring up N peers; create identity and
authorization events on one peer; assert they replicate to the others via gossip within
the expected window; verify anti-entropy repairs a deliberately-desynced peer; bootstrap a
new peer from a snapshot; simulate a network partition and rejoin; exercise dual-control
(Export Gate refusal/permit + Verifier decision) and revocation propagation within the
Bound-1 window (§4.7). All against **synthetic data only** (M9 generator), with results
asserted automatically, not just eyeballed.

**Rationale.** "Verify the system is working like we'd expect in production" requires
exercising the real replication, security, and authorization paths on a realistic
topology — not a single process. The kind/k3d path catches k8s-specific issues (RBAC,
securityContext, NetworkPolicy) that Compose cannot.

**Attaches to.** Bootstrapped at M4 (the first multi-peer convergence harness lives here),
grows through M5–M7, and reaches full fidelity alongside M8 (Helm) and M9 (conformance +
synthetic data). The test bed and the M9 conformance suite share the synthetic generator
and scenario library; the test bed is the *interactive/local* runner, conformance is the
*CI gate*.

**Acceptance criteria.** A single command spins up a multi-peer network on each path;
the scenario suite runs and asserts convergence, anti-entropy repair, snapshot bootstrap,
partition/rejoin, dual-control, and revocation latency; the kind/k3d path uses the
unmodified Helm chart under the restricted Pod Security Standard.

**Lives in.** `testbed/` (`compose/`, `kind/`, and a shared `scenarios/` library).

---

## DQ-4 — Container base images: Fedora Hummingbird (FIPS, distroless) — REQUIRED

**Requirement.** All Creda container images — both **build** stages and **runtime** stages,
for every binary — are based on **Fedora Hummingbird** hardened distroless images, using the
**FIPS-validated variants by default**.

**What Hummingbird is.** A catalog of minimal, hardened, distroless OCI images (no package
manager, no shell, just the app and its strict runtime deps) kept at near-zero CVE via
reproducible builds from pinned package lists and continuous Syft/Grype scanning, with FIPS
variants and multi-arch support (x86_64 and **aarch64**). It ships images for our exact
stacks: a **Rust** image (Core, Export Gate, Verifier) and an **OpenJDK** image (the HAPI
FHIR Bridge).

**Decided scope.** Container images only. Creda standardizes its build and runtime base
images on Hummingbird; the **host OS remains the operator's choice** (Hummingbird also ships
a bootable read-only-root OS, noted here as an option operators may adopt for extra posture,
but Creda does not require it).

**Why.**
- **Lower attack surface + near-zero CVE.** Distroless removes the shell and package
  manager; Hummingbird's pipeline rebuilds and reships on upstream patches, keeping CVE
  counts near zero — a meaningful upgrade over a static generic distroless base.
- **FIPS by default fits the domain.** PHI handling and Creda's FIPS 204 (ML-DSA-65) /
  FIPS 205 (SLH-DSA) post-quantum signature choices align with FIPS-validated crypto in the
  base image; many healthcare and federal deployments require it.
- **Reinforces DQ-1.** Distroless nonroot images are exactly what the non-root requirement
  needs; pairs cleanly with read-only root filesystem and dropped capabilities.

**How to apply.**
- **Dockerfiles (M8):** multi-stage — Hummingbird Rust image as the Rust builder → Hummingbird
  minimal distroless (FIPS) runtime for Core/Export Gate/Verifier; Hummingbird OpenJDK image
  as the Bridge builder/runtime (FIPS). Pin image digests; non-root `USER`.
- **Defaults & opt-out:** FIPS variant is the default tag; a documented non-FIPS opt-out
  exists for deployments that explicitly do not need it.
- **Multi-arch:** publish x86_64 and aarch64 (Hummingbird supports both).
- **CI (M8):** the existing "no root" gate (DQ-1) plus a base-image check that fails if an
  image is not a pinned Hummingbird base; surface the Grype/CVE status in CI.

**Acceptance criteria.** Every Creda image FROMs a pinned Fedora Hummingbird base (FIPS by
default); images run non-root under the restricted Pod Security Standard (DQ-1); a CVE scan
of the shipped images shows near-zero high/critical findings; images build and run on both
x86_64 and aarch64.

**Supersedes.** The generic "distroless" base-image choice (spec §10.6 / the original
decision record). This is a *specialization* of "distroless," not a contradiction — Creda's
distroless base is now specifically Fedora Hummingbird.

---

## DQ-5 — Reproducible developer environment: Docker-only, deps auto-provisioned — REQUIRED

**Requirement.** A developer (or CI) must be able to build and test Creda **without manually
installing a toolchain**. The Rust toolchain, the C compiler (for pqcrypto), and all
dependencies are evaluated and provisioned by the environment, not by a human following a
checklist. The only host prerequisite is a container engine — Podman or Docker.

**Decided shape.**
- A **dev/build container** (`docker/dev.Dockerfile`) carries the full toolchain.
- A **task runner** (`Makefile`) runs `cargo` inside that container as the host user
  (`make test`, `test-fast`, `fmt`, `fmt-check`, `clippy`, `build`, `ci`, `shell`, `clean`).
- A **devcontainer** (`.devcontainer/devcontainer.json`) gives VS Code / Codespaces users
  the same environment automatically, running as a non-root `dev` user.
- The dev base image is **Fedora** (parity with the Hummingbird/Fedora shipped images, DQ-4):
  building dev/CI on the same OS family we ship on keeps glibc/system-library/packaging behavior
  consistent dev↔prod. The Dockerfile is package-manager-agnostic, so `DEV_BASE` can fall back to
  the official Debian Rust image (`docker.io/library/rust:1-bookworm`) instantly if needed. The
  **shipped** images remain hardened distroless Hummingbird (DQ-4); this item governs only the
  local build/test environment.
- The dependency cache lives in a gitignored in-repo dir to avoid named-volume permission
  issues with a non-root container user.

**Why.** "No manual install steps for devs" was an explicit requirement. A containerized,
single-command workflow is reproducible, matches our container-first posture, and gives
developers and CI identical environments.

**Attaches to.** Cross-cutting / developer tooling; underpins every milestone's "Done when"
(tests must run). CI (M0 `ci-rust.yml`) should converge on the same container path over time.

**Acceptance criteria.** On a machine with only Docker installed, `make test` builds the dev
image and runs the workspace test suite green; `make ci` reproduces the CI gates locally;
opening the repo in a Dev Container yields a working `rust-analyzer` setup with no local Rust
install.

**Docs.** `docs/DEVELOPMENT.md`.

---

## DQ-6 — Build/prod parity everywhere (development principle) — REQUIRED

**Principle.** Build and CI environments should match the OS family and runtime they ship on, so
glibc, system libraries, packaging, and crypto behavior are the same in development as in
production. We catch family-specific issues at build time, not at deploy time. This is a
standing principle, applied to every component, not a one-off.

**Application.**
- **Rust crates:** dev/build image is Fedora (DQ-5), shipped images are Fedora Hummingbird
  distroless FIPS (DQ-4). ✔
- **FHIR Bridge (Java/Kotlin):** shipped image is Hummingbird OpenJDK FIPS (DQ-4); the **build**
  image (`make bridge`) should likewise be a Fedora + OpenJDK base rather than a Debian-based
  `gradle` image. Because Hummingbird is distroless (no Gradle, no shell), the *build* image is a
  Fedora base with OpenJDK + Gradle installed (the same dev-vs-shipped split used for Rust: a
  buildable base of the same OS family, the hardened distroless variant for shipping).
- **General rule:** when adding any new build/CI image, default it to the Fedora/Hummingbird
  family; a non-family fallback may exist via an override variable but is not the default.

**Note on sequencing:** switching a build image is itself unverifiable here (no Docker), so when
a component is mid-debug (e.g., the bridge's first compile), keep the known-good build image until
it is green, then switch to the parity image — so an image-build hiccup never tangles with a
code-debug loop.

---

## Decisions log (for these items)

| Item | Decision | Date |
|---|---|---|
| DQ-1 | Non-root is a hard requirement in all environments | 2026-05-20 |
| DQ-2 | Ansible automates **deploy onto existing cluster** (not host provisioning) | 2026-05-20 |
| DQ-3 | Test bed provides **both** Compose (fast) and kind/k3d (production-like) paths | 2026-05-20 |
| DQ-4 | Base images = **Fedora Hummingbird**, **FIPS by default**, **container images only** (host OS = operator's choice) | 2026-05-20 |
| DQ-5 | Dev environment = **Docker-only**, deps auto-provisioned (dev container + Makefile + devcontainer); dev base = **Fedora** (parity with Hummingbird family), Debian Rust image as fallback via `DEV_BASE` | 2026-05-20 |
| DQ-6 | **Build/prod parity everywhere** is a standing development principle: build/CI images match the OS family they ship on (Fedora/Hummingbird); applies to the bridge build image too | 2026-05-20 |
