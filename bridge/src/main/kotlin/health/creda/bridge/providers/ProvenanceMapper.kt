package health.creda.bridge.providers

import health.creda.bridge.cbor.EventPayloadCbor
import org.hl7.fhir.r4.model.CodeableConcept
import org.hl7.fhir.r4.model.Coding
import org.hl7.fhir.r4.model.Extension
import org.hl7.fhir.r4.model.InstantType
import org.hl7.fhir.r4.model.Provenance
import org.hl7.fhir.r4.model.Reference
import java.util.Base64

/**
 * Maps an `IdentityEventNode` (CBOR-encoded, returned by Core's CreateEvent / GetEvent) into a
 * `CredaProvenance` FHIR resource, per the spec §8.2.3 mapping table:
 *
 * | Creda field | FHIR Provenance |
 * |---|---|
 * | Event UUID                       | `Provenance.id` |
 * | Event type                       | `Provenance.activity` (CodeableConcept) |
 * | Parent UUIDs                     | `Provenance.entity[].what` |
 * | Institution fingerprint          | `Provenance.agent.who` |
 * | Wall-clock                       | `Provenance.recorded` |
 * | Logical clock                    | extension `http://credara.network/StructureDefinition/logical-clock` |
 * | Signature                        | extension `http://credara.network/StructureDefinition/event-signature` |
 * | Payload (type-specific fields)   | extension `http://credara.network/StructureDefinition/event-payload` |
 *
 * The payload extension carries the event-type-specific fields the clinician read path projects
 * from (`$creda-provenance` → PatientDetailPage/WorklistPage): Assert verification method +
 * demographic tokens, Link confidence/method, Attest purpose, Amend corrected-DOB token + reason,
 * Contest reason. Tokens cross as tokens — de-tokenization for display is the client's concern
 * (§3.2; demo tokens embed their display form, e.g. `tok:demo:1971-08-04`).
 */
internal object ProvenanceMapper {

    private const val EVENT_TYPE_SYSTEM = "http://credara.network/CodeSystem/event-type"
    private const val EXT_LOGICAL_CLOCK = "http://credara.network/StructureDefinition/logical-clock"
    private const val EXT_SIGNATURE = "http://credara.network/StructureDefinition/event-signature"
    private const val EXT_PAYLOAD = "http://credara.network/StructureDefinition/event-payload"

    fun fromEventCbor(eventCbor: ByteArray): Provenance {
        val node = EventPayloadCbor.decodeEventNode(eventCbor)
        val p = Provenance()
        p.id = node.id.toString()
        // Provenance.recorded is an `instant` per the FHIR R4 schema, so HAPI types it as
        // InstantType (not DateTimeType). Same RFC 3339 string on the wire — just the Java
        // setter discriminates.
        p.recordedElement = InstantType(node.wallClockTimestamp)

        // activity = the event type (Assert / Attest / Link / …)
        p.activity = CodeableConcept().addCoding(
            Coding().setSystem(EVENT_TYPE_SYSTEM).setCode(node.eventType).setDisplay(node.eventType),
        )

        // agent.who = the originating institution, referenced by certificate-fingerprint hex.
        val fingerprintHex = node.institutionFingerprint.joinToString("") { "%02x".format(it) }
        p.addAgent().who = Reference("Organization/fpr:$fingerprintHex")

        // entity[].what = parent event ids (Provenance.entity.role = derivation)
        for (parent in node.parentIds) {
            p.addEntity()
                .setRole(Provenance.ProvenanceEntityRole.DERIVATION)
                .what = Reference("Provenance/${parent}")
        }

        // logical-clock extension — opaque integer in the Creda DAG, useful for re-ordering.
        p.addExtension(Extension(EXT_LOGICAL_CLOCK).setValue(
            org.hl7.fhir.r4.model.UnsignedIntType(node.logicalClock.toInt()),
        ))

        // event-signature extension carries algorithm + base64 sig bytes + key fingerprint, so
        // the SPA can show "ed25519:verified ✓" with real provenance behind it.
        val sigExt = Extension(EXT_SIGNATURE)
        sigExt.addExtension(Extension("algorithm").setValue(
            org.hl7.fhir.r4.model.CodeType(node.signatureAlgorithm),
        ))
        sigExt.addExtension(Extension("publicKeyFingerprint").setValue(
            org.hl7.fhir.r4.model.StringType(
                node.signaturePublicKeyFingerprint.joinToString("") { "%02x".format(it) },
            ),
        ))
        sigExt.addExtension(Extension("signature").setValue(
            org.hl7.fhir.r4.model.Base64BinaryType(
                Base64.getEncoder().encodeToString(node.signatureBytes).toByteArray(),
            ),
        ))
        p.addExtension(sigExt)

        // event-payload extension — the type-specific fields the clinician projection reads.
        // Sub-extensions are present only when the variant carries the field; an event whose
        // payload yields nothing (e.g. the root-Assert stub's empty demographics) gets no
        // payload extension at all, which the client treats the same as "no detail".
        val details = EventPayloadCbor.decodePayloadDetails(eventCbor)
        val payloadExt = Extension(EXT_PAYLOAD)
        fun addCode(name: String, value: String?) {
            if (value != null) payloadExt.addExtension(Extension(name).setValue(org.hl7.fhir.r4.model.CodeType(value)))
        }
        fun addString(name: String, value: String?) {
            if (value != null) payloadExt.addExtension(Extension(name).setValue(org.hl7.fhir.r4.model.StringType(value)))
        }
        addCode("verificationMethod", details.verificationMethod)
        addString("dateOfBirth", details.dateOfBirthToken)
        addString("nameFamily", details.nameFamilyToken)
        addString("nameGiven", details.nameGivenToken)
        details.confidenceScoreBps?.let { bps ->
            payloadExt.addExtension(Extension("confidenceScore").setValue(
                org.hl7.fhir.r4.model.UnsignedIntType(bps),
            ))
        }
        addCode("linkMethod", details.linkMethod)
        addCode("purpose", details.purpose)
        addString("amendmentReason", details.amendmentReason)
        addString("contestReason", details.contestReason)
        if (payloadExt.extension.isNotEmpty()) p.addExtension(payloadExt)

        return p
    }
}
