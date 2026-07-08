#!/usr/bin/env bash
# Partition-rejoin scenario (§6.1.7 partition tolerance, §6.1.8 anti-entropy backstop).
#
# Two peers, meshed. Confirm they gossip. Then PARTITION them — drop all traffic between the two
# peer pod IPs at the kind node level (iptables in the node containers), which is CNI-agnostic and
# doesn't depend on kindnet's partial NetworkPolicy enforcement. During the partition BOTH sides
# keep accepting writes (each injects an event locally); assert neither event crossed. Then HEAL
# (remove the iptables rules) and assert the two divergent DAGs reconcile — each side's
# partition-time event reaches the other. Peers do not re-gossip old events, so reconciliation
# rides the periodic anti-entropy round (§6.1.8), same backstop as ae-repair.
#
# Assertions: (1) baseline gossip works; (2) during partition, each side's event is ABSENT at the
# other; (3) after heal, both events reconcile within RECONCILE_BUDGET_MS.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/partition-rejoin"
mkdir -p "$RUN_DIR"

NS_A="creda-peer-a"
NS_B="creda-peer-b"

BASELINE_BUDGET_MS=15000     # pre-partition connectivity check
SETTLE_SECS=8                # partition dwell before asserting isolation (>> gossip Bound 1 ~2s)
# Reconciliation after heal = libp2p reconnect + the next anti-entropy round (30s interval), so it
# is AE-paced, not gossip-paced. Generous headroom for reconnect + Job scheduling.
RECONCILE_BUDGET_MS=120000

CHART="$REPO_ROOT/deploy/helm/creda"
DRIVER_IMAGE="peer-driver:testbed"
DOCKER="${DOCKER:-docker}"   # resolves to podman on the maintainers' macs, like the other scripts

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"
PEER_DNS="peer-0.peer-headless:50051"

if ! "$DOCKER" image inspect "$DRIVER_IMAGE" >/dev/null 2>&1; then
  echo "ERROR: image $DRIVER_IMAGE not present locally; run 'make up' (or 'make images')" >&2
  echo "       (this scenario adds the check-absent peer-driver subcommand — rebuild the image)" >&2
  exit 2
fi

# kind node containers hosting the cluster — where we insert/remove the partition iptables rules.
NODES="$(kind get nodes --name "$CLUSTER")"

# Partition state (pod IPs), set once the peers are Ready. Guarded so cleanup is safe if we fail early.
IP_A=""
IP_B=""

partition() {
  echo "==> PARTITION: dropping all traffic between peer-a ($IP_A) and peer-b ($IP_B) on the kind nodes"
  local node
  for node in $NODES; do
    "$DOCKER" exec "$node" iptables -I FORWARD 1 -s "$IP_A" -d "$IP_B" -j DROP
    "$DOCKER" exec "$node" iptables -I FORWARD 1 -s "$IP_B" -d "$IP_A" -j DROP
  done
}

heal() {
  # Idempotent + guarded: safe to call from the cleanup trap even if we never partitioned.
  [[ -n "$IP_A" && -n "$IP_B" && -n "$NODES" ]] || return 0
  local node
  for node in $NODES; do
    "$DOCKER" exec "$node" iptables -D FORWARD -s "$IP_A" -d "$IP_B" -j DROP 2>/dev/null || true
    "$DOCKER" exec "$node" iptables -D FORWARD -s "$IP_B" -d "$IP_A" -j DROP 2>/dev/null || true
  done
}

dump_diagnostics() {
  for NS in "$NS_A" "$NS_B"; do
    echo "------ $NS pods ------" >&2
    $kc -n "$NS" get pods -o wide 2>/dev/null || true
    for POD in $($kc -n "$NS" get pods -o name 2>/dev/null); do
      echo "------ logs $NS/$POD creda-core ------" >&2
      $kc -n "$NS" logs "$POD" -c creda-core --tail=80 2>/dev/null || true
    done
  done
}

