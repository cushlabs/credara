//! Signing abstraction (spec §10.1.4 `Signer`).
//!
//! `Signer` hides the institution's signing key so the default in-memory implementation can be
//! swapped for an HSM, cloud KMS, or hardware token later without touching the engine. It
//! encapsulates *event creation + signing* together so the engine never handles raw keys.

use creda_events::{
    CertificateFingerprint, EventId, EventPayload, IdentityEventNode, RedistributionPolicy,
    SignatureAlgorithm, SigningKey, VerifyingKey,
};

/// Abstracts the institution's signing key (§10.1.4).
pub trait Signer: Send + Sync {
    /// The institution's UDAP fingerprint — the `institution_id` on every event it creates.
    fn institution_id(&self) -> CertificateFingerprint;

    /// Build, validate, and sign a new event. Wraps [`IdentityEventNode::create`] so the key
    /// never leaves the signer (HSM/KMS implementations keep it remote).
    fn create_event(
        &self,
        payload: EventPayload,
        parent_ids: Vec<EventId>,
        logical_clock: u64,
        wall_clock: String,
        redistribution_policy: Option<RedistributionPolicy>,
    ) -> creda_events::Result<IdentityEventNode>;
}

/// In-memory signer: holds an `ed25519`/PQC signing key directly. The default for development
/// and for deployments that source the private key from a k8s Secret (§10.1.4).
pub struct InMemorySigner {
    key: SigningKey,
}

impl InMemorySigner {
    /// Wrap an existing signing key.
    pub fn from_key(key: SigningKey) -> Self {
        Self { key }
    }

    /// Generate a fresh Ed25519 signer (development / `creda init`).
    pub fn generate() -> creda_events::Result<Self> {
        Ok(Self {
            key: SigningKey::generate(SignatureAlgorithm::Ed25519)?,
        })
    }

    /// Load an Ed25519 signer from a file containing the raw 32-byte secret. This is how the
    /// daemon picks up its institutional signing key from a k8s Secret mounted as a file
    /// (§10.1.4). The file is expected to be exactly 32 bytes — no PEM wrapper, no hex encoding.
    /// For the testbed and for `kubectl create secret generic --from-file=...`.
    pub fn from_ed25519_secret_file<P: AsRef<std::path::Path>>(
        path: P,
    ) -> creda_events::Result<Self> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| {
            creda_events::Error::MalformedKey(format!(
                "reading signing key file {:?}: {e}",
                path.as_ref()
            ))
        })?;
        Ok(Self {
            key: SigningKey::ed25519_from_secret_bytes(&bytes)?,
        })
    }

    /// This signer's public verifying key — what a peer needs to authenticate events this signer
    /// produced (the value a [`crate::engine::VerifyingKeyResolver`] would return for our
    /// fingerprint).
    pub fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }
}

impl Signer for InMemorySigner {
    fn institution_id(&self) -> CertificateFingerprint {
        CertificateFingerprint::new(self.key.verifying_key().fingerprint())
    }

    fn create_event(
        &self,
        payload: EventPayload,
        parent_ids: Vec<EventId>,
        logical_clock: u64,
        wall_clock: String,
        redistribution_policy: Option<RedistributionPolicy>,
    ) -> creda_events::Result<IdentityEventNode> {
        IdentityEventNode::create(
            payload,
            parent_ids,
            &self.key,
            logical_clock,
            wall_clock,
            redistribution_policy,
        )
    }
}
