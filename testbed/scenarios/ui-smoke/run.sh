#!/usr/bin/env bash
# UI smoke scenario.
#
# Deploys the persona front-end clients (clients/) into the testbed kind cluster, then runs
# Playwright as an in-cluster Job against http://creda-clients:8080. Mock-mode for now —
# clients render against in-memory fixtures whose shape matches the bridge's FHIR responses.
#
# Same execution model as scenarios/gossip-convergence/run.sh: no host port-forward, no host
# Node toolchain, no Mac-vs-Linux branching.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/ui-smoke"
mkdir -p "$RUN_DIR"

NS="creda-ui-smoke"
CHART="$TESTBED/helm/clients"
CLIENTS_IMAGE="creda-clients:testbed"
E2E_IMAGE="creda-clients-e2e:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

for img in "$CLIENTS_IMAGE" "$E2E_IMAGE"; do
  if ! docker image inspect "$img" >/dev/null 2>&1; then
    echo "ERROR: image $img not present locally; run 'make up' (or 'make images')" >&2
    exit 2
  fi
done

dump_diagnostics() {
  echo "------ $NS pods ------" >&2
  $kc -n "$NS" get pods 2>/dev/null || true
  for POD in $($kc -n "$NS" get pods -o name 2>/dev/null); do
    echo "------ describe $NS/$POD ------" >&2
    $kc -n "$NS" describe "$POD" 2>/dev/null | tail -40 || true
    echo "------ logs $NS/$POD ------" >&2
    $kc -n "$NS" logs "$POD" --all-containers --tail=120 2>/dev/null || true
  done
  echo "------ ui-smoke job logs ------" >&2
  $kc -n "$NS" logs job/ui-smoke --tail=400 2>/dev/null || true
}

cleanup() {
  local rc=$?
  if [[ $rc -ne 0 ]]; then
    echo "==> failure detected (rc=$rc); dumping diagnostics" >&2
    dump_diagnostics
  fi
  if [[ "${KEEP_NAMESPACES:-0}" = "1" ]]; then
    echo "==> KEEP_NAMESPACES=1; leaving $NS in place for manual inspection"
    exit "$rc"
  fi
  echo "==> cleanup"
  $hm uninstall -n "$NS" clients 2>/dev/null || true
  $kc delete namespace "$NS" --wait=false --ignore-not-found 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT

# ---- namespace ----------------------------------------------------------------------------
echo "==> creating namespace $NS"
$kc create namespace "$NS" >/dev/null
$kc label namespace "$NS" pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null

# ---- install clients chart ---------------------------------------------------------------
echo "==> installing clients (mock mode)"
$hm install -n "$NS" clients "$CHART" \
  --set image.repository=creda-clients \
  --set image.tag=testbed \
  --set image.pullPolicy=Never \
  --wait --timeout 120s >/dev/null

bash "$TESTBED/scripts/wait-ready.sh" "$NS" "" 120 \
  || $kc -n "$NS" rollout status deploy/creda-clients --timeout=120s

# ---- run Playwright as an in-cluster Job -------------------------------------------------
echo "==> running playwright e2e (in-cluster)"
cat <<EOF | $kc -n "$NS" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: ui-smoke
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 600
  template:
    spec:
      restartPolicy: Never
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        runAsGroup: 1000
        fsGroup: 1000
        seccompProfile: { type: RuntimeDefault }
      containers:
        - name: playwright
          image: $E2E_IMAGE
          imagePullPolicy: Never
          securityContext:
            allowPrivilegeEscalation: false
            runAsNonRoot: true
            capabilities: { drop: ["ALL"] }
          env:
            - name: CLIENTS_URL
              value: "http://creda-clients:8080"
            - name: CI
              value: "1"
          # /tmp gives Playwright a writable scratch dir without needing a hostPath volume.
          # The default base image is layered for non-root, but Playwright traces go in cwd.
          workingDir: /tmp
          command: ["pnpm"]
          args: ["--prefix", "/app", "test:e2e"]
EOF

# Tail the Playwright output while the Job runs; \`wait\` returns when the Job finishes (or
# its deadline elapses), at which point we print the final status line.
PLAYWRIGHT_BUDGET_S=240
$kc -n "$NS" wait --for=condition=complete --timeout=${PLAYWRIGHT_BUDGET_S}s job/ui-smoke \
  || {
    echo "FAIL: ui-smoke Job did not complete within ${PLAYWRIGHT_BUDGET_S}s" >&2
    $kc -n "$NS" logs job/ui-smoke --tail=400 >&2 || true
    exit 1
  }

# Print the Job logs unconditionally on success so the scenario output shows the test summary
# (number of specs passed, etc.) — matches the gossip-convergence scenario's latency print.
echo "------ playwright output ------"
$kc -n "$NS" logs job/ui-smoke --tail=200

echo "PASS: ui-smoke (clients chart + playwright e2e against in-cluster Service)"
