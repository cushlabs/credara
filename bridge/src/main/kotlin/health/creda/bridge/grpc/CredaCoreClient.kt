package health.creda.bridge.grpc

import com.google.protobuf.ByteString
import health.creda.bridge.cbor.EventPayloadCbor
import health.creda.grpc.AuthReply
import health.creda.grpc.AuthRequest
import health.creda.grpc.CreateEventRequest
import health.creda.grpc.CredaGrpc
import health.creda.grpc.Empty
import health.creda.grpc.EntryPoints
import health.creda.grpc.GetEventRequest
import health.creda.grpc.GrantPurpose
import health.creda.grpc.MatchRequest
import health.creda.grpc.RequesterContext
import health.creda.grpc.SubgraphEventsRequest
import health.creda.grpc.UseMode
import io.grpc.ManagedChannel
import io.grpc.netty.NettyChannelBuilder
import io.netty.channel.epoll.EpollDomainSocketChannel
import io.netty.channel.epoll.EpollEventLoopGroup
import io.netty.channel.unix.DomainSocketAddress
import jakarta.annotation.PreDestroy
import org.springframework.beans.factory.annotation.Value
import org.springframework.stereotype.Component

/**
 * Thin gRPC client to Creda Core over the in-pod Unix domain socket (§8.3.1, §10.4.3). Wraps the
 * generated [CredaGrpc] stub; performs no logic of its own. Events/payloads cross the boundary as
 * canonical-CBOR bytes — the same bytes Core signs and hashes (§10.1.3) — so the Bridge never has
 * to mirror the event schema.
 *
 * TODO(bridge-verify): netty's epoll transport is Linux-only (the Bridge runs in a Linux pod). For
 * local macOS development, swap to the kqueue transport. The exact NettyChannelBuilder UDS API is
 * version-sensitive.
 */
