package health.creda.bridge.cbor

import com.upokecenter.cbor.CBORObject
import com.upokecenter.cbor.CBOREncodeOptions
import java.nio.ByteBuffer
import java.util.UUID

/**
 * Hand-rolled canonical-CBOR helpers for the EventPayload wire shape (§3.4, §5.1).
 *
 * The Rust side (`crates/creda-events/src/canonical.rs`) emits RFC 8949 deterministic CBOR:
 * map keys sorted by encoded-bytes lex order, optional `None` fields omitted, no floats. We
 * match that here using `upokecenter:cbor` with [CBOREncodeOptions("ctap2Canonical=true")],
 * which produces the same sort and length-minimization rules.
 *
 * Two pieces are non-obvious and worth stating up front:
 *
 * 1. **`EventPayload` is a serde externally-tagged enum** — `Assert/Attest/Link/...` serialize
 *    as a one-pair outer map whose key is the variant name and whose value is the inner field
 *    map. So an Attest payload is `{"Attest": {target_event_ids: [...], purpose: "treatment"}}`,
 *    NOT a flat `{type: "Attest", ...}`.
 *
 * 2. **`EventId` is a `uuid::Uuid` and serializes as a 16-byte CBOR byte string** in
 *    non-human-readable mode (which is what ciborium reports). NOT a hex/text string. We
 *    therefore emit each event id as a `bstr` of length 16, MSB-first per RFC 4122.
 *
 * Both rules are checked by the conformance suite once those tests are wired in (the round-
 * trip property test in `crates/creda-events/tests/roundtrip.rs` is the spine).
 */
object EventPayloadCbor {

    private val canonical: CBOREncodeOptions = CBOREncodeOptions("ctap2Canonical=true")

    // ---- Encoders for the EventPayload variants the UI exercises today ---------------------

    /**
     * Build the canonical CBOR for `EventPayload::Attest { target_event_ids, purpose }`.
     * @param targetEventIds parent event UUIDs the clinician is attesting reliance on
     * @param purpose AttestPurpose discriminant in kebab-case ("treatment", "payment", …)
     */
    fun encodeAttest(targetEventIds: List<UUID>, purpose: String): ByteArray =
        wrapVariant("Attest", CBORObject.NewMap().apply {
            // Key order matters for *intent* but the canonical encoder will re-sort on
            // EncodeToBytes(); we still keep insertion order matching the Rust struct so the
            // intermediate object is readable in a debugger.
            Add("target_event_ids", uuidArray(targetEventIds))
            Add("purpose", purpose)
        }).EncodeToBytes(canonical)

    /** Valid `ContestReasonCode` values (§3.4.3), kebab-case to mirror the Rust enum. */
    val CONTEST_REASON_CODES = setOf(
        "distinct-patients", "demographic-conflict", "duplicate-record", "other",
    )

    /**
     * Build the canonical CBOR for `EventPayload::Contest { target_link_id, reason }`.
     *
     * `reason` is the Rust struct `ContestReason { code, detail? }` (§3.4.3) — a map with a kebab
     * `code` and an optional free-text `detail`, NOT the legacy externally-tagged `{Other: text}`.
     * `detail` is omitted when null, matching serde `skip_serializing_if = "Option::is_none"`.
     *
     * @param targetLinkId the Link event being contested
     * @param code one of [CONTEST_REASON_CODES]
     * @param detail optional free-text elaboration
     */
    fun encodeContest(targetLinkId: UUID, code: String, detail: String? = null): ByteArray {
        require(code in CONTEST_REASON_CODES) { "invalid contest reason code: $code" }
        return wrapVariant("Contest", CBORObject.NewMap().apply {
            Add("target_link_id", uuidBytes(targetLinkId))
            Add("reason", CBORObject.NewMap().apply {
                Add("code", code)
                if (detail != null) Add("detail", detail)
            })
        }).EncodeToBytes(canonical)
    }

    /** Valid `TombstoneBasis` values (§3.4.6), kebab-case to mirror the Rust enum. */
    val TOMBSTONE_BASES = setOf(
        "right-to-be-forgotten", "state-law", "court-order", "other",
    )

