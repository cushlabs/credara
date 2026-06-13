package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.Create
import ca.uhn.fhir.rest.annotation.IdParam
import ca.uhn.fhir.rest.annotation.Operation
import ca.uhn.fhir.rest.annotation.RequiredParam
import ca.uhn.fhir.rest.annotation.ResourceParam
import ca.uhn.fhir.rest.annotation.Search
import ca.uhn.fhir.rest.api.MethodOutcome
import ca.uhn.fhir.rest.param.ReferenceParam
import ca.uhn.fhir.rest.server.IResourceProvider
import ca.uhn.fhir.rest.server.exceptions.InvalidRequestException
import org.hl7.fhir.r4.model.DateTimeType
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.OperationOutcome
import org.hl7.fhir.r4.model.Reference
import org.hl7.fhir.r4.model.Task
import org.springframework.stereotype.Component
import java.util.Date
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/**
 * The OFF-CHAIN half of the hybrid access-request workflow (spec §4.3 design note). A requesting
 * institution asks for access to a patient by creating a `Task` (status=requested); the patient's
 * client lists pending Tasks and answers with an ON-CHAIN `AuthorizationGrant` ($creda-authorize),
 * then resolves the Task. The disclosure that follows is the existing on-chain `ExportReceipt`.
 *
 * Deliberately:
 *  - **Not a DAG event** and **not persisted** — an access request is transient coordination/intent,
 *    not identity, so it never pollutes the append-only identity graph (which can't forget a
 *    frivolous or spammy request) and never broadcasts "institution X is interested in patient Y"
 *    to every peer (the §13.3 value-privacy concern). The on-chain *answer* (the Grant) is what's
 *    auditable and portable.
 *  - **Ephemeral, lost on restart** — that's the point of off-chain.
 *  - **Single-bridge delivery only** for the pilot: the requester and patient reach the same bridge.
 *    Cross-peer off-chain delivery (an encrypted requester→patient channel) is a real-PHI design
 *    item, tracked separately; on-chain gossip is the alternative we explicitly chose not to use.
 *
 * This is the bridge's ONE piece of mutable state, and it holds no identity/authorization data of
 * record — Core remains the single source of truth for those (§8.3.3, translator-not-reasoner).
 */
@Component
class TaskResourceProvider : IResourceProvider {

    private data class Req(
        val id: String,
        val patientId: String,
        val requester: String,
        val purpose: String,
        val useMode: String,
        val authoredOn: Date,
    )

    private val pending = ConcurrentHashMap<String, Req>()

    override fun getResourceType(): Class<Task> = Task::class.java

    /** `POST /Task` — a requesting institution asks for access. `description` carries `purpose|use`. */
    @Create
    fun create(@ResourceParam task: Task): MethodOutcome {
        val patientId = task.getFor()?.reference?.removePrefix("Patient/")
            ?: throw InvalidRequestException("Task.for must reference the Patient access is requested for")
        val requester = task.requester?.display
            ?: throw InvalidRequestException("Task.requester.display (the requesting institution) is required")
        val parts = (task.description ?: "Treatment|Read & rely").split("|")
        val id = UUID.randomUUID().toString()
        val req = Req(
            id = id,
            patientId = patientId,
            requester = requester,
            purpose = parts.getOrElse(0) { "Treatment" },
            useMode = parts.getOrElse(1) { "Read & rely" },
            authoredOn = Date(),
        )
        pending[id] = req
        return MethodOutcome(IdType("Task", id)).apply { resource = toFhir(req) }
    }

    /** `GET /Task?patient={id}` — the patient's pending access requests. */
    @Search
    fun searchByPatient(@RequiredParam(name = Task.SP_PATIENT) patient: ReferenceParam): List<Task> {
        val pid = patient.idPart.removePrefix("urn:uuid:")
        return pending.values.filter { it.patientId == pid }.map { toFhir(it) }
    }

    /** `$creda-resolve-request` on a Task — drop it once the patient has granted or dismissed it. */
    @Operation(name = "\$creda-resolve-request", typeName = "Task", idempotent = false)
    fun resolve(@IdParam id: IdType): OperationOutcome {
        pending.remove(id.idPart)
        return OperationOutcome()
    }

    private fun toFhir(r: Req): Task = Task().apply {
        id = r.id
        status = Task.TaskStatus.REQUESTED
        intent = Task.TaskIntent.ORDER
        setFor(Reference("Patient/${r.patientId}"))
        requester = Reference().setDisplay(r.requester)
        authoredOnElement = DateTimeType(r.authoredOn)
        // `purpose|useMode` — the access the requester is asking for; the patient client splits it
        // and pre-fills the grant. Kept as a simple delimited string (this resource is bridge-internal
        // and ephemeral; no need for the full Task.input ceremony).
        description = "${r.purpose}|${r.useMode}"
    }
}
