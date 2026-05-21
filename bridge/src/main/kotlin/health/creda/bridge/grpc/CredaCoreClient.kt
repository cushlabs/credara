package health.creda.bridge.grpc

import com.google.protobuf.ByteString
import health.creda.grpc.AuthReply
import health.creda.grpc.AuthRequest
import health.creda.grpc.CreateEventRequest
import health.creda.grpc.CredaGrpc
import health.creda.grpc.Empty
import health.creda.grpc.EntryPoints
import health.creda.grpc.GetEventRequest
import health.creda.grpc.MatchRequest
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
    private val eventLoopGroup = EpollEventLoopGroup()
    private val channel = NettyChannelBuilder
        .forAddress(DomainSocketAddress(socketPath))
        .eventLoopGroup(eventLoopGroup)
        .channelType(EpollDomainSocketChannel::class.java)
        .usePlaintext() // confidentiality/auth are provided by the pod boundary, not TLS here
        .build()
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

    /** EvaluateAuthorization (§4.6). NOTE: Core's gRPC wiring for this is a follow-up (returns
     *  UNIMPLEMENTED today); the engine path is implemented and tested directly. */
    fun evaluateAuthorization(entryPoints: List<ByteArray>, queryCbor: ByteArray): AuthReply {
        val req = AuthRequest.newBuilder()
            .apply { entryPoints.forEach { addEntryPoints(ByteString.copyFrom(it)) } }
            .setQueryCbor(ByteString.copyFrom(queryCbor))
            .build()
        return stub.evaluateAuthorization(req)
    }

    /** GetMetrics (§10.1.3): local event count. */
    fun eventCount(): Long = stub.getMetrics(Empty.getDefaultInstance()).eventCount

    @PreDestroy
    fun shutdown() {
        channel.shutdownNow()
        eventLoopGroup.shutdownGracefully()
    }
}