    /**
     * Build the canonical CBOR for `EventPayload::Tombstone { target_event_ids, legal_basis }`
     * (§3.4.6, right-to-be-forgotten). The targets are the events whose stored demographic content
     * Core scrubs to husks on applying this event; `legalBasis` is one of [TOMBSTONE_BASES]. Shape
     * mirrors `encodeAttest`: a `Vec<EventId>` of 16-byte UUID byte strings plus a kebab-case
     * unit-enum string (`TombstoneBasis` carries `#[serde(rename_all = "kebab-case")]`).
     */
    fun encodeTombstone(targetEventIds: List<UUID>, legalBasis: String): ByteArray {
        require(targetEventIds.isNotEmpty()) { "Tombstone must reference at least one target event" }
        require(legalBasis in TOMBSTONE_BASES) { "invalid tombstone legal basis: $legalBasis" }
        return wrapVariant("Tombstone", CBORObject.NewMap().apply {
            Add("target_event_ids", uuidArray(targetEventIds))
            Add("legal_basis", legalBasis)
        }).EncodeToBytes(canonical)
    }

    // ---- Decoder for the IdentityEventNode response shape ----------------------------------

    /**
     * Pluck the fields of an `IdentityEventNode` (the CBOR Core returns from CreateEvent) that
     * the bridge needs to build a CredaProvenance. Intentionally **not** a full round-trip —
     * we only need enough to project a FHIR Provenance for the UI.
     */
    data class EventNodeView(
        val id: UUID,
        val eventType: String,
        val institutionFingerprint: ByteArray,
        val wallClockTimestamp: String,
        val logicalClock: Long,
        val parentIds: List<UUID>,
        val signatureAlgorithm: String,
        val signaturePublicKeyFingerprint: ByteArray,
        val signatureBytes: ByteArray,
    )

    fun decodeEventNode(cbor: ByteArray): EventNodeView {
        val obj = CBORObject.DecodeFromBytes(cbor)
        // id: 16-byte bstr (uuid::Uuid in non-human-readable serde mode)
        // id / parent_ids are `Uuid` → 16-byte bstr; institution_id and the signature's
        // fingerprint/bytes are `Vec<u8>` → CBOR array of ints (see fingerprintArray). The two
        // byte-valued shapes differ on the wire, so they decode through different helpers.
        val id = bytesToUuid(obj["id"].GetByteString())
        val eventType = obj["event_type"].AsString()
        val institutionFingerprint = bytesFromCborArray(obj["institution_id"])
        val wall = obj["wall_clock_timestamp"].AsString()
        val logical = obj["logical_clock"].AsInt64Value()
        val parents = if (obj.ContainsKey("parent_ids")) {
            (0 until obj["parent_ids"].size())
                .map { bytesToUuid(obj["parent_ids"][it].GetByteString()) }
        } else emptyList()
        val sig = obj["signature"]
        return EventNodeView(
            id = id,
            eventType = eventType,
            institutionFingerprint = institutionFingerprint,
            wallClockTimestamp = wall,
            logicalClock = logical,
            parentIds = parents,
            signatureAlgorithm = sig["algorithm"].AsString(),
            signaturePublicKeyFingerprint = bytesFromCborArray(sig["public_key_fingerprint"]),
            signatureBytes = bytesFromCborArray(sig["signature_bytes"]),
        )
    }

    // ---- Payload-detail decoder for the Provenance projection (§8.2.3 follow-up) ------------

    /**
     * The event-type-specific payload fields the Provenance projection surfaces to clients via
     * the `event-payload` extension (the §8.2.3 mapper's documented follow-up). Fields are null
     * when the variant does not carry them. Tokens stay tokens — the bridge never de-tokenizes
     * (§3.2); rendering is a client concern (demo tokens embed their display form, e.g.
     * `tok:demo:1971-08-04`).
     */
    data class PayloadDetails(
        val verificationMethod: String? = null,
        val dateOfBirthToken: String? = null,
        val nameFamilyToken: String? = null,
        val nameGivenToken: String? = null,
        val confidenceScoreBps: Int? = null,
        val linkMethod: String? = null,
        val purpose: String? = null,
        val amendmentReason: String? = null,
        val contestReason: String? = null,
    )

