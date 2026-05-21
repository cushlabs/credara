# Creda developer / build container.
#
# Carries the full toolchain (Rust + rustfmt + clippy, a C/C++ compiler + libclang for
# rust-rocksdb, and protoc for the gRPC feature) so nobody installs any of it by hand. Used by
# both the root Makefile task runner and devcontainer.json.
#
# Base: Fedora, for parity with the shipped images, which are built on Fedora Hummingbird
# (docs/DESIGN_QUEUE.md DQ-4). Building dev/CI on the same OS family we ship on means glibc,
# system libraries, and packaging behave the same in development as in production.
#
# This is the *development/build* image, NOT a shipped artifact (shipped runtime images are the
# hardened distroless Hummingbird ones, DQ-4). The Dockerfile is package-manager-agnostic, so
# `DEV_BASE` can still point at the official Debian `rust` image as an instant fallback:
#   make DEV_BASE=docker.io/library/rust:1-bookworm anchor
ARG BASE=registry.fedoraproject.org/fedora:41
FROM ${BASE}

# Install build dependencies with whichever package manager the base provides:
#   * Fedora / Hummingbird family -> dnf
#   * official Debian `rust` image (fallback) -> apt-get
# Provides the C/C++ toolchain + libclang (rust-rocksdb's librocksdb-sys runs bindgen and builds
# RocksDB from source) and protoc (the creda-core `grpc` feature).
RUN set -eux; \
    if command -v dnf >/dev/null 2>&1; then \
        dnf -y install gcc gcc-c++ clang clang-devel llvm-devel cmake \
            protobuf-compiler openssl-devel pkgconf-pkg-config curl ca-certificates \
            findutils which && \
        dnf clean all; \
    elif command -v apt-get >/dev/null 2>&1; then \
        apt-get update && \
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
            clang libclang-dev llvm-dev g++ cmake pkg-config protobuf-compiler \
            curl ca-certificates && \
        rm -rf /var/lib/apt/lists/*; \
    fi

# Ensure a Rust toolchain is present. The official `rust` image already has it; on a bare Fedora
# base we bootstrap via rustup into a SHARED location (world-readable) so the container can run
# as any non-root UID — including the host UID the Makefile passes via `--user`.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN set -eux; \
    if ! command -v cargo >/dev/null 2>&1; then \
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
            sh -s -- -y --profile minimal --default-toolchain stable --component rustfmt clippy; \
        chmod -R a+rwX "$RUSTUP_HOME" "$CARGO_HOME"; \
    else \
        rustup component add rustfmt clippy 2>/dev/null || true; \
    fi

# Non-root user for the VS Code devcontainer path. The Makefile path overrides the user with the
# host UID/GID via `docker run --user`, so file ownership stays correct either way. (Note: at
# runtime the Makefile sets CARGO_HOME=/work/.cargo-cache for the dependency cache; the toolchain
# binaries above stay on PATH at /usr/local/cargo/bin.)
RUN (command -v useradd >/dev/null 2>&1 && useradd --create-home --shell /bin/bash dev) || true

WORKDIR /work
