package health.creda.bridge.providers

import ca.uhn.fhir.interceptor.api.Hook
import ca.uhn.fhir.interceptor.api.Interceptor
import ca.uhn.fhir.interceptor.api.Pointcut
import ca.uhn.fhir.rest.api.server.RequestDetails
import org.springframework.beans.factory.ObjectProvider
import org.springframework.stereotype.Component
import java.time.Instant

/**
 * Captures a read-side access record for every completed Bridge interaction (§8.2.4, §9.1.6) and
 * hands it to the [AccessAuditSink] (default: structured audit log → SIEM). This is the "who queried
 * which subgraph" stream — distinct from the on-chain disclosure ledger ([AuditEventResourceProvider],
 * the ExportReceipt record of *what data moved*).
 *
 * Hooks `SERVER_PROCESSING_COMPLETED_NORMALLY`, so a record is written after a successful
 * interaction. The sink is always present (resolved to [Slf4jAccessAuditSink] when no SIEM bean is
 * registered) — auditing must not fail open. Registered on the server in `CredaRestfulServer.initialize()`.
 */
@Interceptor
@Component
class BridgeAccessAuditInterceptor(
    sinks: ObjectProvider<AccessAuditSink>,
) {
    private val sink: AccessAuditSink = sinks.getIfAvailable { Slf4jAccessAuditSink() }

    @Hook(Pointcut.SERVER_PROCESSING_COMPLETED_NORMALLY)
    fun onCompleted(requestDetails: RequestDetails) {
        sink.record(
            AccessAuditRecord(
                recordedAt = Instant.now(),
                operation = requestDetails.restOperationType?.name ?: "UNKNOWN",
                resourceType = requestDetails.resourceName,
                resourceId = requestDetails.id?.value,
                requestPath = requestDetails.requestPath,
                requestId = requestDetails.requestId,
            ),
        )
    }
}
