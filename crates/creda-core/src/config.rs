//! Hierarchical configuration (spec §10.1.6).
//!
//! Precedence, lowest to highest: **defaults baked into the binary → TOML file → environment
//! variables → CLI flags**. Configuration is validated at startup and fails loudly — no silent
//! fallback to defaults that could mask misconfiguration (§10.1.6).

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Default authorization posture when no Grant covers a request (spec §9.3.2, §4.6 step 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PostureSetting {
    DenyByDefault,
    TreatmentPresumed,
}

impl PostureSetting {
    /// Map to the graph layer's posture type.
    pub fn to_graph(self) -> creda_graph::DefaultPosture {
        match self {
            PostureSetting::DenyByDefault => creda_graph::DefaultPosture::DenyByDefault,
            PostureSetting::TreatmentPresumed => creda_graph::DefaultPosture::TreatmentPresumed,
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "deny-by-default" => Ok(PostureSetting::DenyByDefault),
            "treatment-presumed" => Ok(PostureSetting::TreatmentPresumed),
            other => Err(Error::Config(format!(
                "unknown default_posture {other:?} (expected deny-by-default or treatment-presumed)"
            ))),
        }
    }
}

/// Resolved peer configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredaConfig {
    /// Directory for the event store and local state.
    pub data_dir: String,
    /// Unix domain socket path for the gRPC API (§10.1.1).
    pub grpc_socket: String,
    /// libp2p listen multiaddr.
    pub libp2p_listen: String,
    /// Default authorization posture.
    pub default_posture: PostureSetting,
    /// Snapshot interval in seconds (§7.3.3; default 6h).
    pub snapshot_interval_secs: u64,
    /// Topic buckets this peer subscribes to (§6.2.4). Empty = none until rebalancing.
    pub subscribed_buckets: Vec<u64>,
    /// Path to the participant key registry — a directory of admitted-participant key files used
    /// to authenticate received events at ingest (§3.6). `None` = no participants admitted, so
    /// inbound replication is refused. Populating it from a UDAP/TEFCA registry is an open
    /// question (App C); see [`crate::registry`].
    pub participant_registry: Option<String>,
    /// libp2p bootstrap peers (§6.1.3) as multiaddrs of the form
    /// `/ip4/.../tcp/.../p2p/<peer-id>`. Empty means rely on Kademlia random-walk / gossip mesh
    /// push (works once the peer has any inbound connection). Populated for the testbed and for
    /// new institutional peers joining an established network.
    pub bootstrap_peers: Vec<String>,
    /// Path to the institutional Ed25519 signing key (raw 32 bytes, §10.1.4). When `None`, the
    /// daemon generates an ephemeral key per startup — fine for unit tests and CLI commands,
    /// fatal for a peer that needs to be recognized across restarts (its institution_id would
    /// change each time). Production mounts a k8s Secret at this path.
    pub signing_key_path: Option<String>,
    /// `host:port` for the HTTP health endpoint (§10.5.3 — `/livez`, `/readyz`, `/metrics`).
    /// Bound by the daemon. Kubelet calls these for liveness and readiness probes; without it
    /// bound the chart's StatefulSet pod will never reach Ready.
    pub health_listen: String,
    /// Testbed convenience: when `true`, the daemon subscribes to every bucket (0..BUCKET_COUNT)
    /// regardless of `subscribed_buckets`. Heavy in production (1024 gossipsub topics per
    /// peer); fine for kind/k3d test beds where the synthetic data could land anywhere in the
    /// bucket space and the gossip volume is negligible.
    pub subscribe_all_buckets: bool,
    /// **Synthetic-only guardrail** (closed-pilot safety, §11.4 / docs/PILOT.md). When `true`:
    /// every locally created event is auto-tagged as `test_data`, and any ingested event that is
    /// NOT `test_data`-tagged is **rejected**. This makes "synthetic only" an enforced network
    /// invariant rather than a policy — a misconfigured client physically cannot put real data on
    /// the network, and untagged events cannot propagate in. Default `false` (normal operation).
    pub synthetic_only: bool,
}

