#!/usr/bin/env bash
# Rolling-upgrade scenario (§10.6.7 — within-institution maintenance path).
#
# Credara is designed to be upgraded without coordinated network downtime. This exercises the
# within-institution mechanic on the real StatefulSet + Helm path:
#
#   peer-a is the REST OF THE NETWORK — it stays up and keeps serving throughout.
#   peer-b is the institution being upgraded — a `helm upgrade` changes its config, which changes
#          the pod template's `checksum/config` annotation, which drives the StatefulSet's default
#          RollingUpdate to roll the pod (OrderedReady, /readyz-gated, §10.6.7).
#
# Because the testbed pins a fixed image tag, we can't trigger the roll with a new image; instead we
# change a benign config value (snapshotIntervalSecs), which changes the config checksum and drives
# the IDENTICAL RollingUpdate a real image/chart bump would. The mechanic under test is the same.
#
# Assertions:
#   1. The upgrade actually rolled — the StatefulSet advances to a new updateRevision AND peer-b-0's
#      pod UID changes (a genuine pod replacement, not a no-op re-apply).
#   2. Data survives the rotation — an event written to peer-b BEFORE the roll is still present after
#      (the StatefulSet's PVC re-attaches to the rolled pod, §10.6.3).
#   3. The rest of the network kept serving — peer-a accepted a write DURING peer-b's roll window.
#   4. The rolled peer rejoined and reconverged — that write reaches peer-b within budget once it is
#      Ready again (bootstrap rejoin §11.1.2 + gossip/anti-entropy §6.1.8), so no event is lost
#      across the upgrade.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/rolling-upgrade"
mkdir -p "$RUN_DIR"

NS_A="creda-peer-a"
NS_B="creda-peer-b"

# The rolled peer must re-bootstrap and catch up; correctness, not latency, is the point here. The
# during-roll write is likely MISSED by live gossip while peer-b is down, so it catches up via the
# periodic anti-entropy round (§6.1.8) after rejoin — the budget accommodates a full AE round, as in
# partition-rejoin, rather than assuming instant live delivery.
BASELINE_BUDGET_MS=15000
RECONVERGE_BUDGET_MS=120000
ROLLOUT_TIMEOUT_S=180

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
  $kc wait --for=delete "namespace/$NS_A" "namespace/$NS_B" --timeout=120s 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT

# driver_job NS JOB WAIT_S <driver args...> — run the peer-driver as an in-cluster Job (restricted
# Pod Security Standard), wait for completion, echo the Job's last stdout line (an event id or a
# latency in ms). Non-zero on timeout, which aborts the scenario under `set -e`.
driver_job() {
  local ns="$1" job="$2" wait_s="$3"; shift 3
  local items=""
  local a
  for a in "$@"; do
    items+=$'\n            - "'"$a"'"'
  done
  cat <<EOF | $kc -n "$ns" apply -f - >/dev/null
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
  if ! $kc -n "$ns" wait --for=condition=complete --timeout="${wait_s}s" "job/$job" >/dev/null; then
    echo "ERROR: job $job in $ns did not complete within ${wait_s}s" >&2
    $kc -n "$ns" logs "job/$job" >&2 2>/dev/null || true
    return 1
  fi
  $kc -n "$ns" logs "job/$job" --tail=1 | tr -d '[:space:]'
}

# ---- keygen (host-side, Docker-only) ---------------------------------------------------------
echo "==> generating Ed25519 keypairs"
head -c 32 /dev/urandom >"$RUN_DIR/peer-a.key"
head -c 32 /dev/urandom >"$RUN_DIR/peer-b.key"

