//! Prometheus text exposition for the peer's `/metrics` endpoint (spec §11.2).
//!
//! A real exporter for the operational signals the peer can **truthfully** report at scrape time:
//! build metadata, readiness (mirrors `/readyz`), process start, and store-derived gauges (event
//! and institution counts). It reports only measured values.
//!
//! The golden-signal *counters and histograms* of §11.2.1 — gRPC/FHIR/gossip/AE traffic, latency
//! distributions, and error rates — require request-path instrumentation (a tonic tower layer in
//! Core, the Bridge's HAPI interceptor, gossip/AE hooks). That is the next instrumentation slice;
//! those series are deliberately **not** emitted here rather than reported as fabricated zeros.
//!
//! Hand-rolled exposition (no Prometheus client dependency), matching this crate's dependency-
//! minimal health server. Output is the text exposition format (v0.0.4) Prometheus scrapes by
//! default. Note the two store-derived gauges each scan the store; at pilot scale that is fine
//! (the §11.2.1 note), and they move to incrementally-maintained counters with the instrumentation
//! slice above.

use crate::engine::CredaCore;
use crate::health::ReadyFlag;

/// The point-in-time values the exporter renders. Split from the formatting so exposition is
/// unit-testable without standing up an engine.
pub struct Snapshot {
    pub events: u64,
    pub institutions: u64,
    pub ready: bool,
    pub process_start_secs: u64,
}

/// Gather a [`Snapshot`] from live state and render it. `process_start_secs` is the peer process's
/// Unix start time, captured once by the health server.
pub fn render(core: &CredaCore, ready: &ReadyFlag, process_start_secs: u64) -> String {
    let snapshot = Snapshot {
        events: core.event_count().unwrap_or(0) as u64,
        institutions: core.list_institutions().map(|v| v.len() as u64).unwrap_or(0),
        ready: ready.is_ready(),
        process_start_secs,
    };
    render_snapshot(&snapshot)
}

/// Render a [`Snapshot`] as Prometheus text exposition (v0.0.4).
pub fn render_snapshot(s: &Snapshot) -> String {
    let mut out = String::new();
    metric(
        &mut out,
        "creda_build_info",
        "Build metadata; constant 1 with the version carried as a label.",
        "gauge",
        &format!("creda_build_info{{version=\"{}\"}} 1", env!("CARGO_PKG_VERSION")),
    );
    metric(
        &mut out,
        "creda_up",
        "1 while the metrics exporter is serving.",
        "gauge",
        "creda_up 1",
    );
    metric(
        &mut out,
        "creda_ready",
        "1 once the peer has completed startup (mirrors /readyz), else 0.",
        "gauge",
        &format!("creda_ready {}", u8::from(s.ready)),
    );
    metric(
        &mut out,
        "creda_process_start_time_seconds",
        "Unix start time of the peer process (uptime = time() - this).",
        "gauge",
        &format!("creda_process_start_time_seconds {}", s.process_start_secs),
    );
    metric(
        &mut out,
        "creda_events_total",
        "Events currently in the local store.",
        "gauge",
        &format!("creda_events_total {}", s.events),
    );
    metric(
        &mut out,
        "creda_institutions_total",
        "Distinct grant audiences (institutions) known store-wide.",
        "gauge",
        &format!("creda_institutions_total {}", s.institutions),
    );
    out
}

/// Emit one metric's `# HELP` / `# TYPE` / sample lines in exposition order.
fn metric(out: &mut String, name: &str, help: &str, typ: &str, sample: &str) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push('\n');
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push(' ');
    out.push_str(typ);
    out.push('\n');
    out.push_str(sample);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_snapshot_is_valid_exposition() {
        let body = render_snapshot(&Snapshot {
            events: 42,
            institutions: 3,
            ready: true,
            process_start_secs: 1_700_000_000,
        });

        assert!(body.contains("# HELP creda_events_total"));
        assert!(body.contains("# TYPE creda_events_total gauge"));
        assert!(body.contains("\ncreda_events_total 42\n"));
        assert!(body.contains("\ncreda_institutions_total 3\n"));
        assert!(body.contains("\ncreda_ready 1\n"));
        assert!(body.contains("creda_build_info{version=\""));
        // Well-formed: every metric has exactly one HELP and one TYPE.
        assert_eq!(body.matches("# HELP ").count(), body.matches("# TYPE ").count());
        assert_eq!(body.matches("# HELP ").count(), 6);
    }

    #[test]
    fn ready_renders_zero_when_not_ready() {
        let body = render_snapshot(&Snapshot {
            events: 0,
            institutions: 0,
            ready: false,
            process_start_secs: 0,
        });
        assert!(body.contains("\ncreda_ready 0\n"));
    }
}
