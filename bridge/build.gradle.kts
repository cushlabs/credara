import com.google.protobuf.gradle.id

// Creda FHIR Bridge build (M7, spec §8, §10.4).
//
// TODO(bridge-verify): this project was authored without a JDK in the build environment, so it
// has not been compiled. The version-sensitive surface is the HAPI FHIR / grpc-java / protobuf
// dependency set and the protobuf-gradle-plugin codegen block below — reconcile against the
// pinned versions in gradle.properties on first `./gradlew build`.

val hapiVersion: String by project
val grpcVersion: String by project
val protobufVersion: String by project
val protobufPluginVersion: String by project
val nettyVersion: String by project

plugins {
    kotlin("jvm") version "1.9.24"
    kotlin("plugin.spring") version "2.3.21"
    id("org.springframework.boot") version "3.3.2"
    id("io.spring.dependency-management") version "1.1.6"
    id("com.google.protobuf") version "0.9.4"
}

group = "health.creda"
version = "0.1.0"

java {
    toolchain { languageVersion.set(JavaLanguageVersion.of(21)) }
}

repositories { mavenCentral() }

dependencies {
    implementation("org.springframework.boot:spring-boot-starter-web")
    implementation("org.jetbrains.kotlin:kotlin-reflect")

    // HAPI FHIR R4 — Plain Server mode (NOT JPA, §8.3.3). The Bridge is a translator, not a
    // reasoner (§8.3.2): all identity logic lives in Creda Core.
    implementation("ca.uhn.hapi.fhir:hapi-fhir-base:$hapiVersion")
    implementation("ca.uhn.hapi.fhir:hapi-fhir-structures-r4:$hapiVersion")
    implementation("ca.uhn.hapi.fhir:hapi-fhir-server:$hapiVersion")
    // US Core / IG validation support (CredaPatient conforms to US Core Patient, §8.2.1).
    implementation("ca.uhn.hapi.fhir:hapi-fhir-validation:$hapiVersion")

    // gRPC client to Creda Core over a Unix domain socket (§8.3.1). netty epoll/kqueue provides
    // the UDS transport.
    implementation("io.grpc:grpc-netty:$grpcVersion")
    implementation("io.grpc:grpc-protobuf:$grpcVersion")
    implementation("io.grpc:grpc-stub:$grpcVersion")
    implementation("com.google.protobuf:protobuf-java:$protobufVersion")
    implementation("io.netty:netty-transport-native-epoll:$nettyVersion:linux-x86_64")
    implementation("io.netty:netty-transport-native-epoll:$nettyVersion:linux-aarch_64")
    // Needed for the @Generated annotation grpc-java emits when compiled with newer JDKs.
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

protobuf {
    protoc { artifact = "com.google.protobuf:protoc:$protobufVersion" }
    plugins {
        id("grpc") { artifact = "io.grpc:protoc-gen-grpc-java:$grpcVersion" }
    }
    generateProtoTasks {
        all().forEach { task ->
            task.plugins { id("grpc") }
        }
    }
}

tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile> {
    kotlinOptions { jvmTarget = "21" }
}
