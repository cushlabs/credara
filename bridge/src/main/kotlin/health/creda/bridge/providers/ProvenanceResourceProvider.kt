package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.IdParam
import ca.uhn.fhir.rest.annotation.Operation
import ca.uhn.fhir.rest.annotation.Read
import ca.uhn.fhir.rest.annotation.ResourceParam
import ca.uhn.fhir.rest.server.IResourceProvider
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.Parameters
import org.hl7.fhir.r4.model.Provenance
import org.springframework.stereotype.Component

/**
 * Each Creda identity event maps to a CredaProvenance resource (§8.2.3) — Provenance, not
 * AuditEvent, because the events are *constitutive* of the patient (§8.2.4). Thin translator:
 * `read` fetches one event from Core and maps it; `$creda-contest` writes a Contest event.
 */
@Component
class ProvenanceResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    override fun getResourceType(): Class<Provenance> = Provenance::class.java

    @Read
    fun read(@IdParam id: IdType): Provenance {
        val eventCbor = core.getEvent(id.idPart.toByteArray())
            ?: throw ca.uhn.fhir.rest.server.exceptions.ResourceNotFoundException(id)
        return ProvenanceMapper.fromEventCbor(eventCbor) // TODO(bridge-verify)
    }

    /** `$creda-contest` (§8.2.7): contest a Link Provenance. Party-of-subgraph is enforced in Core. */
    @Operation(name = "\$creda-contest")
    fun contest(@IdParam linkId: IdType, @ResourceParam params: Parameters): Provenance {
        val payloadCbor = encodeContest(params)
        val eventCbor = core.createEvent(payloadCbor, listOf(linkId.idPart.toByteArray()))
        return ProvenanceMapper.fromEventCbor(eventCbor)
    }
}

internal fun encodeContest(@Suppress("UNUSED_PARAMETER") params: Parameters): ByteArray =
    TODO("bridge-verify: encode a Contest EventPayload (reason) as canonical CBOR")
