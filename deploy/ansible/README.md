# deploy/ansible — Install onto an existing Kubernetes cluster (M8, DQ-2)

Ansible automation that deploys Creda onto an **existing** k8s cluster. It does not
provision the cluster or the OS/host — see `docs/DESIGN_QUEUE.md` DQ-2 for scope.

Plays will:
1. Validate prerequisites (reachable cluster + kubeconfig, Helm present, API versions).
2. Idempotently install cluster dependencies: **cert-manager** (UDAP cert rotation) and
   **SPIRE** (SPIFFE workload identity), pinned to known-good versions.
3. Deploy the Creda **Helm release** with a supplied values file, including the non-root
   securityContext settings (DQ-1).
4. Verify rollout (pods Ready, probes passing) and report status.

Target invocation (once implemented):
`ansible-playbook deploy.yml -e @cluster-values.yml`

Idempotent: re-running makes no changes on a converged cluster. Governing spec: §10.6, §11.
