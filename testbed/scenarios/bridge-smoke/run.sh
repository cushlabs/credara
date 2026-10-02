#!/usr/bin/env bash
# Bridge smoke test (Docker-only host prerequisites).
#
# The one gate scenario that runs the HAPI FHIR Bridge, and the one that exercises the PRODUCTION
# Core<->Bridge transport: a Unix domain socket on the shared `uds` emptyDir (§10.5.1), dialed by
# CredaCoreClient with netty's epoll domain-socket channel. Every other testbed values file
# overrides grpcSocket to tcp:// so the peer-driver Jobs can reach Core from another pod; this one
# does not, because it asserts over HTTP against the Bridge rather than gRPC against Core.
#
# Assertions run as a Kubernetes Job inside the cluster — no port-forward, no host toolchain, same
# execution model as the other scenarios.
#
# What each assertion actually proves:
#   1. helm --wait succeeds           -> the Bridge's readinessProbe (GET /fhir/metadata) passed,
#                                        so Spring Boot booted, Tomcat bound :8080, and HAPI's
#                                        RestfulServer registered its providers.
#   2. GET /fhir/metadata -> 200       -> CapabilityStatement generation + the Creda interceptor.
#   3. GET /Patient/not-a-uuid -> 400  -> request routing and exception rendering. No Core call.
#   4. GET /Patient/<uuid> -> 404      -> THE UDS ASSERTION. A 404 means the Bridge reached Core
#                                        over the socket and Core answered "no events". A 500 (or
#                                        a hang) is what a broken epoll/UDS transport looks like.
#                                        404-not-500 is the whole point of this line.
#   5. GET /AuditEvent?patient=<uuid>  -> a second Core round-trip, through the search path rather
#      -> 200 + empty Bundle              than read, returning an honest empty ledger.
#   6. Bridge log clean                -> no NoSuchMethodError/NoClassDefFoundError naming Spring,
#                                        which is the signature of the HAPI-on-Spring-Framework-7
#                                        risk (HAPI 8.10.1 CI-tests against Spring Framework 6.2.18).
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"
RUN_DIR="$TESTBED/.run/bridge-smoke"
mkdir -p "$RUN_DIR"

NS="creda-bridge-smoke"

CHART="$REPO_ROOT/deploy/helm/creda"
DRIVER_IMAGE="peer-driver:testbed"
BRIDGE_IMAGE="creda-bridge:testbed"

CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

# In-cluster FHIR base. `fullnameOverride: peer` makes the ClusterIP Service `peer-fhir`; the
# chart's NetworkPolicy allows ingress on ports.http from any source, so a Job in this namespace
# reaches it without extra rules.
FHIR_BASE="http://peer-fhir:8080/fhir"

# The Bridge is Java and starts behind a readinessProbe with initialDelaySeconds=20, so it needs a
# more generous window than the Rust-only scenarios.
INSTALL_TIMEOUT=300

for IMG in "$DRIVER_IMAGE" "$BRIDGE_IMAGE"; do
  if ! docker image inspect "$IMG" >/dev/null 2>&1; then
    echo "ERROR: image $IMG not present locally; run 'make up' (or 'make images')" >&2
    exit 2
  fi
done

dump_diagnostics() {
  echo "------ $NS pods ------" >&2
  $kc -n "$NS" get pods 2>/dev/null || true
  for POD in $($kc -n "$NS" get pods -o name 2>/dev/null); do
    echo "------ describe $NS/$POD ------" >&2
    $kc -n "$NS" describe "$POD" 2>/dev/null | tail -40 || true
    echo "------ logs $NS/$POD creda-core ------" >&2
    $kc -n "$NS" logs "$POD" -c creda-core --tail=80 2>/dev/null || true
    # The Bridge log is the high-value artifact for this scenario specifically — a Spring 7 or
    # HAPI 8 incompatibility surfaces as a stack trace here and nowhere else. Deeper tail.
    echo "------ logs $NS/$POD hapi-bridge ------" >&2
    $kc -n "$NS" logs "$POD" -c hapi-bridge --tail=200 2>/dev/null || true
  done
  echo "------ assertion Job log ------" >&2
  $kc -n "$NS" logs "job/$CHECK_JOB" 2>/dev/null || true
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
  $kc delete namespace "$NS" --ignore-not-found 2>/dev/null || true
  # Block until the namespace is FULLY gone, not just Terminating, so a re-run does not hit
  # "object is being deleted: namespaces already exists".
  $kc wait --for=delete "namespace/$NS" --timeout=120s 2>/dev/null || true
  exit "$rc"
}
CHECK_JOB="bridge-check"
trap cleanup EXIT

