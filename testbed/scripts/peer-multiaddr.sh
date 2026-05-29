#!/usr/bin/env bash
# Print a peer's libp2p multiaddr in the form `/ip4/<podIP>/tcp/4001/p2p/<peer-id>`, suitable for
# wiring into another peer's `config.bootstrapPeers`.
#
# Strategy:
#   - PodIP comes from `kubectl get pod` (cluster-internal IP; peer-b reaches peer-a directly via
#     this since kindnet routes pod IPs across nodes).
#   - PeerId is derived by asking peer-a's daemon for it. Since the peer-id is logged at startup,
#     we grep the logs. (A cleaner long-term path is to expose it on /readyz or a small
#     `/peer-id` HTTP endpoint; tracked as a follow-up.)
set -euo pipefail

NAMESPACE="${1:?usage: peer-multiaddr.sh NAMESPACE [POD-NAME]}"
POD="${2:-peer-0}"

POD_IP="$(kubectl -n "$NAMESPACE" get pod "$POD" -o jsonpath='{.status.podIP}')"
if [[ -z "$POD_IP" ]]; then
  echo "ERROR: pod $NAMESPACE/$POD has no podIP yet (not Ready?)" >&2
  exit 1
fi

# The daemon logs the local peer id at startup; grep it out of the Core container's logs. The
# log line we look for: "local libp2p peer id: 12D3KooW..." (added by the daemon at startup;
# follow-up if not present yet — see scenarios/gossip-convergence/README.md).
PEER_ID="$(
  kubectl -n "$NAMESPACE" logs "$POD" -c creda-core \
    | grep -oE '12D3KooW[A-Za-z0-9]+' \
    | head -n 1 \
    || true
)"
if [[ -z "$PEER_ID" ]]; then
  echo "ERROR: could not find peer id in $NAMESPACE/$POD logs" >&2
  echo "       expected a line containing a peer id starting with '12D3KooW'." >&2
  echo "       (the daemon may not yet log it at startup — see testbed README follow-ups.)" >&2
  exit 1
fi

echo "/ip4/${POD_IP}/tcp/4001/p2p/${PEER_ID}"
