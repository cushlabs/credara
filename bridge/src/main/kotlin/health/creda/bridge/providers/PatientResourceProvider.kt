package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.Create
import ca.uhn.fhir.rest.annotation.Delete
import ca.uhn.fhir.rest.annotation.IdParam
import ca.uhn.fhir.rest.annotation.Operation
import ca.uhn.fhir.rest.annotation.OptionalParam
import ca.uhn.fhir.rest.annotation.Read
import ca.uhn.fhir.rest.annotation.ResourceParam
import ca.uhn.fhir.rest.annotation.Search
import ca.uhn.fhir.rest.param.TokenAndListParam
import ca.uhn.fhir.rest.server.IResourceProvider
import ca.uhn.fhir.rest.server.exceptions.MethodNotAllowedException
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.Parameters
import org.hl7.fhir.r4.model.Patient
import org.hl7.fhir.r4.model.Provenance
import org.springframework.stereotype.Component

/**
 * Patient is a **projection, not a record** (§8.1.1). This provider translates FHIR Patient
 * operations into Creda Core gRPC calls and back — it holds no identity logic (§8.3.2).
 *
 * Demonstrates the full translator pattern: `read`/`search` project from Core; direct
 * `create`/`delete` are rejected (clients must use the `$creda-*` write operations); and the
 * `$creda-*` operations are thin wrappers over Core RPCs.
 *
 * TODO(bridge-verify): mapping a structured CredaPatient (US Core + extensions, §8.2.2) requires
 * Core's `GetEffectiveIdentity` to return structured data (it returns a debug rendering today,
 * see creda-core/src/grpc.rs). Until then `read` returns a minimal Patient shell.
 */
@Component
class PatientResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    override fun getResourceType(): Class<Patient> = Patient::class.java

    /** read = project the effective identity for this subgraph (§5.2.4). */
    @Read
    fun read(@IdParam id: IdType): Patient {
        val debug = core.effectiveIdentityDebug(listOf(id.idPart.toByteArray()))
        // TODO(bridge-verify): build a CredaPatient (US Core Patient + subgraph-identifier slice,
        // per-field confidence + disputed-value extensions, §8.1.2-§8.1.4) once Core returns a
        // structured projection. For now, surface the projection text as a stub.
        return Patient().apply {
            this.id = id.idPart
            addExtension("http://creda.health/StructureDefinition/effective-identity-debug", null)
            this.text.div.setValueAsString("<div>$debug</div>")
        }
    }

    /** search by demographic token (§8.2.11): `Patient?_creda-token=...` -> MatchByTokens. */
    @Search
    fun searchByToken(
        @OptionalParam(name = "_creda-token") tokens: TokenAndListParam?,
    ): List<Patient> {
        val tokenValues = tokens
            ?.valuesAsQueryTokens
            ?.flatMap { it.valuesAsQueryTokens }
            ?.map { it.value }
            ?: emptyList()
        return core.matchByTokens(tokenValues).map { idBytes ->
            Patient().apply { id = bytesToUuidString(idBytes) }
        }
    }

    /** Patient is a projection — direct create is rejected; use the `$creda-*` write operations. */
    @Create
    fun create(@ResourceParam patient: Patient): Nothing =
        throw MethodNotAllowedException(
            "Patient is a projection, not a writable resource (§8.3.3). " +
                "Use \$creda-attest / \$creda-link / \$creda-authorize, etc.",
        )

    /** Patient deletion is a tombstone operation, not a DELETE (§8.3.3). */
    @Delete
    fun delete(@IdParam id: IdType): Nothing =
        throw MethodNotAllowedException("Patient deletion is the \$creda-tombstone operation.")

    /**
     * `$creda-attest` (§8.2.6): record an Attest event on the patient's chain — the most common
     * FHIR-side write. Representative of the operation set; translates Parameters -> Core
     * CreateEvent and returns the resulting CredaProvenance.
     */
    @Operation(name = "\$creda-attest")
    fun attest(@IdParam id: IdType, @ResourceParam params: Parameters): Provenance {
        val payloadCbor = AttestPayloadEncoder.encode(id, params) // TODO(bridge-verify): real encoder
        val eventCbor = core.createEvent(payloadCbor, listOf(id.idPart.toByteArray()))
        return ProvenanceMapper.fromEventCbor(eventCbor) // TODO(bridge-verify): real mapper
    }

    // The remaining Patient operations follow the SAME thin pattern — translate Parameters to a
    // Core CreateEvent (or query) and map the response to FHIR — and are intentionally left as
    // documented stubs here (§8.2.5-§8.2.10):
    //   $creda-provenance (GET)   -> Core GetSubgraph -> Bundle<CredaProvenance>
    //   $creda-link / $creda-contest / $creda-tombstone -> Core CreateEvent
    //   $match            -> Core MatchByTokens (scored candidates)
    //   $creda-disambiguate / $creda-self-verify -> Core disambiguation RPCs (scaffolded)
    //   $export           -> Core + Bulk Data NDJSON (§8.2.14)
}

/** Placeholders for the FHIR<->CBOR mapping helpers (real implementations are M7 follow-ups). */
internal object AttestPayloadEncoder {
    fun encode(@Suppress("UNUSED_PARAMETER") id: IdType, @Suppress("UNUSED_PARAMETER") params: Parameters): ByteArray =
        TODO("bridge-verify: encode an Attest EventPayload as canonical CBOR for Core")
}

internal object ProvenanceMapper {
    fun fromEventCbor(@Suppress("UNUSED_PARAMETER") eventCbor: ByteArray): Provenance =
        TODO("bridge-verify: map a Creda event (CBOR) to a CredaProvenance resource (§8.2.3)")
}

internal fun bytesToUuidString(@Suppress("UNUSED_PARAMETER") idBytes: ByteArray): String =
    TODO("bridge-verify: format 16 UUID bytes as a canonical UUID string")
