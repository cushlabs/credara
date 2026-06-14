package health.creda.bridge.providers

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.util.UUID

/**
 * `$creda-cleartext` (§9.2) integration seam. Contracts under test:
 *  - a provider may signal "no record here" by returning null (the operation turns that into a 404,
 *    and absence of any provider bean into a 501 — never a fabricated demographic);
 *  - [CleartextMapper] emits **real** (unmasked) demographics, scoped to the requested fields — this
 *    response is past the consent gate, so unlike `Patient/read` it is not masked.
 */
class CleartextProviderTest {

    private val demo = CleartextDemographics(
        family = "Whitfield",
        given = listOf("James", "A"),
        birthDate = "1971-08-04",
        addressText = "1 Mercy Way, Springfield",
    )

    @Test
    fun `a provider may report no cleartext for a patient by returning null`() {
        val empty = CleartextProvider { _, _ -> null }
        assertNull(empty.cleartext(UUID.randomUUID(), emptySet()))
    }

    @Test
    fun `mapper emits real unmasked demographics when no fields are scoped`() {
        val p = CleartextMapper.toPatient("patient-123", demo, emptySet())

        assertEquals("patient-123", p.idElement.idPart)
        val name = p.nameFirstRep
        assertEquals("Whitfield", name.family)
        assertEquals(listOf("James", "A"), name.given.map { it.value })
        assertFalse(name.hasExtension(), "cleartext name carries no data-absent-reason")
        assertTrue(p.birthDateElement.hasValue())
        assertEquals("1971-08-04", p.birthDateElement.valueAsString)
        assertEquals("1 Mercy Way, Springfield", p.addressFirstRep.text)
    }

    @Test
    fun `field scoping releases only what was asked for`() {
        val p = CleartextMapper.toPatient("p", demo, setOf("birthDate"))

        assertFalse(p.hasName(), "name not requested")
        assertFalse(p.hasAddress(), "address not requested")
        assertTrue(p.birthDateElement.hasValue())
        assertEquals("1971-08-04", p.birthDateElement.valueAsString)
    }

    @Test
    fun `mapper omits fields the institution did not supply`() {
        val partial = CleartextDemographics(birthDate = "1971-08-04")
        val p = CleartextMapper.toPatient("p", partial, emptySet())

        assertFalse(p.hasName())
        assertFalse(p.hasAddress())
        assertTrue(p.birthDateElement.hasValue())
    }
}
