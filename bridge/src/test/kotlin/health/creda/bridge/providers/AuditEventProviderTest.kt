package health.creda.bridge.providers

import org.hl7.fhir.r4.model.AuditEvent
import org.hl7.fhir.r4.model.Reference
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.time.Instant
import java.util.Date

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
}