    /**
     * Decode the type-specific payload fields of an `IdentityEventNode`. Best-effort by design:
     * an unknown variant or a tombstoned/empty payload yields an all-null view rather than an
     * error, so the projection keeps working across event-model evolution.
     */
    fun decodePayloadDetails(cbor: ByteArray): PayloadDetails {
        val obj = CBORObject.DecodeFromBytes(cbor)
        val payload = opt(obj, "payload") ?: return PayloadDetails()
        // EventPayload is a serde externally-tagged enum: a one-pair map {VariantName: fields}.
        if (payload.getType() != com.upokecenter.cbor.CBORType.Map || payload.size() == 0) {
            return PayloadDetails()
        }
        val variant = payload.keys.firstOrNull()?.AsString() ?: return PayloadDetails()
        val body = opt(payload, variant) ?: return PayloadDetails()
        return when (variant) {
            "Assert" -> {
                val demo = opt(body, "demographics")
                PayloadDetails(
                    verificationMethod = opt(body, "verification_method")?.AsString(),
                    dateOfBirthToken = opt(demo, "date_of_birth")?.AsString(),
                    nameFamilyToken = firstToken(opt(demo, "name_family")),
                    nameGivenToken = firstToken(opt(demo, "name_given")),
                )
            }
            "Link" -> PayloadDetails(
                confidenceScoreBps = opt(body, "confidence_score")?.AsInt32Value(),
                linkMethod = opt(body, "method")?.AsString(),
            )
            "Attest" -> PayloadDetails(purpose = opt(body, "purpose")?.AsString())
            "Amend" -> PayloadDetails(
                dateOfBirthToken = opt(opt(body, "updated_demographics"), "date_of_birth")?.AsString(),
                amendmentReason = opt(body, "amendment_reason")?.AsString(),
            )
            "Contest" -> PayloadDetails(contestReason = decodeContestReason(opt(body, "reason")))
            "AuthorizationGrant" -> PayloadDetails(purpose = opt(body, "purpose")?.AsString())
            else -> PayloadDetails()
        }
    }

    /** Map-safe key lookup: the value at [key] if [obj] is a map containing it, else null. */
    private fun opt(obj: CBORObject?, key: String): CBORObject? =
        if (obj != null && obj.getType() == com.upokecenter.cbor.CBORType.Map && obj.ContainsKey(key)) {
            obj[key]
        } else {
            null
        }

    /**
     * Render a ContestReason for display. The canonical shape (what both Core and our own
     * encodeContest now emit) is the struct `{code, detail?}`. The legacy externally-tagged
     * `{"Other": <text>}` and a bare text string are still accepted defensively, so any event
     * written before the reconciliation still renders rather than dropping its reason.
     */
    private fun decodeContestReason(reason: CBORObject?): String? = when {
        reason == null -> null
        opt(reason, "code") != null -> {
            val code = reason["code"].AsString()
            val detail = opt(reason, "detail")?.AsString()
            if (detail != null) "$code: $detail" else code
        }
        opt(reason, "Other") != null -> reason["Other"].AsString()
        reason.getType() == com.upokecenter.cbor.CBORType.TextString -> reason.AsString()
        else -> null
    }

    /** First entry of a `Vec<TokenizedString>` CBOR array, or null. */
    private fun firstToken(arr: CBORObject?): String? =
        if (arr != null && arr.getType() == com.upokecenter.cbor.CBORType.Array && arr.size() > 0) {
            arr[0].AsString()
        } else {
            null
        }

    // ---- Encoders for the portable-authorization payloads (§4.3) — F0 ----------------------

