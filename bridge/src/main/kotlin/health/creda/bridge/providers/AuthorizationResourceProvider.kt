package health.creda.bridge.providers

import ca.uhn.fhir.rest.annotation.IdParam
import ca.uhn.fhir.rest.annotation.Operation
import ca.uhn.fhir.rest.annotation.ResourceParam
import health.creda.bridge.cbor.EventPayloadCbor
import health.creda.bridge.grpc.CredaCoreClient
import health.creda.grpc.GrantPurpose
import health.creda.grpc.UseMode
import org.hl7.fhir.r4.model.AuditEvent
import org.hl7.fhir.r4.model.CodeType
import org.hl7.fhir.r4.model.CodeableConcept
import org.hl7.fhir.r4.model.Coding
import org.hl7.fhir.r4.model.Consent
import org.hl7.fhir.r4.model.DateTimeType
import org.hl7.fhir.r4.model.Extension
import org.hl7.fhir.r4.model.Identifier
import org.hl7.fhir.r4.model.IdType
import org.hl7.fhir.r4.model.InstantType
import org.hl7.fhir.r4.model.Parameters
import org.hl7.fhir.r4.model.Period
import org.hl7.fhir.r4.model.Reference
import org.hl7.fhir.r4.model.StringType
import org.springframework.stereotype.Component
import java.util.UUID

/**
 * Portable authorization through FHIR (§8.2.9). The CredaAuthorization profile is based on FHIR
 * Consent (§4, §9.3). These operations are thin translators over Creda Core (§8.3.2): the FHIR
 * Parameters in, a canonical-CBOR EventPayload to Core's CreateEvent, and a FHIR projection of
 * the returned event back out. No authorization logic lives here — Core owns the seven-step
 * evaluation (§4.6) and signing (§5.1).
 *
 *   $creda-authorize -> CreateEvent(AuthorizationGrant)        -> Consent (active)
 *   $creda-revoke    -> CreateEvent(AuthorizationRevocation)   -> Consent (inactive)
 *   $creda-export    -> CreateEvent(ExportReceipt)             -> AuditEvent  (disclosure)
 *   $creda-verify    -> EvaluateAuthorization                  -> Parameters (decision)
 *
 * F0 delivers the working FHIR<->CBOR mappers for the three authorization event types. The
 * richer FASTConsent-conformant projection (grantee/controller/manager, FASTReference) is F1
 * (§8.5.6); this projection is the faithful-but-minimal CredaAuthorization shape on FHIR Consent.
 */
