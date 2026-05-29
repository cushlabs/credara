#!/usr/bin/env bash
# Create the kind cluster if it doesn't already exist. Idempotent.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
HERE="$(cd "$(dirname "$0")" && pwd)"

if kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  echo "==> kind cluster '$CLUSTER' already exists; skipping create"
else
  echo "==> creating kind cluster '$CLUSTER'"
  kind create cluster --name "$CLUSTER" --config "$HERE/cluster.yaml" --wait 120s
fi

# Make sure kubectl is pointing at it.
kubectl cluster-info --context "kind-$CLUSTER" >/dev/null
echo "==> kind cluster '$CLUSTER' ready"
