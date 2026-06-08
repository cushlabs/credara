#!/usr/bin/env bash
# Wait for a Creda peer's pod to be Ready in the given namespace. The peer's StatefulSet name
# defaults to the Helm release name; pass it as $2 if different.
#
# KUBE_CONTEXT (env, optional): kubectl context to target. Callers MUST set this when they pin a
# context themselves (ui-up-real and the scenario runners all do `kubectl --context=kind-$CLUSTER`)
# — otherwise this script silently follows the user's *current* kubectl context, and with a second
# kind instance (or any other cluster) selected it fails with a misleading
# `namespaces "<ns>" not found` even though the namespace exists on the testbed cluster.
set -euo pipefail

NAMESPACE="${1:?usage: [KUBE_CONTEXT=ctx] wait-ready.sh NAMESPACE [STATEFULSET-NAME] [TIMEOUT-SEC]}"
STS_NAME="${2:-peer}"
TIMEOUT="${3:-180}"

kc() { kubectl ${KUBE_CONTEXT:+--context="$KUBE_CONTEXT"} "$@"; }

echo "==> waiting up to ${TIMEOUT}s for $STS_NAME in $NAMESPACE to be Ready${KUBE_CONTEXT:+ (context $KUBE_CONTEXT)}"
kc -n "$NAMESPACE" rollout status statefulset/"$STS_NAME" --timeout="${TIMEOUT}s"

# Belt-and-braces: also wait for the pod's Ready condition. The rollout-status above checks the
# StatefulSet, but the readiness probe is what the libp2p mesh actually depends on.
POD="$STS_NAME-0"
kc -n "$NAMESPACE" wait --for=condition=Ready pod/"$POD" --timeout="${TIMEOUT}s"
echo "==> $NAMESPACE/$POD Ready"
