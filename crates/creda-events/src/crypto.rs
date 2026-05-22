//! Algorithm-agile signatures — spec §5.1.2 and §3.6.
//!
//! Every event is signed over the canonical serialization of all node fields except the
//! signature itself (§3.6). The [`CryptoSignature`] carries an algorithm identifier so the
//! network can migrate from classical to post-quantum signatures without a schema change.
//!
//! Supported algorithms:
//! - [`SignatureAlgorithm::Ed25519`] — classical default, aligns with current UDAP infra.
//! - [`SignatureAlgorithm::MlDsa65`] — FIPS 204 (ML-DSA-65), the primary PQC choice.
//! - [`SignatureAlgorithm::SlhDsa256s`] — FIPS 205 (SLH-DSA-SHA2-256s), stateless fallback.
//! - [`SignatureAlgorithm::Ed25519MlDsa65`] — hybrid; BOTH components must verify, defending
//!   against "harvest now, decrypt later".
//!
//! The PQC algorithms are behind the `pqc` feature (on by default). When the feature is off,
//! the enum still carries all four variants (the on-wire schema is stable), but constructing
//! or verifying a PQC key returns [`Error::AlgorithmUnavailable`]. All pqcrypto interaction is
//! isolated in [`pqc`].

use ed25519_dalek::{Signer, Verifier};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[cfg(feature = "pqc")]
mod pqc;

/// Identifier for the signature algorithm used (stored in every [`CryptoSignature`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureAlgorithm {
    /// Ed25519 — classical default.
    Ed25519,
    /// ML-DSA-65 (FIPS 204) — PQC primary.
    MlDsa65,
    /// SLH-DSA-SHA2-256s (FIPS 205) — PQC stateless fallback.
    SlhDsa256s,
    /// Hybrid Ed25519 + ML-DSA-65 — both signatures must verify.
    Ed25519MlDsa65,
}

impl std::fmt::Display for SignatureAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SignatureAlgorithm::Ed25519 => "Ed25519",
            SignatureAlgorithm::MlDsa65 => "ML-DSA-65",
            SignatureAlgorithm::SlhDsa256s => "SLH-DSA-256s",
            SignatureAlgorithm::Ed25519MlDsa65 => "Ed25519+ML-DSA-65",
        };
        f.write_str(s)
    }
}

impl SignatureAlgorithm {
    /// Parse from the `Display` token (case-insensitive); the inverse of `to_string()`. Used by
    /// the participant registry loader to read `<algorithm> <hex-key>` entries.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ed25519" => Some(Self::Ed25519),
            "ml-dsa-65" | "mldsa65" => Some(Self::MlDsa65),
            "slh-dsa-256s" | "slhdsa256s" => Some(Self::SlhDsa256s),
            "ed25519+ml-dsa-65" | "hybrid" => Some(Self::Ed25519MlDsa65),
            _ => None,
        }
    }
}

/// A signature plus the metadata needed to identify the verifying key (§5.1.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoSignature {
    pub algorithm: SignatureAlgorithm,
    /// Blake3 fingerprint of the public key, for key lookup (not the key itself).
    pub public_key_fingerprint: Vec<u8>,
    /// The signature bytes. For the hybrid algorithm this is a canonical-CBOR encoding of the
    /// two component signatures (see [`HybridSig`]).
    pub signature_bytes: Vec<u8>,
}

/// Wire form of a hybrid signature: the two component signatures, both required to verify.
#[derive(Serialize, Deserialize)]
struct HybridSig {
    ed: Vec<u8>,
    mldsa: Vec<u8>,
}

/// A private signing key for one algorithm. Key material is never serialized into the graph.
pub enum SigningKey {
    Ed25519(Box<ed25519_dalek::SigningKey>),
    #[cfg(feature = "pqc")]
    MlDsa65 { secret: Vec<u8>, public: Vec<u8> },
    #[cfg(feature = "pqc")]
    SlhDsa256s { secret: Vec<u8>, public: Vec<u8> },
    #[cfg(feature = "pqc")]
    Hybrid {
        ed: Box<ed25519_dalek::SigningKey>,
        mldsa_secret: Vec<u8>,
        mldsa_public: Vec<u8>,
    },
}