# ---- keygen (host-side, Docker-only) ---------------------------------------------------------
echo "==> generating Ed25519 keypair"
head -c 32 /dev/urandom >"$RUN_DIR/peer.key"

PUB="$(docker run --rm -v "$RUN_DIR":/keys:ro "$DRIVER_IMAGE" derive-pubkey --secret-file /keys/peer.key)"
mkdir -p "$RUN_DIR/participants"
echo "$PUB" >"$RUN_DIR/participants/peer.key"

# ---- namespace, Secret, ConfigMap ------------------------------------------------------------
echo "==> creating namespace + secrets"
$kc create namespace "$NS" >/dev/null
$kc label namespace "$NS" pod-security.kubernetes.io/enforce=restricted --overwrite >/dev/null
$kc -n "$NS" create secret generic creda-signing-key \
  --from-file=signing.key="$RUN_DIR/peer.key" >/dev/null
$kc -n "$NS" create configmap creda-participants \
  --from-file="$RUN_DIR/participants/peer.key" >/dev/null

# ---- install the peer (Core + Bridge, UDS transport) -----------------------------------------
# `--wait` is load-bearing: the Bridge container's readinessProbe is GET /fhir/metadata, so the
# pod does not report Ready until HAPI is actually serving. If Spring Boot 4 / Spring Framework 7
# / HAPI 8 fail to come up together, this install times out and the scenario fails here, before a
# single assertion runs. That is the cheapest possible detection of the highest-severity failure.
echo "==> installing peer (Core + Bridge, UDS transport)"
$hm install -n "$NS" peer "$CHART" \
  -f "$TESTBED/helm/values-bridge-smoke.yaml" \
  --set signingKey.secretName=creda-signing-key \
  --set participantRegistry.configMapName=creda-participants \
  --wait --timeout "${INSTALL_TIMEOUT}s" >/dev/null

KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS" peer "$INSTALL_TIMEOUT"
echo "    Bridge is Ready (readinessProbe GET /fhir/metadata passed)"

# ---- assertions via in-cluster Job ------------------------------------------------------------
echo "==> asserting the FHIR surface"
$kc -n "$NS" delete job "$CHECK_JOB" --ignore-not-found >/dev/null 2>&1 || true

