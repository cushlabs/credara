package health.creda.bridge.cbor

import com.upokecenter.cbor.CBORObject
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Test
import java.util.UUID

/**
 * Wire-shape tests for `EventPayload::Contest { target_link_id, reason }` (§3.4.3).
 *
 * The reason is the Rust struct `ContestReason { code, detail? }` — NOT the legacy externally-
 * tagged `{Other: text}`. The golden hex below was produced by the independent Python `cbor2`
 * (`canonical=True`) oracle (see AuthorizationPayloadCborTest for the ciborium 0.2.2 rules it
 * mirrors): a struct is a text-keyed map in canonical (length-first) key order, so the body orders
 * `reason` (6) before `target_link_id` (14), and within the reason map `code` (4) before
 * `detail` (6); `detail` is omitted entirely when null (serde `skip_serializing_if`).
 */
class ContestPayloadCborTest {

    // UUID 00010203-0405-0607-0809-0a0b0c0d0e0f == bytes 0x00..0x0f.
    private val gid = UUID.fromString("00010203-0405-0607-0809-0a0b0c0d0e0f")

    @Test
    fun `contest with detail matches golden vector`() {
        val bytes = EventPayloadCbor.encodeContest(gid, "distinct-patients", "different humans")
        assertEquals(GOLDEN_CONTEST_WITH_DETAIL, toHex(bytes))
    }

    @Test
    fun `contest without detail omits the field`() {
        val bytes = EventPayloadCbor.encodeContest(gid, "demographic-conflict")
        assertEquals(GOLDEN_CONTEST_NO_DETAIL, toHex(bytes))
    }

    @Test
    fun `an invalid reason code is rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            EventPayloadCbor.encodeContest(gid, "not-a-real-code", null)
        }
    }

    @Test
    fun `decodePayloadDetails renders code and detail (inverse of the encoder)`() {
        val payload = EventPayloadCbor.encodeContest(gid, "distinct-patients", "different humans")
        val node = nodeWrapping(payload, gid)
        val details = EventPayloadCbor.decodePayloadDetails(node)
        assertEquals("distinct-patients: different humans", details.contestReason)
    }

    // ---- helpers ---------------------------------------------------------------------------

    /** Minimal IdentityEventNode envelope around a payload, matching Core's wire shape. */
    private fun nodeWrapping(payloadCbor: ByteArray, id: UUID): ByteArray {
        val node = CBORObject.NewMap()
        node.Add("id", CBORObject.FromObject(EventPayloadCbor.uuidBytes(id)))
        val instArr = CBORObject.NewArray()
        byteArrayOf(0xAA.toByte(), 0xBB.toByte()).forEach { instArr.Add(it.toInt() and 0xFF) }
        node.Add("institution_id", instArr)
        node.Add("wall_clock_timestamp", "2026-06-03T12:00:00Z")
        node.Add("payload", CBORObject.DecodeFromBytes(payloadCbor))
        return node.EncodeToBytes()
    }

    private fun toHex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    private companion object {
        // Golden vectors — Python cbor2 (canonical=True); do not hand-edit.
        const val GOLDEN_CONTEST_WITH_DETAIL =
            "a167436f6e74657374a266726561736f6ea264636f64657164697374696e63742d70617469656e74736664657461696c70646966666572656e742068756d616e736e7461726765745f6c696e6b5f696450000102030405060708090a0b0c0d0e0f"
        const val GOLDEN_CONTEST_NO_DETAIL =
            "a167436f6e74657374a266726561736f6ea164636f64657464656d6f677261706869632d636f6e666c6963746e7461726765745f6c696e6b5f696450000102030405060708090a0b0c0d0e0f"
    }
}
