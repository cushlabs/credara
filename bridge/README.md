# bridge — HAPI FHIR Bridge (M7)

The FHIR R4 integration surface. Java/Kotlin, built with Gradle.

**Governing spec sections:** §8 (FHIR Integration), §10.4 (HAPI FHIR Bridge).

Will contain: HAPI FHIR in **Plain Server** mode (never JPA — the event store is the source of
truth, no parallel relational store); custom resource providers (Patient, Provenance,
Authorization, AuditEvent); custom operations (`$creda-provenance`, `$creda-attest`,
`$creda-link`, `$creda-contest`, `$creda-tombstone`, `$creda-authorize`, `$creda-revoke`,
`$creda-verify`, `$creda-export`, `$creda-disambiguate` scaffold, `$creda-self-verify`); FHIR
profiles on US Core; the `_creda-token` SearchParameter; CapabilityStatement; Subscription;
Bulk Data export — all delegating to Creda Core over the in-pod gRPC socket.

**Assemble:** HAPI FHIR (do NOT write a FHIR server), the US Core IG, HAPI's `@Operation`
framework, validator, Subscription and Bulk Data support.
**Write:** thin resource providers, FHIR↔trust-event mapping, SMART-scope→Creda-operation mapping.

> **Critical constraint:** the Bridge is a TRANSLATOR, NOT A REASONER (§10.4.2). All identity
> logic, confidence computation, traversal, and authorization evaluation live in Creda Core.

## Status: scaffold (M7) — separate toolchain, not yet compiled

This is the one **Java/Kotlin** component (Spring Boot + HAPI FHIR R4 + grpc-java, built with
Gradle). It builds **separately** from the Rust workspace — `anchor creda` does not touch it — and
it was authored without a JDK in the build environment, so it has **not been compiled**.
Version-sensitive spots are marked `TODO(bridge-verify)`; reconcile them on first `gradle build`.

### Layout
- `build.gradle.kts` / `gradle.properties` — deps (HAPI, Spring Boot, grpc-java, netty UDS) and
  pinned versions; generates the gRPC Java stubs from the **shared** proto
  (`../crates/creda-core/proto/creda.proto`) — one contract, two languages.
- `src/main/kotlin/health/creda/bridge/`
  - `CredaBridgeApplication.kt` — Spring Boot entrypoint.
  - `FhirServerConfig.kt` — HAPI `RestfulServer` in **Plain Server** mode (§8.3.3) at `/fhir/*`.
  - `grpc/CredaCoreClient.kt` — thin gRPC client to Core over the in-pod **Unix domain socket**
    (§8.3.1); events cross as canonical-CBOR bytes.
  - `providers/` — `PatientResourceProvider` (read=project §8.1.1, search by `_creda-token`
    §8.2.11, create/delete rejected §8.3.3, `$creda-attest` §8.2.6), `AuthorizationResourceProvider`
    (`$creda-authorize`/`$creda-verify` §8.2.9), `ProvenanceResourceProvider` (events→Provenance
    §8.2.3, `$creda-contest`), `AuditEventResourceProvider` (read-side audit only §8.2.4).

### Translator-not-reasoner discipline
Every provider method does only FHIR↔gRPC mapping. The FHIR↔CBOR encoders/mappers
(`*PayloadEncoder`, `ProvenanceMapper`, `ConsentMapper`) and the remaining operations
(`$creda-provenance`/`link`/`tombstone`/`disambiguate`/`self-verify`/`$match`/`$export`,
Subscription→gossip §8.2.13, Bulk Data §8.2.14, CapabilityStatement customization §8.2.12, the
CredaPatient US-Core profile §8.2.2) follow the same thin pattern and are documented stubs in
this scaffold. They depend on Core exposing structured projections over gRPC (today
`GetEffectiveIdentity` returns a debug string and `EvaluateAuthorization` is unimplemented — see
`crates/creda-core/src/grpc.rs`), so completing them is paired with that Core gRPC follow-up.

### Build
Needs a JDK 21 + Gradle (not in the Rust dev image). CI builds it via `ci-java.yml`
(`actions/setup-java` + `gradle/actions/setup-gradle` → `gradle build`); the protobuf gradle
plugin fetches `protoc` and the grpc-java plugin from Maven, so no system protoc is required. The
**shipped** image is the Fedora Hummingbird OpenJDK base (DQ-4).