@Component
class AuthorizationResourceProvider(
    private val core: CredaCoreClient,
) {

    // These operations live on the *Patient* resource (`POST Patient/[id]/$creda-...`, §8.2.9) —
    // that is where the spec and the persona clients call them — yet they return Consent/AuditEvent
    // rather than the resource they are invoked on. To attach Patient-typed operations from a class
    // that is NOT the Patient resource provider, this is registered as a HAPI **plain provider**
    // (see CredaRestfulServer.registerProvider) and each @Operation carries `typeName = "Patient"`.
    // It deliberately does NOT implement IResourceProvider: a second Patient resource provider would
    // collide with PatientResourceProvider (HAPI forbids two resource providers for one type), and a
    // Consent-typed resource provider registers the ops under /Consent/{id}/... — which is why the
    // earlier attempt 400'd with "No methods exist for resource: null" on /Patient/{id}/$creda-*.

    /** `$creda-authorize` (§8.2.9): create an AuthorizationGrant from the Parameters. */
    @Operation(name = "\$creda-authorize", typeName = "Patient")
    fun authorize(@IdParam patient: IdType, @ResourceParam params: Parameters): Consent {
        val grant = parseGrant(params)
        val payload = EventPayloadCbor.encodeAuthorizationGrant(
            scope = grant.scope,
            audience = grant.audience,
            purpose = grant.purpose,
            useMode = grant.useMode,
            expiration = grant.expiration,
        )
        // The Grant is attached to the patient subgraph via its parent (the entry-point event).
        val eventCbor = core.createEvent(payload, listOf(entryPointBytes(patient)))
        return ConsentMapper.fromGrantCbor(eventCbor, patient.idPart)
    }

    /**
     * `$creda-revoke` (§8.2.9): create an AuthorizationRevocation referencing a prior Grant. The
     * revoked Grant is the revocation's parent (every non-Assert event must reference a parent,
     * and the target Grant is the natural one). Projects as a Consent in the `inactive` state.
     */
    @Operation(name = "\$creda-revoke", typeName = "Patient")
    fun revoke(@IdParam patient: IdType, @ResourceParam params: Parameters): Consent {
        val targetGrantId = requireUuid(
            referencedId(params, "grant") ?: referencedId(params, "target"),
            "grant",
        )
        val payload = EventPayloadCbor.encodeAuthorizationRevocation(targetGrantId)
        val eventCbor = core.createEvent(payload, listOf(EventPayloadCbor.uuidBytes(targetGrantId)))
        return ConsentMapper.fromRevocationCbor(eventCbor, patient.idPart)
    }

    /**
     * `$creda-export` (§8.2.9): record an ExportReceipt when data is released under a Grant —
     * typically invoked by the Export Gate (§10.2), not a clinical user. Projects as an
     * AuditEvent (the FAST $record-disclosure shape, §8.5.3).
     */
    @Operation(name = "\$creda-export", typeName = "Patient")
    fun export(@IdParam patient: IdType, @ResourceParam params: Parameters): AuditEvent {
        val governingGrantId = requireUuid(referencedId(params, "grant"), "grant")
        val requestingInstitution = hexToBytes(
            requireNotNull(valueString(params, "requestingInstitution")) {
                "\$creda-export requires a 'requestingInstitution' fingerprint (hex)"
            },
        )
        val scope = scopeFromParam(valueString(params, "scope"))
        val payload = EventPayloadCbor.encodeExportReceipt(governingGrantId, requestingInstitution, scope)
        val eventCbor = core.createEvent(payload, listOf(EventPayloadCbor.uuidBytes(governingGrantId)))
        return AuditEventMapper.fromExportReceiptCbor(eventCbor)
    }

    /**
     * `$creda-provenance` (§8.2.5): the patient's full provenance graph — every subgraph event
     * projected as CredaProvenance, in logical-clock order (Core sorts). GET-invocable
     * (idempotent); this is the read the clinician DAG/detail views consume.
     */
    @Operation(name = "\$creda-provenance", typeName = "Patient", idempotent = true)
    fun provenance(@IdParam patient: IdType): org.hl7.fhir.r4.model.Bundle {
        val events = core.getSubgraphEvents(listOf(entryPointBytes(patient)), emptyList())
        return org.hl7.fhir.r4.model.Bundle().apply {
            type = org.hl7.fhir.r4.model.Bundle.BundleType.SEARCHSET
            events.forEach { addEntry().resource = ProvenanceMapper.fromEventCbor(it) }
        }
    }

    /**
     * `$creda-amend` (§3.4.5): amend a prior Assert's demographics — the clinician DOB-resolution
     * flow. Identity-side rather than authorization-side, but it lives in this plain provider
     * because that is where Patient-typed operations attach (see class comment). DOB-only for
     * now; the Amend references its target as parent, and Core enforces the originating-
     * institution rule at the graph layer (§3.4.5).
     */
    @Operation(name = "\$creda-amend", typeName = "Patient")
    fun amend(@IdParam patient: IdType, @ResourceParam params: Parameters): org.hl7.fhir.r4.model.Provenance {
        val target = requireUuid(referencedId(params, "target"), "target")
        val dob = requireNotNull(valueString(params, "dateOfBirth")) { "\$creda-amend requires a 'dateOfBirth'" }
        val reason = requireNotNull(valueString(params, "reason")) { "\$creda-amend requires a 'reason'" }
        val payload = EventPayloadCbor.encodeAmendDob(target, dob, reason)
        val eventCbor = core.createEvent(payload, listOf(EventPayloadCbor.uuidBytes(target)))
        return ProvenanceMapper.fromEventCbor(eventCbor)
    }

    /**
     * `$creda-verify` (§8.2.9): run Core's authorization evaluation for a requesting institution
     * and return a decision plus the governing Grant. The Verifier is local (§10.3.3), so this
     * may be served from stale state.
     */
    @Operation(name = "\$creda-verify", typeName = "Patient")
    fun verify(@IdParam patient: IdType, @ResourceParam params: Parameters): Parameters {
        val q = parseAuthQuery(params)
        val reply = core.evaluateAuthorization(
            entryPoints = listOf(entryPointBytes(patient)),
            requesterFingerprint = q.requesterFingerprint,
            purpose = q.purpose,
            useMode = q.useMode,
        )
        return Parameters().apply {
            addParameter().setName("decision")
                .setValue(CodeType(if (reply.authorized) "authorized" else "denied"))
            addParameter().setName("reason").setValue(StringType(reply.reason))
            reply.coveringGrantsList.forEach { g ->
                addParameter().setName("governingGrant")
                    .setValue(Reference("Consent/${EventPayloadCbor.bytesToUuid(g.toByteArray())}"))
            }
        }
    }

    // ---- Parameters parsing ----------------------------------------------------------------

    private data class ParsedGrant(
        val scope: EventPayloadCbor.Scope,
        val audience: EventPayloadCbor.Audience,
        val purpose: String,
        val useMode: String,
        val expiration: String?,
    )

    private fun parseGrant(params: Parameters): ParsedGrant {
        // Strict to the §8.2.9 contract: parameter names and FHIR codes as specified. Clients
        // conform to the bridge, not the reverse — a non-conformant request is a 400, not a guess.
        val purpose = requireNotNull(valueString(params, "purpose")) {
            "\$creda-authorize requires a 'purpose'"
        }
        require(purpose in GRANT_PURPOSES) { "unknown grant purpose '$purpose' (expected a §4.3.1 code)" }

        val useMode = requireNotNull(valueString(params, "useMode")) {
            "\$creda-authorize requires a 'useMode'"
        }
        require(useMode in USE_MODES) { "unknown use-mode '$useMode' (expected read-only|read-and-rely|read-and-export)" }

        val audience = parseAudience(
            requireNotNull(valueString(params, "audience")) { "\$creda-authorize requires an 'audience'" },
        )
        return ParsedGrant(
            scope = scopeFromParam(valueString(params, "scope")),
            audience = audience,
            purpose = purpose,
            useMode = useMode,
            expiration = valueString(params, "expiration"),
        )
    }

    /**
     * Heuristic audience parse for F0. Prefixes disambiguate the externally-tagged GrantAudience
     * variants; a bare string is treated as an institutional class (the common case, e.g.
     * "any-tefca-qhin"). F1 replaces this with the FASTConsent grantee mapping.
     *   "id:<hex-fingerprint>"  -> InstitutionId
     *   "wildcard:<pattern>"    -> ConstrainedWildcard
     *   "<class>"               -> InstitutionClass
     */
    private fun parseAudience(raw: String): EventPayloadCbor.Audience = when {
        raw.startsWith("id:") -> EventPayloadCbor.Audience.InstitutionId(hexToBytes(raw.removePrefix("id:")))
        raw.startsWith("wildcard:") -> EventPayloadCbor.Audience.ConstrainedWildcard(raw.removePrefix("wildcard:"))
        else -> EventPayloadCbor.Audience.InstitutionClass(raw)
    }

    /** "full-subgraph" or absent => whole subgraph (empty scope); otherwise a data category. */
    private fun scopeFromParam(raw: String?): EventPayloadCbor.Scope =
        if (raw == null || raw == "full-subgraph") {
            EventPayloadCbor.Scope()
        } else {
            EventPayloadCbor.Scope(dataCategories = listOf(raw))
        }

    private fun parseAuthQuery(params: Parameters): AuthQuery {
        val purpose = requireNotNull(valueString(params, "purpose")) { "\$creda-verify requires a 'purpose'" }
        val useMode = requireNotNull(valueString(params, "useMode")) { "\$creda-verify requires a 'useMode'" }
        val requester = requireNotNull(valueString(params, "requester")) {
            "\$creda-verify requires a 'requester' fingerprint (hex)"
        }
        return AuthQuery(
            requesterFingerprint = hexToBytes(requester),
            purpose = purposeToProto(purpose),
            useMode = useModeToProto(useMode),
        )
    }

    // ---- small Parameters / hex / uuid helpers ---------------------------------------------

    /** Primitive (StringType/CodeType/DateTimeType) value of a named parameter, or null. */
    private fun valueString(params: Parameters, name: String): String? {
        val v = params.parameter.firstOrNull { it.name == name }?.value ?: return null
        return v.primitiveValue() // null for complex types
    }

    /** The referenced logical id of a named parameter (valueReference or a primitive id/string). */
    private fun referencedId(params: Parameters, name: String): String? {
        val v = params.parameter.firstOrNull { it.name == name }?.value ?: return null
        val raw = (v as? Reference)?.reference ?: v.primitiveValue() ?: return null
        return raw.substringAfterLast('/').removePrefix("urn:uuid:")
    }

    private fun requireUuid(raw: String?, field: String): UUID =
        try {
            UUID.fromString(requireNotNull(raw) { "missing '$field' reference" })
        } catch (e: IllegalArgumentException) {
            throw IllegalArgumentException("'$field' is not a valid UUID: $raw", e)
        }

    /**
     * Patient idPart -> 16-byte entry-point UUID. Core's parent_ids are `EventId::from_slice`, so
     * a Patient logical id in Creda *is* a subgraph entry-point event UUID (§8.1.1). A non-UUID id
     * is a client conformance error and is rejected, not coerced.
     */
    private fun entryPointBytes(patient: IdType): ByteArray =
        EventPayloadCbor.uuidBytes(requireUuid(patient.idPart.removePrefix("urn:uuid:"), "patient"))

    private fun hexToBytes(hex: String): ByteArray {
        val clean = hex.removePrefix("0x").lowercase()
        require(clean.length % 2 == 0) { "hex fingerprint must have even length: $hex" }
        return ByteArray(clean.length / 2) { i ->
            ((Character.digit(clean[i * 2], 16) shl 4) + Character.digit(clean[i * 2 + 1], 16)).toByte()
        }
    }

    private fun purposeToProto(kebab: String): GrantPurpose = when (kebab) {
        "treatment" -> GrantPurpose.TREATMENT
        "payment" -> GrantPurpose.PAYMENT
        "operations" -> GrantPurpose.OPERATIONS
        "public-health" -> GrantPurpose.PUBLIC_HEALTH
        "research" -> GrantPurpose.RESEARCH
        "ai-training" -> GrantPurpose.AI_TRAINING
        "ai-inference" -> GrantPurpose.AI_INFERENCE
        "federal-program" -> GrantPurpose.FEDERAL_PROGRAM
        else -> throw IllegalArgumentException("unknown grant purpose '$kebab'")
    }

    private fun useModeToProto(kebab: String): UseMode = when (kebab) {
        "read-only" -> UseMode.READ_ONLY
        "read-and-rely" -> UseMode.READ_AND_RELY
        "read-and-export" -> UseMode.READ_AND_EXPORT
        else -> throw IllegalArgumentException("unknown use-mode '$kebab'")
    }

    private companion object {
        val GRANT_PURPOSES = setOf(
            "treatment", "payment", "operations", "public-health",
            "research", "ai-training", "ai-inference", "federal-program",
        )
        val USE_MODES = setOf("read-only", "read-and-rely", "read-and-export")
    }
}

