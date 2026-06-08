package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.RequiredParam
import ca.uhn.fhir.rest.annotation.Search
import ca.uhn.fhir.rest.param.ReferenceParam
import ca.uhn.fhir.rest.server.IResourceProvider
import ca.uhn.fhir.rest.server.exceptions.InvalidRequestException
import health.creda.bridge.cbor.EventPayloadCbor
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.Consent
import org.springframework.stereotype.Component
import java.util.UUID

/**
 * Consent reads — the authorization read-back surface (§8.2.9). `GET Consent?patient={id}` lists
 * the patient's AuthorizationGrants projected as CredaAuthorization Consents, with status
 * `inactive` for any Grant a stored AuthorizationRevocation references (events in the store have
 * already passed mandatory signature verification at ingest, §3.6 — the same active/revoked
 * split as §4.6 steps 1–2, applied for display rather than enforcement).
 *
 * This is the resource-provider face of Consent; the Patient-scoped *operations*
 * ($creda-authorize / -revoke / -export / -verify) live in [AuthorizationResourceProvider], a
 * plain provider. Translator-not-reasoner (§8.3.2): this class only maps gRPC bytes to FHIR —
 * subgraph materialization and ordering happen in Core (GetSubgraphEvents).
 *
 * This search is also where the F1 FASTConsent projection will surface (§8.5.6): same query,
 * FASTConsent-conformant resources.
 */
@Component
class ConsentResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    override fun getResourceType(): Class<Consent> = Consent::class.java

    /** `GET Consent?patient={id}` — the patient's grants, revoked ones marked inactive. */
    @Search
    fun searchByPatient(@RequiredParam(name = Consent.SP_PATIENT) patient: ReferenceParam): List<Consent> {
        val patientId = patient.idPart.removePrefix("urn:uuid:")
        val entry = try {
            EventPayloadCbor.uuidBytes(UUID.fromString(patientId))
        } catch (e: IllegalArgumentException) {
            throw InvalidRequestException(
                "Consent?patient= requires a subgraph entry-point UUID as the patient id, got '$patientId'",
            )
        }

        val events = core.getSubgraphEvents(
            entryPoints = listOf(entry),
            eventTypes = listOf("AuthorizationGrant", "AuthorizationRevocation"),
        )

        val revokedGrantIds: Set<UUID> = events
            .filter { EventPayloadCbor.eventTypeOf(it) == "AuthorizationRevocation" }
            .map { EventPayloadCbor.decodeRevocationNode(it).targetGrantId }
            .toSet()

        return events
            .filter { EventPayloadCbor.eventTypeOf(it) == "AuthorizationGrant" }
            .map { cbor ->
                ConsentMapper.fromGrantCbor(cbor, patientId).apply {
                    if (UUID.fromString(idElement.idPart) in revokedGrantIds) {
                        status = Consent.ConsentState.INACTIVE
                    }
                }
            }
    }
}
