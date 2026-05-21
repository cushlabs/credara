# Creda task runner.
#
# The ONLY host prerequisite is Docker. Every target runs inside the dev container
# (docker/dev.Dockerfile), which carries the Rust toolchain, a C compiler, and all
# dependencies — so no developer installs cargo/rustc/clippy by hand. See docs/DEVELOPMENT.md.
#
# Usage:
#   make anchor      # full suite, single-threaded — the "known-good" run (= test JOBS=1).
#                    #   Also available as `anchor creda` (see the ./anchor wrapper).
#   make test        # full workspace test suite (PQC algorithms included)
#   make test-fast   # Ed25519-only fast path (no pqcrypto / C build)
#   make fmt         # apply rustfmt
#   make fmt-check   # check formatting (CI parity)
#   make clippy      # lint with warnings-as-errors (CI parity)
#   make build       # release build of the workspace
#   make shell       # interactive shell in the dev container
#   make clean       # remove build artifacts and the dependency cache
#
# Low-memory machines: compiling RocksDB from source (librocksdb-sys) is memory-hungry and
# can trip the OOM killer ("Killed signal terminated program cc1plus"). Either give Docker
# more memory (Docker Desktop -> Settings -> Resources -> Memory: 6-8 GB), or cap build
# parallelism so fewer compilers run at once (slower, but bounded memory):
#   make test JOBS=1     # one compiler at a time — safest on constrained Docker memory
#   make test JOBS=2

DEV_IMAGE      ?= creda-dev:local
DEV_DOCKERFILE := .devcontainer/Dockerfile

# Base image for the dev/build container. Default = Fedora, for parity with the shipped images
# (Fedora Hummingbird family, DQ-4). The Dockerfile is package-manager-agnostic, so you can fall
# back to the prebuilt official Debian Rust image instantly if needed:
#   make DEV_BASE=docker.io/library/rust:1-bookworm anchor
DEV_BASE ?= registry.fedoraproject.org/fedora:41

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
RUN  = docker run --rm \
	-v "$(CURDIR)":/work -w /work \
	-e CARGO_HOME=/work/.cargo-cache \
	-e HOME=/tmp \
	--user $(UID):$(GID) \
	$(DEV_IMAGE)

.PHONY: anchor dev-image test test-fast fmt fmt-check clippy build shell ci clean

dev-image:
	docker build -t $(DEV_IMAGE) --build-arg BASE=$(DEV_BASE) -f $(DEV_DOCKERFILE) .

# The "anchor" run: full workspace suite, single-threaded so the RocksDB from-source compile
# stays within a memory-limited Docker VM (the known-good command). Same as `anchor creda`.
anchor: dev-image
	$(RUN) cargo test --workspace --jobs 1

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

# Everything CI checks, in one go.
ci: fmt-check clippy test

shell: dev-image
	docker run --rm -it \
		-v "$(CURDIR)":/work -w /work \
		-e CARGO_HOME=/work/.cargo-cache -e HOME=/tmp \
		--user $(UID):$(GID) \
		$(DEV_IMAGE) bash

clean:
	rm -rf target .cargo-cache
