//! Signature creation and verification across all algorithms — spec §3.6, §5.1.2.

mod common;

use creda_events::{IdentityEventNode, SignatureAlgorithm, SigningKey};

#[test]
fn ed25519_node_verifies_and_rejects_tamper() {
    let key = common::ed_key();
    let node = common::build_assert(&key);
    let vk = key.verifying_key();

    // Honest verification passes.
    node.verify_signature(&vk)
        .expect("valid signature should verify");

    // Tampering with the payload breaks verification (the signed bytes change).
    let mut tampered = node.clone();
    tampered.logical_clock += 1;
    assert!(
        tampered.verify_signature(&vk).is_err(),
        "tampered node must not verify"
    );

    // A different institution's key must not verify the signature.
    let other = common::ed_key();
    assert!(
        node.verify_signature(&other.verifying_key()).is_err(),
        "wrong key must not verify"
    );
}

#[test]
fn signature_survives_canonical_reencode() {
    // Serialize, deserialize, and confirm the signature still verifies — replication-realistic.
    let key = common::ed_key();
    let node = common::build_assert(&key);
    let bytes = creda_events::canonical::to_vec(&node).unwrap();
    let decoded: IdentityEventNode = creda_events::canonical::from_slice(&bytes).unwrap();
    decoded
        .verify_signature(&key.verifying_key())
        .expect("verify after re-encode");
}

#[test]
fn institution_id_must_match_signing_key() {
    let key = common::ed_key();
    let node = common::build_assert(&key);
    // institution_id is derived from the signing key's fingerprint.
    assert_eq!(
        node.institution_id.as_bytes(),
        key.verifying_key().fingerprint().as_slice()
    );
}

// ---- PQC algorithms (feature = "pqc", on by default) ---------------------------------------

#[cfg(feature = "pqc")]
fn pqc_roundtrip(alg: SignatureAlgorithm) {
    let key = SigningKey::generate(alg).unwrap();
    let node = common::build_assert(&key);
    assert_eq!(node.signature_algorithm(), alg);
    node.verify_signature(&key.verifying_key())
        .unwrap_or_else(|e| panic!("{alg} should verify: {e}"));

    // Tamper detection.
    let mut tampered = node.clone();
    tampered.logical_clock = tampered.logical_clock.wrapping_add(1);
    assert!(tampered.verify_signature(&key.verifying_key()).is_err());
}

#[cfg(feature = "pqc")]
#[test]
fn mldsa65_node_verifies() {
    pqc_roundtrip(SignatureAlgorithm::MlDsa65);
}

#[cfg(feature = "pqc")]
#[test]
fn slhdsa256s_node_verifies() {
    pqc_roundtrip(SignatureAlgorithm::SlhDsa256s);
}

#[cfg(feature = "pqc")]
#[test]
fn hybrid_node_verifies_and_requires_both_components() {
    pqc_roundtrip(SignatureAlgorithm::Ed25519MlDsa65);

    // Corrupt only the embedded Ed25519 component of a hybrid signature: verification must
    // still fail, proving BOTH components are required (harvest-now-decrypt-later defense).
    let key = SigningKey::generate(SignatureAlgorithm::Ed25519MlDsa65).unwrap();
    let mut node = common::build_assert(&key);
    if let Some(last) = node.signature.signature_bytes.last_mut() {
        *last ^= 0xFF;
    }
    assert!(
        node.verify_signature(&key.verifying_key()).is_err(),
        "corrupting one hybrid component must fail verification"
    );
}

#[cfg(feature = "pqc")]
#[test]
fn algorithm_mismatch_is_rejected() {
    // A signature made with one algorithm must not verify against a key of another.
    let ed = common::ed_key();
    let node = common::build_assert(&ed);
    let pqc_key = SigningKey::generate(SignatureAlgorithm::MlDsa65).unwrap();
    assert!(node.verify_signature(&pqc_key.verifying_key()).is_err());
}