impl Default for CredaConfig {
    fn default() -> Self {
        // Sensible production-ready defaults — a peer runs unconfigured for development (§10.1.6).
        Self {
            data_dir: "/var/lib/creda".into(),
            grpc_socket: "/run/creda/creda.sock".into(),
            libp2p_listen: "/ip4/0.0.0.0/tcp/0".into(),
            default_posture: PostureSetting::TreatmentPresumed,
            snapshot_interval_secs: 6 * 3600,
            subscribed_buckets: Vec::new(),
            participant_registry: None,
            bootstrap_peers: Vec::new(),
            signing_key_path: None,
            // Matches the chart's `ports.health` default (9090). Override per pod with
            // CREDA_HEALTH_LISTEN if you bind a different port.
            health_listen: "0.0.0.0:9090".into(),
            subscribe_all_buckets: false,
            synthetic_only: false,
        }
    }
}

/// A partial overlay parsed from a TOML file: every field optional, so a file may set any subset.
#[derive(Debug, Default, Deserialize)]
struct Overlay {
    data_dir: Option<String>,
    grpc_socket: Option<String>,
    libp2p_listen: Option<String>,
    default_posture: Option<PostureSetting>,
    snapshot_interval_secs: Option<u64>,
    subscribed_buckets: Option<Vec<u64>>,
    participant_registry: Option<String>,
    bootstrap_peers: Option<Vec<String>>,
    signing_key_path: Option<String>,
    health_listen: Option<String>,
    subscribe_all_buckets: Option<bool>,
}

impl CredaConfig {
    /// Build the configuration: defaults, then (optional) TOML, then environment. CLI flags are
    /// applied by the caller afterward via the public setters, then [`Self::validate`] is run.
    pub fn load(toml_str: Option<&str>) -> Result<Self> {
        let mut config = CredaConfig::default();
        if let Some(s) = toml_str {
            config.apply_toml(s)?;
        }
        config.apply_env()?;
        Ok(config)
    }

    /// Overlay a TOML document (only the keys present override).
    pub fn apply_toml(&mut self, toml_str: &str) -> Result<()> {
        let overlay: Overlay =
            toml::from_str(toml_str).map_err(|e| Error::Config(format!("invalid TOML: {e}")))?;
        if let Some(v) = overlay.data_dir {
            self.data_dir = v;
        }
        if let Some(v) = overlay.grpc_socket {
            self.grpc_socket = v;
        }
        if let Some(v) = overlay.libp2p_listen {
            self.libp2p_listen = v;
        }
        if let Some(v) = overlay.default_posture {
            self.default_posture = v;
        }
        if let Some(v) = overlay.snapshot_interval_secs {
            self.snapshot_interval_secs = v;
        }
        if let Some(v) = overlay.subscribed_buckets {
            self.subscribed_buckets = v;
        }
        if let Some(v) = overlay.participant_registry {
            self.participant_registry = Some(v);
        }
        if let Some(v) = overlay.bootstrap_peers {
            self.bootstrap_peers = v;
        }
        if let Some(v) = overlay.signing_key_path {
            self.signing_key_path = Some(v);
        }
        if let Some(v) = overlay.health_listen {
            self.health_listen = v;
        }
        if let Some(v) = overlay.subscribe_all_buckets {
            self.subscribe_all_buckets = v;
        }
        Ok(())
    }