/** The structured pieces of a `$creda-verify` request, parsed from the FHIR Parameters. */
internal data class AuthQuery(
    val requesterFingerprint: ByteArray,
    val purpose: GrantPurpose,
    val useMode: UseMode,
)

/**
 * Projects authorization event nodes to the CredaAuthorization profile on FHIR Consent (§4, §8.2.9).
 * Minimal-but-faithful for F0; FASTConsent fidelity (grantee/controller/manager) is F1 (§8.5.6).
 */
internal object ConsentMapper {

    private const val SCOPE_SYSTEM = "http://terminology.hl7.org/CodeSystem/consentscope"
    private const val CATEGORY_SYSTEM = "http://loinc.org"
    private const val PURPOSE_SYSTEM = "http://creda.health/fhir/CodeSystem/grant-purpose"

    fun fromGrantCbor(cbor: ByteArray, patientId: String): Consent {
        val v = EventPayloadCbor.decodeGrantNode(cbor)
        return baseConsent(v.id, patientId, Consent.ConsentState.ACTIVE, v.wallClockTimestamp, v.institutionFingerprint)
            .apply {
                provision = Consent.provisionComponent().apply {
                    type = Consent.ConsentProvisionType.PERMIT
                    if (v.expiration != null) period = Period().setEndElement(DateTimeType(v.expiration))
                    addPurpose(Coding(PURPOSE_SYSTEM, v.purpose, v.purpose))
                    addActor(audienceActor(v.audience))
                    // use_mode is recorded as a Creda extension on the provision (F1 formalizes the URL).
                    addExtension(
                        Extension("http://creda.health/fhir/StructureDefinition/use-mode", CodeType(v.useMode)),
                    )
                }
            }
    }

