//! The identity event node and event-type enum — spec §3.4, §4.3, §5.1.
//!
//! Every node in the DAG is an event. All event types share this one node schema and one
//! replication fabric; they are distinguished by [`IdentityEventType`] and carry an
//! [`EventPayload`]. Identity events are evaluated to compute *who a patient is* (advisory);
//! authorization events are evaluated to determine *what is permitted* (enforced) — see §4.8.

use serde::{Deserialize, Serialize};

use crate::canonical;
use crate::crypto::{CryptoSignature, SignatureAlgorithm, SigningKey, VerifyingKey};
use crate::demographics::Demographics;
use crate::error::{Error, Result};
use crate::hash::ContentHash;
use crate::ids::{new_event_id, CertificateFingerprint, EventId};
use crate::payload::EventPayload;

/// The event type discriminant (§3.4). One shared enum spans identity continuity and
/// portable authorization. The enum is **extensible** (§3.4): nodes that encounter an unknown
/// type must preserve and propagate it even if they ignore it during local traversal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdentityEventType {
    // Identity continuity (§3.4)
    Assert,
    Link,
    Contest,
    Attest,
    Amend,
    Tombstone,
    DeceasedDeclaration,
    // Portable authorization (§4.3)
    AuthorizationGrant,
    AuthorizationRevocation,
    ExportReceipt,
    TPODisclosure,
}

impl IdentityEventType {
    /// Whether an event of this type may be a root (have no parents). Only `Assert` begins an
    /// independent identity subgraph (§3.4.1); every other type references prior events.
    pub fn may_be_root(&self) -> bool {
        matches!(self, IdentityEventType::Assert)
    }

    /// Whether this is one of the portable-authorization event types (§4.3).
    pub fn is_authorization(&self) -> bool {
        matches!(
            self,
            IdentityEventType::AuthorizationGrant
                | IdentityEventType::AuthorizationRevocation
                | IdentityEventType::ExportReceipt
                | IdentityEventType::TPODisclosure
        )
    }
}

/// Originating-institution redistribution policy for the event, evaluated in cross-
/// institutional policy honoring (§4.6 step 6). Set at creation time; the most restrictive of
/// the patient grant, this policy, and the responder's posture governs a response.
///
/// The concrete value set is a Phase-0 refinement (the spec names the field but not its
/// vocabulary); these variants are a conservative starting point with a `Custom` escape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedistributionPolicy {
    /// No additional restriction beyond the patient grant and responder posture.
    Open,
    /// Recipients must not redistribute these events further.
    NoRedistribution,
    /// Only the originating institution may serve these events.
    OriginatingInstitutionOnly,
    /// An out-of-band policy identifier, to be interpreted by agreement.
    Custom(String),
}

/// A signed identity event node (§5.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEventNode {
    /// Stable primary key (UUIDv7). References between events use this id, never the content
    /// hash, so tombstoning content does not break the graph (§3.4.6).
    pub id: EventId,

    /// Optional integrity check over the canonical payload. `None` after tombstoning.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content_hash: Option<ContentHash>,

    /// `true` once a tombstone has voided the content hash. Distinguishes "never computed"
    /// from "invalidated by a legitimate tombstone".
    pub content_hash_voided: bool,

    pub event_type: IdentityEventType,

    /// Parent event ids. Empty = root (only valid for `Assert`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parent_ids: Vec<EventId>,

    pub payload: EventPayload,

    /// The creating institution (UDAP certificate fingerprint).
    pub institution_id: CertificateFingerprint,

    /// Signature over the canonical serialization of every field except this one (§3.6).
    pub signature: CryptoSignature,

    /// Real-world creation time (RFC3339). Not trusted for causal ordering (§3.5).
    pub wall_clock_timestamp: String,

    /// Per-subgraph causal ordering (§3.5).
    pub logical_clock: u64,

    /// Originating-institution redistribution policy (§4.6 step 6).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub redistribution_policy: Option<RedistributionPolicy>,

    /// Test-data tag (§11.4): present only on **synthetic** events. Synthetic events propagate
    /// and replicate like real events, but are filtered from clinical FHIR responses and from
    /// real patients' confidence scoring, while remaining visible to operator-scoped queries.
    /// `None` on real events (and omitted from the canonical bytes, so real-event signatures are
    /// unaffected by the existence of this field).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub test_data: Option<TestDataTag>,
}

/// Marks an event as synthetic test data (§11.4.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestDataTag {
    /// Why the data exists: `integration-testing`, `load-testing`, `compliance-validation`, etc.
    pub purpose: String,
    /// Identifier of the test plan that generated the data.
    pub originating_test: String,
    /// When the test data should be tombstoned (RFC3339), if set.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expiration_time: Option<String>,
}