    /// Overlay from `CREDA_*` environment variables (for secrets and per-pod values, §10.1.6).
    pub fn apply_env(&mut self) -> Result<()> {
        if let Ok(v) = std::env::var("CREDA_DATA_DIR") {
            self.data_dir = v;
        }
        if let Ok(v) = std::env::var("CREDA_GRPC_SOCKET") {
            self.grpc_socket = v;
        }
        if let Ok(v) = std::env::var("CREDA_LIBP2P_LISTEN") {
            self.libp2p_listen = v;
        }
        if let Ok(v) = std::env::var("CREDA_DEFAULT_POSTURE") {
            self.default_posture = PostureSetting::parse(&v)?;
        }
        if let Ok(v) = std::env::var("CREDA_SNAPSHOT_INTERVAL_SECS") {
            self.snapshot_interval_secs = v.parse().map_err(|_| {
                Error::Config(format!("CREDA_SNAPSHOT_INTERVAL_SECS not a number: {v}"))
            })?;
        }
        if let Ok(v) = std::env::var("CREDA_PARTICIPANT_REGISTRY") {
            self.participant_registry = Some(v);
        }
        if let Ok(v) = std::env::var("CREDA_BOOTSTRAP_PEERS") {
            // Comma-separated multiaddrs; whitespace tolerated; empty entries skipped.
            self.bootstrap_peers = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
        }
        if let Ok(v) = std::env::var("CREDA_SIGNING_KEY_PATH") {
            self.signing_key_path = Some(v);
        }
        if let Ok(v) = std::env::var("CREDA_HEALTH_LISTEN") {
            self.health_listen = v;
        }
        // Comma-separated u64 list; whitespace tolerated; empty entries skipped.
        if let Ok(v) = std::env::var("CREDA_SUBSCRIBED_BUCKETS") {
            self.subscribed_buckets = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<u64>().ok())
                .collect();
        }
        if let Ok(v) = std::env::var("CREDA_SUBSCRIBE_ALL_BUCKETS") {
            let lower = v.trim().to_ascii_lowercase();
            self.subscribe_all_buckets = matches!(lower.as_str(), "1" | "true" | "yes" | "on");
        }
        if let Ok(v) = std::env::var("CREDA_SYNTHETIC_ONLY") {
            let lower = v.trim().to_ascii_lowercase();
            self.synthetic_only = matches!(lower.as_str(), "1" | "true" | "yes" | "on");
        }
        Ok(())
    }

    /// Validate the resolved configuration. Fails loudly on bad config (§10.1.6).
    pub fn validate(&self) -> Result<()> {
        if self.data_dir.trim().is_empty() {
            return Err(Error::Config("data_dir must not be empty".into()));
        }
        if self.grpc_socket.trim().is_empty() {
            return Err(Error::Config("grpc_socket must not be empty".into()));
        }
        if self.snapshot_interval_secs == 0 {
            return Err(Error::Config("snapshot_interval_secs must be > 0".into()));
        }
        for &b in &self.subscribed_buckets {
            if b >= creda_net::BUCKET_COUNT {
                return Err(Error::Config(format!(
                    "subscribed bucket {b} out of range (0..{})",
                    creda_net::BUCKET_COUNT
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        CredaConfig::default().validate().unwrap();
    }

    #[test]
    fn toml_overrides_defaults() {
        let mut c = CredaConfig::default();
        c.apply_toml(
            r#"data_dir = "/data"
snapshot_interval_secs = 900
default_posture = "deny-by-default"
"#,
        )
        .unwrap();
        assert_eq!(c.data_dir, "/data");
        assert_eq!(c.snapshot_interval_secs, 900);
        assert_eq!(c.default_posture, PostureSetting::DenyByDefault);
        // Untouched keys keep their defaults.
        assert_eq!(c.grpc_socket, CredaConfig::default().grpc_socket);
    }

    #[test]
    fn validation_fails_loudly() {
        let c = CredaConfig {
            snapshot_interval_secs: 0,
            ..Default::default()
        };
        assert!(c.validate().is_err());

        let c2 = CredaConfig {
            subscribed_buckets: vec![1024],
            ..Default::default()
        }; // out of range
        assert!(c2.validate().is_err());
    }

    #[test]
    fn posture_maps_to_graph() {
        assert_eq!(
            PostureSetting::DenyByDefault.to_graph(),
            creda_graph::DefaultPosture::DenyByDefault
        );
    }

    #[test]
    fn bootstrap_peers_from_toml() {
        let mut c = CredaConfig::default();
        c.apply_toml(r#"bootstrap_peers = ["/ip4/10.0.0.1/tcp/4001/p2p/12D3KooWAlice", "/ip4/10.0.0.2/tcp/4001/p2p/12D3KooWBob"]"#)
            .unwrap();
        assert_eq!(c.bootstrap_peers.len(), 2);
        assert!(c.bootstrap_peers[0].ends_with("12D3KooWAlice"));
    }
}