    fun fromRevocationCbor(cbor: ByteArray, patientId: String): Consent {
        val v = EventPayloadCbor.decodeRevocationNode(cbor)
        return baseConsent(v.id, patientId, Consent.ConsentState.INACTIVE, v.wallClockTimestamp, v.institutionFingerprint)
            .apply {
                // Point back at the Grant this revocation supersedes (§4.3.2).
                addExtension(
                    Extension("http://creda.health/fhir/StructureDefinition/revokes-grant")
                        .setValue(Reference("Consent/${v.targetGrantId}")),
                )
            }
    }

    private fun baseConsent(
        id: UUID,
        patientId: String,
        state: Consent.ConsentState,
        recorded: String,
        institutionFingerprint: ByteArray,
    ): Consent = Consent().apply {
        setId(id.toString())
        status = state
        scope = CodeableConcept().addCoding(Coding(SCOPE_SYSTEM, "patient-privacy", "Privacy Consent"))
        addCategory(CodeableConcept().addCoding(Coding(CATEGORY_SYSTEM, "59284-0", "Consent Document")))
        patient = Reference("Patient/$patientId")
        dateTimeElement = DateTimeType(recorded)
        addOrganization(orgRef(institutionFingerprint))
    }

    private fun audienceActor(audience: EventPayloadCbor.Audience): Consent.provisionActorComponent {
        val recipientRole = CodeableConcept().addCoding(
            Coding("http://terminology.hl7.org/CodeSystem/v3-RoleClass", "IRCP", "information recipient"),
        )
        val reference = when (audience) {
            is EventPayloadCbor.Audience.InstitutionId -> orgRef(audience.fingerprint)
            is EventPayloadCbor.Audience.InstitutionClass ->
                Reference().setDisplay(audience.name).setType("Organization")
            is EventPayloadCbor.Audience.ConstrainedWildcard ->
                Reference().setDisplay(audience.pattern).setType("Organization")
        }
        return Consent.provisionActorComponent().setRole(recipientRole).setReference(reference)
    }

