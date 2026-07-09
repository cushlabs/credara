#!/usr/bin/env bash
# Storage-class scenario (§10.6.8 — persistent storage survives a peer restart).
#
# A Credara peer's identity DAG lives in RocksDB on a PersistentVolume. §10.6.8 requires that the
# store survive a peer restart on any supported storage class: the PVC re-attaches to the rolled pod
# and RocksDB reopens the store (WAL replay), with no committed events lost.
#
# This runs a SINGLE seed peer on the storage class under test, writes marker events, restarts the
# pod (delete → the StatefulSet recreates peer-0, re-binding the SAME PVC), and asserts every marker
# is still present afterward.
#
# SCOPE — read this before reading the result. kind ships one provisioner (rancher local-path), so
# the default run tests the cluster-default class. A pod delete does NOT wipe the PV, so this
# validates PV persistence + RocksDB reopen on the class; it does NOT reproduce a power-loss fsync
# failure (the §10.6.8 durability concern) — that needs real hardware and diskchecker.pl, not kind.
# On a cluster that provisions the real matrix (gp3, Longhorn, OpenEBS LocalPV), set STORAGE_CLASS
# to run the identical assertions against it.
#
# Assertions:
#   1. The restart really replaced the pod — peer-0's UID changes.
#   2. Every marker written before the restart is present after — the store persisted on this class.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/storage-class"
mkdir -p "$RUN_DIR"

NS="creda-storage"

# The storage class to test. Empty = the cluster's default StorageClass (kind: rancher local-path).
# Override for a real matrix class, e.g. STORAGE_CLASS=longhorn make storage-class.
STORAGE_CLASS="${STORAGE_CLASS:-}"
CLASS_LABEL="${STORAGE_CLASS:-<cluster-default>}"

# Correctness, not latency: markers must be present after the restart, within a generous read budget.
PRESENT_BUDGET_MS=10000
RESTART_TIMEOUT_S=180

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
  echo "------ $NS pods ------" >&2
  $kc -n "$NS" get pods 2>/dev/null || true
  echo "------ $NS pvc ------" >&2
  $kc -n "$NS" get pvc 2>/dev/null || true
  for POD in $($kc -n "$NS" get pods -o name 2>/dev/null); do
    echo "------ describe $NS/$POD ------" >&2
    $kc -n "$NS" describe "$POD" 2>/dev/null | tail -40 || true
    echo "------ logs $NS/$POD creda-core ------" >&2
    $kc -n "$NS" logs "$POD" -c creda-core --tail=80 2>/dev/null || true
  done
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
  $hm uninstall -n "$NS" peer 2>/dev/null || true
  # StatefulSet PVCs are retained on uninstall; deleting the namespace cascades them away so a
  # re-run starts from a fresh volume rather than re-attaching stale RocksDB data.
  $kc delete namespace "$NS" --ignore-not-found 2>/dev/null || true
  $kc wait --for=delete "namespace/$NS" --timeout=120s 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT

# driver_job JOB WAIT_S <driver args...> — run the peer-driver as an in-cluster Job (restricted Pod
# Security Standard), wait for completion, echo the Job's last stdout line. Non-zero on timeout.
driver_job() {
  local job="$1" wait_s="$2"; shift 2
  local items=""
  local a
  for a in "$@"; do
    items+=$'\n            - "'"$a"'"'
  done
  cat <<EOF | $kc -n "$NS" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $job
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 900
  template:
    spec:
      restartPolicy: Never
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
          args:$items
EOF
  if ! $kc -n "$NS" wait --for=condition=complete --timeout="${wait_s}s" "job/$job" >/dev/null; then
    echo "ERROR: job $job did not complete within ${wait_s}s" >&2
    $kc -n "$NS" logs "job/$job" >&2 2>/dev/null || true
    return 1
  fi
  $kc -n "$NS" logs "job/$job" --tail=1 | tr -d '[:space:]'
}

echo "==> storage class under test: $CLASS_LABEL"

# ---- keygen (host-side, Docker-only) ---------------------------------------------------------
echo "==> generating Ed25519 keypair"
head -c 32 /dev/urandom >"$RUN_DIR/peer.key"
PUB="$(docker run --rm -v "$RUN_DIR":/keys:ro "$DRIVER_IMAGE" derive-pubkey --secret-file /keys/peer.key)"
mkdir -p "$RUN_DIR/participants"
echo "$PUB" >"$RUN_DIR/participants/peer.key"

# ---- namespace, Secret, ConfigMap ------------------------------------------------------------
echo "==> creating namespace + secret"
$kc create namespace "$NS" >/dev/null
$kc label namespace "$NS" pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null
$kc -n "$NS" create secret generic creda-signing-key \
  --from-file=signing.key="$RUN_DIR/peer.key" >/dev/null
$kc -n "$NS" create configmap creda-participants \
  --from-file="$RUN_DIR/participants/peer.key" >/dev/null

