package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.RequiredParam
import ca.uhn.fhir.rest.annotation.Search
import ca.uhn.fhir.rest.param.ReferenceParam
import ca.uhn.fhir.rest.server.IResourceProvider
import ca.uhn.fhir.rest.server.exceptions.InvalidRequestException
import health.creda.bridge.cbor.EventPayloadCbor
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.AuditEvent
import org.hl7.fhir.r4.model.Reference
import org.springframework.stereotype.Component
import java.util.UUID

/**
 * The **disclosure** audit ledger (§4.3.3, §8.2.4). `GET AuditEvent?patient={id}` returns the
 * patient's `ExportReceipt` events — the signed, on-chain record of *what data moved, under which
 * Grant, to whom* — projected as FHIR `AuditEvent` ([AuditEventMapper], the FAST `$record-disclosure`
 * shape, §8.5.3). This is the non-repudiable half of audit and lives in the DAG; an empty result is
 * the honest answer until real `$creda-export` events exist (no fabricated ledger).
 *
 * The *other* half — read-side access logging ("who **queried** which subgraph", §8.2.4) — is NOT
 * here: it is captured by [BridgeAccessAuditInterceptor] and emitted to the institution's SIEM via
 * [AccessAuditSink], stored separately from the identity DAG as the spec requires.
 *
 * Translator-not-reasoner (§8.3.2): this only maps gRPC bytes to FHIR; subgraph materialization and
 * ordering happen in Core (`GetSubgraphEvents`). Events in the store have already passed mandatory
 * signature verification at ingest (§3.6).
 */
@Component
class AuditEventResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    override fun getResourceType(): Class<AuditEvent> = AuditEvent::class.java

    /** `GET AuditEvent?patient={id}` — the patient's disclosures (ExportReceipts), newest first. */
    @Search
    fun searchByPatient(@RequiredParam(name = AuditEvent.SP_PATIENT) patient: ReferenceParam): List<AuditEvent> {
        val patientId = patient.idPart.removePrefix("urn:uuid:")
        val entry = try {
            EventPayloadCbor.uuidBytes(UUID.fromString(patientId))
        } catch (e: IllegalArgumentException) {
            throw InvalidRequestException(
                "AuditEvent?patient= requires a subgraph entry-point UUID as the patient id, got '$patientId'",
            )
        }

        val disclosures = core.getSubgraphEvents(listOf(entry), listOf("ExportReceipt"))
            .filter { EventPayloadCbor.eventTypeOf(it) == "ExportReceipt" }
            .map { AuditEventMapper.fromExportReceiptCbor(it) }

        return AuditEventProjection.decorate(disclosures, patientId)
    }
}

/** Pure post-projection: tie each disclosure to the patient whose data moved, newest first. */
internal object AuditEventProjection {
    fun decorate(disclosures: List<AuditEvent>, patientId: String): List<AuditEvent> =
        disclosures
            .onEach {
                it.addEntity(
                    AuditEvent.AuditEventEntityComponent().setWhat(Reference("Patient/$patientId")),
                )
            }
            .sortedByDescending { it.recorded?.time ?: Long.MIN_VALUE }
}
