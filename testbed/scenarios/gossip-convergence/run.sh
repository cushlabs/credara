#!/usr/bin/env bash
# Gossip-convergence smoke test (Docker-only host prerequisites).
#
# Brings up two peers in their own namespaces, wires peer-b to use peer-a as bootstrap, then
# injects an event at peer-a and waits for it to appear at peer-b. The peer-driver runs as a
# Kubernetes Job inside the cluster — no port-forward, no host Rust toolchain, no Mac-vs-Linux
# network-mode branching. Same execution model as a production operator would use.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/gossip-convergence"
mkdir -p "$RUN_DIR"

NS_A="creda-peer-a"
NS_B="creda-peer-b"
SMOKE_BUDGET_MS=5000

CHART="$REPO_ROOT/deploy/helm/creda"
DRIVER_IMAGE="peer-driver:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

# Peer DNS inside its namespace (StatefulSet pod 0, headless service named `peer-headless`).
PEER_DNS="peer-0.peer-headless:50051"

if ! docker image inspect "$DRIVER_IMAGE" >/dev/null 2>&1; then
  echo "ERROR: image $DRIVER_IMAGE not present locally; run 'make up' (or 'make images')" >&2
  exit 2
fi

dump_diagnostics() {
  for NS in "$NS_A" "$NS_B"; do
    echo "------ $NS pods ------" >&2
    $kc -n "$NS" get pods 2>/dev/null || true
    for POD in $($kc -n "$NS" get pods -o name 2>/dev/null); do
      echo "------ describe $NS/$POD ------" >&2
      $kc -n "$NS" describe "$POD" 2>/dev/null | tail -40 || true
      echo "------ logs $NS/$POD creda-core ------" >&2
      $kc -n "$NS" logs "$POD" -c creda-core --tail=80 2>/dev/null || true
      echo "------ logs $NS/$POD hapi-bridge ------" >&2
      $kc -n "$NS" logs "$POD" -c hapi-bridge --tail=40 2>/dev/null || true
    done
  done
}

