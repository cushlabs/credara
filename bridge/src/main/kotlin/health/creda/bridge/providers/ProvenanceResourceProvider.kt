package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.IdParam
import ca.uhn.fhir.rest.annotation.Operation
import ca.uhn.fhir.rest.annotation.Read
import ca.uhn.fhir.rest.annotation.ResourceParam
import ca.uhn.fhir.rest.server.IResourceProvider
import ca.uhn.fhir.rest.server.exceptions.InvalidRequestException
import health.creda.bridge.cbor.EventPayloadCbor
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.Parameters
import org.hl7.fhir.r4.model.Provenance
import org.springframework.stereotype.Component
import java.util.UUID

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
        val uuid = try {
            UUID.fromString(id.idPart)
        } catch (_: IllegalArgumentException) {
            throw ca.uhn.fhir.rest.server.exceptions.ResourceNotFoundException(id)
        }
        val eventCbor = core.getEvent(EventPayloadCbor.uuidBytes(uuid))
            ?: throw ca.uhn.fhir.rest.server.exceptions.ResourceNotFoundException(id)
        return ProvenanceMapper.fromEventCbor(eventCbor)
    }

    /** `$creda-contest` (§8.2.7): contest a Link Provenance. Party-of-subgraph is enforced in Core. */
    @Operation(name = "\$creda-contest")
    fun contest(@IdParam linkId: IdType, @ResourceParam params: Parameters): Provenance {
        val targetLinkUuid = try {
            UUID.fromString(linkId.idPart)
        } catch (_: IllegalArgumentException) {
            throw ca.uhn.fhir.rest.server.exceptions.ResourceNotFoundException(linkId)
        }
        // ContestReason {code, detail?} (§3.4.3). `code` is a kebab ContestReasonCode; `detail`
        // is optional free text. For backward compatibility a caller that sends only the legacy
        // free-text `reason` is treated as code=other with that text as the detail.
        fun param(name: String): String? =
            params.parameter.firstOrNull { it.name == name }?.value?.primitiveValue()
        val codeParam = param("code")
        val code = codeParam ?: "other"
        if (code !in EventPayloadCbor.CONTEST_REASON_CODES) {
            throw InvalidRequestException(
                "contest 'code' must be one of ${EventPayloadCbor.CONTEST_REASON_CODES}",
            )
        }
        val detail = param("detail") ?: if (codeParam == null) param("reason") else null

        val payloadCbor = EventPayloadCbor.encodeContest(targetLinkUuid, code, detail)
        val parentBytes = EventPayloadCbor.uuidBytes(targetLinkUuid)
        val eventCbor = core.createEvent(payloadCbor, listOf(parentBytes))
        return ProvenanceMapper.fromEventCbor(eventCbor)
    }
}
