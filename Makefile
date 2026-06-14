# Creda task runner.
#
# The ONLY host prerequisite is a container engine — Podman or Docker. Every target runs
# inside the dev container (docker/dev.Dockerfile), which carries the Rust toolchain, a C
# compiler, and all dependencies — so no developer installs cargo/rustc/clippy by hand. The
# `docker` invocations below run unchanged under Podman's Docker-compatible CLI (the
# maintainers build under Podman). See docs/DEVELOPMENT.md.
#
# Usage:
#   make anchor      # the known-good run: whole workspace, single-threaded build, with ONE
#                    #   rolled-up test summary (cargo-nextest). Also `make summary` / `anchor creda`.
#   make test        # full workspace test suite (PQC algorithms included)
#   make test-fast   # Ed25519-only fast path (no pqcrypto / C build)
#   make fmt         # apply rustfmt
#   make fmt-check   # check formatting (CI parity)
#   make clippy      # lint with warnings-as-errors (CI parity)
#   make build       # release build of the workspace
#   make grpc        # build + lint + test creda-core with the opt-in gRPC feature (needs protoc;
#                    #   the dev image carries it). Not part of `anchor creda`'s default build.
#   make libp2p      # compile-check creda-core with gRPC + libp2p (the shipped feature set). This
#                    #   is the reconciliation entry point for the quarantined libp2p adapter; kept
#                    #   out of CI by design (libp2p churn must not gate the workspace).
#   make bridge      # build the HAPI FHIR Bridge (Java/Kotlin) in a Gradle+JDK container (M7)
#   make shell       # interactive shell in the dev container
#   make clean       # remove build artifacts and the dependency cache
#
# Low-memory machines: compiling RocksDB from source (librocksdb-sys) is memory-hungry and
# can trip the OOM killer ("Killed signal terminated program cc1plus"). Either give the engine
# more memory (Podman: `podman machine set --memory 8192`; Docker Desktop -> Settings ->
# Resources -> Memory: 6-8 GB), or cap build
# parallelism so fewer compilers run at once (slower, but bounded memory):
#   make test JOBS=1     # one compiler at a time — safest on constrained Docker memory
#   make test JOBS=2

# Container CLI. Defaults to `docker`; override for Podman (which many contributors use):
#   make ci DOCKER=podman      — or export DOCKER=podman once in your shell.
# Podman's CLI is Docker-compatible, so every invocation below works unchanged either way.
DOCKER ?= docker

DEV_IMAGE      ?= creda-dev:local
DEV_DOCKERFILE := .devcontainer/Dockerfile

# Base image for the dev/build container. Default = Fedora, for parity with the shipped images
# (Fedora Hummingbird family, DQ-4). The Dockerfile is package-manager-agnostic, so you can fall
# back to the prebuilt official Debian Rust image instantly if needed:
#   make DEV_BASE=docker.io/library/rust:1-bookworm anchor
DEV_BASE ?= registry.fedoraproject.org/fedora:41

# FHIR Bridge build image (M7): Fedora + OpenJDK + Gradle, built from .devcontainer/bridge.Dockerfile
# for build/prod parity (DQ-6 — same OS family as the shipped Hummingbird OpenJDK image). The
# prebuilt Debian-based gradle image remains a fallback via `make bridge-stock`.
BRIDGE_DEV_IMAGE   ?= creda-bridge-dev:local
BRIDGE_DOCKERFILE  := .devcontainer/bridge.Dockerfile
BRIDGE_BASE        ?= registry.fedoraproject.org/fedora:41
BRIDGE_STOCK_IMAGE ?= docker.io/library/gradle:8.10-jdk21

# Optional cap on build parallelism. Empty = use all cores (fastest). Set JOBS=1 (or 2) to
# bound peak memory when compiling RocksDB on a memory-limited Docker VM. A single `-j` also
# limits the cc crate's parallel C/C++ compiles (it derives NUM_JOBS from cargo's job count).
JOBS ?=
CARGO_JOBS := $(if $(JOBS),--jobs $(JOBS),)

# Run cargo in the container as the host user so files written to the mounted repo (target/,
# the dependency cache) are owned by you, not root. CARGO_HOME lives in a gitignored repo
# dir, which sidesteps named-volume permission issues with a non-root user.
UID := $(shell id -u)
GID := $(shell id -g)
RUN  = $(DOCKER) run --rm \
	-v "$(CURDIR)":/work -w /work \
	-e CARGO_HOME=/work/.cargo-cache \
	-e HOME=/tmp \
	--user $(UID):$(GID) \
	$(DEV_IMAGE)

