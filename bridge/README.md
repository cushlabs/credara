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

## Status: M7 scaffold — builds green ✓ (logic stubs are follow-ups)

This is the one **Java/Kotlin** component (Spring Boot + HAPI FHIR R4 + grpc-java, built with
Gradle). It builds **separately** from the Rust workspace — `anchor creda` does not touch it; use
`make bridge`. `gradle build` is **green** (compile, tests, and `bootJar`; the gRPC stubs generate
from the shared proto). The FHIR↔CBOR encoders/mappers and the remaining operations are runtime
`TODO(bridge-verify)` stubs (they throw `TODO()` until implemented), so the build compiles but
those operations are not yet functional — see below.

> **Dependency upgrade, 2026-08-19.** Spring Boot 3.3.2 → **4.1.0**, HAPI FHIR 7.2.0 → **8.10.1**,
> Kotlin 1.9.24 → **2.4.10**, grpc-java 1.66.0 → **1.83.1**, Netty 4.1 → **4.2**, Gradle 8.10 →
> **8.14.5**. Motive: Spring Boot 3.3.x left OSS support in June 2025 and every 3.x branch is now
> unsupported, so the module was running on an unpatched web framework.
>
> Exactly **one** source change was needed across the whole module: `Consent.provisionComponent` →
> `Consent.ProvisionComponent` in `AuthorizationResourceProvider.kt`. That is a FHIR R4 model
> rename in `org.hl7.fhir.core` (6.1.2.2 → 6.9.12, pulled in by HAPI 8), which corrected the casing
> on that one generated class — `provisionActorComponent` and `provisionDataComponent` are still
> lowercase, so do not "fix" those. Every other HAPI and Spring symbol the Bridge imports is
> unchanged across both hops.
>
> Residual risk is documented at the top of `build.gradle.kts`: HAPI 8.10.1 is CI-tested against
> Spring Framework 6.2.18, not the Spring Framework 7 that Boot 4.1 brings. Compile and tests pass;
> that is not the same as proven at runtime. Fallback if a runtime failure lands inside HAPI on a
> Spring type: Spring Boot 3.5.9, keeping HAPI 8.10.1 and Kotlin 2.4.10.

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
Every provider method does only FHIR↔gRPC mapping.

**Implemented (F0, §8.5.6):** the authorization FHIR↔CBOR mappers and the five authorization
operations. `EventPayloadCbor` now encodes/decodes the four authorization payloads
(`AuthorizationGrant`, `AuthorizationRevocation`, `ExportReceipt`, and `TPODisclosure` — the grant-less §4.3.5 disclosure) as canonical CBOR, and
`AuthorizationResourceProvider` wires `$creda-authorize` → Consent, `$creda-revoke` → Consent
(inactive), `$creda-export` → AuditEvent, `$creda-tpo-disclose` → AuditEvent (the grant-less §4.3.5
disclosure), and `$creda-verify` → decision. `AuditEvent?patient=` returns both ExportReceipt and
TPODisclosure disclosures. Wire shapes are pinned
by golden-vector tests (`src/test/.../AuthorizationPayloadCborTest.kt`) generated from an
independent CBOR oracle to match serde+ciborium 0.2.2 exactly — notably `Uuid` as a 16-byte byte
string but `Vec<u8>` fingerprints as a **CBOR array of ints** (the one easy mistake). This also
fixed a latent bug in `decodeEventNode`, which read those `Vec<u8>` fields as byte strings.

**Still stubs:** the remaining encoders/mappers (`ProvenanceMapper`) and operations
(`$creda-provenance`/`link`/`tombstone`/`disambiguate`/`self-verify`/`$match`, Subscription→gossip
§8.2.13, Bulk Data §8.2.14, CapabilityStatement customization §8.2.12, the CredaPatient US-Core
profile §8.2.2) follow the same thin pattern and are documented stubs in this scaffold. The FHIR
projection here is the minimal-but-faithful CredaAuthorization shape; the FASTConsent-conformant
projection (grantee/controller/manager, FASTReference) is F1 (§8.5.6). `$creda-verify` depends on
Core's `EvaluateAuthorization` gRPC (the engine path exists; the gRPC wiring is a Core follow-up —
see `crates/creda-core/src/grpc.rs`).

### Build
Needs a JDK 21 + **Gradle 8.14 or later** (not in the Rust dev image). The Gradle floor is hard:
the Spring Boot 4.1 plugin throws below 8.14. Gradle 9.x also works now — the Boot 3.3.2
`getDirMode()` incompatibility that forced the old 8.10.2 pin is gone — but `ci-java.yml` stays on
8.14.3 so the framework bump moves one variable at a time.

CI builds it via `ci-java.yml` (`actions/setup-java` + `gradle/actions/setup-gradle` →
`gradle build`); the protobuf gradle plugin fetches `protoc` and the grpc-java plugin from Maven,
so no system protoc is required. The **shipped** image is the Fedora Hummingbird OpenJDK base
(DQ-4).

Two dependency-pairing constraints are load-bearing and explained in `gradle.properties`:

- **grpc-java is pinned to 1.83.1, above Boot's managed 1.80.0.** Boot 4.1.0's BOM ships Netty
  4.2.15.Final but pins grpc-bom to 1.80.0, which is a Netty-4.1 build. grpc-java 1.83.x is the
  first line built against Netty 4.2, so moving grpc up lands on Boot's Netty exactly instead of
  dragging Netty back down. The `extra["grpc-java.version"]` override in `build.gradle.kts` moves
  the whole transitive BOM, not just the artifacts declared by name.
- **Jackson 2 and Jackson 3 both sit on the classpath.** Boot 4 defaults to Jackson 3
  (`tools.jackson`); HAPI FHIR 8.10.1 is Jackson 2 only (`com.fasterxml.jackson`). Different Maven
  coordinates *and* different Java packages, so they coexist by design. Only the Jackson 2
  classes are pulled in — not `spring-boot-jackson2`, which exists for the deprecated Jackson 2
  auto-configuration the Bridge does not use.

`io.netty.channel.epoll.EpollEventLoopGroup` (used by `grpc/CredaCoreClient.kt`) is deprecated in
Netty 4.2 in favour of `MultiThreadIoEventLoopGroup` + `EpollIoHandler.newFactory()`. It still
exists and still extends `MultiThreadIoEventLoopGroup`, so behaviour is unchanged; expect a
deprecation warning and treat the migration as a separate follow-up.
