package health.creda.bridge

import ca.uhn.fhir.context.FhirContext
import ca.uhn.fhir.rest.server.RestfulServer
import health.creda.bridge.providers.AuditEventResourceProvider
import health.creda.bridge.providers.AuthorizationResourceProvider
import health.creda.bridge.providers.ConsentResourceProvider
import health.creda.bridge.providers.OrganizationResourceProvider
import health.creda.bridge.providers.PatientResourceProvider
import health.creda.bridge.providers.TaskResourceProvider
import health.creda.bridge.providers.ProvenanceResourceProvider
import org.springframework.boot.web.servlet.ServletRegistrationBean
import org.springframework.context.annotation.Bean
import org.springframework.context.annotation.Configuration
import org.springframework.stereotype.Component

/**
 * Registers HAPI FHIR's [RestfulServer] in **Plain Server** mode (§8.3.3) on the `/fhir` base
 * path, with the custom resource providers. HAPI auto-generates the [org.hl7.fhir.r4.model.CapabilityStatement]
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
    private val consent: ConsentResourceProvider,
    private val organization: OrganizationResourceProvider,
    private val task: TaskResourceProvider,
    private val authorization: AuthorizationResourceProvider,
    // AuditEventResourceProvider intentionally NOT injected — see initialize() for why.
) : RestfulServer(FhirContext.forR4()) {

    private val log = org.slf4j.LoggerFactory.getLogger(CredaRestfulServer::class.java)

    /**
     * Spring's lazy servlet init can call `initialize()` more than once under load — the
     * first request after startup triggers init, and any concurrent in-flight requests can
     * trigger a second pass before the first completes. Without a guard, each pass calls
     * `setResourceProviders(...)` which **adds to** (does not replace) the registered set;
     * HAPI then logs `registered twice` for every method and, depending on the pass order,
     * fails the second init with a misleading ConfigurationException. Guard with a single
     * AtomicBoolean so init is idempotent regardless of how Spring calls us.
     */
    private val initialized = java.util.concurrent.atomic.AtomicBoolean(false)

    override fun initialize() {
        super.initialize()
        if (!initialized.compareAndSet(false, true)) return
        // Plain Server: providers translate FHIR <-> Core gRPC; no JPA, no parallel store.
        //
        // AuditEventResourceProvider is *intentionally not registered yet*: HAPI's
        // RestfulServer rejects any IResourceProvider that has zero annotated methods with
        // HAPI-0289, and AuditEventResourceProvider is a deferred-work stub today (the
        // auditing interceptor that would populate it is an M-?? follow-up). When the
        // interceptor lands and the provider gains real @Read / @Search methods, add it
        // back to this call.
        setResourceProviders(patient, provenance, consent, organization, task)
        // AuthorizationResourceProvider is a *plain* provider, not a resource provider: its
        // operations are Patient-typed (@Operation typeName="Patient") but it cannot be a second
        // Patient IResourceProvider (PatientResourceProvider already is — HAPI forbids two for one
        // type). registerProvider is HAPI's path for operation-only providers that attach to an
        // existing resource type; without this the ops 404/400 as "No methods exist for resource".
        registerProvider(authorization)
        log.info(
            "Creda RestfulServer initialized — 5 resource providers (Patient, Provenance, Consent, Organization, Task) + " +
                "1 plain provider (authorization ops on Patient); AuditEvent deferred until interceptor lands",
        )
        // TODO(bridge-verify): attach a custom ServerCapabilityStatementProvider that declares
        // `CapabilityStatement.implementationGuide = http://credara.network/fhir/ig/v1` and the
        // Creda profiles (CredaPatient/CredaProvenance/CredaAuthorization, §8.2.12).
    }
}
