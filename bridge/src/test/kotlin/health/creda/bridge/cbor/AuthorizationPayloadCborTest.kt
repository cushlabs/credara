package health.creda.bridge.cbor

import com.upokecenter.cbor.CBORObject
import org.junit.jupiter.api.Assertions.assertArrayEquals
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import java.util.UUID

/**
 * Wire-shape tests for the F0 authorization CBOR mappers (§4.3, §8.5.6).
 *
 * The golden hex vectors are an **independent oracle**: they were produced with Python `cbor2`
 * (`canonical=True`) constructed to mirror the exact serde+ciborium 0.2.2 rules that Creda Core
 * uses — confirmed by reading `ciborium-0.2.2/src/ser/mod.rs`:
 *   - structs            -> CBOR map, text keys, canonical (length-first) key order
 *   - unit enum variant  -> text string (kebab-case where the Rust enum renames)
 *   - struct/newtype enum variant -> one-pair map {Variant: value}  (externally tagged)
 *   - `Uuid`             -> serialize_bytes  -> 16-byte CBOR byte string (head 0x50)
 *   - `Vec<u8>` (fingerprint) -> serialize_seq -> CBOR ARRAY of ints (head 0x8N)  <-- not a bstr
 *   - `Option::None` (skip_serializing_if) -> field omitted
 *
 * If Core's encoding ever drifts, these vectors break here before they break in the field. The
 * round-trip tests additionally prove the decoders are exact inverses of the encoders.
 */
class AuthorizationPayloadCborTest {

    // UUID 00010203-0405-0607-0809-0a0b0c0d0e0f == bytes 0x00..0x0f.
    private val gid = UUID.fromString("00010203-0405-0607-0809-0a0b0c0d0e0f")
    private val fp = byteArrayOf(
        0xAA.toByte(), 0xBB.toByte(), 0xCC.toByte(), 0xDD.toByte(), 0xEE.toByte(), 0xFF.toByte(),
    )

    @Test
    fun `grant with institution-class audience matches golden vector`() {
        val bytes = EventPayloadCbor.encodeAuthorizationGrant(
            scope = EventPayloadCbor.Scope(),
            audience = EventPayloadCbor.Audience.InstitutionClass("any-tefca-qhin"),
            purpose = "treatment",
            useMode = "read-and-rely",
            expiration = "2027-05-11T00:00:00Z",
        )
        assertEquals(GOLDEN_GRANT_CLASS, toHex(bytes))
    }

    @Test
    fun `grant with institution-id audience encodes fingerprint as an int array not a bstr`() {
        val bytes = EventPayloadCbor.encodeAuthorizationGrant(
            scope = EventPayloadCbor.Scope(),
            audience = EventPayloadCbor.Audience.InstitutionId(fp),
            purpose = "research",
            useMode = "read-only",
        )
        assertEquals(GOLDEN_GRANT_ID, toHex(bytes))
    }

    @Test
    fun `revocation matches golden vector and uuid is a 16-byte bstr`() {
        val bytes = EventPayloadCbor.encodeAuthorizationRevocation(gid)
        assertEquals(GOLDEN_REVOCATION, toHex(bytes))
    }

    @Test
    fun `export receipt matches golden vector`() {
        val bytes = EventPayloadCbor.encodeExportReceipt(
            governingGrantId = gid,
            requestingInstitution = fp,
            releasedScope = EventPayloadCbor.Scope(),
        )
        assertEquals(GOLDEN_EXPORT, toHex(bytes))
    }

    @Test
    fun `decodeGrantNode is the inverse of the grant encoder`() {
        val payload = EventPayloadCbor.encodeAuthorizationGrant(
            scope = EventPayloadCbor.Scope(),
            audience = EventPayloadCbor.Audience.InstitutionId(fp),
            purpose = "treatment",
            useMode = "read-and-export",
            expiration = "2030-01-01T00:00:00Z",
        )
        val node = nodeWrapping(payload, gid, fp, "2026-06-03T12:00:00Z")
        val v = EventPayloadCbor.decodeGrantNode(node)

        assertEquals(gid, v.id)
        assertArrayEquals(fp, v.institutionFingerprint)
        assertEquals("2026-06-03T12:00:00Z", v.wallClockTimestamp)
        assertEquals("treatment", v.purpose)
        assertEquals("read-and-export", v.useMode)
        assertEquals("2030-01-01T00:00:00Z", v.expiration)
        val aud = v.audience as EventPayloadCbor.Audience.InstitutionId
        assertArrayEquals(fp, aud.fingerprint)
    }

    @Test
    fun `decodeExportReceiptNode round-trips governing grant and requesting institution`() {
        val payload = EventPayloadCbor.encodeExportReceipt(gid, fp, EventPayloadCbor.Scope())
        val node = nodeWrapping(payload, gid, fp, "2026-06-03T12:00:00Z")
        val v = EventPayloadCbor.decodeExportReceiptNode(node)

        assertEquals(gid, v.governingGrantId)
        assertArrayEquals(fp, v.requestingInstitution)
    }

