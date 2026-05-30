#!/usr/bin/env bash
# Bring up the persona front-end clients in the testbed cluster for manual user-acceptance
# testing. Persistent — survives `make ui-smoke` runs (those use a separate namespace).
#
# After this returns, run `make ui-forward` in a second terminal to port-forward the UI to
# http://localhost:5173 on your laptop. Repeat installs are idempotent — re-running this
# script on top of an existing install upgrades the chart in place.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"

NS="creda-ui"
CHART="$TESTBED/helm/clients"
CLIENTS_IMAGE="creda-clients:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

if ! docker image inspect "$CLIENTS_IMAGE" >/dev/null 2>&1; then
  echo "ERROR: image $CLIENTS_IMAGE not present locally; run 'make up' (or 'make images') first" >&2
  exit 2
fi

# Ensure the cluster is reachable before we touch helm — the error from helm if the cluster
# is down is cryptic. `kubectl version --request-timeout` returns within a second.
if ! $kc version --request-timeout=2s >/dev/null 2>&1; then
  echo "ERROR: kind cluster '$CLUSTER' is not reachable; run 'make up' first" >&2
  exit 2
fi

# Idempotent namespace + PSS label. `kc create ns --dry-run | apply` so re-runs don't fail.
$kc create namespace "$NS" --dry-run=client -o yaml | $kc apply -f - >/dev/null
$kc label namespace "$NS" pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null

# On any failure below this point, dump pod state + container logs for the namespace so the
# operator sees *why* the pod didn't come Ready instead of just the "context deadline
# exceeded" message helm prints. Matches the diagnostics block in scenarios/*-/run.sh.
dump_diagnostics() {
  echo "------ $NS pods ------" >&2
  $kc -n "$NS" get pods 2>/dev/null || true
  for POD in $($kc -n "$NS" get pods -o name 2>/dev/null); do
    echo "------ describe $NS/$POD ------" >&2
    $kc -n "$NS" describe "$POD" 2>/dev/null | tail -60 || true
    echo "------ logs $NS/$POD (current) ------" >&2
    $kc -n "$NS" logs "$POD" --all-containers --tail=120 2>/dev/null || true
    echo "------ logs $NS/$POD (previous, if any) ------" >&2
    $kc -n "$NS" logs "$POD" --all-containers --previous --tail=120 2>/dev/null || true
  done
}
trap 'rc=$?; if [[ $rc -ne 0 ]]; then echo "==> ui-up failed (rc=$rc); dumping diagnostics" >&2; dump_diagnostics; fi' EXIT

# `helm upgrade --install` so the first run installs and subsequent runs upgrade in place.
echo "==> installing clients into namespace $NS"
$hm upgrade --install -n "$NS" clients "$CHART" \
  --set image.repository=creda-clients \
  --set image.tag=testbed \
  --set image.pullPolicy=Never \
  --wait --timeout 120s >/dev/null

$kc -n "$NS" rollout status deploy/creda-clients --timeout=120s

cat <<EOF

==> UI is up in namespace '$NS' (mock-mode FHIR fixtures).

   Open it in your browser by running this in a second terminal:

       cd testbed && make ui-forward

   That forwards http://localhost:5173 → the in-cluster Service. Ctrl-C kills the
   forwarder only; the UI keeps running. Tear the UI down with 'make ui-down'.

   Personas:
     http://localhost:5173/            — landing (links to all five)
     http://localhost:5173/clinician
     http://localhost:5173/prior-auth
     http://localhost:5173/steward
     http://localhost:5173/patient
     http://localhost:5173/audit

EOF
