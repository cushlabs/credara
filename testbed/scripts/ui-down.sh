#!/usr/bin/env bash
# Tear down the UAT clients install (the persistent 'creda-ui' namespace). Has no effect on
# the ephemeral 'creda-ui-smoke' namespace that scenarios/ui-smoke uses.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
NS="creda-ui"
CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

# `|| true` because the targets should be safe to run twice — both `helm uninstall` and
# `kubectl delete namespace` are no-ops once the target is already gone.
echo "==> uninstalling clients release"
$hm uninstall -n "$NS" clients 2>/dev/null || true
echo "==> deleting namespace $NS"
$kc delete namespace "$NS" --wait=false --ignore-not-found 2>/dev/null || true
echo "==> UI down"
