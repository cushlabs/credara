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
import ca.uhn.fhir.rest.server.exceptions.InvalidRequestException
import ca.uhn.fhir.rest.server.exceptions.MethodNotAllowedException
import ca.uhn.fhir.rest.server.exceptions.ResourceNotFoundException
import health.creda.bridge.cbor.EventPayloadCbor
import health.creda.bridge.grpc.CredaCoreClient
import org.hl7.fhir.r4.model.Bundle
import org.hl7.fhir.r4.model.CodeType
import org.hl7.fhir.r4.model.CodeableConcept
import org.hl7.fhir.r4.model.Coding
import org.hl7.fhir.r4.model.DateType
import org.hl7.fhir.r4.model.Enumerations
import org.hl7.fhir.r4.model.Extension
import org.hl7.fhir.r4.model.HumanName
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.Parameters
import org.hl7.fhir.r4.model.Patient
import org.hl7.fhir.r4.model.Provenance
import org.hl7.fhir.r4.model.StringType
import org.hl7.fhir.r4.model.UnsignedIntType
import org.springframework.stereotype.Component
import java.util.UUID

/**
 * Patient is a **projection, not a record** (§8.1.1). This provider translates FHIR Patient
 * operations into Creda Core gRPC calls and back — it holds no identity logic (§8.3.2) and no
 * mutable state: every operation resolves against real Core events. `read` / `search` project from
 * Core; direct `create` / `delete` are rejected (§8.3.3); the `$creda-*` operations are thin
 * wrappers over Core RPCs. A patient id is always a real subgraph entry-point event UUID (resolved
 * by clients via `Patient?_creda-token=`), so writes attach to the real subgraph, never a stub.
 */
