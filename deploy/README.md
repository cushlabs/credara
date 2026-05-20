# deploy — Deployment Packaging (M8)

**Governing spec sections:** §10.5 (Peer Daemon), §10.6 (Container Image / Kubernetes), §7.4
(tooling matrix), §11 (Operations).

- `docker/`  — multi-stage Dockerfiles per binary (Rust builder → distroless for
  Core/Export Gate/Verifier; Gradle builder → distroless-java for the Bridge).
- `compose/` — Docker Compose for laptop development (single-node dev instance).
- `helm/creda/` — Helm chart: StatefulSet, Services, ConfigMap, Secret references,
  ServiceAccount + minimal RBAC, NetworkPolicy, PodDisruptionBudget, and the scheduled
  operational tasks as k8s CronJobs (snapshot generation, retention pruning, reputation decay).

**Assemble:** Helm, k8s primitives, distroless images, MinIO, cert-manager, SPIRE,
Prometheus/Grafana/OpenTelemetry. **Write:** Helm templates, Dockerfiles, Compose file, CronJobs.

> **Critical constraint:** the same image and the same Helm chart must work on a laptop (Compose),
> on-prem k8s (bundled MinIO), and cloud k8s (S3) — only configuration values change.
