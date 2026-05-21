# Developing Creda

**The only thing you install is Docker.** The Rust toolchain, the C compiler, and every
dependency live inside a dev container, so nobody sets up `cargo`, `rustc`, `clippy`, or
build tooling by hand. A task runner (`make`) and a VS Code dev container both drive that
same container.

## Quick start

Install Docker (Docker Desktop on macOS/Windows, or Docker Engine on Linux), then from the
repository root:

```sh
make test-fast   # quickest check: builds the dev image, runs the Ed25519-only test path
make test        # full workspace test suite, including the PQC algorithms
```

The first invocation builds the dev image and downloads dependencies; later runs are cached.
That's the whole setup — there is no "install Rust" step.

## Task runner

| Command | What it does |
|---|---|
| `make test` | Full workspace tests (PQC on by default). |
| `make test-fast` | Tests with `--no-default-features` (Ed25519 only; skips the pqcrypto C build). |
| `make fmt` | Apply `rustfmt`. |
| `make fmt-check` | Check formatting (matches CI). |
| `make clippy` | Lint with warnings-as-errors (matches CI). |
| `make build` | Release build of the workspace. |
| `make ci` | `fmt-check` + `clippy` + `test` — everything CI gates on. |
| `make shell` | Open an interactive shell in the dev container. |
| `make clean` | Remove `target/` and the dependency cache. |

Every target runs `cargo` inside the dev container **as your host user**, so files it writes
(`target/`, the cache) are owned by you, not root. The dependency cache lives in a gitignored
`./.cargo-cache/` directory.

## VS Code / Codespaces

Open the repo in VS Code with the Dev Containers extension (or in a GitHub Codespace) and
choose "Reopen in Container." You get the same toolchain plus `rust-analyzer`, TOML support,
and the LLDB debugger preconfigured — no local Rust install. The container runs as a non-root
`dev` user.

## The dev image vs. the shipped images

The dev/build container (`.devcontainer/Dockerfile`) defaults to the official Rust image so
the workflow works the moment Docker is present. This is **only** the local build/test
environment.

The **shipped** product images (the M8 Dockerfiles that produce Core, Export Gate, Verifier,
and the Bridge) are built on **Fedora Hummingbird** hardened distroless base images, FIPS by
default — see `docs/DESIGN_QUEUE.md` DQ-4. To build and test locally against the same
Hummingbird Rust base for full parity, point the dev base at it:

```sh
make DEV_BASE=<fedora-hummingbird-rust-image> test
```

(Replace with the pinned Hummingbird Rust image reference once chosen.)

## Native builds (optional)

Nothing stops you from running a native `cargo test -p creda-events` if you already maintain
your own Rust toolchain — the `rust-toolchain.toml` pins the channel and components. But the
container workflow above is the supported, reproducible path, and it's what CI runs.

## Troubleshooting

**`Killed signal terminated program cc1plus` / `librocksdb-sys` build fails.** This is the
OS out-of-memory killer terminating the C++ compiler while RocksDB builds from source. RocksDB
is large and its parallel compile is memory-hungry. Fixes, in order of preference:

1. **Give Docker more memory** — Docker Desktop → Settings → Resources → Memory → 6–8 GB,
   then re-run. This is the real fix; the build is fast with headroom.
2. **Cap build parallelism** so fewer compilers run at once (slower, but bounded memory):
   `make test JOBS=1` (one at a time) or `make test JOBS=2`.
3. **Skip RocksDB while iterating** — `make test-fast` builds with `--no-default-features`,
   which exercises `creda-events` and the `creda-store` MemoryStore contract without compiling
   RocksDB at all.

## Notes for the `creda-events` crate (M1)

The PQC algorithms (ML-DSA-65, SLH-DSA-256s, hybrid) are behind the default-on `pqc` feature.
`make test-fast` exercises the Ed25519-only path (no C build); `make test` adds the PQC
algorithms. All pqcrypto interaction is isolated in `crates/creda-events/src/crypto/pqc.rs`.