cleanup() {
  local rc=$?
  # ALWAYS remove the partition rules first — a failed run must not leave the node iptables in a
  # partitioned state for the next scenario.
  echo "==> removing any partition rules"
  heal 2>/dev/null || true
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
# Pod Security Standard), wait for completion, echo the Job's last stdout line. Non-zero if the Job
# does not complete in time — which, under set -e, aborts and triggers diagnostics.
driver_job() {
  local ns="$1" job="$2" wait_s="$3"; shift 3
  local items="" a
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

# ---- keygen + participants -------------------------------------------------------------------
echo "==> generating Ed25519 keypairs"
head -c 32 /dev/urandom >"$RUN_DIR/peer-a.key"
head -c 32 /dev/urandom >"$RUN_DIR/peer-b.key"
derive_pubkey() {
  "$DOCKER" run --rm -v "$RUN_DIR":/keys:ro "$DRIVER_IMAGE" derive-pubkey --secret-file "/keys/${1}.key"
}
PUB_A="$(derive_pubkey peer-a)"
PUB_B="$(derive_pubkey peer-b)"
mkdir -p "$RUN_DIR/participants"
echo "$PUB_A" >"$RUN_DIR/participants/peer-a.key"
echo "$PUB_B" >"$RUN_DIR/participants/peer-b.key"

echo "==> creating namespaces + secrets"
for NS in "$NS_A" "$NS_B"; do
  $kc create namespace "$NS" >/dev/null
  $kc label namespace "$NS" pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null
done
$kc -n "$NS_A" create secret generic creda-signing-key --from-file=signing.key="$RUN_DIR/peer-a.key" >/dev/null
$kc -n "$NS_B" create secret generic creda-signing-key --from-file=signing.key="$RUN_DIR/peer-b.key" >/dev/null
for NS in "$NS_A" "$NS_B"; do
  $kc -n "$NS" create configmap creda-participants \
    --from-file="$RUN_DIR/participants/peer-a.key" \
    --from-file="$RUN_DIR/participants/peer-b.key" >/dev/null
done

# ---- install peer-a, then peer-b bootstrapped to peer-a (both up, meshed) --------------------
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

# ---- baseline: prove the two peers actually gossip BEFORE we partition -----------------------
echo "==> baseline: injecting at peer-a and observing at peer-b (mesh must be live pre-partition)"
E0="$(driver_job "$NS_A" pr-baseline-inject 60 --peer "http://$PEER_DNS" inject --tag "pr-$$-base")"
BASE_LAT="$(driver_job "$NS_B" pr-baseline-observe $((BASELINE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$E0" --timeout-ms "$BASELINE_BUDGET_MS")"
echo "    baseline gossip works (peer-b saw $E0 in ${BASE_LAT} ms)"

# ---- partition ------------------------------------------------------------------------------
IP_A="$($kc -n "$NS_A" get pod peer-0 -o jsonpath='{.status.podIP}')"
IP_B="$($kc -n "$NS_B" get pod peer-0 -o jsonpath='{.status.podIP}')"
[[ -n "$IP_A" && -n "$IP_B" ]] || { echo "ERROR: could not resolve both peer pod IPs ($IP_A / $IP_B)" >&2; exit 1; }
partition

# ---- both sides keep working during the partition -------------------------------------------
echo "==> injecting on BOTH sides during the partition"
E_A="$(driver_job "$NS_A" pr-part-inject-a 60 --peer "http://$PEER_DNS" inject --tag "pr-$$-a")"
echo "    peer-a wrote $E_A"
E_B="$(driver_job "$NS_B" pr-part-inject-b 60 --peer "http://$PEER_DNS" inject --tag "pr-$$-b")"
echo "    peer-b wrote $E_B"

echo "==> settling ${SETTLE_SECS}s, then asserting the partition held (neither event crossed)"
sleep "$SETTLE_SECS"
driver_job "$NS_B" pr-absent-a 40 --peer "http://$PEER_DNS" check-absent --event-id "$E_A" >/dev/null
driver_job "$NS_A" pr-absent-b 40 --peer "http://$PEER_DNS" check-absent --event-id "$E_B" >/dev/null
echo "    isolation confirmed: peer-a's event absent at peer-b, and vice versa"

# ---- heal + reconcile -----------------------------------------------------------------------
echo "==> HEAL: removing the partition rules"
heal

echo "==> waiting for the DAGs to reconcile via anti-entropy (budget ${RECONCILE_BUDGET_MS} ms)"
RE_A="$(driver_job "$NS_B" pr-reconcile-a $((RECONCILE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$E_A" --timeout-ms "$RECONCILE_BUDGET_MS")"
echo "    peer-a's event reached peer-b (${RE_A} ms after heal-observe start)"
RE_B="$(driver_job "$NS_A" pr-reconcile-b $((RECONCILE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$E_B" --timeout-ms "$RECONCILE_BUDGET_MS")"
echo "    peer-b's event reached peer-a (${RE_B} ms after heal-observe start)"

echo "PASS: partition-rejoin (baseline gossip live; both sides wrote under a real node-level partition; isolation held; both events reconciled after heal via anti-entropy, §6.1.7/§6.1.8)"
