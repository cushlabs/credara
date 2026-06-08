#!/usr/bin/env bash
# Tear down the real-mode UAT install (the persistent 'creda-uat' namespace). Has no effect
# on the 'creda-ui' (mock-mode UAT) or 'creda-ui-smoke' (ephemeral test) namespaces.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
NS="creda-uat"
CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
hm="helm --kube-context=${CTX}"

echo "==> uninstalling clients release"
$hm uninstall -n "$NS" clients 2>/dev/null || true
echo "==> uninstalling peer release"
$hm uninstall -n "$NS" peer 2>/dev/null || true
echo "==> deleting namespace $NS"
$kc delete namespace "$NS" --wait=false --ignore-not-found 2>/dev/null || true
echo "==> real-mode UAT down"