derive_pubkey() {
  local label="$1"
  docker run --rm -v "$RUN_DIR":/keys:ro "$DRIVER_IMAGE" derive-pubkey --secret-file "/keys/${label}.key"
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

# ---- install peer-a (seed), then peer-b bootstrapped to it (meshed) --------------------------
echo "==> installing peer-a"
$hm install -n "$NS_A" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-a.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --wait --timeout 180s >/dev/null
KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS_A" peer 180

PEER_A_MULTIADDR="$(bash "$TESTBED/scripts/peer-multiaddr.sh" "$NS_A" peer-0)"
echo "==> peer-a multiaddr: $PEER_A_MULTIADDR"

echo "==> installing peer-b (bootstrap → peer-a)"
$hm install -n "$NS_B" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-b.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --set-string "config.bootstrapPeers[0]=$PEER_A_MULTIADDR" \
  --wait --timeout 180s >/dev/null
KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS_B" peer 180

TAG="roll-$$"

# ---- baseline: the mesh is live before we touch anything -------------------------------------
echo "==> baseline: injecting at peer-a, observing at peer-b (mesh must be live pre-upgrade)"
BASE_EVENT="$(driver_job "$NS_A" peer-driver-baseline 60 \
  --peer "http://$PEER_DNS" inject --tag "$TAG-base")"
driver_job "$NS_B" peer-driver-baseline-observe $((BASELINE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$BASE_EVENT" --timeout-ms "$BASELINE_BUDGET_MS" >/dev/null
echo "    baseline gossip works ($BASE_EVENT present at peer-b)"

# ---- pre-roll marker written to peer-b (must survive the pod rotation) ------------------------
echo "==> injecting a pre-roll marker at peer-b (must persist across the rotation)"
E0="$(driver_job "$NS_B" peer-driver-preroll 60 \
  --peer "http://$PEER_DNS" inject --tag "$TAG-preroll")"
echo "    pre-roll marker = $E0"

# ---- capture the pre-upgrade StatefulSet revision + pod identity -----------------------------
REV_BEFORE="$($kc -n "$NS_B" get statefulset peer -o jsonpath='{.status.updateRevision}')"
UID_BEFORE="$($kc -n "$NS_B" get pod peer-0 -o jsonpath='{.metadata.uid}')"
echo "==> peer-b before upgrade: revision=$REV_BEFORE podUID=$UID_BEFORE"

# ---- the rolling upgrade -----------------------------------------------------------------------
# A benign config change (snapshotIntervalSecs 21600 -> 21601) rewrites the config checksum, which
# rolls the pod exactly as an image/chart bump would. Run WITHOUT --wait so we can prove the rest of
# the network keeps serving during the roll window (next step) before we block on completion.
echo "==> helm upgrade peer-b (config change → RollingUpdate; the roll begins)"
$hm upgrade -n "$NS_B" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-b.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --set-string "config.bootstrapPeers[0]=$PEER_A_MULTIADDR" \
  --set config.snapshotIntervalSecs=21601 >/dev/null

# ---- the rest of the network keeps serving DURING the roll -----------------------------------
# peer-a is untouched by peer-b's upgrade; a write to it must succeed even while peer-b is rolling.
# This is the "no coordinated network downtime" guarantee (§10.6.7).
echo "==> injecting at peer-a DURING peer-b's roll (the network must keep serving)"
E1="$(driver_job "$NS_A" peer-driver-during-roll 60 \
  --peer "http://$PEER_DNS" inject --tag "$TAG-during")"
echo "    peer-a accepted a write during the roll: $E1"

# ---- wait for the roll to complete -----------------------------------------------------------
echo "==> waiting for the RollingUpdate to complete (/readyz-gated, ${ROLLOUT_TIMEOUT_S}s budget)"
$kc -n "$NS_B" rollout status statefulset/peer --timeout="${ROLLOUT_TIMEOUT_S}s" >/dev/null
KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS_B" peer "$ROLLOUT_TIMEOUT_S"

# ---- assertion 1: the upgrade actually rolled the pod ----------------------------------------
REV_AFTER="$($kc -n "$NS_B" get statefulset peer -o jsonpath='{.status.updateRevision}')"
UID_AFTER="$($kc -n "$NS_B" get pod peer-0 -o jsonpath='{.metadata.uid}')"
echo "==> peer-b after upgrade:  revision=$REV_AFTER podUID=$UID_AFTER"
if [[ "$REV_AFTER" == "$REV_BEFORE" ]]; then
  echo "FAIL: StatefulSet revision did not change ($REV_BEFORE) — the upgrade did not roll" >&2
  exit 1
fi
if [[ "$UID_AFTER" == "$UID_BEFORE" ]]; then
  echo "FAIL: peer-b-0 pod UID unchanged ($UID_BEFORE) — the pod was not replaced" >&2
  exit 1
fi
echo "    confirmed: new revision + new pod (the RollingUpdate replaced peer-b-0)"

# ---- assertion 2: pre-roll data survived the rotation ----------------------------------------
echo "==> asserting the pre-roll marker survived the rotation (PVC re-attach)"
driver_job "$NS_B" peer-driver-preroll-survives 60 \
  --peer "http://$PEER_DNS" observe --event-id "$E0" --timeout-ms 5000 >/dev/null
echo "    confirmed: $E0 still present at peer-b after the roll"

# ---- assertion 3+4: the rolled peer rejoined and caught up the during-roll write -------------
echo "==> asserting the during-roll write reached the rolled peer-b (rejoin + AE catch-up, budget ${RECONVERGE_BUDGET_MS} ms)"
driver_job "$NS_B" peer-driver-reconverge $((RECONVERGE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$E1" --timeout-ms "$RECONVERGE_BUDGET_MS" >/dev/null
echo "    confirmed: $E1 present at peer-b (no event lost across the upgrade)"

echo "PASS: rolling-upgrade (helm upgrade rolled peer-b to a new revision; pre-roll data persisted; peer-a served throughout; rolled peer rejoined and caught up — §10.6.7)"
