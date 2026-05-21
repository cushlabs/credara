package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.IdParam
import ca.uhn.fhir.rest.annotation.Operation
import ca.uhn.fhir.rest.annotation.ResourceParam
import ca.uhn.fhir.rest.server.IResourceProvider
import health.creda.bridge.grpc.CredaCoreClient
import health.creda.grpc.GrantPurpose
import health.creda.grpc.UseMode
import org.hl7.fhir.r4.model.Consent
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.Parameters
import org.springframework.stereotype.Component

/**
 * Portable authorization through FHIR (§8.2.9). The CredaAuthorization profile is based on FHIR
 * Consent (§4, §9.3). These operations are thin translators over Creda Core: `$creda-authorize`
 * / `$creda-revoke` map to Core CreateEvent (AuthorizationGrant / AuthorizationRevocation);
 * `$creda-verify` maps to Core's authorization evaluation (§4.6); `$creda-export` records an
 * ExportReceipt (usually invoked by the Export Gate, §10.2). No authorization logic lives here.
 */
@Component
class AuthorizationResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    override fun getResourceType(): Class<Consent> = Consent::class.java

    /** `$creda-authorize` (§8.2.9): create an AuthorizationGrant from the Parameters. */
    @Operation(name = "\$creda-authorize")
    fun authorize(@IdParam patient: IdType, @ResourceParam params: Parameters): Consent {
        val grantPayloadCbor = encodeGrant(params) // TODO(bridge-verify)
        val eventCbor = core.createEvent(grantPayloadCbor, listOf(patient.idPart.toByteArray()))
        return ConsentMapper.fromGrantCbor(eventCbor) // TODO(bridge-verify)
    }

    /**
     * `$creda-verify` (§8.2.9): run Core's authorization evaluation for a requesting institution
     * and return a decision (`authorized` / `denied-revoked` / `denied-expired` / ...) plus the
     * governing Grant. Because the Verifier is local (§10.3.3), this may be served from stale
     * state, in which case the response includes the DAG view's age.
     *
     * NOTE: Core's gRPC EvaluateAuthorization wiring is a follow-up; the engine path is implemented.
     */
    @Operation(name = "\$creda-verify")
    fun verify(@IdParam patient: IdType, @ResourceParam params: Parameters): Parameters {
        val q = parseAuthQuery(params) // TODO(bridge-verify)
        val reply = core.evaluateAuthorization(
            entryPoints = listOf(patient.idPart.toByteArray()),
            requesterFingerprint = q.requesterFingerprint,
            purpose = q.purpose,
            useMode = q.useMode,
        )
        return Parameters().apply {
            addParameter().setName("decision")
                .setValue(org.hl7.fhir.r4.model.CodeType(if (reply.authorized) "authorized" else "denied"))
            addParameter().setName("reason").setValue(org.hl7.fhir.r4.model.StringType(reply.reason))
        }
    }

    // Same thin pattern, left as documented stubs (§8.2.9):
    //   $creda-revoke  -> Core CreateEvent (AuthorizationRevocation referencing a prior Grant)
    //   $creda-export  -> Core CreateEvent (ExportReceipt); typically called by the Export Gate
}

internal object ConsentMapper {
    fun fromGrantCbor(@Suppress("UNUSED_PARAMETER") cbor: ByteArray): Consent =
        TODO("bridge-verify: map an AuthorizationGrant event to a CredaAuthorization (Consent)")
}

internal fun encodeGrant(@Suppress("UNUSED_PARAMETER") params: Parameters): ByteArray =
    TODO("bridge-verify: encode an AuthorizationGrant EventPayload as canonical CBOR")

/** The structured pieces of a `$creda-verify` request, parsed from the FHIR Parameters. */
internal data class AuthQuery(
    val requesterFingerprint: ByteArray,
    val purpose: GrantPurpose,
    val useMode: UseMode,
)

internal fun parseAuthQuery(@Suppress("UNUSED_PARAMETER") params: Parameters): AuthQuery =
    TODO("bridge-verify: parse requester fingerprint / purpose / use-mode from the \$creda-verify Parameters")
