package health.creda.bridge.cbor

import com.upokecenter.cbor.CBORObject
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Test
import java.util.UUID

/**
 * Wire-shape tests for the `$creda-tombstone` payload mapper (§3.4.6, right-to-be-forgotten).
 *
 * The golden hex is an independent oracle produced with Python `cbor2(canonical=True)`, mirroring
 * the serde+ciborium 0.2.2 rules Creda Core uses: `EventPayload` is externally tagged
 * (`{"Tombstone": {...}}`), `TombstoneBasis` is a kebab-case unit enum (a text string), `EventId`
 * is a 16-byte CBOR byte string, and canonical encoding sorts the inner keys — `legal_basis`
 * (length 11) before `target_event_ids` (length 16). If Core's encoding ever drifts, this breaks
 * here before it breaks in the field.
 */
class TombstonePayloadCborTest {

    // UUID 00010203-0405-0607-0809-0a0b0c0d0e0f == bytes 0x00..0x0f.
    private val tid = UUID.fromString("00010203-0405-0607-0809-0a0b0c0d0e0f")

    @Test
    fun `tombstone matches golden vector — externally tagged, kebab basis, uuid bstr`() {
        val bytes = EventPayloadCbor.encodeTombstone(listOf(tid), "right-to-be-forgotten")
        assertEquals(GOLDEN_TOMBSTONE_SINGLE, toHex(bytes))
    }

    @Test
    fun `multi-target tombstone matches golden vector`() {
        val other = UUID.fromString("ffffffff-ffff-ffff-ffff-ffffffffffff")
        val bytes = EventPayloadCbor.encodeTombstone(listOf(tid, other), "court-order")
        assertEquals(GOLDEN_TOMBSTONE_TWO, toHex(bytes))
    }

    @Test
    fun `decodes back to the Tombstone variant with its fields`() {
        val bytes = EventPayloadCbor.encodeTombstone(listOf(tid), "state-law")
        val body = CBORObject.DecodeFromBytes(bytes)["Tombstone"]
        assertEquals("state-law", body["legal_basis"].AsString())
        assertEquals(1, body["target_event_ids"].size())
        val got = EventPayloadCbor.bytesToUuid(body["target_event_ids"][0].GetByteString())
        assertEquals(tid, got)
    }

    @Test
    fun `rejects an empty target list`() {
        assertThrows(IllegalArgumentException::class.java) {
            EventPayloadCbor.encodeTombstone(emptyList(), "right-to-be-forgotten")
        }
    }

    @Test
    fun `rejects an unknown legal basis`() {
        assertThrows(IllegalArgumentException::class.java) {
            EventPayloadCbor.encodeTombstone(listOf(tid), "because-i-said-so")
        }
    }

    private fun toHex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    private companion object {
        // Golden vectors — produced by Python cbor2 (canonical=True) per the ciborium 0.2.2 rules
        // documented in the class header. Do not hand-edit.
        const val GOLDEN_TOMBSTONE_SINGLE =
            "a169546f6d6273746f6e65a26b6c6567616c5f62617369737572696768742d746f2d62652d666f72676f7474656e707461726765745f6576656e745f6964738150000102030405060708090a0b0c0d0e0f"
        const val GOLDEN_TOMBSTONE_TWO =
            "a169546f6d6273746f6e65a26b6c6567616c5f62617369736b636f7572742d6f72646572707461726765745f6576656e745f6964738250000102030405060708090a0b0c0d0e0f50ffffffffffffffffffffffffffffffff"
    }
}
