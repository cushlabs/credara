package health.creda.bridge.providers

import org.hl7.fhir.r4.model.Address
import org.hl7.fhir.r4.model.DateType
import org.hl7.fhir.r4.model.Patient
import java.util.UUID

/**
 * The institution-side integration seam for cleartext demographics (§9.2).
 *
 * Credara **never stores cleartext** — only tokens traverse the network. Cleartext lives solely in
 * the originating institution's own systems (EHR / MPI). So `$creda-cleartext`, after the consent
 * gate authorizes a requester, delegates the actual cleartext lookup to whatever the deploying
 * institution wires up here. Provide a Spring bean implementing this interface that reads from your
 * source of truth, keyed by the Credara subgraph entry-point id (map it to your local patient id).
 *
 * No bean at all ⇒ `$creda-cleartext` **fails loudly** with `501` (deployment not integrated). A
 * bean that returns `null` for a given patient ⇒ `404` (integrated, but no cleartext held here for
 * that record). Neither path ever fabricates a demographic — there is no Credara-side cleartext to
 * fall back on ("no silent fakes", §9.2).
 */
fun interface CleartextProvider {
    /**
     * Cleartext demographics for a locally-held patient, restricted to [fields] (empty = all the
     * caller asked for). Return `null` if this institution has no cleartext source for the patient.
     */
    fun cleartext(patientId: UUID, fields: Set<String>): CleartextDemographics?
}

/** Cleartext demographic fields an institution may release under consent (§9.2). */
data class CleartextDemographics(
    val family: String? = null,
    val given: List<String> = emptyList(),
    /** ISO-8601 date, e.g. `1971-08-04`. */
    val birthDate: String? = null,
    val addressText: String? = null,
)

/** Builds the authorized-cleartext FHIR Patient response (real values — this is past the gate). */
object CleartextMapper {
    fun toPatient(patientId: String, d: CleartextDemographics, fields: Set<String>): Patient {
        fun wants(f: String) = fields.isEmpty() || fields.contains(f)
        return Patient().apply {
            id = patientId
            if (wants("name") && (d.family != null || d.given.isNotEmpty())) {
                addName().apply {
                    d.family?.let { family = it }
                    d.given.forEach { addGiven(it) }
                }
            }
            if (wants("birthDate") && d.birthDate != null) {
                birthDateElement = DateType(d.birthDate)
            }
            if (wants("address") && d.addressText != null) {
                addAddress(Address().setText(d.addressText))
            }
        }
    }
}