    /**
     * Audience of an `AuthorizationGrant` (§4.3.1). Mirrors the Rust `GrantAudience` *externally
     * tagged* enum: each variant serializes as a one-pair map `{VariantName: value}`.
     *
     * The byte-shape distinction that matters: `InstitutionId` wraps a `CertificateFingerprint`
     * (`Vec<u8>`), which ciborium serializes via `serialize_seq` as a **CBOR array of integers**
     * — NOT a byte string (unlike `Uuid`, which is a 16-byte bstr). `InstitutionClass` /
     * `ConstrainedWildcard` wrap plain text.
     */
    sealed interface Audience {
        data class InstitutionId(val fingerprint: ByteArray) : Audience
        data class InstitutionClass(val name: String) : Audience
        data class ConstrainedWildcard(val pattern: String) : Audience
    }

    /**
     * Scope of an `AuthorizationGrant` (§4.3.1). Each field is `skip_serializing_if` empty on the
     * Rust side, so empty collections are omitted; an all-empty scope is the empty map `{}` and
     * means "the whole subgraph the grant is attached to".
     */
    data class Scope(
        val subgraphSegments: List<UUID> = emptyList(),
        val eventTypes: List<String> = emptyList(),
        val dataCategories: List<String> = emptyList(),
    )

    /** Optional quantitative bounds (§4.3.1); any absent field is omitted from the CBOR. */
    data class Volume(
        val maxRecords: Long? = null,
        val maxRequests: Long? = null,
        val ratePerHour: Long? = null,
    )

    /**
     * Build canonical CBOR for `EventPayload::AuthorizationGrant` (§4.3.1). Field presence mirrors
     * the Rust struct's `skip_serializing_if`: `expiration` and `volume_constraints` are omitted
     * when null; empty scope sub-collections are omitted. `purpose` and `useMode` are the
     * kebab-case discriminants ("treatment", "read-and-rely", …) — callers pass validated tokens.
     */
    fun encodeAuthorizationGrant(
        scope: Scope,
        audience: Audience,
        purpose: String,
        useMode: String,
        expiration: String? = null,
        volume: Volume? = null,
    ): ByteArray =
        wrapVariant("AuthorizationGrant", CBORObject.NewMap().apply {
            Add("scope", encodeScope(scope))
            Add("audience", encodeAudience(audience))
            Add("purpose", purpose)
            Add("use_mode", useMode)
            if (expiration != null) Add("expiration", expiration)
            if (volume != null) Add("volume_constraints", encodeVolume(volume))
        }).EncodeToBytes(canonical)

    /**
     * Build canonical CBOR for `EventPayload::Amend` (§3.4.5) carrying a DOB-only demographics
     * update — the clinician DOB-resolution flow. `Demographics` omits absent optionals, and
     * `TokenizedDate` is a serde newtype (serializes as its inner string), so the updated
     * demographics map is just `{"date_of_birth": <token>}`.
     */
    fun encodeAmendDob(targetEventId: UUID, dobToken: String, reason: String): ByteArray =
        wrapVariant("Amend", CBORObject.NewMap().apply {
            Add("target_event_id", uuidBytes(targetEventId))
            Add("updated_demographics", CBORObject.NewMap().apply { Add("date_of_birth", dobToken) })
            Add("amendment_reason", reason)
        }).EncodeToBytes(canonical)

    /** Build canonical CBOR for `EventPayload::AuthorizationRevocation` (§4.3.2). */
    fun encodeAuthorizationRevocation(targetGrantId: UUID): ByteArray =
        wrapVariant("AuthorizationRevocation", CBORObject.NewMap().apply {
            Add("target_grant_id", uuidBytes(targetGrantId))
        }).EncodeToBytes(canonical)

    /**
     * Build canonical CBOR for `EventPayload::ExportReceipt` (§4.3.3). `requestingInstitution` is a
     * `CertificateFingerprint` and so is emitted as a CBOR array of ints, not a byte string.
     */
    fun encodeExportReceipt(
        governingGrantId: UUID,
        requestingInstitution: ByteArray,
        releasedScope: Scope,
    ): ByteArray =
        wrapVariant("ExportReceipt", CBORObject.NewMap().apply {
            Add("governing_grant_id", uuidBytes(governingGrantId))
            Add("requesting_institution", fingerprintArray(requestingInstitution))
            Add("released_scope", encodeScope(releasedScope))
        }).EncodeToBytes(canonical)