/// The signed view of a node: every field except the signature, in a fixed shape. Built for
/// both signing (at creation) and verification, so the two always agree byte-for-byte.
#[derive(Serialize)]
struct SignableView {
    id: EventId,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_hash: Option<ContentHash>,
    content_hash_voided: bool,
    event_type: IdentityEventType,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parent_ids: Vec<EventId>,
    payload: EventPayload,
    institution_id: CertificateFingerprint,
    wall_clock_timestamp: String,
    logical_clock: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    redistribution_policy: Option<RedistributionPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    test_data: Option<TestDataTag>,
}

impl IdentityEventNode {
    /// Build, validate, and sign a new event node.
    ///
    /// `institution_id` is derived from `signing_key` so the signer and the claimed creator
    /// always agree. The structural invariants ([`Self::validate_structure`]) are checked
    /// *before* signing, so an invalid event is never produced.
    pub fn create(
        payload: EventPayload,
        parent_ids: Vec<EventId>,
        signing_key: &SigningKey,
        logical_clock: u64,
        wall_clock_timestamp: impl Into<String>,
        redistribution_policy: Option<RedistributionPolicy>,
    ) -> Result<Self> {
        Self::build_signed(
            payload,
            parent_ids,
            signing_key,
            logical_clock,
            wall_clock_timestamp.into(),
            redistribution_policy,
            None,
        )
    }