cleanup() {
  local rc=$?
  if [[ $rc -ne 0 ]]; then
    echo "==> failure detected (rc=$rc); dumping diagnostics" >&2
    dump_diagnostics
  fi
  if [[ "${KEEP_NAMESPACES:-0}" = "1" ]]; then
    echo "==> KEEP_NAMESPACES=1; leaving $NS_A and $NS_B in place for manual inspection"
    exit "$rc"
  fi
  echo "==> cleanup"
  $hm uninstall -n "$NS_A" peer 2>/dev/null || true
  $hm uninstall -n "$NS_B" peer 2>/dev/null || true
  $kc delete namespace "$NS_A" "$NS_B" --wait=false --ignore-not-found 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT

# ---- keygen (host-side, Docker-only) ---------------------------------------------------------
echo "==> generating Ed25519 keypairs"
head -c 32 /dev/urandom >"$RUN_DIR/peer-a.key"
head -c 32 /dev/urandom >"$RUN_DIR/peer-b.key"

# Derive public keys via the peer-driver image — no host Rust toolchain needed.
derive_pubkey() {
  local label="$1"
  docker run --rm \
    -v "$RUN_DIR":/keys:ro \
    "$DRIVER_IMAGE" derive-pubkey --secret-file "/keys/${label}.key"
}
PUB_A="$(derive_pubkey peer-a)"
PUB_B="$(derive_pubkey peer-b)"
mkdir -p "$RUN_DIR/participants"
echo "$PUB_A" >"$RUN_DIR/participants/peer-a.key"
echo "$PUB_B" >"$RUN_DIR/participants/peer-b.key"

# ---- namespaces, Secrets, ConfigMaps ---------------------------------------------------------
echo "==> creating namespaces + secrets"
for NS in "$NS_A" "$NS_B"; do
  $kc create namespace "$NS" >/dev/null
  $kc label namespace "$NS" pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null
done
$kc -n "$NS_A" create secret generic creda-signing-key \
  --from-file=signing.key="$RUN_DIR/peer-a.key" >/dev/null
$kc -n "$NS_B" create secret generic creda-signing-key \
  --from-file=signing.key="$RUN_DIR/peer-b.key" >/dev/null
for NS in "$NS_A" "$NS_B"; do
  $kc -n "$NS" create configmap creda-participants \
    --from-file="$RUN_DIR/participants/peer-a.key" \
    --from-file="$RUN_DIR/participants/peer-b.key" >/dev/null
done

# ---- install peer-a (seed) -------------------------------------------------------------------
echo "==> installing peer-a (seed)"
$hm install -n "$NS_A" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-a.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --wait --timeout 180s >/dev/null

"$TESTBED/scripts/wait-ready.sh" "$NS_A" peer 180
PEER_A_MULTIADDR="$("$TESTBED/scripts/peer-multiaddr.sh" "$NS_A" peer-0)"
echo "==> peer-a multiaddr: $PEER_A_MULTIADDR"

# ---- install peer-b (bootstrap → peer-a) -----------------------------------------------------
echo "==> installing peer-b (bootstrap → peer-a)"
$hm install -n "$NS_B" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-b.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --set-string "config.bootstrapPeers[0]=$PEER_A_MULTIADDR" \
  --wait --timeout 180s >/dev/null

"$TESTBED/scripts/wait-ready.sh" "$NS_B" peer 180

echo "==> giving the mesh a moment to form (3s)"
sleep 3

# ---- inject at peer-a via Job ----------------------------------------------------------------
echo "==> injecting event at peer-a"
TAG="smoke-$$"
INJECT_JOB="peer-driver-inject"
cat <<EOF | $kc -n "$NS_A" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $INJECT_JOB
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 600
  template:
    spec:
      restartPolicy: Never
      containers:
        - name: driver
          image: $DRIVER_IMAGE
          imagePullPolicy: Never
          args:
            - "--peer"
            - "http://$PEER_DNS"
            - "inject"
            - "--tag"
            - "$TAG"
EOF

$kc -n "$NS_A" wait --for=condition=complete --timeout=60s "job/$INJECT_JOB" >/dev/null
EVENT_ID="$($kc -n "$NS_A" logs "job/$INJECT_JOB" --tail=1 | tr -d '[:space:]')"
echo "    event-id = $EVENT_ID"

# ---- observe at peer-b via Job ---------------------------------------------------------------
echo "==> observing event at peer-b (budget ${SMOKE_BUDGET_MS} ms)"
OBSERVE_JOB="peer-driver-observe"
cat <<EOF | $kc -n "$NS_B" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $OBSERVE_JOB
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 600
  template:
    spec:
      restartPolicy: Never
      containers:
        - name: driver
          image: $DRIVER_IMAGE
          imagePullPolicy: Never
          args:
            - "--peer"
            - "http://$PEER_DNS"
            - "observe"
            - "--event-id"
            - "$EVENT_ID"
            - "--timeout-ms"
            - "$SMOKE_BUDGET_MS"
EOF

# Wait slightly longer than the smoke budget — the Job has Kubernetes startup overhead on top of
# the in-binary timeout.
$kc -n "$NS_B" wait --for=condition=complete --timeout=$((SMOKE_BUDGET_MS / 1000 + 30))s "job/$OBSERVE_JOB" >/dev/null || {
  echo "FAIL: observe Job did not complete" >&2
  $kc -n "$NS_B" logs "job/$OBSERVE_JOB" >&2 || true
  exit 1
}

LATENCY_MS="$($kc -n "$NS_B" logs "job/$OBSERVE_JOB" --tail=1 | tr -d '[:space:]')"
echo "    converged in ${LATENCY_MS} ms"

if (( LATENCY_MS > SMOKE_BUDGET_MS )); then
  echo "FAIL: latency ${LATENCY_MS}ms exceeded budget ${SMOKE_BUDGET_MS}ms"
  exit 1
fi

echo "PASS: gossip convergence smoke test (${LATENCY_MS}ms)"
