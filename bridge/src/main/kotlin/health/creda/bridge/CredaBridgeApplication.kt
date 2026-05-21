package health.creda.bridge

import org.springframework.boot.autoconfigure.SpringBootApplication
import org.springframework.boot.runApplication

/**
 * Creda FHIR Bridge (spec §8, §10.4): a Spring Boot application embedding HAPI FHIR's
 * `RestfulServer` in **Plain Server** mode (§8.3.3 — never JPA; Creda Core's event store is the
 * single source of truth).
 *
 * The Bridge is a **translator, not a reasoner** (§8.3.2): every FHIR request maps to a Creda
 * Core gRPC call and every Core response maps back to a FHIR resource. It contains no identity
 * logic — confidence, traversal, signature verification, authorization evaluation all live in
 * Core.
 *
 * NOTE: this module is the Java/Kotlin toolchain. It builds separately from the Rust workspace
 * (`anchor creda`) via Gradle, and was authored without a JDK available, so it has not been
 * compiled. `TODO(bridge-verify)` markers flag the HAPI/grpc-java version-sensitive spots.
 */
@SpringBootApplication
class CredaBridgeApplication

fun main(args: Array<String>) {
    runApplication<CredaBridgeApplication>(*args)
}