/// The corresponding public verifying key.
#[derive(Clone)]
pub enum VerifyingKey {
    Ed25519(ed25519_dalek::VerifyingKey),
    #[cfg(feature = "pqc")]
    MlDsa65(Vec<u8>),
    #[cfg(feature = "pqc")]
    SlhDsa256s(Vec<u8>),
    #[cfg(feature = "pqc")]
    Hybrid {
        ed: ed25519_dalek::VerifyingKey,
        mldsa_public: Vec<u8>,
    },
}

impl SigningKey {
    /// Generate a fresh key for the given algorithm using the OS CSPRNG.
    pub fn generate(algorithm: SignatureAlgorithm) -> Result<Self> {
        match algorithm {
            SignatureAlgorithm::Ed25519 => Ok(SigningKey::Ed25519(Box::new(
                ed25519_dalek::SigningKey::generate(&mut rand_core::OsRng),
            ))),
            #[cfg(feature = "pqc")]
            SignatureAlgorithm::MlDsa65 => {
                let (public, secret) = pqc::mldsa65_keypair();
                Ok(SigningKey::MlDsa65 { secret, public })
            }
            #[cfg(feature = "pqc")]
            SignatureAlgorithm::SlhDsa256s => {
                let (public, secret) = pqc::slhdsa256s_keypair();
                Ok(SigningKey::SlhDsa256s { secret, public })
            }
            #[cfg(feature = "pqc")]
            SignatureAlgorithm::Ed25519MlDsa65 => {
                let ed = ed25519_dalek::SigningKey::generate(&mut rand_core::OsRng);
                let (mldsa_public, mldsa_secret) = pqc::mldsa65_keypair();
                Ok(SigningKey::Hybrid {
                    ed: Box::new(ed),
                    mldsa_secret,
                    mldsa_public,
                })
            }
            #[cfg(not(feature = "pqc"))]
            other => Err(Error::AlgorithmUnavailable(other.to_string())),
        }
    }

    /// The algorithm this key signs with.
    pub fn algorithm(&self) -> SignatureAlgorithm {
        match self {
            SigningKey::Ed25519(_) => SignatureAlgorithm::Ed25519,
            #[cfg(feature = "pqc")]
            SigningKey::MlDsa65 { .. } => SignatureAlgorithm::MlDsa65,
            #[cfg(feature = "pqc")]
            SigningKey::SlhDsa256s { .. } => SignatureAlgorithm::SlhDsa256s,
            #[cfg(feature = "pqc")]
            SigningKey::Hybrid { .. } => SignatureAlgorithm::Ed25519MlDsa65,
        }
    }

    /// Derive the public verifying key.
    pub fn verifying_key(&self) -> VerifyingKey {
        match self {
            SigningKey::Ed25519(sk) => VerifyingKey::Ed25519(sk.verifying_key()),
            #[cfg(feature = "pqc")]
            SigningKey::MlDsa65 { public, .. } => VerifyingKey::MlDsa65(public.clone()),
            #[cfg(feature = "pqc")]
            SigningKey::SlhDsa256s { public, .. } => VerifyingKey::SlhDsa256s(public.clone()),
            #[cfg(feature = "pqc")]
            SigningKey::Hybrid {
                ed, mldsa_public, ..
            } => VerifyingKey::Hybrid {
                ed: ed.verifying_key(),
                mldsa_public: mldsa_public.clone(),
            },
        }
    }

    /// Sign `message`, producing a [`CryptoSignature`].
    pub fn sign(&self, message: &[u8]) -> Result<CryptoSignature> {
        let fingerprint = self.verifying_key().fingerprint();
        let signature_bytes = match self {
            SigningKey::Ed25519(sk) => sk.sign(message).to_bytes().to_vec(),
            #[cfg(feature = "pqc")]
            SigningKey::MlDsa65 { secret, .. } => pqc::mldsa65_sign(secret, message)?,
            #[cfg(feature = "pqc")]
            SigningKey::SlhDsa256s { secret, .. } => pqc::slhdsa256s_sign(secret, message)?,
            #[cfg(feature = "pqc")]
            SigningKey::Hybrid {
                ed, mldsa_secret, ..
            } => {
                let ed_sig = ed.sign(message).to_bytes().to_vec();
                let mldsa_sig = pqc::mldsa65_sign(mldsa_secret, message)?;
                crate::canonical::to_vec(&HybridSig {
                    ed: ed_sig,
                    mldsa: mldsa_sig,
                })?
            }
        };
        Ok(CryptoSignature {
            algorithm: self.algorithm(),
            public_key_fingerprint: fingerprint,
            signature_bytes,
        })
    }
}

