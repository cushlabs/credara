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
