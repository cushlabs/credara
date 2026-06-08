package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.Create
import ca.uhn.fhir.rest.annotation.Delete
import ca.uhn.fhir.rest.annotation.IdParam
import ca.uhn.fhir.rest.annotation.Operation
import ca.uhn.fhir.rest.annotation.OptionalParam
import ca.uhn.fhir.rest.annotation.Read
import ca.uhn.fhir.rest.annotation.ResourceParam
import ca.uhn.fhir.rest.annotation.Search
import ca.uhn.fhir.rest.api.MethodOutcome
import ca.uhn.fhir.rest.param.TokenAndListParam
import ca.uhn.fhir.rest.server.IResourceProvider
import ca.uhn.fhir.rest.server.exceptions.MethodNotAllowedException
import health.creda.bridge.cbor.EventPayloadCbor
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.Parameters
import org.hl7.fhir.r4.model.Patient
import org.hl7.fhir.r4.model.Provenance
import org.springframework.stereotype.Component
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/**
 * Patient is a **projection, not a record** (§8.1.1). This provider translates FHIR Patient
 * operations into Creda Core gRPC calls and back — it holds no identity logic (§8.3.2).
 *
 * `read` / `search` project from Core; direct `create` / `delete` are rejected (§8.3.3);
 * the `$creda-*` operations are thin wrappers over Core RPCs.
 *
 * ## Bridge-seeded root Asserts (demo shim)
 *
 * Identity events other than `Assert` must have parents (§3.4). The M-clients persona UIs
 * carry *projection-level* event identifiers (e.g. `e3`) that are not real Core UUIDs — they
 * are mock-mode IDs. To let the UI's Attest button produce a real event on the gossip mesh
 * without the UI having to discover a Core UUID first, this provider keeps an in-memory
 * `patientId → rootEventId` map. When an Attest comes in and we don't yet have a root for
 * the patient, we synthesize a minimal root Assert (empty Demographics, self-report
 * verification) and pin its returned id. Subsequent Attests for the same patient reuse it.
 *
 * This shim is **demo-only** and will be removed when the bridge can derive a real subgraph
 * head from the patient identifier via Core's MatchByTokens or GetSubgraph. The map is
 * process-local (ConcurrentHashMap) so it does not survive a pod restart — fine for UAT,
 * unacceptable for production.
 */
@Component
class PatientResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    /** patientId (free-form, as the UI sends it) → root Assert event UUID in Core. */
    private val rootByPatient: ConcurrentHashMap<String, UUID> = ConcurrentHashMap()

    override fun getResourceType(): Class<Patient> = Patient::class.java

    /** read = project the effective identity for this subgraph (§5.2.4). */
    @Read
    fun read(@IdParam id: IdType): Patient {
        // Project the effective identity from Core. TODO(bridge-verify): build a structured
        // CredaPatient (US Core Patient + subgraph-identifier slice, per-field confidence +
        // disputed-value extensions, §8.1.2-§8.1.4) once Core returns a structured projection;
        // today GetEffectiveIdentity returns a debug rendering (see creda-core/src/grpc.rs).
        core.effectiveIdentityDebug(listOf(uuidOrPatientPlaceholder(id.idPart)))
        val patient = Patient()
        patient.id = id.idPart
        return patient
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
            Patient().apply { id = EventPayloadCbor.bytesToUuid(idBytes).toString() }
        }
    }

    /**
     * Patient is a projection — direct create is rejected; use the `$creda-*` write operations.
     *
     * **Why block-body and not `: MethodOutcome = throw …`** — the Kotlin compiler erases the
     * declared return type to `Nothing` (which compiles to `java.lang.Void` in JVM bytecode)
     * when the expression-body form contains nothing but a `throw`. HAPI's `@Create` registrar
     * reflects on the return type via `Method.getReturnType()` and rejects anything that
     * isn't `MethodOutcome`-shaped, surfacing the misleading
     * `HAPI-0394 ... returns java.lang.Void` error. Block body + explicit return type =
     * unambiguous JVM signature `()Lca/uhn/fhir/rest/api/MethodOutcome;`.
     */
    @Create
    fun create(@ResourceParam patient: Patient): MethodOutcome {
        throw MethodNotAllowedException(
            "Patient is a projection, not a writable resource (§8.3.3). " +
                "Use \$creda-attest / \$creda-link / \$creda-authorize, etc.",
        )
    }

    /** Patient deletion is a tombstone operation, not a DELETE (§8.3.3). */
    @Delete
    fun delete(@IdParam id: IdType): MethodOutcome {
        throw MethodNotAllowedException("Patient deletion is the \$creda-tombstone operation.")
    }

    /**
     * `$creda-attest` (§8.2.6): record an Attest event on the patient's chain. The UI sends a
     * patient id and a purpose; the bridge ensures a root Assert exists for that patient (see
     * class doc), then creates the Attest linked to it.
     */
    @Operation(name = "\$creda-attest")
    fun attest(@IdParam id: IdType, @ResourceParam params: Parameters): Provenance {
        val purpose = params.parameterFirstRep("purpose")?.lowercase() ?: "treatment"

        // Step 1 — make sure there is a real Core event we can use as a parent.
        val rootId = rootByPatient.computeIfAbsent(id.idPart) { _ ->
            val rootCbor = EventPayloadCbor.encodeRootAssertStub()
            val rootEventBytes = core.createEvent(rootCbor, parentIds = emptyList())
            EventPayloadCbor.decodeEventNode(rootEventBytes).id
        }

        // Step 2 — append the Attest, linking it to the root Assert.
        val attestPayload = EventPayloadCbor.encodeAttest(
            targetEventIds = listOf(rootId),
            purpose = purpose,
        )
        val parentBytes = EventPayloadCbor.uuidBytes(rootId)
        val eventCbor = core.createEvent(attestPayload, listOf(parentBytes))

        return ProvenanceMapper.fromEventCbor(eventCbor)
    }

    // The remaining Patient operations follow the SAME thin pattern — translate Parameters to a
    // Core CreateEvent (or query) and map the response to FHIR — and are intentionally left as
    // documented stubs here (§8.2.5-§8.2.10):
    //   $creda-provenance (GET)   -> Core GetSubgraph -> Bundle<CredaProvenance>
    //   $creda-link / $creda-tombstone -> Core CreateEvent
    //   $match            -> Core MatchByTokens (scored candidates)
    //   $creda-disambiguate / $creda-self-verify -> Core disambiguation RPCs (scaffolded)
    //   $export           -> Core + Bulk Data NDJSON (§8.2.14)

    /**
     * If the FHIR `Patient/<id>` reference is itself a valid UUID, return its 16-byte form;
     * otherwise hash the projection id into a stable 16-byte placeholder so the gRPC layer
     * has something well-formed to send. The placeholder is only used by `read`'s
     * debug-projection call today.
     */
    private fun uuidOrPatientPlaceholder(idPart: String): ByteArray =
        try {
            EventPayloadCbor.uuidBytes(UUID.fromString(idPart))
        } catch (_: IllegalArgumentException) {
            // Stable placeholder per patient id — NOT a real Core event id.
            val src = "creda-patient-placeholder:$idPart".toByteArray()
            val out = ByteArray(16)
            for ((i, b) in src.withIndex()) out[i % 16] = (out[i % 16].toInt() xor b.toInt()).toByte()
            out
        }
}

/** Helper: first valueString of a named FHIR Parameters parameter, or null. */
private fun Parameters.parameterFirstRep(name: String): String? =
    parameter.firstOrNull { it.name == name }?.value?.primitiveValue()
