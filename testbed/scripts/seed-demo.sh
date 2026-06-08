#!/usr/bin/env bash
# Seed the demo dataset into the real-mode UAT peer (namespace creda-uat) by running the
# peer-driver's `seed-demo` subcommand as an in-cluster Job against Core's TCP gRPC endpoint.
#
# Idempotent in the k8s sense (the previous Job is replaced) but NOT in the DAG sense: the DAG is
# append-forward, so seeding twice creates a second copy of every demo event. To return to a
# clean baseline use `make -C testbed reset` (wipe + seed) instead of seeding repeatedly.
#
# Prints the seeded `name=<uuid>` lines so testers can reference exact event ids.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
NS="creda-uat"
CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
DRIVER_IMAGE="peer-driver:testbed"
JOB="seed-demo"
# fullnameOverride=peer → pod peer-0, headless service peer-headless; gRPC TCP per
# values-uat-peer.yaml (tcp://0.0.0.0:50051) exposed on the headless service's `grpc` port.
PEER_DNS="peer-0.peer-headless:50051"

if ! $kc -n "$NS" get statefulset/peer >/dev/null 2>&1; then
  echo "ERROR: no UAT peer in namespace $NS — run 'make ui-up-real' first" >&2
  exit 2
fi

$kc -n "$NS" delete job "$JOB" --ignore-not-found >/dev/null 2>&1 || true

cat <<EOF | $kc -n "$NS" apply -f - >/dev/null
apiVersion: batch/v1
kind: Job
metadata:
  name: $JOB
spec:
  backoffLimit: 0
  ttlSecondsAfterFinished: 3600
  template:
    spec:
      restartPolicy: Never
      # creda-uat is labeled with the restricted Pod Security Standard (DQ-1); every pod,
      # including this Job, must conform — same shape as the scenario runners' Jobs.
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
            - "seed-demo"
EOF

echo "==> waiting for seed job"
$kc -n "$NS" wait --for=condition=complete --timeout=90s "job/$JOB" >/dev/null
echo "==> demo dataset seeded:"
$kc -n "$NS" logs "job/$JOB"
