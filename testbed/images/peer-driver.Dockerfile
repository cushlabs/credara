# syntax=docker/dockerfile:1
#
# Testbed peer-driver image. Built in the existing dev image (creda-dev:local) so the host needs
# nothing but Docker — no host Rust toolchain. The peer-driver runs as a Kubernetes Job inside the
# kind cluster and talks to peer gRPC services via in-cluster DNS, so the testbed never depends on
# kubectl port-forward or host networking.
#
# Build context = repo root:
#   docker build -f testbed/images/peer-driver.Dockerfile -t peer-driver:testbed .

ARG RUST_BUILDER=creda-dev:local
ARG RUNTIME=registry.fedoraproject.org/fedora-minimal:41

FROM ${RUST_BUILDER} AS builder
WORKDIR /src
COPY . .
ENV HOME=/tmp CARGO_HOME=/tmp/cargo-cache
RUN cargo build --release --manifest-path testbed/tools/peer-driver/Cargo.toml

FROM ${RUNTIME}
COPY --from=builder /src/testbed/tools/peer-driver/target/release/peer-driver /usr/local/bin/peer-driver
ENTRYPOINT ["/usr/local/bin/peer-driver"]
