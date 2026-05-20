# testbed/kind — Production-fidelity path (DQ-3)

Stands up a local kind/k3d cluster and deploys peers from the **unmodified Helm chart**
(`deploy/helm/creda`) under the restricted Pod Security Standard, so the non-root
securityContexts (DQ-1), Services, NetworkPolicy, and CronJobs are exercised exactly as in
production. Runs the shared `../scenarios/`. Slower than the Compose path; higher fidelity.
