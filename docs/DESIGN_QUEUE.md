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
- **Images:** build on distroless *nonroot* variants; set a non-root `USER` in every
  Dockerfile; no `setuid` binaries; read-only root filesystem where possible with explicit
  writable volumes for state.
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

## Decisions log (for these items)

| Item | Decision | Date |
|---|---|---|
| DQ-1 | Non-root is a hard requirement in all environments | 2026-05-20 |
| DQ-2 | Ansible automates **deploy onto existing cluster** (not host provisioning) | 2026-05-20 |
| DQ-3 | Test bed provides **both** Compose (fast) and kind/k3d (production-like) paths | 2026-05-20 |
