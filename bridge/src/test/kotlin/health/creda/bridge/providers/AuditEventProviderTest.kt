package health.creda.bridge.providers

import com.upokecenter.cbor.CBORObject
import health.creda.bridge.cbor.EventPayloadCbor
import org.hl7.fhir.r4.model.AuditEvent
import org.hl7.fhir.r4.model.Reference
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.time.Instant
import java.util.Date
import java.util.UUID

/**
 * The two AuditEvent streams (§8.2.4):
 *  - [AuditEventProjection] — the on-chain **disclosure** ledger post-projection: each ExportReceipt
 *    is tied to the patient whose data moved, newest first; an empty ledger stays empty (no fakes);
 *  - [AccessAuditSink] — the read-side access stream's egress contract.
 */
class AuditEventProviderTest {

    private fun disclosure(id: String, recordedMillis: Long): AuditEvent =
        AuditEvent().apply {
            setId(id)
            setRecorded(Date(recordedMillis))
        }

    @Test
    fun `decorate ties each disclosure to the patient and orders newest first`() {
        val older = disclosure("a", 1_000_000)
        val newer = disclosure("b", 2_000_000)

        val out = AuditEventProjection.decorate(listOf(older, newer), "patient-7")

        assertEquals(listOf("b", "a"), out.map { it.idElement.idPart }, "newest disclosure first")
        out.forEach { ev ->
            assertTrue(
                ev.entity.any { (it.what as? Reference)?.reference == "Patient/patient-7" },
                "each disclosure references the patient whose data moved",
            )
        }
    }

    @Test
    fun `decorate on an empty ledger is empty — honest, no fabricated rows`() {
        assertTrue(AuditEventProjection.decorate(emptyList(), "p").isEmpty())
    }

    @Test
    fun `access sink receives exactly the record it is handed`() {
        val captured = mutableListOf<AccessAuditRecord>()
        val sink = AccessAuditSink { captured.add(it) }

        val rec = AccessAuditRecord(Instant.EPOCH, "READ", "Patient", "Patient/9", "/fhir/Patient/9", "req-1")
        sink.record(rec)

        assertEquals(listOf(rec), captured)
    }

    @Test
    fun `default slf4j sink records a partial event without throwing`() {
        Slf4jAccessAuditSink().record(
            AccessAuditRecord(Instant.now(), "SEARCH_TYPE", "AuditEvent", null, "/fhir/AuditEvent", null),
        )
    }

    @Test
    fun `TPODisclosure projects to an AuditEvent with author and recipient agents`() {
        val id = UUID.fromString("00010203-0405-0607-0809-0a0b0c0d0e0f")
        val author = byteArrayOf(0x11, 0x22, 0x33)
        val recipient = byteArrayOf(0xAA.toByte(), 0xBB.toByte())
        val payload = EventPayloadCbor.encodeTPODisclosure(
            recipient = recipient,
            purpose = "payment",
            disclosedScope = EventPayloadCbor.Scope(),
            dataReference = "Claim/abc",
        )
        val node = nodeWrapping(payload, id, author, "2026-07-06T12:00:00Z")

        val ev = AuditEventMapper.fromTPODisclosureCbor(node)

        assertEquals(id.toString(), ev.idElement.idPart)
        assertEquals("110106", ev.type.code, "ATNA Export event type")
        assertTrue(
            ev.purposeOfEvent.any { it.codingFirstRep.code == "payment" },
            "the grant-less TPO basis rides on purposeOfEvent",
        )
        val authorAgent = ev.agent.first { it.requestor }
        val recipientAgent = ev.agent.first { !it.requestor }
        assertEquals("112233", authorAgent.who.identifier.value, "disclosing institution is the author")
        assertEquals("aabb", recipientAgent.who.identifier.value, "recipient is the payer")
        assertTrue(
            ev.entity.any { (it.what as? Reference)?.reference == "Claim/abc" },
            "the disclosed artifact reference is recorded",
        )
    }

    /** Wrap a payload's CBOR in a minimal IdentityEventNode envelope (id bstr, institution int-array). */
    private fun nodeWrapping(payloadCbor: ByteArray, id: UUID, inst: ByteArray, wall: String): ByteArray {
        val node = CBORObject.NewMap()
        node.Add("id", CBORObject.FromObject(EventPayloadCbor.uuidBytes(id)))
        val instArr = CBORObject.NewArray()
        inst.forEach { instArr.Add(it.toInt() and 0xFF) }
        node.Add("institution_id", instArr)
        node.Add("wall_clock_timestamp", wall)
        node.Add("payload", CBORObject.DecodeFromBytes(payloadCbor))
        return node.EncodeToBytes()
    }
}
