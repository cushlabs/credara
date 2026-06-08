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

# Ensure the cluster is reachable before we touch helm — the error from helm if the cluster
# is down is cryptic. `kubectl version --request-timeout` returns within a second.
if ! $kc version --request-timeout=2s >/dev/null 2>&1; then
  echo "ERROR: kind cluster '$CLUSTER' is not reachable; run 'make up' first" >&2
  exit 2
fi

# Always rebuild + reload images (cheap via Docker layer cache when unchanged). Same reason
# as ui-up-real.sh — `:testbed` + pullPolicy: Never means the kubelet sticks with whatever
# image bytes are already on the node, so an edit loop ("change source, rerun ui-up") needs
# a manual `make images` otherwise. Set CREDA_SKIP_IMAGES=1 to skip when iterating on chart
# values only.
if [[ "${CREDA_SKIP_IMAGES:-0}" != "1" ]]; then
  echo "==> ensuring images are up to date (set CREDA_SKIP_IMAGES=1 to skip)"
  bash "$REPO_ROOT/testbed/images/build-and-load.sh" "$CLUSTER"
fi

if ! docker image inspect "$CLIENTS_IMAGE" >/dev/null 2>&1; then
  echo "ERROR: image $CLIENTS_IMAGE not present locally after build; check the build output above" >&2
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

# Force-delete any stuck clients pods from a previous iteration so the new ReplicaSet
# creates pods that pick up the freshly-loaded image bytes. With pullPolicy: Never +
# unchanged tag, kubelet doesn't restart a CrashLoopBackOff container even when the bytes
# on the node have been updated by `kind load`. See ui-up-real.sh for the longer note.
echo "==> clearing any stuck clients pods so fresh bytes can take effect"
$kc -n "$NS" delete pod -l app.kubernetes.io/name=creda-clients \
  --force --grace-period=0 --ignore-not-found 2>/dev/null || true

# `helm upgrade --install` so the first run installs and subsequent runs upgrade in place.
echo "==> installing clients into namespace $NS"
$hm upgrade --install -n "$NS" clients "$CHART" \
  --set image.repository=creda-clients \
  --set image.tag=testbed \
  --set image.pullPolicy=Never \
  --wait --timeout 120s >/dev/null

# Force a roll *every* run. Helm only triggers a pod restart when the rendered template
# changes; here the image tag stays at `:testbed` between code iterations and the rendered
# YAML is byte-identical, so helm says "no diff" and leaves the running pod alone — even
# though `make images` just refreshed the bytes on each kind node. `kubectl rollout restart`
# recreates the pod so the kubelet picks up the new image archive.
echo "==> forcing clients rollout to pick up the latest creda-clients image"
$kc -n "$NS" rollout restart deploy/creda-clients >/dev/null
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
