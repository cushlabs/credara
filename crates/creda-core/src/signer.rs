//! Signing abstraction (spec §10.1.4 `Signer`).
//!
//! `Signer` hides the institution's signing key so the default in-memory implementation can be
//! swapped for an HSM, cloud KMS, or hardware token later without touching the engine. It
//! encapsulates *event creation + signing* together so the engine never handles raw keys.

use creda_events::{
    CertificateFingerprint, EventId, EventPayload, IdentityEventNode, RedistributionPolicy,
    SignatureAlgorithm, SigningKey, TestDataTag, VerifyingKey,
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

    /// Build, validate, and sign a new **synthetic** event carrying `tag` (§11.4). Used by the
    /// engine's synthetic-only guardrail (docs/PILOT.md) so locally created events are provably
    /// non-clinical. Wraps [`IdentityEventNode::create_test_data`].
    fn create_test_event(
        &self,
        payload: EventPayload,
        parent_ids: Vec<EventId>,
        logical_clock: u64,
        wall_clock: String,
        redistribution_policy: Option<RedistributionPolicy>,
        tag: TestDataTag,
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

    /// The 32-byte Ed25519 secret to seed the libp2p **transport** identity (§6.2.3), so a peer's
    /// `PeerId` is its institution's signing public key — directly verifiable against the
    /// participant registry rather than a throwaway. `None` for PQC/hybrid signers (libp2p has no
    /// ML-DSA key type), which fall back to a generated identity. Stays within the process; the
    /// daemon hands it straight to the libp2p adapter and never serializes it.
    pub fn libp2p_identity_secret(&self) -> Option<[u8; 32]> {
        self.key.ed25519_secret_bytes()
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

    fn create_test_event(
        &self,
        payload: EventPayload,
        parent_ids: Vec<EventId>,
        logical_clock: u64,
        wall_clock: String,
        redistribution_policy: Option<RedistributionPolicy>,
        tag: TestDataTag,
    ) -> creda_events::Result<IdentityEventNode> {
        IdentityEventNode::create_test_data(
            payload,
            parent_ids,
            &self.key,
            logical_clock,
            wall_clock,
            redistribution_policy,
            tag,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libp2p_identity_secret_is_the_institution_ed25519_key() {
        // §6.2.3 foundation: the libp2p transport identity must be the institution's signing key,
        // so the derived PeerId is verifiable against the participant registry, not a throwaway.
        let signer = InMemorySigner::generate().unwrap(); // Ed25519 by default
        let secret = signer
            .libp2p_identity_secret()
            .expect("an Ed25519 signer exposes a libp2p identity secret");

        // Rebuilding a signing key from the exposed secret yields the same public identity — the
        // libp2p key really is this institution's key, so the PeerId is its fingerprint.
        let rebuilt = SigningKey::ed25519_from_secret_bytes(&secret).unwrap();
        assert_eq!(
            rebuilt.verifying_key().fingerprint(),
            signer.verifying_key().fingerprint(),
        );
    }
}
