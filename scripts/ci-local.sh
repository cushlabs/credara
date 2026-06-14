#!/usr/bin/env bash
# Local CI gate — runs the exact checks ci-rust.yml gates (fmt + workspace clippy/test + the
# gRPC-feature clippy/test + the libp2p adapter compile) in a throwaway Rust container, using
# cached named volumes so repeat runs are fast.
#
# This is the lightweight path for contributors who build in containers (no host cargo), using the
# official Debian `rust` image. `make ci` is the equivalent through the project's Fedora dev-image;
# both mirror ci-rust.yml. Run either before pushing — `fmt --check` first, so the formatting slip
# that reached CI can't recur.
#
# Usage:
#   ./scripts/ci-local.sh                 # default: podman
#   ENGINE=docker ./scripts/ci-local.sh   # use docker instead
#   IMAGE=rust:1-bookworm ./scripts/ci-local.sh
set -euo pipefail

ENGINE="${ENGINE:-podman}"
IMAGE="${IMAGE:-rust:1}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

exec "$ENGINE" run --rm \
  -v "$REPO":/w -w /w \
  -v credara-cargo:/usr/local/cargo/registry \
  -v credara-target:/w/target \
  "$IMAGE" bash -c '
    set -euo pipefail
    export PATH="/usr/local/cargo/bin:$PATH" DEBIAN_FRONTEND=noninteractive
    # libclang: RocksDB bindgen FFI; protoc: the gRPC-feature steps.
    apt-get update -qq && apt-get install -y -qq clang libclang-dev protobuf-compiler >/dev/null
    echo "── fmt --check ──";            cargo fmt --all -- --check
    echo "── clippy (workspace) ──";     cargo clippy --workspace --all-targets -- -D warnings
    echo "── test (workspace) ──";       cargo test --workspace
    echo "── clippy (gRPC feature) ──";  cargo clippy -p creda-core --features grpc --all-targets -- -D warnings
    echo "── test (gRPC feature) ──";    cargo test -p creda-core --features grpc
    echo "── clippy (libp2p adapter) ──"; cargo clippy -p creda-net --features libp2p --all-targets -- -D warnings
    echo "✅ local CI gate passed"
  '