@Component
class CredaCoreClient(
    @Value("\${creda.core-socket}") socketPath: String,
) {
    // Two transports, mirroring Core's parse_endpoint (grpc.rs): a `tcp://host:port` value means
    // Core listens on TCP (the testbed's seed/reset Jobs need TCP to reach Core, and TCP also
    // works on macOS where netty's epoll is unavailable); anything else is a Unix-domain-socket
    // path (the in-pod default, §8.3.1). In TCP mode `0.0.0.0` is Core's *listen* address — from
    // the bridge (same pod) the dial address is loopback.
    // Final, initializer-assigned (no init block) so the properties are unambiguously `val` — the
    // channel is built by the top-level `buildChannel` below. eventLoopGroup is declared first so
    // the channel initializer can pass it to the UDS builder; it's null in TCP mode.
    private val eventLoopGroup: EpollEventLoopGroup? =
        if (socketPath.startsWith("tcp://")) null else EpollEventLoopGroup()
    private val channel: ManagedChannel = buildChannel(socketPath, eventLoopGroup)

    private val stub = CredaGrpc.newBlockingStub(channel)

    /** CreateEvent (§10.1.3): payload is canonical-CBOR EventPayload; returns the event's CBOR. */
    fun createEvent(payloadCbor: ByteArray, parentIds: List<ByteArray>): ByteArray {
        val req = CreateEventRequest.newBuilder()
            .setEventPayloadCbor(ByteString.copyFrom(payloadCbor))
            .apply { parentIds.forEach { addParentIds(ByteString.copyFrom(it)) } }
            .build()
        return stub.createEvent(req).eventCbor.toByteArray()
    }

    /** GetEvent (§10.1.3): returns the event's CBOR, or null if not present locally. */
    fun getEvent(id: ByteArray): ByteArray? {
        val reply = stub.getEvent(GetEventRequest.newBuilder().setId(ByteString.copyFrom(id)).build())
        return if (reply.found) reply.eventCbor.toByteArray() else null
    }

    /**
     * GetSubgraphEvents (§10.1.3): a subgraph's events as canonical CBOR, optionally filtered by
     * IdentityEventType variant names, sorted by logical clock. The read surface behind the
     * `Consent?patient=` search (§8.2.9 read-back).
     */
    fun getSubgraphEvents(entryPoints: List<ByteArray>, eventTypes: List<String>): List<ByteArray> {
        val req = SubgraphEventsRequest.newBuilder()
            .apply { entryPoints.forEach { addEntryPoints(ByteString.copyFrom(it)) } }
            .addAllEventTypes(eventTypes)
            .build()
        return stub.getSubgraphEvents(req).eventCborList.map { it.toByteArray() }
    }

    /** GetEffectiveIdentity (§5.2.4). Core currently returns a debug rendering (see core grpc.rs). */
    fun effectiveIdentityDebug(entryPoints: List<ByteArray>): String {
        val req = EntryPoints.newBuilder()
            .apply { entryPoints.forEach { addIds(ByteString.copyFrom(it)) } }
            .build()
        return stub.getEffectiveIdentity(req).effectiveIdentityDebug
    }

    /** A demographic field's effective value (§5.3): tokenized value + confidence + supporting ids. */
    data class EffectiveValue(val value: String, val confidence: Int, val supporting: List<String>)

    /** A demographic field's effective projection: confidence-sorted values + the disputed flag. */
    data class EffectiveField(val key: String, val disputed: Boolean, val values: List<EffectiveValue>)

    /**
     * GetEffectiveIdentity structured (§5.2.4 / §5.3): the per-field projection Core computes —
     * confidence-weighted, attestation-amplified, disagreement-flagged. This is the identity
     * reasoning the clients must NOT re-implement (§8.3.2).
     */
    fun effectiveIdentity(entryPoints: List<ByteArray>): List<EffectiveField> {
        val req = EntryPoints.newBuilder()
            .apply { entryPoints.forEach { addIds(ByteString.copyFrom(it)) } }
            .build()
        return stub.getEffectiveIdentity(req).fieldsList.map { f ->
            EffectiveField(
                key = f.key,
                disputed = f.disputed,
                values = f.valuesList.map { v ->
                    EffectiveValue(
                        value = v.value,
                        confidence = v.confidence,
                        supporting = v.supportingList.map {
                            EventPayloadCbor.bytesToUuid(it.toByteArray()).toString()
                        },
                    )
                },
            )
        }
    }

    /** MatchByTokens (§5.2.5): candidate entry-point event UUIDs for the given demographic tokens. */
    fun matchByTokens(tokens: List<String>): List<ByteArray> {
        val req = MatchRequest.newBuilder().addAllTokens(tokens).build()
        return stub.matchByTokens(req).idsList.map { it.toByteArray() }
    }

    /**
     * EvaluateAuthorization (§4.6): run Core's seven-step evaluation for a requesting institution
     * against the patient subgraph. The query is a structured request (Core's `AuthorizationQuery`
     * is not serde-serializable, so the contract is explicit protobuf rather than opaque CBOR).
     */
    fun evaluateAuthorization(
        entryPoints: List<ByteArray>,
        requesterFingerprint: ByteArray,
        purpose: GrantPurpose,
        useMode: UseMode,
        requestedEventTypes: List<String> = emptyList(),
        requestedSegments: List<ByteArray> = emptyList(),
        requestedDataCategories: List<String> = emptyList(),
    ): AuthReply {
        val req = AuthRequest.newBuilder()
            .apply { entryPoints.forEach { addEntryPoints(ByteString.copyFrom(it)) } }
            .setRequester(
                RequesterContext.newBuilder()
                    .setFingerprint(ByteString.copyFrom(requesterFingerprint))
                    .build(),
            )
            .setPurpose(purpose)
            .setUseMode(useMode)
            .addAllRequestedEventTypes(requestedEventTypes)
            .apply { requestedSegments.forEach { addRequestedSegments(ByteString.copyFrom(it)) } }
            .addAllRequestedDataCategories(requestedDataCategories)
            .build()
        return stub.evaluateAuthorization(req)
    }

    /** GetMetrics (§10.1.3): local event count. */
    fun eventCount(): Long = stub.getMetrics(Empty.getDefaultInstance()).eventCount

    /**
     * ListInstitutions: the distinct institution audience names appearing in AuthorizationGrants
     * across the local store — the read behind `GET /Organization`. A discovery surface, not a
     * directory: Creda models institutions as identities/fingerprints, not full Organization records.
     */
    fun listInstitutions(): List<String> =
        stub.listInstitutions(Empty.getDefaultInstance()).namesList

    /** A subgraph's §8.2.2 CredaPatient identity envelope (deterministic, peer-identical). */
    data class SubgraphIdentity(
        val subgraphId: ByteArray,
        val rootSet: List<java.util.UUID>,
        val lastModifiedEvent: java.util.UUID?,
    )

    /**
     * GetSubgraphIdentity (§8.2.2): the deterministic subgraph identifier (Blake3 of the sorted
     * root set), the root set, and the last-modified event — the data behind CredaPatient's
     * mustSupport extensions. Computed in Core's shared graph logic so peers agree byte-for-byte.
     */
    fun subgraphIdentity(entryPoints: List<ByteArray>): SubgraphIdentity {
        val req = EntryPoints.newBuilder()
            .apply { entryPoints.forEach { addIds(ByteString.copyFrom(it)) } }
            .build()
        val reply = stub.getSubgraphIdentity(req)
        val lastModified = reply.lastModifiedEvent.toByteArray()
        return SubgraphIdentity(
            subgraphId = reply.subgraphId.toByteArray(),
            rootSet = reply.rootSetList.map { EventPayloadCbor.bytesToUuid(it.toByteArray()) },
            lastModifiedEvent =
                if (lastModified.isNotEmpty()) EventPayloadCbor.bytesToUuid(lastModified) else null,
        )
    }

    @PreDestroy
    fun shutdown() {
        channel.shutdownNow()
        eventLoopGroup?.shutdownGracefully()
    }
}

/**
 * Build the gRPC channel for the given `creda.core-socket` (mirrors Core's `parse_endpoint`):
 *   - `tcp://host:port` → TCP (testbed seed/reset Jobs reach Core; also works on macOS where
 *     netty epoll is unavailable). `0.0.0.0` is Core's listen address; the bridge dials loopback.
 *   - anything else → Unix domain socket (in-pod default, §8.3.1). The `:authority` is overridden
 *     to a fixed sentinel because the UDS path contains slashes that Core's tonic server rejects
 *     as a PROTOCOL_ERROR.
 * Free function (no `this`) so it can initialize the `channel` property directly — no init block.
 */
private fun buildChannel(socketPath: String, group: EpollEventLoopGroup?): ManagedChannel =
    if (socketPath.startsWith("tcp://")) {
        val target = socketPath.removePrefix("tcp://").replace("0.0.0.0", "127.0.0.1")
        NettyChannelBuilder.forTarget(target).usePlaintext().build()
    } else {
        NettyChannelBuilder
            .forAddress(DomainSocketAddress(socketPath))
            .eventLoopGroup(group)
            .channelType(EpollDomainSocketChannel::class.java)
            .overrideAuthority("creda-core.local")
            .usePlaintext()
            .build()
    }
