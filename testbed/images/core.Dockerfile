# syntax=docker/dockerfile:1
#
# Testbed-only Core image. Uses the existing dev image (creda-dev:local) as the builder — which
# already carries the full Rust + clang + protoc toolchain — and a small Fedora-minimal as the
# runtime base.
#
# The production image (deploy/docker/core.Dockerfile) targets Hummingbird FIPS for DQ-4; those
# images aren't publicly available yet, so the testbed substitutes working public bases. Kubernetes
# sets runAsUser at pod runtime (Helm values default 65532), so the runtime image doesn't need a
# specific UID baked in.
#
# Build context = repo root:
#   docker build -f testbed/images/core.Dockerfile -t creda-core:testbed .

ARG RUST_BUILDER=creda-dev:local
ARG RUNTIME=registry.fedoraproject.org/fedora-minimal:41
ARG FEATURES=grpc,libp2p

FROM ${RUST_BUILDER} AS builder
# ARGs declared before the first FROM are NOT in scope inside build stages — they have to be
# re-declared after FROM to be visible. Without this, ${FEATURES} below resolves to empty and
# cargo builds with default features only (no grpc, no libp2p) — and `creda serve` errors out
# at runtime saying gRPC is missing.
ARG FEATURES
# CACHEBUST is a content hash of the Rust sources, supplied by build-and-load.sh. Consuming it in
# a RUN before the COPY invalidates the COPY + cargo layers whenever the workspace changes —
# defeating podman-machine's stale COPY-layer cache (same fix as bridge.Dockerfile) — while
# leaving them cached (fast) when nothing changed.
ARG CACHEBUST=dev
RUN echo "core source hash: ${CACHEBUST}"
WORKDIR /src
COPY . .
# CARGO_HOME / target live inside the layer so the build artifacts go into the image. We set
# HOME=/tmp so cargo doesn't try to write under a non-existent user home.
ENV HOME=/tmp CARGO_HOME=/tmp/cargo-cache
RUN cargo build --release -p creda-core --features "${FEATURES}"

FROM ${RUNTIME}
COPY --from=builder /src/target/release/creda /usr/local/bin/creda
EXPOSE 4001 9090
# Declare non-root UID at the image level so the image is self-describing (PodSecurity-restricted
# friendly) and we don't depend on the chart's runtime override to do the right thing. Matches
# the chart's containerSecurityContext.runAsUser default of 65532 (DQ-1).
USER 65532:65532
# Entrypoint is just the binary; the Helm chart provides the subcommand via container args
# (default `["serve"]`). Avoids double-`serve` when the chart adds its own args.
ENTRYPOINT ["/usr/local/bin/creda"]