impl VerifyingKey {
    /// The algorithm this key verifies.
    pub fn algorithm(&self) -> SignatureAlgorithm {
        match self {
            VerifyingKey::Ed25519(_) => SignatureAlgorithm::Ed25519,
            #[cfg(feature = "pqc")]
            VerifyingKey::MlDsa65(_) => SignatureAlgorithm::MlDsa65,
            #[cfg(feature = "pqc")]
            VerifyingKey::SlhDsa256s(_) => SignatureAlgorithm::SlhDsa256s,
            #[cfg(feature = "pqc")]
            VerifyingKey::Hybrid { .. } => SignatureAlgorithm::Ed25519MlDsa65,
        }
    }

    /// Raw public-key bytes (used to derive the fingerprint). For the hybrid key this is the
    /// Ed25519 public bytes concatenated with the ML-DSA public bytes.
    pub fn public_key_bytes(&self) -> Vec<u8> {
        match self {
            VerifyingKey::Ed25519(vk) => vk.to_bytes().to_vec(),
            #[cfg(feature = "pqc")]
            VerifyingKey::MlDsa65(pk) => pk.clone(),
            #[cfg(feature = "pqc")]
            VerifyingKey::SlhDsa256s(pk) => pk.clone(),
            #[cfg(feature = "pqc")]
            VerifyingKey::Hybrid { ed, mldsa_public } => {
                let mut v = ed.to_bytes().to_vec();
                v.extend_from_slice(mldsa_public);
                v
            }
        }
    }

    /// Blake3 fingerprint of the public key (matches `CryptoSignature.public_key_fingerprint`).
    pub fn fingerprint(&self) -> Vec<u8> {
        blake3::hash(&self.public_key_bytes()).as_bytes().to_vec()
    }

    /// Reconstruct a verifying key from its algorithm and raw public-key bytes — the inverse of
    /// [`Self::public_key_bytes`]. Used by the participant registry to load admitted-peer keys
    /// from disk. PQC variants require the `pqc` feature; without it they are unavailable.
    pub fn from_public_key_bytes(algorithm: SignatureAlgorithm, bytes: &[u8]) -> Result<Self> {
        match algorithm {
            SignatureAlgorithm::Ed25519 => {
                let arr: [u8; 32] = bytes.try_into().map_err(|_| {
                    Error::MalformedKey(format!(
                        "Ed25519 public key must be 32 bytes, got {}",
                        bytes.len()
                    ))
                })?;
                let vk = ed25519_dalek::VerifyingKey::from_bytes(&arr)
                    .map_err(|e| Error::MalformedKey(format!("invalid Ed25519 public key: {e}")))?;
                Ok(VerifyingKey::Ed25519(vk))
            }
            #[cfg(feature = "pqc")]
            SignatureAlgorithm::MlDsa65 => Ok(VerifyingKey::MlDsa65(bytes.to_vec())),
            #[cfg(feature = "pqc")]
            SignatureAlgorithm::SlhDsa256s => Ok(VerifyingKey::SlhDsa256s(bytes.to_vec())),
            #[cfg(feature = "pqc")]
            SignatureAlgorithm::Ed25519MlDsa65 => {
                if bytes.len() <= 32 {
                    return Err(Error::MalformedKey(format!(
                        "hybrid public key must be 32 (Ed25519) + ML-DSA bytes, got {}",
                        bytes.len()
                    )));
                }
                let ed_arr: [u8; 32] = bytes[..32].try_into().expect("checked length > 32");
                let ed = ed25519_dalek::VerifyingKey::from_bytes(&ed_arr).map_err(|e| {
                    Error::MalformedKey(format!("invalid hybrid Ed25519 component: {e}"))
                })?;
                Ok(VerifyingKey::Hybrid { ed, mldsa_public: bytes[32..].to_vec() })
            }
            #[cfg(not(feature = "pqc"))]
            other => Err(Error::AlgorithmUnavailable(other.to_string())),
        }
    }

