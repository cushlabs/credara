#!/usr/bin/env bash
#
# The "anchor creda" run: build + test the whole workspace and print ONE rolled-up summary
# instead of a separate result block per test binary. Run inside the dev container by the
# Makefile `anchor` target (`bash tools/anchor-run.sh`).
#
# - CARGO_BUILD_JOBS=1 bounds compile parallelism so the RocksDB from-source build stays within
#   a memory-limited Docker VM (no OOM) — runner-agnostic.
# - Prefers cargo-nextest (one workspace-wide summary; `--status-level fail` shows only failures
#   plus that summary). Falls back to plain `cargo test` if nextest is absent, so the run never
#   breaks.
# - nextest does not run doctests, so those run separately afterward.
set -euo pipefail

export CARGO_BUILD_JOBS=1

echo "== Creda test bank — building and running the whole workspace (single-threaded build) =="
if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace --status-level fail
else
  echo "(cargo-nextest not found — falling back to 'cargo test'; you'll see one block per test binary)"
  cargo test --workspace
fi

echo
# Doctests run separately because nextest does not execute them. Today no crate has runnable
# `///` code examples, so this is a no-op — but keeping it means the moment someone adds (or
# breaks) a doc example, it is actually compiled and tested. Stay quiet on success; show detail
# only on failure, so a 0-doctest run adds just one line.
if doc_output="$(cargo test --workspace --doc --quiet 2>&1)"; then
  echo "doctests: ok"
else
  printf '%s\n' "$doc_output"
  echo "doctests: FAILED"
  exit 1
fi

echo
echo "⚓ anchor creda: complete — see the rolled-up summary above."
