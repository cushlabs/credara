# Creda FHIR Bridge build image (M7) — Fedora + OpenJDK + Gradle.
#
# Build/prod parity (DQ-6): the bridge builds on the same OS family it ships on (Fedora
# Hummingbird OpenJDK FIPS, DQ-4). Hummingbird itself is distroless (no Gradle, no shell), so the
# *build* image is a plain Fedora base with OpenJDK + Gradle — the same dev-vs-shipped split used
# for the Rust side (Fedora dev image, Hummingbird distroless shipped).
#
# Fallback: `make bridge-stock` uses the prebuilt Debian-based gradle image if this ever hiccups.
ARG BASE=registry.fedoraproject.org/fedora:41
FROM ${BASE}

ARG GRADLE_VERSION=8.10

# OpenJDK 21 + the small tools needed to fetch and unpack the Gradle distribution.
RUN dnf -y install java-21-openjdk-devel curl ca-certificates unzip findutils which \
    && dnf clean all

# Install Gradle from the official distribution (Fedora's packaged Gradle can lag); pin the
# version so the build is reproducible.
RUN curl -fsSL "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" \
        -o /tmp/gradle.zip \
    && unzip -q /tmp/gradle.zip -d /opt \
    && ln -s "/opt/gradle-${GRADLE_VERSION}/bin/gradle" /usr/local/bin/gradle \
    && rm /tmp/gradle.zip

ENV JAVA_HOME=/usr/lib/jvm/java-21-openjdk
WORKDIR /work/bridge
