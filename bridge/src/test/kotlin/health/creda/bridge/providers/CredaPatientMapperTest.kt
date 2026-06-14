package health.creda.bridge.providers

import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.Enumerations
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.util.UUID

/**
 * CredaPatient projection (§8.2.2). Verifies the honest privacy contract: a valid US Core Patient
 * with the mustSupport extensions and the structural identity the Bridge legitimately holds, with
 * name/DOB MASKED (data-absent-reason) because cleartext is not at the Bridge (§9.2).
 */
class CredaPatientMapperTest {

    private val base = "http://credara.network"
    private val dataAbsent = "http://hl7.org/fhir/StructureDefinition/data-absent-reason"

    private fun field(key: String, value: String, confidence: Int = 9000, disputed: Boolean = false) =
        CredaCoreClient.EffectiveField(
            key = key,
            disputed = disputed,
            values = listOf(
                CredaCoreClient.EffectiveValue(value = value, confidence = confidence, supporting = emptyList()),
            ),
        )

    @Test
    fun `projects a US Core Patient with masked PHI, real gender, identifiers and extensions`() {
        val rootA = UUID.fromString("00000000-0000-0000-0000-0000000000a1")
        val rootB = UUID.fromString("00000000-0000-0000-0000-0000000000a2")
        val last = UUID.fromString("00000000-0000-0000-0000-0000000000ff")
        val identity = CredaCoreClient.SubgraphIdentity(
            subgraphId = byteArrayOf(0xDE.toByte(), 0xAD.toByte(), 0xBE.toByte(), 0xEF.toByte()),
            rootSet = listOf(rootA, rootB),
            lastModifiedEvent = last,
        )
        val fields = listOf(
            field("sex", "female"),
            field("name-family", "tok:demo:whitfield", confidence = 9300),
            field("date-of-birth", "tok:demo:1971-08-04", confidence = 9000, disputed = true),
            field("mrn", "tok:demo:Mercy General\u001Ftok:demo:5582019"),
        )

        val p = CredaPatientMapper.project("patient-123", identity, fields)

        // CredaPatient profile + the three §8.2.2 mustSupport extensions.
        assertTrue(p.meta.profile.any { it.value == "$base/fhir/StructureDefinition/CredaPatient" })
        assertEquals(
            "deadbeef",
            p.getExtensionByUrl("$base/StructureDefinition/subgraph-identifier").value.primitiveValue(),
        )
        assertEquals(2, p.getExtensionsByUrl("$base/StructureDefinition/root-set").size)
        assertEquals(
            last.toString(),
            p.getExtensionByUrl("$base/StructureDefinition/last-modified-event").value.primitiveValue(),
        )

        // Stable subgraph identifier + the de-tokenized MRN, as identifiers.
        assertTrue(p.identifier.any { it.system == "$base/identifier/subgraph" && it.value == "deadbeef" })
        assertTrue(p.identifier.any { it.value == "5582019" })

        // Gender is real (not masked).
        assertEquals(Enumerations.AdministrativeGender.FEMALE, p.gender)

        // Name + birthDate are MASKED: present, but data-absent-reason and no fabricated value.
        val name = p.nameFirstRep
        assertFalse(name.hasFamily())
        assertFalse(name.hasGiven())
        assertEquals("masked", name.getExtensionByUrl(dataAbsent).value.primitiveValue())
        assertFalse(p.birthDateElement.hasValue())
        assertEquals("masked", p.birthDateElement.getExtensionByUrl(dataAbsent).value.primitiveValue())
        // The DOB dispute flag from Core rode along on the masked element.
        assertEquals(
            "true",
            p.birthDateElement.getExtensionByUrl("$base/StructureDefinition/disputed-value").value.primitiveValue(),
        )
    }

    @Test
    fun `gender falls back to unknown for an unexpected token`() {
        val identity = CredaCoreClient.SubgraphIdentity(byteArrayOf(1), emptyList(), null)
        val p = CredaPatientMapper.project("p", identity, listOf(field("sex", "intersex")))
        assertEquals(Enumerations.AdministrativeGender.UNKNOWN, p.gender)
    }
}