.PHONY: anchor summary dev-image test test-fast fmt fmt-check clippy build grpc libp2p shell ci clean bridge bridge-image bridge-stock

dev-image:
	$(DOCKER) build -t $(DEV_IMAGE) --build-arg BASE=$(DEV_BASE) -f $(DEV_DOCKERFILE) .

# The "anchor" run (= `anchor creda`): build + test the whole workspace single-threaded (so the
# RocksDB from-source compile stays within a memory-limited Docker VM) and print ONE rolled-up
# summary via cargo-nextest (failures-only + a workspace-wide total) instead of a result block
# per test binary. Falls back to `cargo test` if nextest is missing. See tools/anchor-run.sh.
anchor: dev-image
	$(RUN) bash tools/anchor-run.sh

# Alias.
summary: anchor

test: dev-image
	$(RUN) cargo test --workspace $(CARGO_JOBS)

test-fast: dev-image
	$(RUN) cargo test --workspace --no-default-features $(CARGO_JOBS)

fmt: dev-image
	$(RUN) cargo fmt --all

fmt-check: dev-image
	$(RUN) cargo fmt --all -- --check

clippy: dev-image
	$(RUN) cargo clippy --workspace --all-targets $(CARGO_JOBS) -- -D warnings

build: dev-image
	$(RUN) cargo build --workspace --release $(CARGO_JOBS)

# Build + lint + test the opt-in gRPC server (feature `grpc`). Compiles the proto via protoc
# (present in the dev image) and runs the grpc.rs unit + UDS-serve tests. Kept separate from
# `anchor creda` so the default build stays fast and protoc-free.
grpc: dev-image
	$(RUN) cargo clippy -p creda-core --features grpc --all-targets $(CARGO_JOBS) -- -D warnings
	$(RUN) cargo test -p creda-core --features grpc $(CARGO_JOBS)

# Compile-check the shipped feature set (gRPC + libp2p). libp2p is the one quarantined dependency
# whose API churns between versions, so this is its reconciliation entry point. As of the
# `libp2p-adapter` job in ci-rust.yml it IS exercised in CI (a dedicated, non-core job so a libp2p
# API shift can't turn the *core* workspace red); re-check the `libp2p 0.56` spots on a version bump.
libp2p: dev-image
	$(RUN) cargo clippy -p creda-core --features grpc,libp2p --all-targets $(CARGO_JOBS) -- -D warnings

# Everything CI checks, in one go — run this before pushing. Mirrors the ci-rust workflow:
# fmt-check + workspace clippy/test, the gRPC-feature clippy/test, and the libp2p adapter compile.
ci: fmt-check clippy test grpc libp2p

shell: dev-image
	$(DOCKER) run --rm -it \
		-v "$(CURDIR)":/work -w /work \
		-e CARGO_HOME=/work/.cargo-cache -e HOME=/tmp \
		--user $(UID):$(GID) \
		$(DEV_IMAGE) bash

# Build the HAPI FHIR Bridge (M7) — the one Java/Kotlin component, NOT part of `anchor creda`.
# Default builds on the Fedora+OpenJDK+Gradle parity image (DQ-6). Runs as the host user; the
# Gradle/Maven cache lives in a gitignored in-repo dir; the repo root is mounted because the bridge
# generates its gRPC stubs from the shared proto under crates/creda-core/proto.
bridge-image:
	$(DOCKER) build -t $(BRIDGE_DEV_IMAGE) --build-arg BASE=$(BRIDGE_BASE) -f $(BRIDGE_DOCKERFILE) .

bridge: bridge-image
	$(DOCKER) run --rm \
		-v "$(CURDIR)":/work -w /work/bridge \
		-e GRADLE_USER_HOME=/work/.gradle-cache -e HOME=/work/.gradle-cache \
		--user $(UID):$(GID) \
		$(BRIDGE_DEV_IMAGE) gradle build --no-daemon

# Fallback: build on the prebuilt Debian-based gradle image (no custom image build) if the Fedora
# parity image ever hiccups.
bridge-stock:
	$(DOCKER) run --rm \
		-v "$(CURDIR)":/work -w /work/bridge \
		-e GRADLE_USER_HOME=/work/.gradle-cache -e HOME=/work/.gradle-cache \
		--user $(UID):$(GID) \
		$(BRIDGE_STOCK_IMAGE) gradle build --no-daemon

clean:
	rm -rf target .cargo-cache bridge/build bridge/.gradle .gradle-cache
