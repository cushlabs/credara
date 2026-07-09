#!/usr/bin/env bash
# Rogue-link scenario (§4.6 step 5.5, §5.3.5).
#
# The multi-peer realization of the cross-institutional link-chain defense. Two peers, meshed:
#
#   peer-b (the RESPONDER) authors the real patient's Assert — its own trusted anchor.
#   peer-a (the ROGUE) gossips a hostile fragment onto that patient: a parallel Assert plus a
#          self-audience Grant, fused to the real patient by a Link it controls.
#
# peer-a mounts TWO fragments onto the one real patient, differing only in the Link that carries
# them across the institutional boundary:
#
#   rogue path   — a `manual` Link at max confidence (10000). Manual links are capped below the
#                  trust floor by their method ceiling (§5.3.5), so the Grant they carry cannot gain
#                  standing over the responder's patient.
#   control path — an `insurance-crosswalk` Link at 9500. That method clears the floor, so the
#                  Grant it carries IS admitted.
#
# The two Grants use different audience classes, so each `check-authz` isolates exactly one Grant.
#
# Assertions (evaluated at peer-b, which runs deny-by-default so a Grant is the ONLY path to a yes):
#   - requester in the rogue class is DENIED  — the manual-Link-reached Grant has no standing.
#   - requester in the control class is AUTHORIZED — the crosswalk-Link-reached Grant is admitted.
#
# This proves the defense is Link-specific (it rejects the rogue path, not all cross-institutional
# links) and that it is wired into the peer's real gRPC EvaluateAuthorization over real gossip.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/rogue-link"
mkdir -p "$RUN_DIR"

NS_A="creda-peer-a"
NS_B="creda-peer-b"

# Budget for the rogue fragment (Asserts, Links, Grants) to replicate peer-a → peer-b before we
# evaluate. Generous: the assertions are correctness, not latency (revocation-latency owns timing).
REPLICATE_BUDGET_MS=15000

CHART="$REPO_ROOT/deploy/helm/creda"
DRIVER_IMAGE="peer-driver:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

PEER_DNS="peer-0.peer-headless:50051"

# Audience classes — one per fragment, so each check considers exactly one Grant.
ROGUE_CLASS="rogue-tpo"
CONTROL_CLASS="crosswalk-tpo"

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
  $kc wait --for=delete "namespace/$NS_A" "namespace/$NS_B" --timeout=120s 2>/dev/null || true
  exit "$rc"
}
trap cleanup EXIT

# driver_job NS JOB WAIT_S <driver args...> — run the peer-driver as an in-cluster Job (restricted
# Pod Security Standard), wait for completion, echo the Job's last stdout line. Non-zero on timeout,
# which aborts the scenario under `set -e` and triggers the diagnostics dump.
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

# ---- install peer-a, then peer-b (deny-by-default, bootstrapped to peer-a) --------------------
echo "==> installing peer-a"
$hm install -n "$NS_A" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-a.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --wait --timeout 180s >/dev/null
KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS_A" peer 180

PEER_A_MULTIADDR="$(bash "$TESTBED/scripts/peer-multiaddr.sh" "$NS_A" peer-0)"
echo "==> peer-a multiaddr: $PEER_A_MULTIADDR"

# peer-b is the responder and must run deny-by-default: under treatment-presumed a Treatment
# request is authorized regardless of Grants, so the Grant (and thus the link-chain verdict) would
# not be decisive. deny-by-default makes the Grant the sole path to a yes.
echo "==> installing peer-b (deny-by-default responder, bootstrap → peer-a)"
$hm install -n "$NS_B" peer "$CHART" \
  -f "$TESTBED/helm/values-peer-b.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --set-string "config.defaultPosture=deny-by-default" \
  --set-string "config.bootstrapPeers[0]=$PEER_A_MULTIADDR" \
  --wait --timeout 180s >/dev/null
KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS_B" peer 180

# ---- responder authors the real patient at peer-b --------------------------------------------
TAG="rogue-$$"

