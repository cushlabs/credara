#!/usr/bin/env bash
# Real-mode UAT — bring up a single Creda peer (Core + Bridge) alongside the persona front-end
# clients, wire the clients' nginx /fhir reverse proxy at the bridge, and leave everything
# running so a reviewer can drive the UI in their browser.
#
# Namespace 'creda-uat' is persistent — separate from 'creda-ui' (mock-mode UAT) and
# 'creda-ui-smoke' (ephemeral test runs). All three can coexist.
#
# Run `make ui-forward` (which forwards http://localhost:5173 → the clients Service in
# whichever namespace has the install) after this returns. Since 'creda-ui' and 'creda-uat'
# can both be up, pick the one you want by setting UAT=1 on the forward script if needed —
# see testbed/scripts/ui-forward.sh.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/ui-up-real"
mkdir -p "$RUN_DIR"

NS="creda-uat"
PEER_CHART="$REPO_ROOT/deploy/helm/creda"
CLIENTS_CHART="$TESTBED/helm/clients"
CORE_IMAGE="creda-core:testbed"
BRIDGE_IMAGE="creda-bridge:testbed"
CLIENTS_IMAGE="creda-clients:testbed"
DRIVER_IMAGE="peer-driver:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

if ! $kc version --request-timeout=2s >/dev/null 2>&1; then
  echo "ERROR: kind cluster '$CLUSTER' is not reachable; run 'make up' first" >&2
  exit 2
fi

# Always rebuild + reload the images. Docker's layer cache makes this a no-op (~30s) when
# nothing changed and a bridge-only rebuild (~90s) when only bridge sources changed.
# This is the cheapest way to guarantee that an iteration-loop change ("edit source → rerun
# ui-up-real") actually picks up the new bytes — without this, a static `:testbed` image tag
# combined with `pullPolicy: Never` means the kubelet happily keeps running the pre-edit
# JAR no matter how many times helm rolls the pod. If you really want to skip the rebuild
# (e.g. iterating on chart values only), set CREDA_SKIP_IMAGES=1.
if [[ "${CREDA_SKIP_IMAGES:-0}" != "1" ]]; then
  echo "==> ensuring images are up to date (set CREDA_SKIP_IMAGES=1 to skip)"
  bash "$TESTBED/images/build-and-load.sh" "$CLUSTER"

  # Each :testbed rebuild orphans the previous image layer; over a triage loop those dangling
  # layers (and stale build cache) accrete into the "hundreds of images" problem. Reclaim them
  # now — AFTER the rebuild, so the just-orphaned previous layers are collected. This prunes ONLY
  # dangling (untagged) images and build cache: tagged creda-*:testbed and creda-dev:local are
  # untouched (next build still hits cache), and the kind cluster + its loaded images are NOT
  # affected — this never tears the cluster down. Non-fatal: a prune hiccup must not block the UAT.
  ENGINE="${ENGINE:-docker}"
  echo "==> reclaiming dangling images + build cache ($ENGINE); cluster is left running"
  "$ENGINE" image prune -f || true
  "$ENGINE" builder prune -f || true
fi

for img in "$CORE_IMAGE" "$BRIDGE_IMAGE" "$CLIENTS_IMAGE" "$DRIVER_IMAGE"; do
  if ! docker image inspect "$img" >/dev/null 2>&1; then
    echo "ERROR: image $img not present locally after build; check the build output above" >&2
    exit 2
  fi
done

