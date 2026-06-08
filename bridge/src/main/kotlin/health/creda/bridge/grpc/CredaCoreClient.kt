package health.creda.bridge.grpc

import com.google.protobuf.ByteString
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
    private val eventLoopGroup: EpollEventLoopGroup?
    private val channel: io.grpc.ManagedChannel

    init {
        if (socketPath.startsWith("tcp://")) {
            val target = socketPath.removePrefix("tcp://").replace("0.0.0.0", "127.0.0.1")
            eventLoopGroup = null
            channel = NettyChannelBuilder.forTarget(target).usePlaintext().build()
        } else {
            eventLoopGroup = EpollEventLoopGroup()
            channel = NettyChannelBuilder
                .forAddress(DomainSocketAddress(socketPath))
                .eventLoopGroup(eventLoopGroup)
                .channelType(EpollDomainSocketChannel::class.java)
                // HTTP/2 :authority must be a valid host:port-style authority (RFC 3986); the
                // default for forAddress(DomainSocketAddress) is the socket path, which contains
                // slashes and is rejected by Core's tonic server as a PROTOCOL_ERROR (RST_STREAM
                // closes every stream before the handler runs). Override to a fixed sentinel —
                // the value is arbitrary, only validity matters; no virtual-hosting on Core.
                .overrideAuthority("creda-core.local")
                .usePlaintext() // confidentiality/auth are provided by the pod boundary, not TLS
                .build()
        }
    }

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

    @PreDestroy
    fun shutdown() {
        channel.shutdownNow()
        eventLoopGroup?.shutdownGracefully()
    }
}
