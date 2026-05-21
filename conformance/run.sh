#!/usr/bin/env bash
# Run the Creda conformance suite (spec §11.4).
#
# This drives synthetic, test-data-tagged events through the store and graph and asserts the
# system's contracts: provenance preservation, authorization + revocation enforcement,
# disagreement surfacing, data-category handling, and test-data filtering.
#
# The deployment / multi-peer parts of conformance (helm install on kind/k3d, gossip
# convergence, anti-entropy repair, partition/rejoin, Bound-1 revocation latency §4.7) require
# real peers and a network; they live in the test bed (DQ-3, see testbed/) and run once the
# libp2p transport + gRPC serve path are wired.
set -euo pipefail

cd "$(dirname "$0")/.."

# Prefer the rolled-up nextest summary if available (matches `anchor creda`); fall back to
# plain `cargo test` so the suite still runs on a stock toolchain.
if cargo nextest --version >/dev/null 2>&1; then
  exec cargo nextest run -p creda-conformance --status-level fail
else
  exec cargo test -p creda-conformance
fi