    @Test
    fun `decodeRevocationNode round-trips the target grant id`() {
        val payload = EventPayloadCbor.encodeAuthorizationRevocation(gid)
        val node = nodeWrapping(payload, gid, fp, "2026-06-03T12:00:00Z")
        val v = EventPayloadCbor.decodeRevocationNode(node)

        assertEquals(gid, v.targetGrantId)
    }

    @Test
    fun `tpo disclosure matches golden vector`() {
        val bytes = EventPayloadCbor.encodeTPODisclosure(
            recipient = fp,
            purpose = "treatment",
            disclosedScope = EventPayloadCbor.Scope(),
        )
        assertEquals(GOLDEN_TPO, toHex(bytes))
    }

    @Test
    fun `decodeTPODisclosureNode round-trips and matches the full golden vector`() {
        val payload = EventPayloadCbor.encodeTPODisclosure(
            recipient = fp,
            purpose = "payment",
            disclosedScope = EventPayloadCbor.Scope(subgraphSegments = listOf(gid)),
            dataReference = "Claim/abc",
        )
        assertEquals(GOLDEN_TPO_FULL, toHex(payload))

        val node = nodeWrapping(payload, gid, fp, "2026-06-03T12:00:00Z")
        val v = EventPayloadCbor.decodeTPODisclosureNode(node)
        assertEquals(gid, v.id)
        assertArrayEquals(fp, v.institutionFingerprint)
        assertArrayEquals(fp, v.recipient)
        assertEquals("payment", v.purpose)
        assertEquals("Claim/abc", v.dataReference)
    }

    @Test
    fun `encodeTPODisclosure rejects a non-TPO purpose`() {
        org.junit.jupiter.api.Assertions.assertThrows(IllegalArgumentException::class.java) {
            EventPayloadCbor.encodeTPODisclosure(fp, "research", EventPayloadCbor.Scope())
        }
    }

    // ---- helpers ---------------------------------------------------------------------------

    /**
     * Wrap a payload's CBOR in a minimal IdentityEventNode envelope, matching Core's wire shape:
     * `id` is a 16-byte bstr (Uuid), `institution_id` is a CBOR array of ints (Vec<u8>), and
     * `payload` is the externally-tagged variant map.
     */
    private fun nodeWrapping(payloadCbor: ByteArray, id: UUID, inst: ByteArray, wall: String): ByteArray {
        val node = CBORObject.NewMap()
        node.Add("id", CBORObject.FromObject(EventPayloadCbor.uuidBytes(id))) // bstr(16)
        val instArr = CBORObject.NewArray()
        inst.forEach { instArr.Add(it.toInt() and 0xFF) }
        node.Add("institution_id", instArr) // Vec<u8> -> array of ints
        node.Add("wall_clock_timestamp", wall)
        node.Add("payload", CBORObject.DecodeFromBytes(payloadCbor))
        return node.EncodeToBytes()
    }

    private fun toHex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    private companion object {
        // Golden vectors — produced by Python cbor2 (canonical=True) per the ciborium 0.2.2 rules
        // documented in the class header. Do not hand-edit; regenerate from gen_vectors.py.
        const val GOLDEN_GRANT_CLASS =
            "a172417574686f72697a6174696f6e4772616e74a56573636f7065a067707572706f73656974726561746d656e746861756469656e6365a170496e737469747574696f6e436c6173736e616e792d74656663612d7168696e687573655f6d6f64656d726561642d616e642d72656c796a65787069726174696f6e74323032372d30352d31315430303a30303a30305a"
        const val GOLDEN_GRANT_ID =
            "a172417574686f72697a6174696f6e4772616e74a46573636f7065a067707572706f73656872657365617263686861756469656e6365a16d496e737469747574696f6e49648618aa18bb18cc18dd18ee18ff687573655f6d6f646569726561642d6f6e6c79"
        const val GOLDEN_REVOCATION =
            "a177417574686f72697a6174696f6e5265766f636174696f6ea16f7461726765745f6772616e745f696450000102030405060708090a0b0c0d0e0f"
        const val GOLDEN_EXPORT =
            "a16d4578706f727452656365697074a36e72656c65617365645f73636f7065a072676f7665726e696e675f6772616e745f696450000102030405060708090a0b0c0d0e0f7672657175657374696e675f696e737469747574696f6e8618aa18bb18cc18dd18ee18ff"
        // TPODisclosure (§4.3.5): recipient fingerprint AA..FF, purpose "treatment", empty scope.
        const val GOLDEN_TPO =
            "a16d54504f446973636c6f73757265a367707572706f73656974726561746d656e7469726563697069656e748618aa18bb18cc18dd18ee18ff6f646973636c6f7365645f73636f7065a0"
        // Full: purpose "payment", disclosed_scope.subgraph_segments=[gid], data_reference "Claim/abc".
        const val GOLDEN_TPO_FULL =
            "a16d54504f446973636c6f73757265a467707572706f7365677061796d656e7469726563697069656e748618aa18bb18cc18dd18ee18ff6e646174615f7265666572656e636569436c61696d2f6162636f646973636c6f7365645f73636f7065a17173756267726170685f7365676d656e74738150000102030405060708090a0b0c0d0e0f"
    }
}