    private fun orgRef(fingerprint: ByteArray): Reference = Reference().setIdentifier(
        Identifier()
            .setSystem("http://creda.health/fhir/sid/udap-fingerprint")
            .setValue(toHex(fingerprint)),
    ).setType("Organization")

    private fun toHex(bytes: ByteArray): String =
        bytes.joinToString("") { "%02x".format(it) }
}

/** Projects an ExportReceipt node to a FHIR AuditEvent — the FAST $record-disclosure shape (§8.5.3). */
internal object AuditEventMapper {
    fun fromExportReceiptCbor(cbor: ByteArray): AuditEvent {
        val v = EventPayloadCbor.decodeExportReceiptNode(cbor)
        return AuditEvent().apply {
            setId(v.id.toString())
            // ATNA "Export" event type (DICOM DCM 110106).
            type = Coding("http://dicom.nema.org/resources/ontology/DCM", "110106", "Export")
            action = AuditEvent.AuditEventAction.R
            recordedElement = InstantType(v.wallClockTimestamp)
            outcome = AuditEvent.AuditEventOutcome._0 // success
            // The source institution that released the data, as the requestor agent.
            addAgent(
                AuditEvent.AuditEventAgentComponent()
                    .setWho(orgRef(v.institutionFingerprint))
                    .setRequestor(true),
            )
            // The institution the data was disclosed to.
            addAgent(
                AuditEvent.AuditEventAgentComponent()
                    .setWho(orgRef(v.requestingInstitution))
                    .setRequestor(false),
            )
            // The governing Grant this disclosure was made under (ties the AuditEvent to the Consent).
            addEntity(
                AuditEvent.AuditEventEntityComponent()
                    .setWhat(Reference("Consent/${v.governingGrantId}")),
            )
        }
    }

    private fun orgRef(fingerprint: ByteArray): Reference = Reference().setIdentifier(
        Identifier()
            .setSystem("http://creda.health/fhir/sid/udap-fingerprint")
            .setValue(fingerprint.joinToString("") { "%02x".format(it) }),
    ).setType("Organization")
}