# ---- fail-time diagnostics (mirrors scenarios/*-/run.sh) -----------------------------------
dump_diagnostics() {
  echo "------ helm releases in $NS ------" >&2
  $hm -n "$NS" list 2>/dev/null || true
  echo "------ all resources in $NS ------" >&2
  $kc -n "$NS" get all 2>/dev/null || true
  echo "------ recent events in $NS ------" >&2
  $kc -n "$NS" get events --sort-by='.lastTimestamp' 2>/dev/null | tail -30 || true
  for POD in $($kc -n "$NS" get pods -o name 2>/dev/null); do
    echo "------ describe $NS/$POD ------" >&2
    $kc -n "$NS" describe "$POD" 2>/dev/null | tail -40 || true
    # Dump logs for every container in the pod, current AND previous incarnation.
    # The container-name list comes from the pod spec — better than guessing.
    for CON in $($kc -n "$NS" get "$POD" -o jsonpath='{.spec.containers[*].name}' 2>/dev/null); do
      echo "------ logs $NS/$POD container=$CON (current) ------" >&2
      $kc -n "$NS" logs "$POD" -c "$CON" --tail=80 2>/dev/null || true
      echo "------ logs $NS/$POD container=$CON (previous, if any) ------" >&2
      $kc -n "$NS" logs "$POD" -c "$CON" --previous --tail=80 2>/dev/null || true
    done
  done
}
trap 'rc=$?; if [[ $rc -ne 0 ]]; then echo "==> ui-up-real failed (rc=$rc); dumping diagnostics" >&2; dump_diagnostics; fi' EXIT

# ---- idempotent namespace + PSS label -----------------------------------------------------
$kc create namespace "$NS" --dry-run=client -o yaml | $kc apply -f - >/dev/null
$kc label namespace "$NS" pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null

# ---- signing key + participant registry ---------------------------------------------------
# Same shape as scenarios/gossip-convergence/run.sh — Ed25519 32-byte secret in a k8s Secret,
# the participant registry naming the peer's own public key so it trusts its own gossip.
echo "==> generating signing key + participant registry"
if [[ ! -f "$RUN_DIR/peer.key" ]]; then
  head -c 32 /dev/urandom >"$RUN_DIR/peer.key"
fi
# Use the peer-driver image to derive the public key — no host Rust toolchain needed.
PUB="$(docker run --rm -v "$RUN_DIR":/keys:ro "$DRIVER_IMAGE" derive-pubkey --secret-file /keys/peer.key)"
mkdir -p "$RUN_DIR/participants"
echo "$PUB" >"$RUN_DIR/participants/peer.key"

# Idempotent Secret + ConfigMap.
$kc -n "$NS" create secret generic creda-signing-key \
  --from-file=signing.key="$RUN_DIR/peer.key" \
  --dry-run=client -o yaml | $kc apply -f - >/dev/null
$kc -n "$NS" create configmap creda-participants \
  --from-file="$RUN_DIR/participants/peer.key" \
  --dry-run=client -o yaml | $kc apply -f - >/dev/null

# If a previous run failed in the middle of an install/upgrade, helm leaves the release in a
# `pending-install` / `pending-upgrade` state and refuses further operations with "another
# operation (install/upgrade/rollback) is in progress". This is recoverable without
# destroying state: roll back to the last good revision (or uninstall if no good revision
# exists). Run this BEFORE `helm upgrade --install` so re-runs are idempotent.
release_unstick() {
  local release="$1"
  local status
  status=$($hm status -n "$NS" "$release" -o json 2>/dev/null | grep -o '"status":"[^"]*"' | head -1 | cut -d'"' -f4 || true)
  case "$status" in
    pending-install|pending-upgrade|pending-rollback)
      echo "==> previous '$release' release is wedged in '$status'; rolling back" >&2
      if $hm rollback -n "$NS" "$release" 2>/dev/null; then
        return 0
      fi
      echo "==> rollback failed; uninstalling the stuck release" >&2
      $hm uninstall -n "$NS" "$release" --no-hooks 2>/dev/null || true
      ;;
  esac
}
release_unstick peer
release_unstick clients

# ---- install / upgrade the peer (Core + Bridge) -------------------------------------------
echo "==> installing peer (Core + Bridge) into namespace $NS"
$hm upgrade --install -n "$NS" peer "$PEER_CHART" \
  -f "$TESTBED/helm/values-uat-peer.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --wait --timeout 240s >/dev/null

