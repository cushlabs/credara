import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// Creda FHIR Bridge build (M7, spec §8, §10.4).
//
// Dependency set upgraded 2026-08-19: Spring Boot 3.3.2 -> 4.1.0, HAPI FHIR 7.2.0 -> 8.10.1,
// Kotlin 1.9.24 -> 2.4.10, grpc-java 1.66.0 -> 1.83.1, Netty 4.1 -> 4.2. `gradle build` is GREEN
// on this set — compile, bootJar, and the full test suite — on Gradle 8.14.5 / JDK 21.
//
// RESIDUAL RISK, Spring Framework 7: HAPI FHIR 8.10.1 is built and CI-tested against Spring
// Framework 6.2.18 / Spring Boot 3.5.14; there is no Spring 7 work in HAPI's tree. The Bridge
// compiles and its tests pass on Spring Framework 7, but that exercises only the paths the tests
// cover. hapi-fhir-server's Spring surface (HttpEntity/HttpHeaders/ResponseEntity,
// ServletServerHttpRequest, Message/MessageHeaders, RestTemplate, web.cors.*, UriComponents*) is
// not fully exercised until the server serves real traffic. If a RUNTIME failure lands inside HAPI
// on a Spring type, that is the first place to look; the fallback is Spring Boot 3.5.9, which
// keeps HAPI 8.10.1 and Kotlin 2.4.10 intact.

val hapiVersion: String by project
val grpcVersion: String by project
val protobufVersion: String by project
val protobufPluginVersion: String by project
val nettyVersion: String by project

plugins {
    kotlin("jvm") version "2.4.10"
    kotlin("plugin.spring") version "2.4.10"
    id("org.springframework.boot") version "4.1.0"
    id("io.spring.dependency-management") version "1.1.7"
    id("com.google.protobuf") version "0.9.6"
}

group = "health.creda"
version = "0.1.0"

java {
    toolchain { languageVersion.set(JavaLanguageVersion.of(21)) }
}

repositories { mavenCentral() }

// Realign Spring Boot 4.1.0's managed versions where its BOM is internally inconsistent for our
// use, or simply behind. `io.spring.dependency-management` reads these `extra` properties, so
// setting them moves the whole transitive BOM (grpc-api, grpc-core, ...) — not just the artifacts
// declared explicitly below. Without this, grpc-netty would be 1.83.1 while its own grpc-core came
// in at Boot's managed 1.80.0.
extra["grpc-java.version"] = grpcVersion
extra["protobuf-java.version"] = protobufVersion
extra["netty.version"] = nettyVersion

