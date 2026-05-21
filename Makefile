# Creda task runner.
#
# The ONLY host prerequisite is Docker. Every target runs inside the dev container
# (docker/dev.Dockerfile), which carries the Rust toolchain, a C compiler, and all
# dependencies — so no developer installs cargo/rustc/clippy by hand. See docs/DEVELOPMENT.md.
#
# Usage:
#   make test        # full workspace test suite (PQC algorithms included)
#   make test-fast   # Ed25519-only fast path (no pqcrypto / C build)
#   make fmt         # apply rustfmt
#   make fmt-check   # check formatting (CI parity)
#   make clippy      # lint with warnings-as-errors (CI parity)
#   make build       # release build of the workspace
#   make shell       # interactive shell in the dev container
#   make clean       # remove build artifacts and the dependency cache

DEV_IMAGE      ?= creda-dev:local
DEV_DOCKERFILE := .devcontainer/Dockerfile

# Base image for the dev/build container. Default = official Rust image (works with only
# Docker). For full parity with the shipped images (DQ-4), set:
#   make DEV_BASE=<fedora-hummingbird-rust-image> test
DEV_BASE ?= docker.io/library/rust:1-bookworm

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

.PHONY: dev-image test test-fast fmt fmt-check clippy build shell ci clean

dev-image:
	docker build -t $(DEV_IMAGE) --build-arg BASE=$(DEV_BASE) -f $(DEV_DOCKERFILE) .

test: dev-image
	$(RUN) cargo test --workspace

test-fast: dev-image
	$(RUN) cargo test --workspace --no-default-features

fmt: dev-image
	$(RUN) cargo fmt --all

fmt-check: dev-image
	$(RUN) cargo fmt --all -- --check

clippy: dev-image
	$(RUN) cargo clippy --workspace --all-targets -- -D warnings

build: dev-image
	$(RUN) cargo build --workspace --release

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
