# deploy — Deployment Packaging (M8)

**Governing spec sections:** §10.5 (Peer Daemon), §10.6 (Container Image / Kubernetes), §7.4
(tooling matrix), §11 (Operations). See also `docs/DESIGN_QUEUE.md` (DQ-1, DQ-2, DQ-4).

- `docker/`      — multi-stage Dockerfiles per binary, all on **Fedora Hummingbird** hardened
  distroless base images (FIPS variants by default, DQ-4): Hummingbird Rust image (builder) →
  Hummingbird distroless **nonroot** runtime for Core/Export Gate/Verifier; Hummingbird
  OpenJDK image for the Bridge. Pin image digests; multi-arch (x86_64 + aarch64).
- `compose/`     — Docker Compose for laptop development (single-node dev instance).
- `helm/creda/`  — Helm chart: StatefulSet, Services, ConfigMap, Secret references,
  ServiceAccount + minimal RBAC, NetworkPolicy, PodDisruptionBudget, and scheduled
  operational tasks as k8s CronJobs (snapshot generation, retention pruning, reputation decay).
- `ansible/`     — automation to deploy Creda onto an **existing** k8s cluster (DQ-2):
  installs cert-manager + SPIRE, then the Helm release; idempotent.

**Assemble:** Helm, k8s primitives, Fedora Hummingbird hardened distroless base images
(FIPS), MinIO, cert-manager, SPIRE, Prometheus/Grafana/OpenTelemetry, Ansible
(kubernetes.core collection).
**Write:** Helm templates, Dockerfiles, Compose file, CronJobs, Ansible plays.

> **Critical constraint (spec §6):** the same image and the same Helm chart must work on a
> laptop (Compose), on-prem k8s (bundled MinIO), and cloud k8s (S3) — only configuration
> values change.

> **Hard requirement — non-root (DQ-1):** every container runs as an unprivileged,
> non-root user in all environments. Fedora Hummingbird distroless *nonroot* base images
> (FIPS by default, DQ-4); pod/container
> `securityContext` with `runAsNonRoot: true`, fixed non-zero UID/GID/fsGroup,
> `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`,
> `capabilities.drop: [ALL]`, `seccompProfile: RuntimeDefault`. The chart must install
> under the **restricted** Pod Security Standard. CI fails on any root container.

## Status: implemented (M8) — artifacts written; full verification on the test bed (DQ-3)

These are deployment manifests/Dockerfiles (no compile), so they're authored faithfully but not
runtime-verified here (no k8s/Docker in the authoring environment). End-to-end verification —
`helm install` on kind/k3d, peers joining and replicating — lives in `testbed/` (DQ-3, M9).

Concrete files:
- `docker/core.Dockerfile`, `docker/bridge.Dockerfile` — multi-stage; Hummingbird FIPS runtime
  bases (DQ-4), non-root. Built from the **repo root** (they need the workspace / shared proto).
- `helm/creda/` — `Chart.yaml`, `values.yaml`, and templates: `statefulset` (Core + Bridge
  containers sharing an `emptyDir` UDS at `/var/run/creda`, §10.5.1; data PVC; full non-root
  `securityContext`; `/livez` `/readyz` probes), `service` (FHIR ClusterIP + libp2p NodePort/LB +
  headless), `configmap`, `rbac` (minimal Role + ServiceAccount), `networkpolicy`,
  `poddisruptionbudget`, `cronjob-snapshot`.
- `compose/docker-compose.yml` — laptop dev (Core + Bridge + optional MinIO via `--profile storage`).
- `ansible/deploy.yml` — deploy onto an existing cluster (DQ-2): restricted-PSS namespace →
  cert-manager + SPIRE (idempotent) → Creda Helm release → verify rollout. `requirements.yml`,
  `inventory.example.ini`, `group_vars/all.example.yml`.

### Known reconciliation items (TODO)
- Pin exact Hummingbird FIPS image references/digests (DQ-4) — placeholders today.
- The core image build enables `--features grpc,libp2p`; that needs the libp2p adapter reconciled
  and protoc in the builder — Compose defaults to `FEATURES=grpc` for now. The in-daemon
  gRPC-serve socket is now wired (`creda serve` binds the Unix domain socket at `grpc_socket`,
  default `/run/creda/creda.sock`; verify with `make grpc`); the **libp2p-transport** wiring is
  still ahead of the daemon (tracked).
- Snapshot CronJob vs. RWO PVC: lightweight snapshots run in-daemon (§10.5.2); the CronJob's
  trigger mechanism is a TODO (see the template).
