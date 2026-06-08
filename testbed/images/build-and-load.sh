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

# Cache-bust the core build on Rust source change — same podman COPY-cache hazard as the bridge
# (see the bridge block below): a Rust edit can otherwise silently ship a stale binary. Hash the
# workspace sources + manifests; unchanged = fast cache hit, changed = forced rebuild.
CORE_SRC_HASH="$(
  find "$REPO_ROOT/crates" \
       "$REPO_ROOT/Cargo.toml" \
       "$REPO_ROOT/Cargo.lock" \
       "$REPO_ROOT/rust-toolchain.toml" \
       -type f -print0 2>/dev/null \
    | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1
)"
echo "==> building $CORE_IMAGE (cachebust=${CORE_SRC_HASH:0:12})"
docker build \
  --build-arg CACHEBUST="$CORE_SRC_HASH" \
  -f "$REPO_ROOT/testbed/images/core.Dockerfile" \
  --build-arg FEATURES="grpc,libp2p" \
  -t "$CORE_IMAGE" \
  "$REPO_ROOT"

# Cache-bust the bridge build on source change. podman-machine on macOS does not reliably
# invalidate the COPY-layer cache when sources change (the same hazard the clients build uses
# --no-cache for), so a bridge code edit can silently ship a stale jar. Rather than always
# --no-cache the bridge (a slow Gradle build every time), hash the bridge sources + the shared
# proto and pass it as a build-arg: a change flips the hash and forces a rebuild; no change is a
# fast cache hit. shasum is present on macOS and in the dev container.
BRIDGE_SRC_HASH="$(
  find "$REPO_ROOT/bridge/src" \
       "$REPO_ROOT/bridge/build.gradle.kts" \
       "$REPO_ROOT/bridge/settings.gradle.kts" \
       "$REPO_ROOT/bridge/gradle.properties" \
       "$REPO_ROOT/crates/creda-core/proto" \
       -type f -print0 2>/dev/null \
    | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1
)"
echo "==> building $BRIDGE_IMAGE (cachebust=${BRIDGE_SRC_HASH:0:12})"
docker build \
  --build-arg CACHEBUST="$BRIDGE_SRC_HASH" \
  -f "$REPO_ROOT/testbed/images/bridge.Dockerfile" \
  -t "$BRIDGE_IMAGE" \
  "$REPO_ROOT"

# Cache-bust the driver build on source change (same podman hazard as core/bridge). The driver
# compiles against creda-events + the shared proto, so those are part of the hash.
DRIVER_SRC_HASH="$(
  find "$REPO_ROOT/testbed/tools" "$REPO_ROOT/crates" \
       -type f -not -path "*/target/*" -print0 2>/dev/null \
    | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1
)"
echo "==> building $DRIVER_IMAGE (cachebust=${DRIVER_SRC_HASH:0:12})"
docker build \
  --build-arg CACHEBUST="$DRIVER_SRC_HASH" \
  -f "$REPO_ROOT/testbed/images/peer-driver.Dockerfile" \
  -t "$DRIVER_IMAGE" \
  "$REPO_ROOT"

# Clients + e2e runner — used by scenarios/ui-smoke. Built unconditionally so `make up`
# leaves a fully-loaded cluster regardless of which scenario the user runs next. Both
# Dockerfiles live under clients/ rather than testbed/images/ because the same Dockerfiles
# build the production image (clients/Dockerfile) and the dev e2e runner.
#
# `--no-cache` here is deliberate: podman-machine on macOS does not always invalidate the
# COPY-layer cache when source files change (the same layer digest gets reused across
# iterations even though clients/docker-entrypoint.sh / clients/nginx.conf / clients/src/
# changed). The clients build is fast (~30s), so guaranteeing fresh bytes every iteration
# is the right tradeoff. Core/Bridge keep their caches — those are slow builds and the
# pattern hasn't shown the same cache-staleness issue in practice.
echo "==> building $CLIENTS_IMAGE (no-cache to defeat podman COPY-cache staleness)"
docker build --no-cache \
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
# Enumerate the kind node containers so we can run pre-load housekeeping on each one.
# The naming convention is `<cluster>-control-plane`, `<cluster>-worker`, `<cluster>-worker2`,
# … — kind exposes them as docker/podman containers.
kind_nodes() {
  docker ps --filter "name=^${CLUSTER}-" --format '{{.Names}}'
}

# Before loading a freshly-built image with an existing tag (e.g. `creda-bridge:testbed`),
# remove the OLD tag from each kind node's containerd image store. Loading a same-tag image
# only *dereferences* the previous content — containerd keeps the orphaned layers around
# until garbage collection runs, and kind doesn't trigger GC automatically. Over an
# iteration loop this fills /var/lib/containerd and `kind load` eventually fails with
# "no space left on device". Removing the tag first lets containerd's automatic gc reclaim
# the unreferenced bytes between loads.
#
# We swallow errors because the image may not exist on the node yet (first load) or kind
# may use a slightly different name format. The worst case is no-op, not a fatal error.
prune_old_image_on_nodes() {
  local canonical="$1"
  for node in $(kind_nodes); do
    docker exec "$node" ctr --namespace=k8s.io images rm "${canonical}:testbed" >/dev/null 2>&1 || true
  done
}

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

  # Strip "creda-foo:testbed" → "docker.io/library/creda-foo" for the prune step.
  prune_old_image_on_nodes "$(echo "$canonical" | cut -d: -f1)"

  docker save "$canonical" -o "$tmp"
  kind load image-archive "$tmp" --name "$CLUSTER"
}

load_image_into_kind "$CORE_IMAGE"
load_image_into_kind "$BRIDGE_IMAGE"
load_image_into_kind "$DRIVER_IMAGE"
load_image_into_kind "$CLIENTS_IMAGE"
load_image_into_kind "$E2E_IMAGE"

echo "==> images ready: $CORE_IMAGE, $BRIDGE_IMAGE, $DRIVER_IMAGE, $CLIENTS_IMAGE, $E2E_IMAGE"