echo "==> [peer-b] injecting the real patient Assert (the responder's trusted anchor)"
REAL="$(driver_job "$NS_B" peer-driver-real 60 \
  --peer "http://$PEER_DNS" inject --tag "$TAG")"
echo "    real patient = $REAL"

# ---- rogue peer-a mounts two fragments onto the real patient ---------------------------------
echo "==> [peer-a] injecting two rogue Asserts (parallel identities peer-a controls)"
ROGUE1="$(driver_job "$NS_A" peer-driver-rogue1 60 \
  --peer "http://$PEER_DNS" inject --tag "$TAG-rogue")"
ROGUE2="$(driver_job "$NS_A" peer-driver-rogue2 60 \
  --peer "http://$PEER_DNS" inject --tag "$TAG-control")"
echo "    rogue1 = $ROGUE1"
echo "    rogue2 = $ROGUE2"

echo "==> [peer-a] fusing rogue1 → real with a MANUAL Link @10000 (capped below the floor)"
LINK1="$(driver_job "$NS_A" peer-driver-link-rogue 60 \
  --peer "http://$PEER_DNS" inject-link --a "$ROGUE1" --b "$REAL" --method manual --confidence 10000)"
echo "    rogue link = $LINK1"

echo "==> [peer-a] fusing rogue2 → real with an INSURANCE-CROSSWALK Link @9500 (clears the floor)"
LINK2="$(driver_job "$NS_A" peer-driver-link-control 60 \
  --peer "http://$PEER_DNS" inject-link --a "$ROGUE2" --b "$REAL" --method insurance-crosswalk --confidence 9500)"
echo "    control link = $LINK2"

echo "==> [peer-a] self-issuing a Grant on each rogue fragment (distinct audience classes)"
GRANT1="$(driver_job "$NS_A" peer-driver-grant-rogue 60 \
  --peer "http://$PEER_DNS" inject-grant --subject "$ROGUE1" --audience-class "$ROGUE_CLASS")"
GRANT2="$(driver_job "$NS_A" peer-driver-grant-control 60 \
  --peer "http://$PEER_DNS" inject-grant --subject "$ROGUE2" --audience-class "$CONTROL_CLASS")"
echo "    rogue grant   = $GRANT1  (audience class $ROGUE_CLASS)"
echo "    control grant = $GRANT2  (audience class $CONTROL_CLASS)"

# ---- wait for the whole rogue fragment to replicate to the responder -------------------------
echo "==> confirming both Grants have replicated peer-a → peer-b"
driver_job "$NS_B" peer-driver-observe-grant-rogue $((REPLICATE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$GRANT1" --timeout-ms "$REPLICATE_BUDGET_MS" >/dev/null
driver_job "$NS_B" peer-driver-observe-grant-control $((REPLICATE_BUDGET_MS / 1000 + 30)) \
  --peer "http://$PEER_DNS" observe --event-id "$GRANT2" --timeout-ms "$REPLICATE_BUDGET_MS" >/dev/null
echo "    both grants present at peer-b"

# ---- the verdict: evaluate at the responder --------------------------------------------------
echo "==> [peer-b] EvaluateAuthorization — rogue class must be DENIED (§4.6 step 5.5)"
ROGUE_VERDICT="$(driver_job "$NS_B" peer-driver-check-rogue 60 \
  --peer "http://$PEER_DNS" check-authz \
  --entry "$REAL" --requester-class "$ROGUE_CLASS" \
  --purpose treatment --use-mode read-only --expect denied)"
echo "    rogue request → $ROGUE_VERDICT"

echo "==> [peer-b] EvaluateAuthorization — control class must be AUTHORIZED"
CONTROL_VERDICT="$(driver_job "$NS_B" peer-driver-check-control 60 \
  --peer "http://$PEER_DNS" check-authz \
  --entry "$REAL" --requester-class "$CONTROL_CLASS" \
  --purpose treatment --use-mode read-only --expect authorized)"
echo "    control request → $CONTROL_VERDICT"

echo "PASS: rogue-link (manual-Link-reached Grant DENIED; crosswalk-Link-reached Grant AUTHORIZED; §4.6 step 5.5)"