@Component
class PatientResourceProvider(
    private val core: CredaCoreClient,
) : IResourceProvider {

    override fun getResourceType(): Class<Patient> = Patient::class.java

    /**
     * read = the CredaPatient projection (§8.2.2): a valid US Core Patient carrying the Credara
     * `mustSupport` extensions (subgraph identifier, root set, last-modified event) plus the
     * structural identity the Bridge legitimately holds — institutional MRN identifiers, the
     * subgraph identifier, and **gender** (the one demographic that is not tokenized). Name, DOB,
     * and address are emitted **masked** (FHIR `data-absent-reason`): cleartext is deliberately not
     * at the Bridge (privacy by structure, §9.2) and is fetched out-of-band via the consent-gated
     * `$creda-cleartext` operation. This is honest — never a fabricated value.
     */
    @Read
    fun read(@IdParam id: IdType): Patient {
        val patientUuid = try {
            UUID.fromString(id.idPart)
        } catch (e: IllegalArgumentException) {
            throw InvalidRequestException(
                "Patient/read requires a subgraph entry-point UUID, got '${id.idPart}'",
            )
        }
        val entry = EventPayloadCbor.uuidBytes(patientUuid)
        val identity = core.subgraphIdentity(listOf(entry))
        val fields = core.effectiveIdentity(listOf(entry))
        // No events reachable from this entry point → the patient does not exist here.
        if (identity.rootSet.isEmpty() && identity.lastModifiedEvent == null && fields.isEmpty()) {
            throw ResourceNotFoundException(id)
        }
        return CredaPatientMapper.project(id.idPart, identity, fields)
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
     * `Patient/$match` (FHIR Patient `$match`): scored identity matching. The query Patient carries
     * **tokenized** demographics (cleartext never reaches the Bridge, §9.2) — name tokens in `name`,
     * other field tokens as identifiers under the match-token system. Core blocks on any token hit
     * (MatchByTokens); each candidate is then scored by real per-field token agreement against its
     * effective identity ([PatientMatcher]) and returned as a CredaPatient with `search.score` + the
     * FHIR `match-grade` extension, best first. Honors `count` and `onlyCertainMatches`. The scoring
     * weights/thresholds are uncalibrated (§5.3.2) — the ranking is on real agreement, not fabricated.
     */
    @Operation(name = "\$match")
    fun match(@ResourceParam params: Parameters): Bundle {
        val query = queryTokens(params)
        if (query.isEmpty()) {
            throw InvalidRequestException(
                "\$match requires tokenized demographics on the query Patient: name tokens, or " +
                    "identifiers under '$MATCH_TOKEN_SYS<field>'",
            )
        }
        val limit = params.parameterFirstRep("count")?.toIntOrNull()
        val onlyCertain = params.parameterFirstRep("onlyCertainMatches")?.toBoolean() ?: false

        val ranked = core.matchByTokens(query.values.toList())
            .mapNotNull { idBytes ->
                val fields = core.effectiveIdentity(listOf(idBytes))
                val candidate = fields
                    .filter { it.values.isNotEmpty() }
                    .associate { it.key to it.values.first().value }
                val llr = PatientMatcher.logLikelihoodRatio(query, candidate)
                val grade = PatientMatcher.grade(llr)
                if (grade == "certainly-not" || (onlyCertain && grade != "certain")) {
                    null
                } else {
                    val uuid = EventPayloadCbor.bytesToUuid(idBytes)
                    val identity = core.subgraphIdentity(listOf(idBytes))
                    Match(CredaPatientMapper.project(uuid.toString(), identity, fields), PatientMatcher.score01(llr), grade)
                }
            }
            .sortedByDescending { it.score }
            .let { if (limit != null && limit >= 0) it.take(limit) else it }

        return Bundle().apply {
            type = Bundle.BundleType.SEARCHSET
            total = ranked.size
            ranked.forEach { m ->
                addEntry().apply {
                    resource = m.patient
                    search.apply {
                        mode = Bundle.SearchEntryMode.MATCH
                        setScore(java.math.BigDecimal.valueOf(m.score).setScale(3, java.math.RoundingMode.HALF_UP))
                        addExtension(Extension(MATCH_GRADE_EXT).setValue(CodeType(m.grade)))
                    }
                }
            }
        }
    }

    private data class Match(val patient: Patient, val score: Double, val grade: String)

    /** Per-field query tokens from the input Patient: name from `name`, others from match-token ids. */
    private fun queryTokens(params: Parameters): Map<String, String> {
        val patient = params.parameter.firstOrNull { it.name == "resource" }?.resource as? Patient
            ?: return emptyMap()
        val tokens = linkedMapOf<String, String>()
        patient.nameFirstRep.let { n ->
            if (n.hasFamily()) tokens["name-family"] = n.family
            n.given.firstOrNull()?.value?.let { tokens["name-given"] = it }
        }
        for (id in patient.identifier) {
            val sys = id.system ?: continue
            if (sys.startsWith(MATCH_TOKEN_SYS) && id.hasValue()) {
                tokens[sys.removePrefix(MATCH_TOKEN_SYS)] = id.value
            }
        }
        return tokens
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
     * `$creda-attest` (§8.2.6): record an Attest event affirming reliance on real events.
     *
     * The Attest targets the events named in `references` (the Asserts/Links being affirmed) —
     * e.g. the clinician DOB-resolution attests the supporting Assert so its confidence rises in the
     * effective identity. Targets are also the parents, so the Attest lands INSIDE the patient's
     * subgraph. When no `references` are supplied, the Attest affirms the patient's subgraph entry
     * point itself (the id is a real event UUID); the entry point must exist in Core, or this 404s.
     * No synthesized stubs and no per-process state — every write attaches to a real event.
     */
    @Operation(name = "\$creda-attest")
    fun attest(@IdParam id: IdType, @ResourceParam params: Parameters): Provenance {
        val purpose = params.parameterFirstRep("purpose")?.lowercase() ?: "treatment"
        val parents: List<UUID> = attestReferences(params).ifEmpty {
            val entry = try {
                UUID.fromString(id.idPart)
            } catch (e: IllegalArgumentException) {
                throw InvalidRequestException(
                    "\$creda-attest needs a 'references' target, or a subgraph entry-point UUID " +
                        "as the patient id; got '${id.idPart}'",
                )
            }
            if (core.getEvent(EventPayloadCbor.uuidBytes(entry)) == null) {
                throw ResourceNotFoundException(id)
            }
            listOf(entry)
        }

        val attestPayload = EventPayloadCbor.encodeAttest(targetEventIds = parents, purpose = purpose)
        val eventCbor = core.createEvent(attestPayload, parents.map { EventPayloadCbor.uuidBytes(it) })
        return ProvenanceMapper.fromEventCbor(eventCbor)
    }

    /**
     * Extract target event UUIDs from the `references` parameter(s). Tolerant by construction: the
     * value may arrive as proper repeated params, a `Provenance/<uuid>` reference, or (legacy) a
     * single JSON-stringified array `["<uuid>", …]` — we regex every UUID out of whatever form
     * shows up, so the operation is robust to client encoding drift.
     */
    private fun attestReferences(params: Parameters): List<UUID> {
        val uuidRe = Regex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        return params.parameter
            .filter { it.name == "references" }
            .mapNotNull { it.value?.primitiveValue() }
            .flatMap { raw -> uuidRe.findAll(raw).mapNotNull { runCatching { UUID.fromString(it.value) }.getOrNull() } }
            .distinct()
    }

    // The remaining Patient operations follow the SAME thin pattern — translate Parameters to a
    // Core CreateEvent (or query) and map the response to FHIR — and are intentionally left as
    // documented stubs here (§8.2.5-§8.2.10):
    //   $creda-provenance (GET)   -> Core GetSubgraph -> Bundle<CredaProvenance>
    //   $creda-link / $creda-tombstone -> Core CreateEvent
    //   $creda-disambiguate / $creda-self-verify -> Core disambiguation RPCs (scaffolded)
    //   $export           -> Core + Bulk Data NDJSON (§8.2.14)
}

/** Helper: first valueString of a named FHIR Parameters parameter, or null. */
private fun Parameters.parameterFirstRep(name: String): String? =
    parameter.firstOrNull { it.name == name }?.value?.primitiveValue()

/** System under which a `Patient/$match` query carries non-name field tokens (e.g. `…/date-of-birth`). */
private const val MATCH_TOKEN_SYS = "http://credara.network/fhir/sid/match-token/"

/** FHIR match-grade extension on `Bundle.entry.search` (value from the match-grade CodeSystem). */
private const val MATCH_GRADE_EXT = "http://hl7.org/fhir/StructureDefinition/match-grade"

/**
 * Projects a subgraph's §8.2.2 identity envelope + effective identity into a **CredaPatient** — a
 * valid US Core Patient. Pure (no gRPC), so it is unit-testable.
 *
 * What it carries and why:
 *  - the three `mustSupport` extensions (subgraph identifier, root set, last-modified event) and the
 *    subgraph identifier as a stable `Patient.identifier`;
 *  - institutional **MRNs** as identifiers (these are identifiers, not cleartext demographics);
 *  - **gender**, the one demographic that is a plain enum rather than a token (§5.3.1);
 *  - **name / birthDate masked** via FHIR `data-absent-reason` — cleartext is deliberately not at
 *    the Bridge (§9.2) and is retrieved out-of-band via the consent-gated `$creda-cleartext` op.
 *    Core's per-field confidence and dispute flag ride along as extensions so a consumer knows how
 *    trustworthy the eventual cleartext is. Nothing here is ever a fabricated value.
 */
internal object CredaPatientMapper {
    private const val BASE = "http://credara.network"
    private const val PROFILE = "$BASE/fhir/StructureDefinition/CredaPatient"
    private const val SUBGRAPH_SYSTEM = "$BASE/identifier/subgraph"
    private const val EXT_SUBGRAPH_ID = "$BASE/StructureDefinition/subgraph-identifier"
    private const val EXT_ROOT_SET = "$BASE/StructureDefinition/root-set"
    private const val EXT_LAST_MODIFIED = "$BASE/StructureDefinition/last-modified-event"
    private const val EXT_FIELD_CONFIDENCE = "$BASE/StructureDefinition/field-confidence"
    private const val EXT_DISPUTED = "$BASE/StructureDefinition/disputed-value"
    private const val DATA_ABSENT = "http://hl7.org/fhir/StructureDefinition/data-absent-reason"
    private const val MRN_SYSTEM_PREFIX = "$BASE/identifier/mrn/"
    private val DETOKEN = Regex("^tok:[^:]+:(.+)$")
    private const val UNIT_SEP = "\u001F"

    fun project(
        patientId: String,
        identity: CredaCoreClient.SubgraphIdentity,
        fields: List<CredaCoreClient.EffectiveField>,
    ): Patient {
        val p = Patient()
        p.id = patientId
        p.meta.addProfile(PROFILE)

        val subgraphHex = identity.subgraphId.joinToString("") { "%02x".format(it) }
        // §8.2.2 subgraph identifier — both a stable cross-institution Patient.identifier and the
        // mustSupport extension. Root set + last-modified event are the other two mustSupport exts.
        p.addIdentifier().setSystem(SUBGRAPH_SYSTEM).setValue(subgraphHex)
        p.addExtension(Extension(EXT_SUBGRAPH_ID).setValue(StringType(subgraphHex)))
        identity.rootSet.forEach {
            p.addExtension(Extension(EXT_ROOT_SET).setValue(StringType(it.toString())))
        }
        identity.lastModifiedEvent?.let {
            p.addExtension(Extension(EXT_LAST_MODIFIED).setValue(StringType(it.toString())))
        }

        // Institutional MRNs from the effective identity (issuer travels in the value, unit-sep).
        fields.firstOrNull { it.key == "mrn" }?.values?.forEach { v ->
            val parts = v.value.split(UNIT_SEP)
            val mrn = detoken(parts.getOrNull(1))
            if (mrn != null) {
                p.addIdentifier().apply {
                    type = CodeableConcept().addCoding(
                        Coding()
                            .setSystem("http://terminology.hl7.org/CodeSystem/v2-0203")
                            .setCode("MR")
                            .setDisplay("Medical record number"),
                    )
                    detoken(parts.getOrNull(0))?.let { inst -> system = MRN_SYSTEM_PREFIX + inst }
                    value = mrn
                }
            }
        }

        // Gender — a plain enum, not tokenized (§5.3.1), so available in the clear.
        fields.firstOrNull { it.key == "sex" }?.values?.firstOrNull()?.value?.let { sex ->
            p.gender = when (sex) {
                "male" -> Enumerations.AdministrativeGender.MALE
                "female" -> Enumerations.AdministrativeGender.FEMALE
                "other" -> Enumerations.AdministrativeGender.OTHER
                else -> Enumerations.AdministrativeGender.UNKNOWN
            }
        }

        // Name + birthDate: cleartext is held only at the originating institution (§9.2). Emit them
        // MASKED so the Patient is US-Core-valid yet never carries a fabricated value; cleartext is
        // fetched via the consent-gated $creda-cleartext op.
        val name = p.addName()
        name.addExtension(Extension(DATA_ABSENT).setValue(CodeType("masked")))
        fields.firstOrNull { it.key == "name-family" }?.values?.firstOrNull()?.let {
            name.addExtension(Extension(EXT_FIELD_CONFIDENCE).setValue(UnsignedIntType(it.confidence)))
        }

        val birthDate = DateType()
        birthDate.addExtension(Extension(DATA_ABSENT).setValue(CodeType("masked")))
        fields.firstOrNull { it.key == "date-of-birth" }?.let { dob ->
            dob.values.firstOrNull()?.let {
                birthDate.addExtension(
                    Extension(EXT_FIELD_CONFIDENCE).setValue(UnsignedIntType(it.confidence)),
                )
            }
            if (dob.disputed) {
                birthDate.addExtension(
                    Extension(EXT_DISPUTED).setValue(org.hl7.fhir.r4.model.BooleanType(true)),
                )
            }
        }
        p.birthDateElement = birthDate

        return p
    }

    private fun detoken(token: String?): String? {
        if (token == null) return null
        return DETOKEN.find(token)?.groupValues?.get(1) ?: token
    }
}