    /// Verify `signature` over `message`. Returns `Ok(())` only on a valid signature whose
    /// algorithm matches this key.
    pub fn verify(&self, message: &[u8], signature: &CryptoSignature) -> Result<()> {
        if signature.algorithm != self.algorithm() {
            return Err(Error::AlgorithmMismatch {
                expected: self.algorithm().to_string(),
                got: signature.algorithm.to_string(),
            });
        }
        match self {
            VerifyingKey::Ed25519(vk) => {
                let sig = ed25519_signature_from_bytes(&signature.signature_bytes)?;
                vk.verify(message, &sig).map_err(|_| Error::SignatureInvalid)
            }
            #[cfg(feature = "pqc")]
            VerifyingKey::MlDsa65(pk) => pqc::mldsa65_verify(pk, message, &signature.signature_bytes),
            #[cfg(feature = "pqc")]
            VerifyingKey::SlhDsa256s(pk) => {
                pqc::slhdsa256s_verify(pk, message, &signature.signature_bytes)
            }
            #[cfg(feature = "pqc")]
            VerifyingKey::Hybrid { ed, mldsa_public } => {
                let hybrid: HybridSig = crate::canonical::from_slice(&signature.signature_bytes)
                    .map_err(|e| Error::MalformedSignature(e.to_string()))?;
                // BOTH components must verify.
                let ed_sig = ed25519_signature_from_bytes(&hybrid.ed)?;
                ed.verify(message, &ed_sig)
                    .map_err(|_| Error::SignatureInvalid)?;
                pqc::mldsa65_verify(mldsa_public, message, &hybrid.mldsa)
            }
        }
    }
}

fn ed25519_signature_from_bytes(bytes: &[u8]) -> Result<ed25519_dalek::Signature> {
    let arr: [u8; 64] = bytes
        .try_into()
        .map_err(|_| Error::MalformedSignature(format!("expected 64 bytes, got {}", bytes.len())))?;
    Ok(ed25519_dalek::Signature::from_bytes(&arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ed25519_sign_and_verify() {
        let sk = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        let vk = sk.verifying_key();
        let msg = b"creda event bytes";
        let sig = sk.sign(msg).unwrap();
        assert_eq!(sig.algorithm, SignatureAlgorithm::Ed25519);
        assert_eq!(sig.public_key_fingerprint, vk.fingerprint());
        vk.verify(msg, &sig).unwrap();
    }

    #[test]
    fn ed25519_pubkey_round_trips_through_bytes() {
        let sk = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        let vk = sk.verifying_key();
        let restored =
            VerifyingKey::from_public_key_bytes(SignatureAlgorithm::Ed25519, &vk.public_key_bytes())
                .unwrap();
        assert_eq!(restored.fingerprint(), vk.fingerprint());
        // The reconstructed key verifies a real signature.
        let sig = sk.sign(b"creda").unwrap();
        restored.verify(b"creda", &sig).unwrap();
    }

    #[test]
    fn algorithm_parse_round_trips() {
        for a in [
            SignatureAlgorithm::Ed25519,
            SignatureAlgorithm::MlDsa65,
            SignatureAlgorithm::SlhDsa256s,
            SignatureAlgorithm::Ed25519MlDsa65,
        ] {
            assert_eq!(SignatureAlgorithm::parse(&a.to_string()), Some(a));
        }
        assert_eq!(SignatureAlgorithm::parse("nonsense"), None);
    }

    #[test]
    fn from_bytes_rejects_wrong_length() {
        assert!(VerifyingKey::from_public_key_bytes(SignatureAlgorithm::Ed25519, &[0u8; 16]).is_err());
    }

    #[test]
    fn ed25519_rejects_tamper_and_wrong_key() {
        let sk = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        let vk = sk.verifying_key();
        let sig = sk.sign(b"original").unwrap();
        assert!(vk.verify(b"tampered", &sig).is_err());

        let other = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        assert!(other.verifying_key().verify(b"original", &sig).is_err());
    }
}
