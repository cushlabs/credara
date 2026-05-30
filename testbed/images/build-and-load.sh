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
CLIENTS_IMAGE="creda-clients:testbed"
E2E_IMAGE="creda-clients-e2e:testbed"

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

# Clients + e2e runner — used by scenarios/ui-smoke. Built unconditionally so `make up`
# leaves a fully-loaded cluster regardless of which scenario the user runs next. Both
# Dockerfiles live under clients/ rather than testbed/images/ because the same Dockerfiles
# build the production image (clients/Dockerfile) and the dev e2e runner.
echo "==> building $CLIENTS_IMAGE"
docker build \
  -f "$REPO_ROOT/clients/Dockerfile" \
  -t "$CLIENTS_IMAGE" \
  "$REPO_ROOT"

echo "==> building $E2E_IMAGE"
docker build \
  -f "$REPO_ROOT/clients/e2e.Dockerfile" \
  -t "$E2E_IMAGE" \
  "$REPO_ROOT"

echo "==> loading images into kind cluster '$CLUSTER'"
# Use `save | load image-archive` instead of `kind load docker-image`. The latter is unreliable
# under Podman: Podman stores images under a `localhost/` prefix by default, and kind's
# `docker-image` path resolves names through Docker's daemon, not Podman's image store.
#
# Even with `save | image-archive`, Podman writes the saved archive's RepoTags using the
# stored name (`localhost/creda-core:testbed`), which is not what kubelet asks for when the
# Helm chart references `creda-core:testbed`. Containerd does not do partial-name matching, so
# the image lands but kubelet can't find it.
#
# Fix: before save, ensure a canonical `docker.io/library/<image>` tag exists (containerd's
# default resolution for a bare name), then save by that tag. This works under Docker (the
# extra tag is harmless) and under Podman (it strips the localhost prefix in the archive).
load_image_into_kind() {
  local image="$1"
  local canonical="docker.io/library/$image"
  local tmp
  tmp="$(mktemp -t kind-load.XXXXXX.tar)"
  trap 'rm -f "$tmp"' RETURN

  # Make the canonical tag exist regardless of how the image was stored locally.
  docker tag "$image" "$canonical" 2>/dev/null \
    || docker tag "localhost/$image" "$canonical" 2>/dev/null \
    || {
      echo "ERROR: cannot retag $image as $canonical; image not found locally" >&2
      return 1
    }

  docker save "$canonical" -o "$tmp"
  kind load image-archive "$tmp" --name "$CLUSTER"
}

load_image_into_kind "$CORE_IMAGE"
load_image_into_kind "$BRIDGE_IMAGE"
load_image_into_kind "$DRIVER_IMAGE"
load_image_into_kind "$CLIENTS_IMAGE"
load_image_into_kind "$E2E_IMAGE"

echo "==> images ready: $CORE_IMAGE, $BRIDGE_IMAGE, $DRIVER_IMAGE, $CLIENTS_IMAGE, $E2E_IMAGE"
