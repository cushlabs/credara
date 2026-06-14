package health.creda.bridge.providers

import ca.uhn.fhir.interceptor.api.Hook
import ca.uhn.fhir.interceptor.api.Interceptor
import ca.uhn.fhir.interceptor.api.Pointcut
import org.hl7.fhir.instance.model.api.IBaseConformance
import org.hl7.fhir.r4.model.CapabilityStatement
import org.springframework.stereotype.Component

/**
 * Declares the Credara IG and profiles on the auto-generated `CapabilityStatement` (§8.2.12).
 *
 * HAPI builds the base `metadata` statement from the registered providers' `@Operation`/`@Search`
 * (so the `$creda-*` operations and the `_creda-token` search param are already advertised). This
 * hook annotates that statement with what HAPI can't infer: the IG it implements and the Credara
 * profile each resource conforms to — so a conformance check or IG validator sees what the peer
 * actually implements (Patient→CredaPatient, Provenance→CredaProvenance, Consent→CredaAuthorization).
 * AuditEvent is intentionally left unprofiled: it's standard FHIR AuditEvent (the disclosure
 * projection is the FAST `$record-disclosure` shape, not a distinct Credara StructureDefinition).
 */
@Interceptor
@Component
class CredaCapabilityStatementInterceptor {

    @Hook(Pointcut.SERVER_CAPABILITY_STATEMENT_GENERATED)
    fun customize(theConformance: IBaseConformance) {
        val cs = theConformance as? CapabilityStatement ?: return
        cs.addImplementationGuide(IG)
        cs.setPublisher(PUBLISHER)
        for (rest in cs.rest) {
            for (resource in rest.resource) {
                PROFILES[resource.type]?.let { resource.setProfile(it) }
            }
        }
    }

    private companion object {
        const val IG = "http://credara.network/fhir/ig/v1"
        const val PUBLISHER = "Credara"
        const val SD = "http://credara.network/fhir/StructureDefinition"
        val PROFILES = mapOf(
            "Patient" to "$SD/CredaPatient",
            "Provenance" to "$SD/CredaProvenance",
            "Consent" to "$SD/CredaAuthorization",
        )
    }
}