    /// Like [`Self::create`], but tags the event as synthetic test data (§11.4). The tag is
    /// signed along with the rest of the node, so a synthetic event cannot be silently relabeled
    /// as real (or vice versa) without breaking the signature.
    pub fn create_test_data(
        payload: EventPayload,
        parent_ids: Vec<EventId>,
        signing_key: &SigningKey,
        logical_clock: u64,
        wall_clock_timestamp: impl Into<String>,
        redistribution_policy: Option<RedistributionPolicy>,
        tag: TestDataTag,
    ) -> Result<Self> {
        Self::build_signed(
            payload,
            parent_ids,
            signing_key,
            logical_clock,
            wall_clock_timestamp.into(),
            redistribution_policy,
            Some(tag),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_signed(
        payload: EventPayload,
        parent_ids: Vec<EventId>,
        signing_key: &SigningKey,
        logical_clock: u64,
        wall_clock_timestamp: String,
        redistribution_policy: Option<RedistributionPolicy>,
        test_data: Option<TestDataTag>,
    ) -> Result<Self> {
        let verifying_key = signing_key.verifying_key();
        let institution_id = CertificateFingerprint::new(verifying_key.fingerprint());
        let event_type = payload.event_type();

        let content_hash = Some(ContentHash::blake3(&canonical::to_vec(&payload)?));

        let mut node = IdentityEventNode {
            id: new_event_id(&institution_id),
            content_hash,
            content_hash_voided: false,
            event_type,
            parent_ids,
            payload,
            institution_id,
            // Placeholder; replaced below. Excluded from the signed bytes anyway.
            signature: CryptoSignature {
                algorithm: signing_key.algorithm(),
                public_key_fingerprint: Vec::new(),
                signature_bytes: Vec::new(),
            },
            wall_clock_timestamp,
            logical_clock,
            redistribution_policy,
            test_data,
        };

        node.validate_structure()?;
        let message = node.signable_bytes()?;
        node.signature = signing_key.sign(&message)?;
        Ok(node)
    }

    /// Whether this event is synthetic test data (§11.4) — propagates but is filtered from
    /// clinical responses.
    pub fn is_test_data(&self) -> bool {
        self.test_data.is_some()
    }

    /// The canonical bytes that are (or must be) signed — every field except the signature.
    pub fn signable_bytes(&self) -> Result<Vec<u8>> {
        let view = SignableView {
            id: self.id,
            content_hash: self.content_hash.clone(),
            content_hash_voided: self.content_hash_voided,
            event_type: self.event_type,
            parent_ids: self.parent_ids.clone(),
            payload: self.payload.clone(),
            institution_id: self.institution_id.clone(),
            wall_clock_timestamp: self.wall_clock_timestamp.clone(),
            logical_clock: self.logical_clock,
            redistribution_policy: self.redistribution_policy.clone(),
            test_data: self.test_data.clone(),
        };
        canonical::to_vec(&view)
    }

    /// Verify the node's signature against the supplied public key. Also confirms the key's
    /// fingerprint matches both the signature and the claimed `institution_id`.
    ///
    /// Signature verification is mandatory during replication (§3.6).
    pub fn verify_signature(&self, verifying_key: &VerifyingKey) -> Result<()> {
        let fp = verifying_key.fingerprint();
        if self.signature.public_key_fingerprint != fp {
            return Err(Error::SignatureInvalid);
        }
        if self.institution_id.as_bytes() != fp.as_slice() {
            return Err(Error::SignatureInvalid);
        }
        let message = self.signable_bytes()?;
        verifying_key.verify(&message, &self.signature)
    }

    /// Verify the optional content hash against the current payload. Returns `true` if there
    /// is a (non-voided) hash and it matches; `false` if it does not match. `None` is returned
    /// when there is no usable hash (never computed, or voided by tombstone).
    pub fn verify_content_hash(&self) -> Option<bool> {
        if self.content_hash_voided {
            return None;
        }
        let payload_bytes = canonical::to_vec(&self.payload).ok()?;
        self.content_hash
            .as_ref()
            .map(|h| h.matches(&payload_bytes))
    }

    /// Void the content hash, e.g. as part of applying a tombstone to this node (§3.4.6).
    /// Graph traversal is unaffected because references use the UUID, not the hash.
    pub fn void_content_hash(&mut self) {
        self.content_hash = None;
        self.content_hash_voided = true;
    }

    /// Whether this event carries demographic PII in its payload. These are the events a
    /// tombstone must physically scrub from storage (not merely exclude from projection): only
    /// `Assert` (original demographics) and `Amend` (updated demographics) hold tokenized
    /// demographic content. Every other type is a husk-free structural node.
    pub fn carries_demographics(&self) -> bool {
        matches!(
            self.payload,
            EventPayload::Assert { .. } | EventPayload::Amend { .. }
        )
    }

    /// Reduce this node to a **tombstoned husk** (§3.4.6): strip the demographic payload to an
    /// empty [`Demographics`] and void the content hash, while keeping the structural envelope —
    /// id, type, parents, clocks, institution, and the now-historical signature. The content is
    /// then irrecoverable from this node; the signed `Tombstone` event that authorized the scrub
    /// is the integrity anchor. `verify_content_hash` returns `None` (not `Some(false)`) for the
    /// result, so a husk is never read as a hash mismatch; the original signature no longer
    /// verifies, by design — a husk is a local storage artifact, never a wire object. A no-op for
    /// payloads that carry no demographics.
    #[must_use]
    pub fn into_tombstoned_husk(mut self) -> Self {
        let scrubbed = match &mut self.payload {
            EventPayload::Assert { demographics, .. } => {
                *demographics = Demographics::default();
                true
            }
            EventPayload::Amend {
                updated_demographics,
                ..
            } => {
                *updated_demographics = Demographics::default();
                true
            }
            _ => false,
        };
        if scrubbed {
            self.void_content_hash();
        }
        self
    }

    /// Check the structural invariants that are verifiable from this event alone.
    ///
    /// Graph-dependent invariants are intentionally **not** checked here, because they need
    /// traversal context that this crate does not have; they are enforced in `creda-graph`
    /// (M3): the `Contest` party-of-the-subgraph rule (§3.4.3) and the `Amend`
    /// originating-institution rule (§3.4.5, enforced via signature against the target's
    /// signer).
    pub fn validate_structure(&self) -> Result<()> {
        // 1. Payload must match the declared event type.
        if self.payload.event_type() != self.event_type {
            return Err(Error::Validation(format!(
                "event_type {:?} does not match payload {:?}",
                self.event_type,
                self.payload.event_type()
            )));
        }

        // 2. Only Assert may be a root; every other type must reference at least one parent.
        if self.parent_ids.is_empty() && !self.event_type.may_be_root() {
            return Err(Error::Validation(format!(
                "{:?} event must reference at least one parent (only Assert may be a root)",
                self.event_type
            )));
        }

        // 3. Per-payload structural rules.
        match &self.payload {
            EventPayload::Link {
                target_subgraph_heads,
                confidence_score,
                ..
            } => {
                if *confidence_score > 10_000 {
                    return Err(Error::Validation(format!(
                        "Link confidence_score {confidence_score} exceeds 10000 basis points"
                    )));
                }
                let (a, b) = target_subgraph_heads;
                if a == b {
                    return Err(Error::Validation(
                        "Link target_subgraph_heads must be two distinct subgraph heads".into(),
                    ));
                }
                if !self.parent_ids.contains(a) || !self.parent_ids.contains(b) {
                    return Err(Error::Validation(
                        "Link must reference both target subgraph heads in parent_ids".into(),
                    ));
                }
            }
            EventPayload::Attest {
                target_event_ids, ..
            } if target_event_ids.is_empty() => {
                return Err(Error::Validation(
                    "Attest must reference at least one target event".into(),
                ));
            }
            EventPayload::Tombstone {
                target_event_ids, ..
            } if target_event_ids.is_empty() => {
                return Err(Error::Validation(
                    "Tombstone must reference at least one target event".into(),
                ));
            }
            EventPayload::Amend {
                amendment_reason, ..
            } if amendment_reason.trim().is_empty() => {
                return Err(Error::Validation(
                    "Amend must carry a non-empty amendment_reason".into(),
                ));
            }
            EventPayload::DeceasedDeclaration { date_of_death, .. }
                if date_of_death.trim().is_empty() =>
            {
                return Err(Error::Validation(
                    "DeceasedDeclaration must carry a date_of_death".into(),
                ));
            }
            _ => {}
        }

        Ok(())
    }

    /// Convenience accessor: the algorithm used to sign this node.
    pub fn signature_algorithm(&self) -> SignatureAlgorithm {
        self.signature.algorithm
    }
}
