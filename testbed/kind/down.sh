#!/usr/bin/env bash
# Delete the kind cluster if it exists. Idempotent.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"

if kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  echo "==> deleting kind cluster '$CLUSTER'"
  kind delete cluster --name "$CLUSTER"
else
  echo "==> kind cluster '$CLUSTER' not present; nothing to delete"
fi
