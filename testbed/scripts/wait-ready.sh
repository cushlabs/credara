#!/usr/bin/env bash
# Wait for a Creda peer's pod to be Ready in the given namespace. The peer's StatefulSet name
# defaults to the Helm release name; pass it as $2 if different.
set -euo pipefail

NAMESPACE="${1:?usage: wait-ready.sh NAMESPACE [STATEFULSET-NAME] [TIMEOUT-SEC]}"
STS_NAME="${2:-peer}"
TIMEOUT="${3:-180}"

echo "==> waiting up to ${TIMEOUT}s for $STS_NAME in $NAMESPACE to be Ready"
kubectl -n "$NAMESPACE" rollout status statefulset/"$STS_NAME" --timeout="${TIMEOUT}s"

# Belt-and-braces: also wait for the pod's Ready condition. The rollout-status above checks the
# StatefulSet, but the readiness probe is what the libp2p mesh actually depends on.
POD="$STS_NAME-0"
kubectl -n "$NAMESPACE" wait --for=condition=Ready pod/"$POD" --timeout="${TIMEOUT}s"
echo "==> $NAMESPACE/$POD Ready"
