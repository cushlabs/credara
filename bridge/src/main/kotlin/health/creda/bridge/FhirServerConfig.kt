package health.creda.bridge

import ca.uhn.fhir.context.FhirContext
import ca.uhn.fhir.rest.server.RestfulServer
import health.creda.bridge.providers.AuditEventResourceProvider
import health.creda.bridge.providers.AuthorizationResourceProvider
import health.creda.bridge.providers.PatientResourceProvider
import health.creda.bridge.providers.ProvenanceResourceProvider
import org.springframework.boot.web.servlet.ServletRegistrationBean
import org.springframework.context.annotation.Bean
import org.springframework.context.annotation.Configuration
import org.springframework.stereotype.Component

/**
 * Registers HAPI FHIR's [RestfulServer] in **Plain Server** mode (§8.3.3) at `/fhir/*`, with the
 * custom resource providers. HAPI auto-generates the [org.hl7.fhir.r4.model.CapabilityStatement]
 * from the providers' `@Operation`/`@Search` annotations, advertising the Creda operations and
 * the `_creda-token` search parameter (§8.2.12).
 *
 * TODO(bridge-verify): the ServletRegistrationBean wiring and CapabilityStatement customization
 * are HAPI/Spring-version-sensitive.
 */
@Configuration
class FhirServerConfig {
    @Bean
    fun fhirServletRegistration(
        server: CredaRestfulServer,
    ): ServletRegistrationBean<CredaRestfulServer> =
        ServletRegistrationBean(server, "/fhir/*").apply { setName("fhir") }
}

@Component
class CredaRestfulServer(
    private val patient: PatientResourceProvider,
    private val provenance: ProvenanceResourceProvider,
    private val authorization: AuthorizationResourceProvider,
    private val auditEvent: AuditEventResourceProvider,
) : RestfulServer(FhirContext.forR4()) {

    override fun initialize() {
        super.initialize()
        // Plain Server: providers translate FHIR <-> Core gRPC; no JPA, no parallel store.
        setResourceProviders(patient, provenance, authorization, auditEvent)
        // TODO(bridge-verify): attach a custom ServerCapabilityStatementProvider that declares
        // `CapabilityStatement.implementationGuide = http://creda.health/fhir/ig/v1` and the
        // Creda profiles (CredaPatient/CredaProvenance/CredaAuthorization, §8.2.12).
    }
}