    private fun encodeScope(scope: Scope): CBORObject = CBORObject.NewMap().apply {
        if (scope.subgraphSegments.isNotEmpty()) Add("subgraph_segments", uuidArray(scope.subgraphSegments))
        if (scope.eventTypes.isNotEmpty()) Add("event_types", stringArray(scope.eventTypes))
        if (scope.dataCategories.isNotEmpty()) Add("data_categories", stringArray(scope.dataCategories))
    }

    private fun encodeAudience(audience: Audience): CBORObject = when (audience) {
        is Audience.InstitutionId ->
            CBORObject.NewMap().apply { Add("InstitutionId", fingerprintArray(audience.fingerprint)) }
        is Audience.InstitutionClass ->
            CBORObject.NewMap().apply { Add("InstitutionClass", audience.name) }
        is Audience.ConstrainedWildcard ->
            CBORObject.NewMap().apply { Add("ConstrainedWildcard", audience.pattern) }
    }

    private fun encodeVolume(volume: Volume): CBORObject = CBORObject.NewMap().apply {
        volume.maxRecords?.let { Add("max_records", it) }
        volume.maxRequests?.let { Add("max_requests", it) }
        volume.ratePerHour?.let { Add("rate_per_hour", it) }
    }

    /** Cheap peek at a node's `event_type` discriminant without decoding the full node. */
    fun eventTypeOf(cbor: ByteArray): String =
        CBORObject.DecodeFromBytes(cbor)["event_type"].AsString()

    // ---- Decoders for the returned authorization event nodes (§4.3) — F0 -------------------

    /** Decoded view of the fields a Consent projection needs from a Grant node. */
    data class GrantNodeView(
        val id: UUID,
        val institutionFingerprint: ByteArray,
        val wallClockTimestamp: String,
        val audience: Audience,
        val purpose: String,
        val useMode: String,
        val expiration: String?,
    )

    /** Decoded view of an ExportReceipt node, for the AuditEvent projection. */
    data class ExportReceiptNodeView(
        val id: UUID,
        val institutionFingerprint: ByteArray,
        val wallClockTimestamp: String,
        val governingGrantId: UUID,
        val requestingInstitution: ByteArray,
    )

    /** Decoded view of a Revocation node, for the Consent-inactive projection. */
    data class RevocationNodeView(
        val id: UUID,
        val institutionFingerprint: ByteArray,
        val wallClockTimestamp: String,
        val targetGrantId: UUID,
    )

    fun decodeGrantNode(cbor: ByteArray): GrantNodeView {
        val obj = CBORObject.DecodeFromBytes(cbor)
        val inner = variantBody(obj["payload"], "AuthorizationGrant")
        return GrantNodeView(
            id = bytesToUuid(obj["id"].GetByteString()),
            institutionFingerprint = bytesFromCborArray(obj["institution_id"]),
            wallClockTimestamp = obj["wall_clock_timestamp"].AsString(),
            audience = decodeAudience(inner["audience"]),
            purpose = inner["purpose"].AsString(),
            useMode = inner["use_mode"].AsString(),
            expiration = if (inner.ContainsKey("expiration")) inner["expiration"].AsString() else null,
        )
    }

    fun decodeRevocationNode(cbor: ByteArray): RevocationNodeView {
        val obj = CBORObject.DecodeFromBytes(cbor)
        val inner = variantBody(obj["payload"], "AuthorizationRevocation")
        return RevocationNodeView(
            id = bytesToUuid(obj["id"].GetByteString()),
            institutionFingerprint = bytesFromCborArray(obj["institution_id"]),
            wallClockTimestamp = obj["wall_clock_timestamp"].AsString(),
            targetGrantId = bytesToUuid(inner["target_grant_id"].GetByteString()),
        )
    }

