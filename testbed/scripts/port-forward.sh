#!/usr/bin/env bash
# Start a kubectl port-forward to a peer's gRPC TCP port. Writes the PID to a file so the caller
# can stop it cleanly.
#
# Usage: port-forward.sh NAMESPACE LOCAL_PORT [POD-NAME] [PID-FILE]
set -euo pipefail

NAMESPACE="${1:?usage: port-forward.sh NAMESPACE LOCAL_PORT [POD-NAME] [PID-FILE]}"
LOCAL_PORT="${2:?need LOCAL_PORT}"
POD="${3:-peer-0}"
PID_FILE="${4:-/tmp/port-forward-${NAMESPACE}-${LOCAL_PORT}.pid}"

# Background the port-forward; redirect to /dev/null so it doesn't spam the scenario output.
kubectl -n "$NAMESPACE" port-forward "pod/$POD" "${LOCAL_PORT}:50051" >/dev/null 2>&1 &
PID=$!
echo "$PID" >"$PID_FILE"

# Wait briefly for the forward to be up by polling the local port.
for _ in $(seq 1 30); do
  if (echo >"/dev/tcp/127.0.0.1/${LOCAL_PORT}") 2>/dev/null; then
    echo "==> port-forward $NAMESPACE/$POD :50051 -> localhost:$LOCAL_PORT (pid $PID)"
    exit 0
  fi
  sleep 0.2
done

echo "ERROR: port-forward did not come up within 6s" >&2
kill "$PID" 2>/dev/null || true
exit 1
