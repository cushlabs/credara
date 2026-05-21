# Creda developer / build container.
#
# This image carries the full toolchain (Rust + rustfmt + clippy + a C compiler for the
# pqcrypto crates) so developers and CI never install any of it by hand. It is used by both
# the root Makefile task runner and devcontainer.json.
#
# NOTE: this is the *development/build* image, not a shipped artifact. The shipped product
# images (M8) are built on Fedora Hummingbird per docs/DESIGN_QUEUE.md DQ-4. The default base
# here is the official Rust image so `make test` works with only Docker installed; set BASE
# to the Hummingbird Rust image for full parity (see DEV_BASE in the Makefile).
ARG BASE=docker.io/library/rust:1-bookworm
FROM ${BASE}

# The official Rust image is Debian-based and already ships gcc/make for building C deps
# (needed by pqcrypto). Ensure the components our workflow uses are present (idempotent).
RUN rustup component add rustfmt clippy 2>/dev/null || true

# A non-root user for the VS Code devcontainer path. The Makefile path overrides the user
# with the host UID/GID via `docker run --user`, so file ownership stays correct either way.
ARG USERNAME=dev
RUN useradd --create-home --shell /bin/bash ${USERNAME} 2>/dev/null || true

WORKDIR /work
