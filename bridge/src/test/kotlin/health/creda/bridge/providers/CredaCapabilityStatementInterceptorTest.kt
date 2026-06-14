package health.creda.bridge.providers

import org.hl7.fhir.r4.model.CapabilityStatement
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/**
 * The CapabilityStatement annotation (§8.2.12): the IG is declared and each resource is stamped
 * with its Credara profile, while a resource without a Credara profile (AuditEvent) is left alone.
 */
class CredaCapabilityStatementInterceptorTest {

    @Test
    fun `stamps the IG and Credara profiles, leaving AuditEvent unprofiled`() {
        val cs = CapabilityStatement().apply {
            addRest().apply {
                addResource().setType("Patient")
                addResource().setType("Provenance")
                addResource().setType("Consent")
                addResource().setType("AuditEvent")
            }
        }

        CredaCapabilityStatementInterceptor().customize(cs)

        assertTrue(
            cs.implementationGuide.any { it.value == "http://credara.network/fhir/ig/v1" },
            "declares the Credara IG",
        )
        val byType = cs.restFirstRep.resource.associateBy { it.type }
        assertEquals(
            "http://credara.network/fhir/StructureDefinition/CredaPatient",
            byType.getValue("Patient").profile,
        )
        assertEquals(
            "http://credara.network/fhir/StructureDefinition/CredaProvenance",
            byType.getValue("Provenance").profile,
        )
        assertEquals(
            "http://credara.network/fhir/StructureDefinition/CredaAuthorization",
            byType.getValue("Consent").profile,
        )
        assertFalse(byType.getValue("AuditEvent").hasProfile(), "AuditEvent has no Credara profile")
    }

    @Test
    fun `a statement with no rest resources is annotated without throwing`() {
        val cs = CapabilityStatement()
        CredaCapabilityStatementInterceptor().customize(cs)
        assertTrue(cs.implementationGuide.any { it.value == "http://credara.network/fhir/ig/v1" })
    }
}
