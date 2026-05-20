//! Post-quantum signature wrappers (feature = "pqc") — spec §5.1.2.
//!
//! This module is the **single isolation point** for the pqcrypto C-backed crates. The rest
//! of the crate deals only in `Vec<u8>` key/signature material, so if a pqcrypto crate name,
//! module path, or version needs adjustment on first compile, the fix lives entirely here.
//!
//! Algorithms:
//! - ML-DSA-65  (FIPS 204) via `pqcrypto-mldsa`, module `mldsa65`.
//! - SLH-DSA-SHA2-256s (FIPS 205) via `pqcrypto-sphincsplus`, module `sphincssha2256ssimple`.
//!
//! NOTE TO FIRST BUILDER: the `sphincssha2256ssimple` module name in particular is the item
//! most likely to differ across `pqcrypto-sphincsplus` versions (e.g. `sphincsshasha256...`).
//! If the build complains, the only change needed is the `use` path below.

use pqcrypto_mldsa::mldsa65;
use pqcrypto_sphincsplus::sphincssha2256ssimple as slhdsa;
use pqcrypto_traits::sign::{
    DetachedSignature as _, PublicKey as _, SecretKey as _,
};

use crate::error::{Error, Result};

// ---- ML-DSA-65 (FIPS 204) ------------------------------------------------------------------

/// Generate an ML-DSA-65 keypair, returned as `(public_bytes, secret_bytes)`.
pub fn mldsa65_keypair() -> (Vec<u8>, Vec<u8>) {
    let (pk, sk) = mldsa65::keypair();
    (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
}

/// Produce a detached ML-DSA-65 signature over `message`.
pub fn mldsa65_sign(secret_bytes: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    let sk = mldsa65::SecretKey::from_bytes(secret_bytes)
        .map_err(|e| Error::MalformedKey(format!("ml-dsa-65 secret key: {e}")))?;
    let sig = mldsa65::detached_sign(message, &sk);
    Ok(sig.as_bytes().to_vec())
}

/// Verify a detached ML-DSA-65 signature.
pub fn mldsa65_verify(public_bytes: &[u8], message: &[u8], signature_bytes: &[u8]) -> Result<()> {
    let pk = mldsa65::PublicKey::from_bytes(public_bytes)
        .map_err(|e| Error::MalformedKey(format!("ml-dsa-65 public key: {e}")))?;
    let sig = mldsa65::DetachedSignature::from_bytes(signature_bytes)
        .map_err(|e| Error::MalformedSignature(format!("ml-dsa-65 signature: {e}")))?;
    mldsa65::verify_detached_signature(&sig, message, &pk).map_err(|_| Error::SignatureInvalid)
}

// ---- SLH-DSA-SHA2-256s (FIPS 205) ----------------------------------------------------------

/// Generate an SLH-DSA-256s keypair, returned as `(public_bytes, secret_bytes)`.
pub fn slhdsa256s_keypair() -> (Vec<u8>, Vec<u8>) {
    let (pk, sk) = slhdsa::keypair();
    (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
}

/// Produce a detached SLH-DSA-256s signature over `message`.
pub fn slhdsa256s_sign(secret_bytes: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    let sk = slhdsa::SecretKey::from_bytes(secret_bytes)
        .map_err(|e| Error::MalformedKey(format!("slh-dsa-256s secret key: {e}")))?;
    let sig = slhdsa::detached_sign(message, &sk);
    Ok(sig.as_bytes().to_vec())
}

/// Verify a detached SLH-DSA-256s signature.
pub fn slhdsa256s_verify(public_bytes: &[u8], message: &[u8], signature_bytes: &[u8]) -> Result<()> {
    let pk = slhdsa::PublicKey::from_bytes(public_bytes)
        .map_err(|e| Error::MalformedKey(format!("slh-dsa-256s public key: {e}")))?;
    let sig = slhdsa::DetachedSignature::from_bytes(signature_bytes)
        .map_err(|e| Error::MalformedSignature(format!("slh-dsa-256s signature: {e}")))?;
    slhdsa::verify_detached_signature(&sig, message, &pk).map_err(|_| Error::SignatureInvalid)
}
