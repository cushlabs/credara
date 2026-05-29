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
WORKDIR /src
COPY . .
# CARGO_HOME / target live inside the layer so the build artifacts go into the image. We set
# HOME=/tmp so cargo doesn't try to write under a non-existent user home.
ENV HOME=/tmp CARGO_HOME=/tmp/cargo-cache
RUN cargo build --release -p creda-core --features "${FEATURES}"

FROM ${RUNTIME}
COPY --from=builder /src/target/release/creda /usr/local/bin/creda
EXPOSE 4001 9090
ENTRYPOINT ["/usr/local/bin/creda", "serve"]
