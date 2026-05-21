# syntax=docker/dockerfile:1
#
# HAPI FHIR Bridge image (M7/M8, spec §10.6.1). Multi-stage: Gradle build → Fedora Hummingbird
# OpenJDK FIPS runtime (DQ-4 — the spec's generic distroless-java, §10.6.1, specialized to
# Hummingbird). Runs non-root (DQ-1).
#
# Build context is the REPO ROOT (the bridge generates its gRPC stubs from the shared proto under
# crates/creda-core/proto):
#   docker build -f deploy/docker/bridge.Dockerfile -t creda-bridge:dev .
#
# TODO(DQ-4): pin the exact Hummingbird OpenJDK image references (registry path + digest).
ARG GRADLE_BUILDER=docker.io/library/gradle:8.10-jdk21
ARG RUNTIME=registry.fedoraproject.org/hummingbird/openjdk21-nonroot:fips

FROM ${GRADLE_BUILDER} AS builder
WORKDIR /src
COPY . .
WORKDIR /src/bridge
RUN gradle --no-daemon clean bootJar

FROM ${RUNTIME}
# Distroless Java: just the JRE + the app jar. Non-root by base-image default (DQ-1).
COPY --from=builder /src/bridge/build/libs/*.jar /app/creda-bridge.jar
EXPOSE 8080
# Hummingbird OpenJDK base provides the JRE; entrypoint runs the Spring Boot fat jar.
ENTRYPOINT ["java", "-jar", "/app/creda-bridge.jar"]
