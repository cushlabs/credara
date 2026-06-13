//! Tiny HTTP health server (spec §10.5.3) — `/livez`, `/readyz`, `/metrics`.
//!
//! Bound to the chart-declared `health` port (default 9090). Kubernetes uses these endpoints
//! for liveness and readiness probes:
//!
//! - `/livez`  — always 200 OK while the process is up. Distinguishes "process alive" from
//!   "process able to serve traffic."
//! - `/readyz` — 200 OK once the daemon's startup sequence has completed (engine open, libp2p
//!   listening, registry loaded). 503 before then. Kubernetes withholds traffic from a peer
//!   whose `/readyz` is failing — which is what we want during bootstrap.
//! - `/metrics` — placeholder text/plain endpoint. A real Prometheus exporter lands when the
//!   metric surface from §11.2 is built out; this stub just returns the event count from the
//!   engine so something is present at the chart-declared port for now.
//!
//! No new dependencies. The HTTP we serve is tiny enough to write directly over a tokio
//! TcpListener. This sidesteps pulling in hyper or warp just to answer two-line probes.
//!
//! This module is feature-gated on `grpc` because the daemon (`creda serve`) is gated on `grpc`
//! and there is no other context that wants to spin up an HTTP probe endpoint.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::engine::CredaCore;
use crate::error::{Error, Result};

/// Shared readiness flag. Cloned by the health server and by the daemon's startup sequence;
/// the daemon flips it true once it is ready to serve traffic.
#[derive(Clone, Default)]
pub struct ReadyFlag(Arc<AtomicBool>);

impl ReadyFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_ready(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Run the health server on `addr` (e.g. `0.0.0.0:9090`) until cancelled.
///
/// The server is purposefully minimal — it parses just enough of each HTTP request to find the
/// path on the first line, dispatches to one of `/livez`, `/readyz`, `/metrics`, and writes a
/// short fixed response. No keep-alive, no chunked encoding, no streaming. Each probe is a
/// fresh connection (which matches how kubelet's HTTP probes behave anyway).
pub async fn serve_health(listen: &str, ready: ReadyFlag, core: Arc<CredaCore>) -> Result<()> {
    let listener = TcpListener::bind(listen)
        .await
        .map_err(|e| Error::Io(format!("health: bind {listen}: {e}")))?;
    eprintln!("creda serve: health endpoint listening on http://{listen}");

    loop {
        let (mut stream, _peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("creda serve: health accept error: {e}");
                continue;
            }
        };
        let ready = ready.clone();
        let core = core.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let head = String::from_utf8_lossy(&buf[..n]);
            // Request line is up to the first \r\n; tokenized as "METHOD PATH VERSION".
            let path = head
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            let response = handle(&path, &ready, &core);
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
    }
}

fn handle(path: &str, ready: &ReadyFlag, core: &CredaCore) -> String {
    match path {
        "/livez" => http_response(200, "OK", "text/plain", "alive\n"),
        "/readyz" => {
            if ready.is_ready() {
                http_response(200, "OK", "text/plain", "ready\n")
            } else {
                // 503 is the canonical "not ready yet" for kubelet probes (§10.5.3).
                http_response(503, "Service Unavailable", "text/plain", "starting\n")
            }
        }
        "/metrics" => {
            // Placeholder Prometheus exposition format. A real exporter (§11.2) will replace
            // this — for now, surface event count so kubelet probes see *some* signal at the
            // chart-declared port.
            let events = core.event_count().unwrap_or(0);
            let body = format!(
                "# HELP creda_events_total Number of events in the local store.\n\
                 # TYPE creda_events_total gauge\n\
                 creda_events_total {events}\n"
            );
            http_response(200, "OK", "text/plain; version=0.0.4", &body)
        }
        _ => http_response(404, "Not Found", "text/plain", "not found\n"),
    }
}

fn http_response(code: u16, reason: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readyz_flips_on_set_ready() {
        let r = ReadyFlag::new();
        assert!(!r.is_ready());
        r.set_ready();
        assert!(r.is_ready());
        let clone = r.clone();
        assert!(clone.is_ready(), "ReadyFlag should be Arc-shared, not Copy");
    }

    #[test]
    fn http_response_includes_correct_content_length() {
        let resp = http_response(200, "OK", "text/plain", "hello\n");
        assert!(resp.contains("Content-Length: 6\r\n"));
        assert!(resp.ends_with("hello\n"));
    }
}
