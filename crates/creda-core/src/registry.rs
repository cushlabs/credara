//! Participant key registry — the [`VerifyingKeyResolver`] backing for replication ingest.
//!
//! Creda is a vetted-but-uncoordinated network: institutions are admitted to a trust framework
//! (modeled on DirectTrust), and each peer holds the admitted participants' verifying keys. When
//! a peer receives an event, it authenticates the author by resolving that author's certificate
//! fingerprint to a verifying key *here* — the mandatory signature gate at ingest (§3.6).
//!
//! **Scope / open question.** This holds the *resolved* public keys and answers fingerprint
//! lookups. How keys get into it in production — syncing from a UDAP / TEFCA participant registry,
//! validating certificate chains, and handling rotation and revocation — is the open question
//! (Appendix C / open question 13). The registry abstraction is deliberately small so that
//! integration can populate it without touching the ingest path.

use std::collections::HashMap;
use std::path::Path;

use creda_events::{CertificateFingerprint, SignatureAlgorithm, VerifyingKey};

use crate::engine::VerifyingKeyResolver;
use crate::error::{Error, Result};

/// A set of admitted participants' verifying keys, indexed by certificate fingerprint.
#[derive(Default, Clone)]
pub struct KeyRegistry {
    keys: HashMap<Vec<u8>, VerifyingKey>,
}

impl KeyRegistry {
    /// An empty registry — resolves nothing, so every received event is refused until
    /// participants are admitted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Admit a participant's verifying key, indexed by its fingerprint.
    pub fn insert(&mut self, key: VerifyingKey) {
        self.keys.insert(key.fingerprint(), key);
    }

    /// Build a registry from a set of verifying keys.
    pub fn from_keys(keys: impl IntoIterator<Item = VerifyingKey>) -> Self {
        let mut reg = Self::new();
        for k in keys {
            reg.insert(k);
        }
        reg
    }

    /// Number of admitted participants.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether no participants are admitted (received events will all be refused).
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Load admitted keys from a directory of participant files. Each file holds one entry of the
    /// form `<algorithm> <hex-public-key>` (lines starting with `#` and blank lines are ignored),
    /// where the algorithm token matches [`SignatureAlgorithm`]'s `Display` (case-insensitive),
    /// e.g. `ed25519 3b6a…`. Unreadable or malformed files are skipped with a logged warning so a
    /// single bad entry can't take the peer down.
    pub fn load_dir(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut reg = Self::new();
        let entries = std::fs::read_dir(path).map_err(|e| {
            Error::Config(format!("reading participant registry {}: {e}", path.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::Io(e.to_string()))?;
            let file = entry.path();
            if !file.is_file() {
                continue;
            }
            let contents = match std::fs::read_to_string(&file) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("creda: skipping participant key {}: {e}", file.display());
                    continue;
                }
            };
            match parse_entry(&contents) {
                Ok(vk) => reg.insert(vk),
                Err(e) => eprintln!("creda: skipping participant key {}: {e}", file.display()),
            }
        }
        Ok(reg)
    }
}

impl VerifyingKeyResolver for KeyRegistry {
    fn resolve(&self, fingerprint: &CertificateFingerprint) -> Option<VerifyingKey> {
        self.keys.get(fingerprint.as_bytes()).cloned()
    }
}

/// Parse one `<algorithm> <hex-public-key>` participant entry.
fn parse_entry(s: &str) -> Result<VerifyingKey> {
    let line = s
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .ok_or_else(|| Error::Config("empty participant key file".into()))?;
    let mut parts = line.split_whitespace();
    let algo = parts
        .next()
        .ok_or_else(|| Error::Config("missing algorithm token".into()))?;
    let hex = parts
        .next()
        .ok_or_else(|| Error::Config("missing hex public key".into()))?;
    let algorithm = SignatureAlgorithm::parse(algo)
        .ok_or_else(|| Error::Config(format!("unknown signature algorithm {algo:?}")))?;
    let bytes = decode_hex(hex)?;
    Ok(VerifyingKey::from_public_key_bytes(algorithm, &bytes)?)
}

fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err(Error::Config("hex public key has an odd number of digits".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| Error::Config(format!("invalid hex in public key: {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use creda_events::{
        Demographics, EventPayload, IdentityEventNode, SigningKey, VerificationMethod,
    };
    use creda_store::MemoryStore;

    use crate::config::CredaConfig;
    use crate::engine::{CredaCore, Ingest};
    use crate::signer::InMemorySigner;

    fn ed25519() -> SigningKey {
        SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap()
    }
    fn fp_of(vk: &VerifyingKey) -> CertificateFingerprint {
        CertificateFingerprint::new(vk.fingerprint())
    }
    fn signed_assert(key: &SigningKey) -> IdentityEventNode {
        IdentityEventNode::create(
            EventPayload::Assert {
                demographics: Demographics::default(),
                verification_method: VerificationMethod::SelfReport,
            },
            vec![],
            key,
            1,
            "2026-01-01T00:00:00Z",
            None,
        )
        .unwrap()
    }
    fn core() -> CredaCore {
        CredaCore::new(
            Box::new(MemoryStore::new()),
            Box::new(InMemorySigner::generate().unwrap()),
            CredaConfig::default(),
        )
    }

    #[test]
    fn resolves_admitted_keys_only() {
        let admitted = ed25519().verifying_key();
        let reg = KeyRegistry::from_keys([admitted.clone()]);
        assert!(reg.resolve(&fp_of(&admitted)).is_some());
        let stranger = ed25519().verifying_key();
        assert!(reg.resolve(&fp_of(&stranger)).is_none());
    }

    #[test]
    fn ingest_accepts_admitted_rejects_unknown() {
        let key = ed25519();
        let node = signed_assert(&key);
        // Admitted signer -> accepted.
        let reg = KeyRegistry::from_keys([key.verifying_key()]);
        assert_eq!(core().ingest_event(node.clone(), &reg).unwrap(), Ingest::Accepted);
        // Unknown signer (empty registry) -> rejected.
        let empty = KeyRegistry::new();
        assert!(matches!(core().ingest_event(node, &empty).unwrap(), Ingest::Rejected(_)));
    }

    #[test]
    fn load_dir_round_trips_a_hex_key() {
        let vk = ed25519().verifying_key();
        let hex: String = vk.public_key_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("creda-reg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mercy.key"), format!("# Mercy General\ned25519 {hex}\n")).unwrap();
        std::fs::write(dir.join("note.txt"), "ed25519 not-hex").unwrap(); // malformed -> skipped

        let reg = KeyRegistry::load_dir(&dir).unwrap();
        assert_eq!(reg.len(), 1, "the one valid key loads; the malformed file is skipped");
        assert!(reg.resolve(&fp_of(&vk)).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