dependencies {
    // Boot 4.x renamed this starter: spring-boot-starter-web is deprecated in favour of
    // spring-boot-starter-webmvc. Same contents (Tomcat + Spring MVC + Jackson).
    implementation("org.springframework.boot:spring-boot-starter-webmvc")
    implementation("org.jetbrains.kotlin:kotlin-reflect")

    // Spring Boot 4 defaults to Jackson 3 (`tools.jackson`). HAPI FHIR 8.10.1 is Jackson 2 only
    // (`com.fasterxml.jackson`, zero `tools.jackson` references anywhere in its tree), so the two
    // majors must coexist. They have different Maven coordinates AND different Java packages, so
    // this is a supported configuration, not a clash. Declared without a version: Boot 4.1.0's BOM
    // still manages the Jackson 2 line (jackson-2-bom 2.21.4).
    //
    // Deliberately NOT using `org.springframework.boot:spring-boot-jackson2` — that module exists
    // to restore the deprecated Jackson 2 *auto-configuration* (ObjectMapper bean, spring.jackson2.*
    // properties). The Bridge has no Jackson config of its own and no spring.jackson.* properties in
    // application.yml; HAPI needs the Jackson 2 *classes*, nothing more. Jackson 3 stays the mapper
    // for the FHIR REST surface.
    implementation("com.fasterxml.jackson.core:jackson-databind")

    // HAPI FHIR R4 — Plain Server mode (NOT JPA, §8.3.3). The Bridge is a translator, not a
    // reasoner (§8.3.2): all identity logic lives in Creda Core.
    implementation("ca.uhn.hapi.fhir:hapi-fhir-base:$hapiVersion")
    implementation("ca.uhn.hapi.fhir:hapi-fhir-structures-r4:$hapiVersion")
    implementation("ca.uhn.hapi.fhir:hapi-fhir-server:$hapiVersion")
    // US Core / IG validation support (CredaPatient conforms to US Core Patient, §8.2.1).
    implementation("ca.uhn.hapi.fhir:hapi-fhir-validation:$hapiVersion")

    // gRPC client to Creda Core over a Unix domain socket (§8.3.1). netty epoll/kqueue provides
    // the UDS transport. See gradle.properties for why grpc is pinned to 1.83.1 specifically.
    implementation("io.grpc:grpc-netty:$grpcVersion")
    implementation("io.grpc:grpc-protobuf:$grpcVersion")
    implementation("io.grpc:grpc-stub:$grpcVersion")
    implementation("com.google.protobuf:protobuf-java:$protobufVersion")
    implementation("io.netty:netty-transport-native-epoll:$nettyVersion:linux-x86_64")
    implementation("io.netty:netty-transport-native-epoll:$nettyVersion:linux-aarch_64")

    // Canonical CBOR codec for the wire-shape exchange with Core. EventPayload travels as
    // RFC 8949 deterministic CBOR (§3.4, see crates/creda-events/src/canonical.rs); the
    // upokecenter library supports the deterministic-encoding spec out of the box and is the
    // standard JVM choice for this. Used by AttestPayloadEncoder + ProvenanceMapper.
    // Held at 4.5.4 — untouched by this upgrade, and the golden-vector CBOR tests pin its output.
    implementation("com.upokecenter:cbor:4.5.4")
    // Needed for the @Generated annotation grpc-java emits when compiled with newer JDKs.
    // Likely dead weight now: Boot 4.1's ProtobufPluginAction passes `@generated=omit` to
    // protoc-gen-grpc-java, so the annotation should no longer be emitted at all. Kept until the
    // build is confirmed green — dropping it is a one-line follow-up, not worth coupling to this
    // upgrade.
    compileOnly("org.apache.tomcat:annotations-api:6.0.53")

    testImplementation("org.springframework.boot:spring-boot-starter-test")
}

// Generate the gRPC Java stubs from the SHARED proto that Creda Core (Rust) also compiles —
// one contract, two languages (§10.1.3).
sourceSets {
    main {
        proto {
            srcDir("../crates/creda-core/proto")
        }
    }
}

// Spring Boot 4.1's Gradle plugin reacts to `com.google.protobuf` being applied
// (ProtobufPluginAction) and ALREADY does three things this block used to do itself:
//
//   1. sets protoc's artifact to `com.google.protobuf:protoc`  (versionless)
//   2. creates the `grpc` ExecutableLocator as `io.grpc:protoc-gen-grpc-java`  (versionless)
//   3. registers the `grpc` plugin on every GenerateProtoTask, with option `@generated=omit`
//
// Doing (2) again is what failed the first build after this upgrade — "Cannot add a
// ExecutableLocator with name 'grpc' as a ExecutableLocator with that name already exists". So this
// block now CONFIGURES what Boot created instead of creating it, and (3) is dropped entirely
// because Boot's configureEach already covers every task.
//
// The explicit versions are deliberate, not redundant. Boot leaves both artifacts versionless and
// back-fills the version from the *runtime classpath*: protoc from `com.google.protobuf:protobuf-java`
// (which we do declare), and protoc-gen-grpc-java from `io.grpc:grpc-util` (which we do NOT — we
// declare grpc-netty/-protobuf/-stub). When that lookup finds nothing the alignment silently
// no-ops and the dependency is left requesting version "null". Pinning both here removes any
// dependence on that back-fill.
protobuf {
    protoc { artifact = "com.google.protobuf:protoc:$protobufVersion" }
    plugins {
        named("grpc") { artifact = "io.grpc:protoc-gen-grpc-java:$grpcVersion" }
    }
}

// Kotlin 2.x: `kotlinOptions` is @Deprecated(level = ERROR) and no longer compiles in a build
// script. `compilerOptions` at the extension level is the replacement and covers all compilations
// (main + test) rather than needing a tasks.withType block.
kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_21)
    }
}
