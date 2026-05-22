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
            self.snapshot_interval_secs = v
                .parse()
                .map_err(|_| Error::Config(format!("CREDA_SNAPSHOT_INTERVAL_SECS not a number: {v}")))?;
        }
        if let Ok(v) = std::env::var("CREDA_PARTICIPANT_REGISTRY") {
            self.participant_registry = Some(v);
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
        c.apply_toml(r#"data_dir = "/data"
snapshot_interval_secs = 900
default_posture = "deny-by-default"
"#)
            .unwrap();
        assert_eq!(c.data_dir, "/data");
        assert_eq!(c.snapshot_interval_secs, 900);
        assert_eq!(c.default_posture, PostureSetting::DenyByDefault);
        // Untouched keys keep their defaults.
        assert_eq!(c.grpc_socket, CredaConfig::default().grpc_socket);
    }

    #[test]
    fn validation_fails_loudly() {
        let c = CredaConfig { snapshot_interval_secs: 0, ..Default::default() };
        assert!(c.validate().is_err());

        let c2 = CredaConfig { subscribed_buckets: vec![1024], ..Default::default() }; // out of range
        assert!(c2.validate().is_err());
    }

    #[test]
    fn posture_maps_to_graph() {
        assert_eq!(
            PostureSetting::DenyByDefault.to_graph(),
            creda_graph::DefaultPosture::DenyByDefault
        );
    }
}
