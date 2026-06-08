# syntax=docker/dockerfile:1
#
# Testbed-only Bridge image. Uses the public Gradle+JDK image as the builder and the public
# Eclipse Temurin JRE as the runtime. The production image (deploy/docker/bridge.Dockerfile)
# targets Hummingbird OpenJDK FIPS for DQ-4; until those images publish, the testbed substitutes
# public bases.
#
# Build context = repo root:
#   docker build -f testbed/images/bridge.Dockerfile -t creda-bridge:testbed .

ARG GRADLE_BUILDER=docker.io/library/gradle:8.10-jdk21
ARG RUNTIME=docker.io/library/eclipse-temurin:21-jre-jammy

FROM ${GRADLE_BUILDER} AS builder
# CACHEBUST is a content hash of the bridge sources, supplied by build-and-load.sh. Consuming it
# in a RUN *before* the source COPY invalidates the COPY + Gradle layers whenever the bridge
# changes — defeating podman-machine's stale COPY-layer cache — while leaving them cached (fast)
# when nothing changed. Without this, a bridge edit can silently ship a stale jar.
ARG CACHEBUST=dev
RUN echo "bridge source hash: ${CACHEBUST}"
WORKDIR /src
COPY . .
WORKDIR /src/bridge
RUN gradle --no-daemon clean bootJar

FROM ${RUNTIME}
COPY --from=builder /src/bridge/build/libs/*.jar /app/creda-bridge.jar
EXPOSE 8080
# Non-root UID at the image level — matches the chart's runAsUser default (DQ-1).
USER 65532:65532
ENTRYPOINT ["java", "-jar", "/app/creda-bridge.jar"]