# Force a roll *every* run. Helm only triggers a pod restart when the rendered template
# changes; here the image tag stays at `:testbed` between code iterations and the rendered
# YAML is byte-identical, so helm says "no diff" and leaves the running pod alone — even
# though `make images` just refreshed the bytes on each kind node. `kubectl rollout restart`
# recreates the pod so the kubelet picks up the new image archive. Without this, a code
# change to Core or Bridge ships into kind but never reaches a running container until the
# pod restarts for some other reason.
echo "==> forcing peer rollout to pick up the latest creda-core / creda-bridge images"
$kc -n "$NS" rollout restart statefulset/peer >/dev/null
$kc -n "$NS" rollout status statefulset/peer --timeout=240s

KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS" peer 180

# If a previous iteration left a CrashLoopBackOff pod behind, `helm upgrade --wait` will
# block on that stuck pod and never reach our rollout-restart below. Worse, with
# pullPolicy: Never and an unchanged image tag, kubelet doesn't restart the container even
# when fresh bytes are on the node — it just keeps the failing container in BackOff. So
# force-delete any existing clients pods first; the ReplicaSet (if it exists) will create a
# new one with the freshly-loaded image bytes. --ignore-not-found makes this a no-op on the
# first install.
echo "==> clearing any stuck clients pods so fresh bytes can take effect"
$kc -n "$NS" delete pod -l app.kubernetes.io/name=creda-clients \
  --force --grace-period=0 --ignore-not-found 2>/dev/null || true

# Helpful diagnostic: which digest does each kind node actually have for creda-clients? If
# this doesn't match the local digest reported by `make images`, kind load didn't refresh
# the bytes and the next iteration will see the same stale-image issue.
#
# The trailing `|| true` is load-bearing: awk's early `exit` closes the pipe while `ctr images
# ls` is still writing, so ctr can die of SIGPIPE (rc 141). Under `set -euo pipefail` that
# kills the whole run — and it's timing-dependent (only when ctr's remaining output overflows
# the pipe buffer), so it bites intermittently. This is a diagnostic; it must never be fatal.
echo "==> creda-clients image digest on each kind node:"
for node in $(docker ps --filter "name=^${CLUSTER}-" --format '{{.Names}}' 2>/dev/null); do
  digest=$(docker exec "$node" ctr --namespace=k8s.io images ls 2>/dev/null \
    | awk '/creda-clients:testbed/ {print $3; exit}' || true)
  echo "    $node : ${digest:-<not present>}"
done

# ---- install / upgrade the clients chart, pointed at the bridge ---------------------------
echo "==> installing clients (real mode → http://peer-fhir:8080) into namespace $NS"
$hm upgrade --install -n "$NS" clients "$CLIENTS_CHART" \
  --set image.repository=creda-clients \
  --set image.tag=testbed \
  --set image.pullPolicy=Never \
  --set fhirBase=/fhir \
  --set bridgeUpstream=http://peer-fhir:8080 \
  --wait --timeout 120s >/dev/null

# Same reason as the peer rollout-restart above — force the clients deployment to pick up the
# freshly-loaded image bytes even when the rendered chart is unchanged.
echo "==> forcing clients rollout to pick up the latest creda-clients image"
$kc -n "$NS" rollout restart deploy/creda-clients >/dev/null
$kc -n "$NS" rollout status deploy/creda-clients --timeout=120s

cat <<EOF

==> Real-mode UAT is up in namespace '$NS'.

   Single peer (Core + Bridge) + persona clients pointed at the bridge through nginx.
   Clicking 'Attest' in the clinician UI now produces a *real* signed event in Core
   and publishes it to the gossip mesh. The audit reviewer's ledger is still fixture-
   backed in this build (it doesn't yet read from the bridge), but you can confirm the
   event by tailing Core's log:

       kubectl --context=${CTX} -n $NS logs peer-0 -c creda-core -f

   Open the UI by running this in a second terminal:

       cd testbed && UAT=1 make ui-forward

   That forwards http://localhost:5173 → the clients Service in '$NS'. Ctrl-C kills
   the forwarder only; everything keeps running. Tear it down with 'make ui-down-real'.

EOF
