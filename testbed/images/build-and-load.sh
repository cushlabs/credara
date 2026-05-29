#!/usr/bin/env bash
# Build the Creda Core and Bridge container images for the testbed and load them into the kind
# cluster.
#
# Uses testbed-specific Dockerfiles (testbed/images/{core,bridge}.Dockerfile) that depend only on
# publicly available bases. The production Dockerfiles under deploy/docker/ target Hummingbird FIPS
# images (DQ-4) and are kept aspirational until those images publish.
set -euo pipefail

CLUSTER="${1:-creda-testbed}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

CORE_IMAGE="creda-core:testbed"
BRIDGE_IMAGE="creda-bridge:testbed"
DRIVER_IMAGE="peer-driver:testbed"

# The Core build needs the dev image present locally (it's the builder stage). The user runs
# `make libp2p` (or any `make` target) before `make up`, which builds creda-dev:local; if missing,
# build it now.
if ! docker image inspect creda-dev:local >/dev/null 2>&1; then
  echo "==> creda-dev:local not present; building it (one-time)"
  (cd "$REPO_ROOT" && make dev-image)
fi

echo "==> building $CORE_IMAGE"
docker build \
  -f "$REPO_ROOT/testbed/images/core.Dockerfile" \
  --build-arg FEATURES="grpc,libp2p" \
  -t "$CORE_IMAGE" \
  "$REPO_ROOT"

echo "==> building $BRIDGE_IMAGE"
docker build \
  -f "$REPO_ROOT/testbed/images/bridge.Dockerfile" \
  -t "$BRIDGE_IMAGE" \
  "$REPO_ROOT"

echo "==> building $DRIVER_IMAGE"
docker build \
  -f "$REPO_ROOT/testbed/images/peer-driver.Dockerfile" \
  -t "$DRIVER_IMAGE" \
  "$REPO_ROOT"

echo "==> loading images into kind cluster '$CLUSTER'"
kind load docker-image "$CORE_IMAGE" --name "$CLUSTER"
kind load docker-image "$BRIDGE_IMAGE" --name "$CLUSTER"
kind load docker-image "$DRIVER_IMAGE" --name "$CLUSTER"

echo "==> images ready: $CORE_IMAGE, $BRIDGE_IMAGE, $DRIVER_IMAGE"
