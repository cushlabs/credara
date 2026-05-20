# deploy — Deployment Packaging (M8)

**Governing spec sections:** §10.5 (Peer Daemon), §10.6 (Container Image / Kubernetes), §7.4
(tooling matrix), §11 (Operations). See also `docs/DESIGN_QUEUE.md` (DQ-1, DQ-2).

- `docker/`      — multi-stage Dockerfiles per binary (Rust builder → distroless **nonroot**
  for Core/Export Gate/Verifier; Gradle builder → distroless-java nonroot for the Bridge).
- `compose/`     — Docker Compose for laptop development (single-node dev instance).
- `helm/creda/`  — Helm chart: StatefulSet, Services, ConfigMap, Secret references,
  ServiceAccount + minimal RBAC, NetworkPolicy, PodDisruptionBudget, and scheduled
  operational tasks as k8s CronJobs (snapshot generation, retention pruning, reputation decay).
- `ansible/`     — automation to deploy Creda onto an **existing** k8s cluster (DQ-2):
  installs cert-manager + SPIRE, then the Helm release; idempotent.

**Assemble:** Helm, k8s primitives, distroless images, MinIO, cert-manager, SPIRE,
Prometheus/Grafana/OpenTelemetry, Ansible (kubernetes.core collection).
**Write:** Helm templates, Dockerfiles, Compose file, CronJobs, Ansible plays.

> **Critical constraint (spec §6):** the same image and the same Helm chart must work on a
> laptop (Compose), on-prem k8s (bundled MinIO), and cloud k8s (S3) — only configuration
> values change.

> **Hard requirement — non-root (DQ-1):** every container runs as an unprivileged,
> non-root user in all environments. Distroless *nonroot* base images; pod/container
> `securityContext` with `runAsNonRoot: true`, fixed non-zero UID/GID/fsGroup,
> `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`,
> `capabilities.drop: [ALL]`, `seccompProfile: RuntimeDefault`. The chart must install
> under the **restricted** Pod Security Standard. CI fails on any root container.
