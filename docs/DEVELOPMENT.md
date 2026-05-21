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

## `anchor creda` — settling the workspace into a known-good state

`anchor` (a script at the repo root) settles a project into a known-good state by running its
definitive build + test. For this repo:

```sh
anchor creda      # the known-good full run: workspace tests, single-threaded (= make test JOBS=1)
```

`anchor creda` is the run to trust when you want a definitive green: it builds everything and
runs the whole suite single-threaded (keeping the RocksDB from-source compile within a
memory-limited Docker VM, no OOM) and prints **one rolled-up summary** — not a separate result
block per test binary. It uses [cargo-nextest](https://nexte.st), which runs the whole workspace
and reports a single workspace-wide total; `--status-level fail` suppresses the per-passing-test
rows, so on success you see just the build plus one `Summary: N tests run: N passed` line (plus a
separate doctest line). If nextest is somehow unavailable it falls back to plain `cargo test`.
The logic lives in `tools/anchor-run.sh`. Equivalently, `make anchor` (or `make summary`) runs it
without the wrapper.

To make `anchor creda` work from anywhere, put the script on your PATH — either symlink it:

```sh
ln -s "$(pwd)/anchor" /usr/local/bin/anchor    # or ~/.local/bin/anchor, if that's on your PATH
```

or add a shell alias to `~/.zshrc`:

```sh
alias anchor="$HOME/Documents/projects/Creda/anchor"
```

Other dev tasks are plain `make` targets from the repo root — `make test`, `make ci`, `make fmt`,
`make clippy`, `make shell` (see the task-runner table above).

## VS Code / Codespaces

Open the repo in VS Code with the Dev Containers extension (or in a GitHub Codespace) and
choose "Reopen in Container." You get the same toolchain plus `rust-analyzer`, TOML support,
and the LLDB debugger preconfigured — no local Rust install. The container runs as a non-root
`dev` user.

## The dev image vs. the shipped images

The dev/build container (`.devcontainer/Dockerfile`) is built on **Fedora**, for parity with
the **shipped** product images (the M8 Dockerfiles that produce Core, Export Gate, Verifier, and
the Bridge), which use **Fedora Hummingbird** hardened distroless base images, FIPS by default
(`docs/DESIGN_QUEUE.md` DQ-4). Building dev/CI on the same OS family we ship on means glibc,
system libraries, and packaging behave the same in development as in production. The Fedora base
bootstraps the Rust toolchain via rustup; this is **only** the local build/test environment, not
a shipped artifact.

The Dockerfile is package-manager-agnostic, so if the Fedora path ever hiccups you can fall back
to the prebuilt official Debian Rust image instantly:

```sh
make DEV_BASE=docker.io/library/rust:1-bookworm anchor
```

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

## The FHIR Bridge (Java/Kotlin)

The bridge (`bridge/`) is the one non-Rust component, so it builds separately from `anchor creda`
— Docker-only, no local JDK needed:

```sh
make bridge        # builds bridge/ on the Fedora+OpenJDK+Gradle parity image (DQ-6)
make bridge-stock  # fallback: build on the prebuilt Debian gradle image, if the Fedora image hiccups
```

It runs as your host user, caches Gradle/Maven in a gitignored `./.gradle-cache/`, and mounts the
repo root (the bridge generates its gRPC stubs from `crates/creda-core/proto`). Per the build/prod
parity principle (DQ-6), the build image is Fedora + OpenJDK + Gradle — the same OS family as the
shipped Hummingbird OpenJDK image — built from `.devcontainer/bridge.Dockerfile`.

## Notes for the `creda-events` crate (M1)

The PQC algorithms (ML-DSA-65, SLH-DSA-256s, hybrid) are behind the default-on `pqc` feature.
`make test-fast` exercises the Ed25519-only path (no C build); `make test` adds the PQC
algorithms. All pqcrypto interaction is isolated in `crates/creda-events/src/crypto/pqc.rs`.
