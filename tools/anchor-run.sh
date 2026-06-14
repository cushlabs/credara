#!/usr/bin/env bash
#
# The "anchor creda" run: the complete local gate, mirroring .github/workflows/ci-rust.yml so
# nothing CI checks can slip past a clean local run. Invoked inside the dev container by the
# Makefile `anchor` target (`bash tools/anchor-run.sh`).
#
# Order mirrors ci-rust: format, then workspace clippy/test, then the gRPC-feature clippy/test —
# the ONLY build that compiles the grpc-gated modules (health.rs, metrics.rs, grpc.rs), so a fmt
# or clippy issue there is invisible to a default-feature build — then the creda-net libp2p adapter
# clippy (ci-rust's `libp2p-adapter` job). Doctests run last.
#
# - CARGO_BUILD_JOBS=1 bounds compile parallelism so the RocksDB from-source build (shared by every
#   clippy and test pass below) stays within a memory-limited Docker VM (no OOM) — runner-agnostic.
# - Prefers cargo-nextest for the workspace tests (one rolled-up summary); falls back to cargo test.
# - Fail-fast (set -e): the first lint/test failure stops with a non-zero exit, exactly as CI would.
#   A formatting failure is auto-fixed by `make fmt` (cargo fmt --all).
set -euo pipefail

export CARGO_BUILD_JOBS=1

echo "== lint gate (mirrors ci-rust) =="
echo "-- fmt --check"
cargo fmt --all -- --check
echo "-- clippy (workspace)"
cargo clippy --workspace --all-targets -- -D warnings
echo "-- clippy (creda-core, grpc feature)"
cargo clippy -p creda-core --features grpc --all-targets -- -D warnings
echo "-- clippy (creda-net, libp2p adapter)"
cargo clippy -p creda-net --features libp2p --all-targets -- -D warnings

echo
echo "== test bank — whole workspace (single-threaded build) =="
if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace --status-level fail
else
  echo "(cargo-nextest not found — falling back to 'cargo test'; you'll see one block per test binary)"
  cargo test --workspace
fi

echo
echo "== gRPC-feature tests (grpc-gated modules: health, metrics, grpc) =="
cargo test -p creda-core --features grpc

echo
# Doctests run separately because nextest does not execute them. Today no crate has runnable
# `///` code examples, so this is a no-op — but keeping it means the moment someone adds (or
# breaks) a doc example, it is actually compiled and tested. Stay quiet on success.
if doc_output="$(cargo test --workspace --doc --quiet 2>&1)"; then
  echo "doctests: ok"
else
  printf '%s\n' "$doc_output"
  echo "doctests: FAILED"
  exit 1
fi

echo
echo "⚓ anchor creda: complete — fmt + clippy (workspace + grpc + libp2p) + tests all green."
