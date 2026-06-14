package health.creda.bridge.providers

import org.slf4j.LoggerFactory
import java.time.Instant

/**
 * Read-side access audit (§8.2.4, §9.1.6): every read / search / operation against the Bridge
 * produces an access record — "who **queried** which subgraph, when." Per §8.2.4 these are stored
 * *separately* from the identity DAG and flow to the institution's SIEM. This is that egress seam.
 *
 * The default [Slf4jAccessAuditSink] writes a structured line to the audit logger — a real audit
 * destination (log pipelines forward to SIEM), not a stub. An institution registers its own
 * `AccessAuditSink` bean to ship records straight to its SIEM/SOAR.
 */
fun interface AccessAuditSink {
    fun record(rec: AccessAuditRecord)
}

/**
 * One read-side access event. Only fields the Bridge can truthfully observe — notably **no
 * fabricated principal**: binding the authenticated UDAP/SMART identity is a deployment concern of
 * the auth layer, tracked separately, and is added here when that layer is wired rather than guessed.
 */
data class AccessAuditRecord(
    val recordedAt: Instant,
    val operation: String,
    val resourceType: String?,
    val resourceId: String?,
    val requestPath: String?,
    val requestId: String?,
)

/**
 * Default sink: a structured line on the dedicated audit logger (forwarded to SIEM by the deployment's
 * log pipeline). Always present — auditing is not optional and must not fail open silently.
 */
class Slf4jAccessAuditSink : AccessAuditSink {
    private val audit = LoggerFactory.getLogger("health.creda.bridge.audit.access")

    override fun record(rec: AccessAuditRecord) {
        audit.info(
            "access op={} resourceType={} resourceId={} path={} requestId={} at={}",
            rec.operation,
            rec.resourceType ?: "-",
            rec.resourceId ?: "-",
            rec.requestPath ?: "-",
            rec.requestId ?: "-",
            rec.recordedAt,
        )
    }
}