# ---- install a single seed peer on the class under test --------------------------------------
echo "==> installing a single seed peer (persistence on $CLASS_LABEL)"
SC_ARG=()
if [[ -n "$STORAGE_CLASS" ]]; then
  SC_ARG=(--set-string "persistence.storageClass=$STORAGE_CLASS")
fi
# Note the `${SC_ARG[@]+"${SC_ARG[@]}"}` guard, not a bare "${SC_ARG[@]}": macOS ships bash 3.2,
# where expanding an EMPTY array under `set -u` errors "unbound variable". The guard expands to the
# elements when set and to nothing when empty — safe on bash 3.2 and 5.x alike.
$hm install -n "$NS" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-a.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  ${SC_ARG[@]+"${SC_ARG[@]}"} \
  --wait --timeout 180s >/dev/null
KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS" peer 180

# Sanity: the PVC actually bound on this class (an unbindable class fails here, loudly).
PVC_STATUS="$($kc -n "$NS" get pvc "data-peer-0" -o jsonpath='{.status.phase}' 2>/dev/null || true)"
PVC_CLASS="$($kc -n "$NS" get pvc "data-peer-0" -o jsonpath='{.spec.storageClassName}' 2>/dev/null || true)"
echo "==> PVC data-peer-0: phase=${PVC_STATUS:-<none>} class=${PVC_CLASS:-<default>}"
if [[ "$PVC_STATUS" != "Bound" ]]; then
  echo "FAIL: PVC data-peer-0 is not Bound (phase=${PVC_STATUS:-<none>}) on class $CLASS_LABEL" >&2
  exit 1
fi

# ---- write marker events, confirm they are visible pre-restart -------------------------------
TAG="stor-$$"
echo "==> injecting marker events"
M1="$(driver_job peer-driver-m1 60 --peer "http://$PEER_DNS" inject --tag "$TAG-1")"
M2="$(driver_job peer-driver-m2 60 --peer "http://$PEER_DNS" inject --tag "$TAG-2")"
echo "    markers = $M1 $M2"
driver_job peer-driver-preread1 30 --peer "http://$PEER_DNS" observe --event-id "$M1" --timeout-ms 5000 >/dev/null
driver_job peer-driver-preread2 30 --peer "http://$PEER_DNS" observe --event-id "$M2" --timeout-ms 5000 >/dev/null
echo "    both markers committed and visible before the restart"

# ---- restart the peer: delete the pod; the StatefulSet recreates it on the SAME PVC ----------
# Default grace period, so the daemon gets SIGTERM and closes RocksDB cleanly (the store is also
# WAL-durable, so a reopen would replay regardless). --wait blocks until the OLD pod is fully gone.
UID_BEFORE="$($kc -n "$NS" get pod peer-0 -o jsonpath='{.metadata.uid}')"
echo "==> restarting the peer (delete pod peer-0; the StatefulSet recreates it, re-binding data-peer-0)"
$kc -n "$NS" delete pod peer-0 --wait=true >/dev/null

# Poll for the RECREATED pod (a different UID) to reach Ready. This is race-free where a bare
# `rollout status` is not: with no revision change it can return on stale status before the new pod
# is Ready, and a UID read can catch the gap where peer-0 does not yet exist. The loop's exit
# condition (new UID + Ready=True) IS assertion 1 — a genuine restart that came back healthy.
echo "==> waiting for the recreated peer-0 to reach Ready (${RESTART_TIMEOUT_S}s budget)"
DEADLINE=$((SECONDS + RESTART_TIMEOUT_S))
UID_AFTER=""
while true; do
  UID_AFTER="$($kc -n "$NS" get pod peer-0 -o jsonpath='{.metadata.uid}' 2>/dev/null || true)"
  READY="$($kc -n "$NS" get pod peer-0 -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}' 2>/dev/null || true)"
  if [[ -n "$UID_AFTER" && "$UID_AFTER" != "$UID_BEFORE" && "$READY" == "True" ]]; then
    break
  fi
  if (( SECONDS > DEADLINE )); then
    echo "FAIL: peer-0 did not come back Ready within ${RESTART_TIMEOUT_S}s (uid=${UID_AFTER:-<none>} ready=${READY:-<none>})" >&2
    exit 1
  fi
  sleep 3
done
echo "==> peer-0 restarted: UID before=$UID_BEFORE after=$UID_AFTER (Ready)"

# ---- assertion 2: every marker survived the restart on this storage class --------------------
echo "==> asserting both markers survived the restart (RocksDB reopened from the re-attached PVC)"
driver_job peer-driver-postread1 $((PRESENT_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$M1" --timeout-ms "$PRESENT_BUDGET_MS" >/dev/null
driver_job peer-driver-postread2 $((PRESENT_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$M2" --timeout-ms "$PRESENT_BUDGET_MS" >/dev/null

echo "PASS: storage-class (peer restarted on $CLASS_LABEL; PVC re-attached; RocksDB reopened with both markers intact — §10.6.8)"
