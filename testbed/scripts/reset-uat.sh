#!/usr/bin/env bash
# Reset the real-mode UAT peer to a known seeded baseline WITHOUT cycling the cluster or
# rebuilding images. The DAG is append-forward by design — events cannot be deleted — so
# "return to base" means: drop the peer's event store (the PVC) and reseed the demo dataset.
# This is what makes test scenarios repeatable: every reset yields the same demo subgraphs
# (fresh event ids, same stable tok:demo:* tokens the clients resolve patients by).
#
# Sequence: scale peer to 0 → delete the data PVC → scale to 1 (volumeClaimTemplate recreates
# an empty store) → wait Ready → run the seed job. ~60–90s total; no image or cluster churn.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
NS="creda-uat"
CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TESTBED="$REPO_ROOT/testbed"

if ! $kc -n "$NS" get statefulset/peer >/dev/null 2>&1; then
  echo "ERROR: no UAT peer in namespace $NS — run 'make ui-up-real' first" >&2
  exit 2
fi

echo "==> scaling peer down"
$kc -n "$NS" scale statefulset/peer --replicas=0 >/dev/null
$kc -n "$NS" wait --for=delete pod/peer-0 --timeout=120s 2>/dev/null || true

echo "==> deleting the peer's event store (PVC data-peer-0)"
$kc -n "$NS" delete pvc data-peer-0 --ignore-not-found >/dev/null

echo "==> scaling peer back up (fresh empty store)"
$kc -n "$NS" scale statefulset/peer --replicas=1 >/dev/null
KUBE_CONTEXT="$CTX" bash "$TESTBED/scripts/wait-ready.sh" "$NS" peer 180

bash "$TESTBED/scripts/seed-demo.sh" "$CLUSTER"

echo "==> UAT peer reset to seeded baseline. Note: pod restarted, so re-run your port-forwards"
echo "    (UAT=1 make ui-forward; kubectl -n $NS port-forward svc/peer-fhir 8080:8080) and"
echo "    hard-refresh the browser."
