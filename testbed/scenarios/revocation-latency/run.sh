#!/usr/bin/env bash
# Revocation-latency scenario (§4.3.2, §4.7).
#
# Two peers, meshed from the start. At peer-a: inject a subject Assert, then an AuthorizationGrant
# over it. Confirm the Grant has replicated to peer-b — so peer-b holds the Grant BEFORE the
# revocation arrives, which means the revocation is *validated on arrival* (§4.6 step 2), i.e. it
# takes effect the instant it lands rather than sitting unvalidated. Then inject an
# AuthorizationRevocation at peer-a and measure how long it takes to reach peer-b, against the
# §4.7 Bound-1 (gossip) budget.
#
# Assertion: the revocation propagates to (and is validated at) peer-b within REVOKE_BUDGET_MS.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/revocation-latency"
mkdir -p "$RUN_DIR"

NS_A="creda-peer-a"
NS_B="creda-peer-b"

# Bound 1 is gossip propagation (~1-2s normal). The Grant is already replicated and the mesh is
# warm by the time we revoke, so the revocation observes near steady-state; the budget carries
# headroom for kubectl Job scheduling on top of Bound 1.
GRANT_REPLICATE_BUDGET_MS=15000
REVOKE_BUDGET_MS=5000

CHART="$REPO_ROOT/deploy/helm/creda"
DRIVER_IMAGE="peer-driver:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

PEER_DNS="peer-0.peer-headless:50051"

if ! docker image inspect "$DRIVER_IMAGE" >/dev/null 2>&1; then
  echo "ERROR: image $DRIVER_IMAGE not present locally; run 'make up' (or 'make images')" >&2
  echo "       (this scenario adds new peer-driver subcommands — rebuild the image so they exist)" >&2
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
  # Block until the namespaces are FULLY gone (not just Terminating) — the next scenario reuses
  # these namespace names and would otherwise hit "object is being deleted".
  $kc wait --for=delete "namespace/$NS_A" "namespace/$NS_B" --timeout=120s 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT

# driver_job NS JOB WAIT_S <driver args...> — run the peer-driver as an in-cluster Job (restricted
# Pod Security Standard, DQ-1 production parity), wait for completion, echo the Job's last stdout
# line (an event id, or a latency in ms). Returns non-zero if the Job does not complete in time —
# which, under `set -e`, aborts the scenario and triggers the diagnostics dump.
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

# ---- grant, replicate, then revoke -----------------------------------------------------------
TAG="rev-$$"

echo "==> injecting subject Assert at peer-a"
SUBJECT="$(driver_job "$NS_A" peer-driver-subject 60 \
  --peer "http://$PEER_DNS" inject --tag "$TAG")"
echo "    subject = $SUBJECT"

echo "==> injecting AuthorizationGrant at peer-a"
GRANT="$(driver_job "$NS_A" peer-driver-grant 60 \
  --peer "http://$PEER_DNS" inject-grant --subject "$SUBJECT")"
echo "    grant   = $GRANT"

echo "==> confirming the Grant has replicated to peer-b (so the revocation validates on arrival)"
GRANT_LAT="$(driver_job "$NS_B" peer-driver-observe-grant $((GRANT_REPLICATE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$GRANT" --timeout-ms "$GRANT_REPLICATE_BUDGET_MS")"
echo "    grant present at peer-b (${GRANT_LAT} ms)"

echo "==> injecting the revocation at peer-a and timing its propagation to peer-b (budget ${REVOKE_BUDGET_MS} ms, §4.7 Bound 1)"
# Inject-at-peer-a + poll-at-peer-b run in ONE peer-driver process: t0 is the injecting RPC and t1
# is when peer-b first sees the revocation — the true cross-peer propagation latency, with no
# inter-Job scheduling gap to swallow it. The Job runs in peer-a's namespace and reaches peer-b
# over cross-namespace DNS.
REVOKE_LAT="$(driver_job "$NS_A" peer-driver-time-revoke $((REVOKE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" time-revocation \
  --grant "$GRANT" \
  --observe-peer "http://peer-0.peer-headless.$NS_B:50051" \
  --timeout-ms "$REVOKE_BUDGET_MS")"

echo "PASS: revocation-latency (revocation propagated + validated at peer-b in ${REVOKE_LAT} ms; budget ${REVOKE_BUDGET_MS} ms, §4.7 Bound 1)"
