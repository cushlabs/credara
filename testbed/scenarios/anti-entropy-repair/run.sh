#!/usr/bin/env bash
# Anti-entropy repair scenario.
#
# Stand up peer-a alone, inject N events, then stand up peer-b. Because the events were
# published before peer-b subscribed to any topic bucket, they were NOT gossiped to peer-b — the
# only way they can reach peer-b is via the periodic anti-entropy round (§6.1.8).
#
# Assertion: every injected event arrives at peer-b within AE_BUDGET_MS.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/anti-entropy-repair"
mkdir -p "$RUN_DIR"

NS_A="creda-peer-a"
NS_B="creda-peer-b"
N_EVENTS=3
# 30s AE interval + 30s skip-first-tick + headroom for kubectl Job scheduling.
AE_BUDGET_MS=75000

CHART="$REPO_ROOT/deploy/helm/creda"
DRIVER_IMAGE="peer-driver:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

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
  $kc delete namespace "$NS_A" "$NS_B" --ignore-not-found 2>/dev/null || true
  # Block until the namespaces are FULLY gone, not just Terminating — otherwise the next scenario
  # (these share namespace names) hits "object is being deleted: namespaces already exists".
  $kc wait --for=delete "namespace/$NS_A" "namespace/$NS_B" --timeout=120s 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT

# ---- keygen (host-side, Docker-only) ---------------------------------------------------------
echo "==> generating Ed25519 keypairs"
head -c 32 /dev/urandom >"$RUN_DIR/peer-a.key"
head -c 32 /dev/urandom >"$RUN_DIR/peer-b.key"

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

# ---- install peer-a (alone, no peer-b yet) ---------------------------------------------------
echo "==> installing peer-a"
$hm install -n "$NS_A" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-a.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --wait --timeout 180s >/dev/null

KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS_A" peer 180

# ---- inject N events at peer-a -------------------------------------------------------------
# Each inject runs as its own kubectl Job (unique name per event); we capture event ids from the
# Job's stdout via `kubectl logs`. These events never gossip to peer-b — peer-b doesn't exist.
EVENT_IDS=()
for i in $(seq 1 "$N_EVENTS"); do
  JOB="peer-driver-inject-$i"
  TAG="ae-$$-$i"
  echo "==> injecting event $i/$N_EVENTS at peer-a"
  cat <<EOF | $kc -n "$NS_A" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $JOB
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 900
  template:
    spec:
      restartPolicy: Never
      # The testbed namespaces are labeled with the restricted Pod Security Standard (DQ-1
      # parity with production). Every pod, including the peer-driver Job, must conform.
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: driver
          image: $DRIVER_IMAGE
          imagePullPolicy: Never
          securityContext:
            allowPrivilegeEscalation: false
            runAsNonRoot: true
            capabilities:
              drop: ["ALL"]
          args:
            - "--peer"
            - "http://$PEER_DNS"
            - "inject"
            - "--tag"
            - "$TAG"
EOF
  $kc -n "$NS_A" wait --for=condition=complete --timeout=60s "job/$JOB" >/dev/null
  EVENT_ID="$($kc -n "$NS_A" logs "job/$JOB" --tail=1 | tr -d '[:space:]')"
  echo "    event-id = $EVENT_ID"
  EVENT_IDS+=("$EVENT_ID")
done

# ---- sanity: verify peer-a holds all events (a quick observe Job against itself) -----------
echo "==> sanity: confirming peer-a holds all $N_EVENTS events"
for i in "${!EVENT_IDS[@]}"; do
  JOB="peer-driver-sanity-$((i + 1))"
  ID="${EVENT_IDS[$i]}"
  cat <<EOF | $kc -n "$NS_A" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $JOB
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 900
  template:
    spec:
      restartPolicy: Never
      # The testbed namespaces are labeled with the restricted Pod Security Standard (DQ-1
      # parity with production). Every pod, including the peer-driver Job, must conform.
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: driver
          image: $DRIVER_IMAGE
          imagePullPolicy: Never
          securityContext:
            allowPrivilegeEscalation: false
            runAsNonRoot: true
            capabilities:
              drop: ["ALL"]
          args:
            - "--peer"
            - "http://$PEER_DNS"
            - "observe"
            - "--event-id"
            - "$ID"
            - "--timeout-ms"
            - "5000"
EOF
  $kc -n "$NS_A" wait --for=condition=complete --timeout=20s "job/$JOB" >/dev/null
done
echo "    peer-a has all $N_EVENTS events"

# ---- now (and only now) install peer-b with peer-a as bootstrap ----------------------------
PEER_A_MULTIADDR="$(bash "$TESTBED/scripts/peer-multiaddr.sh" "$NS_A" peer-0)"
echo "==> peer-a multiaddr: $PEER_A_MULTIADDR"

echo "==> installing peer-b (bootstrap → peer-a; events injected pre-join are not gossiped)"
$hm install -n "$NS_B" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-b.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --set-string "config.bootstrapPeers[0]=$PEER_A_MULTIADDR" \
  --wait --timeout 180s >/dev/null

KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS_B" peer 180

# ---- observe each event at peer-b — only AE can deliver these now --------------------------
echo "==> waiting for anti-entropy to heal peer-b (budget ${AE_BUDGET_MS} ms per event)"
PASS=true
LATENCIES=()
for i in "${!EVENT_IDS[@]}"; do
  JOB="peer-driver-observe-$((i + 1))"
  ID="${EVENT_IDS[$i]}"
  cat <<EOF | $kc -n "$NS_B" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $JOB
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 900
  template:
    spec:
      restartPolicy: Never
      # The testbed namespaces are labeled with the restricted Pod Security Standard (DQ-1
      # parity with production). Every pod, including the peer-driver Job, must conform.
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: driver
          image: $DRIVER_IMAGE
          imagePullPolicy: Never
          securityContext:
            allowPrivilegeEscalation: false
            runAsNonRoot: true
            capabilities:
              drop: ["ALL"]
          args:
            - "--peer"
            - "http://$PEER_DNS"
            - "observe"
            - "--event-id"
            - "$ID"
            - "--timeout-ms"
            - "$AE_BUDGET_MS"
EOF
  # Allow extra wall-clock margin on top of the in-binary timeout for Job scheduling overhead.
  if ! $kc -n "$NS_B" wait --for=condition=complete --timeout=$((AE_BUDGET_MS / 1000 + 30))s "job/$JOB" >/dev/null; then
    echo "FAIL: event $ID did not arrive at peer-b within ${AE_BUDGET_MS}ms" >&2
    $kc -n "$NS_B" logs "job/$JOB" >&2 || true
    PASS=false
    continue
  fi
  LATENCY_MS="$($kc -n "$NS_B" logs "job/$JOB" --tail=1 | tr -d '[:space:]')"
  echo "    event $((i + 1))/$N_EVENTS: AE catch-up in ${LATENCY_MS} ms"
  LATENCIES+=("$LATENCY_MS")
done

if ! $PASS; then
  echo "FAIL: anti-entropy did not heal all events within budget"
  exit 1
fi

# Report dominant latency (the first event triggers the AE round; subsequent ones are quick).
MAX=0
for l in "${LATENCIES[@]}"; do
  if (( l > MAX )); then
    MAX=$l
  fi
done
echo "PASS: anti-entropy repair scenario ($N_EVENTS events; max catch-up ${MAX} ms, budget ${AE_BUDGET_MS} ms)"