    fun decodeExportReceiptNode(cbor: ByteArray): ExportReceiptNodeView {
        val obj = CBORObject.DecodeFromBytes(cbor)
        val inner = variantBody(obj["payload"], "ExportReceipt")
        return ExportReceiptNodeView(
            id = bytesToUuid(obj["id"].GetByteString()),
            institutionFingerprint = bytesFromCborArray(obj["institution_id"]),
            wallClockTimestamp = obj["wall_clock_timestamp"].AsString(),
            governingGrantId = bytesToUuid(inner["governing_grant_id"].GetByteString()),
            requestingInstitution = bytesFromCborArray(inner["requesting_institution"]),
        )
    }

    private fun decodeAudience(obj: CBORObject): Audience = when {
        obj.ContainsKey("InstitutionId") -> Audience.InstitutionId(bytesFromCborArray(obj["InstitutionId"]))
        obj.ContainsKey("InstitutionClass") -> Audience.InstitutionClass(obj["InstitutionClass"].AsString())
        obj.ContainsKey("ConstrainedWildcard") -> Audience.ConstrainedWildcard(obj["ConstrainedWildcard"].AsString())
        else -> throw IllegalArgumentException("unrecognized GrantAudience variant: ${obj.keys}")
    }

    /** Unwrap a one-pair externally-tagged variant map, asserting the expected variant name. */
    private fun variantBody(obj: CBORObject, variant: String): CBORObject {
        require(obj.ContainsKey(variant)) { "expected payload variant '$variant', got ${obj.keys}" }
        return obj[variant]
    }

    // ---- UUID <-> 16-byte conversion -------------------------------------------------------

    /** Canonical hex string (8-4-4-4-12) of a 16-byte UUID, MSB-first. */
    fun bytesToUuid(bytes: ByteArray): UUID {
        require(bytes.size == 16) { "expected 16-byte UUID, got ${bytes.size}" }
        val buf = ByteBuffer.wrap(bytes)
        return UUID(buf.long, buf.long)
    }

    /** 16-byte MSB-first encoding of a UUID — the form the Rust side decodes. */
    fun uuidBytes(uuid: UUID): ByteArray {
        val buf = ByteBuffer.allocate(16)
        buf.putLong(uuid.mostSignificantBits)
        buf.putLong(uuid.leastSignificantBits)
        return buf.array()
    }

    // ---- Internal helpers ------------------------------------------------------------------

    private fun uuidArray(uuids: List<UUID>): CBORObject =
        CBORObject.NewArray().apply { uuids.forEach { Add(uuidBytes(it)) } }

    private fun stringArray(values: List<String>): CBORObject =
        CBORObject.NewArray().apply { values.forEach { Add(it) } }

    /**
     * Encode a `Vec<u8>` (e.g. a `CertificateFingerprint`) the way ciborium does: a **CBOR array
     * of unsigned integers**, one per byte — NOT a byte string. serde's `Vec<u8>` goes through
     * `serialize_seq`, and ciborium 0.2.2 has no byte-string special-case for it (confirmed in
     * `ciborium-0.2.2/src/ser/mod.rs`). Getting this wrong is the single most likely F0 mistake,
     * because `Uuid` *does* encode as a bstr — the two byte-valued types diverge on the wire.
     */
    private fun fingerprintArray(bytes: ByteArray): CBORObject =
        CBORObject.NewArray().apply { bytes.forEach { Add(it.toInt() and 0xFF) } }

    /** Inverse of [fingerprintArray]: read a CBOR array of ints back into bytes. */
    fun bytesFromCborArray(obj: CBORObject): ByteArray {
        require(obj.getType() == com.upokecenter.cbor.CBORType.Array) {
            "expected a CBOR array of byte-ints, got ${obj.getType()}"
        }
        val out = ByteArray(obj.size())
        for (i in 0 until obj.size()) out[i] = (obj[i].AsInt32Value() and 0xFF).toByte()
        return out
    }

    private fun wrapVariant(variant: String, inner: CBORObject): CBORObject =
        CBORObject.NewMap().apply { Add(variant, inner) }
}
