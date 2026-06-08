#!/usr/bin/env bash
# Port-forward the in-cluster clients Service to http://localhost:5173. Blocking — Ctrl-C
# stops the forwarder; the UI deployment itself keeps running.
#
# Run after `make ui-up`. Re-runnable: if the port is already taken, kill the previous
# forwarder (the script prints the PID it owns).
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
LOCAL_PORT="${UI_FORWARD_PORT:-5173}"

# UAT=1 targets the real-mode install (namespace 'creda-uat'); default is mock-mode UAT
# (namespace 'creda-ui'). The two namespaces can both be up; setting UAT picks which one
# the port-forward attaches to.
if [[ "${UAT:-0}" = "1" ]]; then
  NS="creda-uat"
else
  NS="creda-ui"
fi
CTX="kind-${CLUSTER}"
kc="kubectl --context=${CTX}"

if ! $kc -n "$NS" get svc creda-clients >/dev/null 2>&1; then
  echo "ERROR: Service creda-clients not found in namespace $NS — run 'make ui-up' first" >&2
  exit 2
fi

echo "==> forwarding http://localhost:${LOCAL_PORT} → svc/creda-clients:8080 (Ctrl-C to stop)"
exec $kc -n "$NS" port-forward --address 127.0.0.1 svc/creda-clients "${LOCAL_PORT}:8080"
