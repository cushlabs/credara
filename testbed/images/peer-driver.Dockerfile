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
# CACHEBUST: content hash of the driver sources + shared crates, supplied by build-and-load.sh.
# Same podman stale-COPY-cache defeat as core/bridge: changed sources force a rebuild, unchanged
# sources stay a fast cache hit.
ARG CACHEBUST=dev
RUN echo "driver source hash: ${CACHEBUST}"
WORKDIR /src
COPY . .
ENV HOME=/tmp CARGO_HOME=/tmp/cargo-cache
RUN cargo build --release --manifest-path testbed/tools/peer-driver/Cargo.toml

FROM ${RUNTIME}
# curl, for scenarios that assert over HTTP rather than gRPC — bridge-smoke overrides the
# ENTRYPOINT below and runs `curl` from this image against the Bridge's FHIR surface. Carrying it
# here avoids introducing a fourth testbed image (and a fourth entry in build-and-load.sh) for a
# handful of status-code assertions. `--allowerasing` because fedora-minimal ships `curl-minimal`,
# which `curl` would otherwise conflict with rather than replace.
RUN microdnf install -y --allowerasing curl \
    && microdnf clean all \
    && rm -rf /var/cache/yum
COPY --from=builder /src/testbed/tools/peer-driver/target/release/peer-driver /usr/local/bin/peer-driver
# Non-root UID at the image level so the image satisfies PodSecurity-restricted on its own.
# The peer-driver only opens an outbound TCP connection and writes to stdout — it needs no
# special privileges. Matches the chart's runAsUser default (DQ-1).
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/peer-driver"]