cat <<EOF | $kc -n "$NS" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $CHECK_JOB
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 600
  template:
    spec:
      restartPolicy: Never
      # Namespace is labeled with the restricted Pod Security Standard (DQ-1 parity with
      # production). Every pod, including this Job, must conform.
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        fsGroup: 65532
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: check
          image: $DRIVER_IMAGE
          imagePullPolicy: Never
          securityContext:
            allowPrivilegeEscalation: false
            runAsNonRoot: true
            capabilities:
              drop: ["ALL"]
          # Override the peer-driver ENTRYPOINT: this Job wants curl from the image, not the
          # driver binary. (curl is installed by testbed/images/peer-driver.Dockerfile.)
          command: ["/bin/sh", "-c"]
          args:
            - |
              set -u
              BASE="$FHIR_BASE"
              UUID="\$(cat /proc/sys/kernel/random/uuid)"
              fail() { echo "FAIL: \$*"; exit 1; }

              # Retry the first call briefly: readiness gates the Service endpoint, but a Job can
              # still race the endpoint controller by a beat.
              n=0
              until curl -sS -o /tmp/meta.json -w '%{http_code}' "\$BASE/metadata" >/tmp/code 2>/tmp/err; do
                n=\$((n+1)); [ \$n -ge 10 ] && fail "metadata unreachable after 10 tries: \$(cat /tmp/err)"
                sleep 2
              done

              code=\$(cat /tmp/code)
              [ "\$code" = "200" ] || fail "metadata: expected 200, got \$code"
              grep -q '"resourceType"[[:space:]]*:[[:space:]]*"CapabilityStatement"' /tmp/meta.json \\
                || fail "metadata: body is not a CapabilityStatement"
              echo "  ok  GET /metadata -> 200 CapabilityStatement"

              code=\$(curl -sS -o /dev/null -w '%{http_code}' "\$BASE/Patient/not-a-uuid")
              [ "\$code" = "400" ] || fail "Patient/not-a-uuid: expected 400, got \$code"
              echo "  ok  GET /Patient/not-a-uuid -> 400"

              # The UDS assertion. 404 == the Bridge reached Core over the Unix socket and Core
              # reported no events. 500 == the transport is broken.
              code=\$(curl -sS -o /tmp/p.json -w '%{http_code}' "\$BASE/Patient/\$UUID")
              case "\$code" in
                404) echo "  ok  GET /Patient/<unknown-uuid> -> 404 (Core round-trip over UDS succeeded)" ;;
                500) fail "Patient/<unknown-uuid>: 500 — Core call failed. Suspect the epoll/UDS transport (netty 4.2). Body: \$(head -c 400 /tmp/p.json)" ;;
                *)   fail "Patient/<unknown-uuid>: expected 404, got \$code. Body: \$(head -c 400 /tmp/p.json)" ;;
              esac

              code=\$(curl -sS -o /tmp/ae.json -w '%{http_code}' "\$BASE/AuditEvent?patient=\$UUID")
              [ "\$code" = "200" ] || fail "AuditEvent?patient: expected 200, got \$code. Body: \$(head -c 400 /tmp/ae.json)"
              grep -q '"resourceType"[[:space:]]*:[[:space:]]*"Bundle"' /tmp/ae.json \\
                || fail "AuditEvent?patient: body is not a Bundle"
              echo "  ok  GET /AuditEvent?patient=<unknown-uuid> -> 200 Bundle (empty ledger)"

              echo "ALL_CHECKS_PASSED"
EOF

if ! $kc -n "$NS" wait --for=condition=complete --timeout=120s "job/$CHECK_JOB" >/dev/null 2>&1; then
  echo "FAIL: assertion Job did not complete" >&2
  $kc -n "$NS" logs "job/$CHECK_JOB" >&2 || true
  exit 1
fi

$kc -n "$NS" logs "job/$CHECK_JOB"
$kc -n "$NS" logs "job/$CHECK_JOB" | grep -q "ALL_CHECKS_PASSED" \
  || { echo "FAIL: assertion Job completed without ALL_CHECKS_PASSED" >&2; exit 1; }

# ---- Bridge log must be free of Spring/HAPI linkage errors -----------------------------------
# HAPI FHIR 8.10.1 is built and CI-tested against Spring Framework 6.2.18; Spring Boot 4.1 brings
# Spring Framework 7. An incompatibility shows up as a linkage error at the moment the affected
# code path is first touched — which can be well after startup, so a Ready pod is not proof.
echo "==> checking the Bridge log for Spring/HAPI linkage errors"
if $kc -n "$NS" logs peer-0 -c hapi-bridge 2>/dev/null \
     | grep -E "NoSuchMethodError|NoClassDefFoundError|ClassNotFoundException" \
     | grep -q "springframework"; then
  echo "FAIL: Bridge log contains a Spring linkage error — HAPI 8 on Spring Framework 7" >&2
  $kc -n "$NS" logs peer-0 -c hapi-bridge 2>/dev/null \
    | grep -E -A5 "NoSuchMethodError|NoClassDefFoundError|ClassNotFoundException" >&2 || true
  exit 1
fi
echo "    clean"

echo "PASS: bridge smoke test (FHIR surface served over the production UDS transport)"
