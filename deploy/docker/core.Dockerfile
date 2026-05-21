# syntax=docker/dockerfile:1
#
# creda-core peer image (M8, spec §10.6.1). Multi-stage: build the Rust binary, then ship it on a
# Fedora Hummingbird hardened distroless FIPS base (DQ-4 — the spec's generic distroless choice,
# §10.6.1, specialized to Hummingbird). Runs non-root (DQ-1).
#
# Build context is the REPO ROOT (creda-core depends on the other crates by path):
#   docker build -f deploy/docker/core.Dockerfile -t creda-core:dev .
#
# TODO(DQ-4): pin the exact Hummingbird image references (registry path + digest) once chosen.
# TODO: the shipped peer needs the gRPC API and libp2p networking, so the build enables
#   --features grpc,libp2p. That requires protoc in the builder (the grpc feature) and the libp2p
#   adapter to be reconciled (TODO(libp2p-verify) in creda-net). Until those are settled, build
#   with `--build-arg FEATURES=grpc` (or none) — the deployment packaging is intentionally ahead
#   of the in-daemon gRPC-serve and libp2p-transport wiring (both tracked follow-ups).
ARG RUST_BUILDER=registry.fedoraproject.org/hummingbird/rust:fips
ARG RUNTIME=registry.fedoraproject.org/hummingbird/minimal-nonroot:fips
ARG FEATURES=grpc,libp2p

FROM ${RUST_BUILDER} AS builder
WORKDIR /src
COPY . .
# The Hummingbird Rust builder is expected to carry the C/C++ toolchain + libclang (for
# rust-rocksdb) and protoc (for the grpc feature); see the dev image for the dependency set.
RUN cargo build --release -p creda-core --features "${FEATURES}"

FROM ${RUNTIME}
# Distroless: no shell, no package manager (§10.6.1). Non-root by base-image default (DQ-1).
COPY --from=builder /src/target/release/creda /usr/local/bin/creda
# Bridge HTTP is exposed by the bridge container; here: libp2p (4001), metrics/health (9090).
EXPOSE 4001 9090
ENTRYPOINT ["/usr/local/bin/creda", "serve"]
