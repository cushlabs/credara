//! Event payloads — spec §5.1.3 (payload schema per event type) and §4.3.
//!
//! [`EventPayload`] is a tagged union discriminated by the event type. Each variant carries
//! exactly the data that event type needs. Authorization payloads (§4.3) and identity
//! payloads (§3.4) live in the same enum because they share one node schema and one fabric.

use serde::{Deserialize, Serialize};

use crate::demographics::{Demographics, VerificationMethod};
use crate::event::IdentityEventType;
use crate::ids::{CertificateFingerprint, EventId};

/// Event-type-specific payload (§5.1.3). The serde tag mirrors the [`IdentityEventType`]
/// discriminant; [`EventPayload::event_type`] returns the matching type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPayload {
    // ---- Identity continuity (§3.4) ----
    Assert {
        demographics: Demographics,
        verification_method: VerificationMethod,
    },
    Link {
        /// The head nodes of the two subgraphs asserted to be the same person.
        target_subgraph_heads: (EventId, EventId),
        /// Match strength as basis points: 0–10000 = 0.00–100.00% (no floats, §5.1.1).
        confidence_score: u16,
        method: LinkMethod,
    },
    Contest {
        target_link_id: EventId,
        reason: ContestReason,
    },
    Attest {
        target_event_ids: Vec<EventId>,
        purpose: AttestPurpose,
    },
    Amend {
        target_event_id: EventId,
        updated_demographics: Demographics,
        amendment_reason: String,
    },
    Tombstone {
        target_event_ids: Vec<EventId>,
        legal_basis: TombstoneBasis,
    },

    // ---- Portable authorization (§4.3) ----
    AuthorizationGrant {
        scope: AuthorizationScope,
        audience: GrantAudience,
        purpose: GrantPurpose,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        expiration: Option<String>, // RFC3339 timestamp, or absent = indefinite
        #[serde(skip_serializing_if = "Option::is_none", default)]
        volume_constraints: Option<VolumeConstraints>,
        use_mode: UseMode,
        // Non-transferability is implicit: a Grant is bound to the patient subgraph it
        // references (via parent_ids) and cannot be reassigned.
    },
    AuthorizationRevocation {
        target_grant_id: EventId,
    },
    ExportReceipt {
        governing_grant_id: EventId,
        requesting_institution: CertificateFingerprint,
        released_scope: AuthorizationScope,
    },

    // ---- Lifecycle (§3.4.7) ----
    DeceasedDeclaration {
        date_of_death: String, // RFC3339 date
        certifier_id: CertificateFingerprint,
        cause_of_death_present: bool, // flag only — cause itself is clinical data, not stored
    },
}

impl EventPayload {
    /// The event type that this payload corresponds to.
    pub fn event_type(&self) -> IdentityEventType {
        match self {
            EventPayload::Assert { .. } => IdentityEventType::Assert,
            EventPayload::Link { .. } => IdentityEventType::Link,
            EventPayload::Contest { .. } => IdentityEventType::Contest,
            EventPayload::Attest { .. } => IdentityEventType::Attest,
            EventPayload::Amend { .. } => IdentityEventType::Amend,
            EventPayload::Tombstone { .. } => IdentityEventType::Tombstone,
            EventPayload::DeceasedDeclaration { .. } => IdentityEventType::DeceasedDeclaration,
            EventPayload::AuthorizationGrant { .. } => IdentityEventType::AuthorizationGrant,
            EventPayload::AuthorizationRevocation { .. } => {
                IdentityEventType::AuthorizationRevocation
            }
            EventPayload::ExportReceipt { .. } => IdentityEventType::ExportReceipt,
        }
    }
}

/// How a `Link` determination was made (§3.4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkMethod {
    Manual,
    Algorithmic,
    Referral,
    InsuranceCrosswalk,
    Other,
}

/// Why a `Link` is being contested (§3.4.3): an enumerated code plus optional free text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestReason {
    pub code: ContestReasonCode,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContestReasonCode {
    DistinctPatients,
    DemographicConflict,
    DuplicateRecord,
    Other,
}

/// The purpose under which an institution attests reliance on a chain (§3.4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttestPurpose {
    Treatment,
    Payment,
    Operations,
    PublicHealth,
    Other,
}

/// The legal basis for a `Tombstone` (§3.4.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TombstoneBasis {
    RightToBeForgotten,
    StateLaw,
    CourtOrder,
    Other,
}

/// What an `AuthorizationGrant` covers (§4.3.1): subgraph segments, event types, and data
/// categories.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationScope {
    /// Subgraph segments the grant applies to, named by entry-point event ids. Empty = the
    /// whole subgraph the grant is attached to.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub subgraph_segments: Vec<EventId>,
    /// Which event types may be served. Empty = all.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub event_types: Vec<IdentityEventType>,
    /// Data categories (e.g. demographics, provenance). Empty = all in scope.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub data_categories: Vec<String>,
}

/// Who an `AuthorizationGrant` is addressed to (§4.3.1, §4.6 step 3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantAudience {
    /// A specific institution, by UDAP fingerprint.
    InstitutionId(CertificateFingerprint),
    /// An institutional class (e.g. "any TEFCA QHIN"), verified against the Participant Registry.
    InstitutionClass(String),
    /// A constrained wildcard (e.g. "any institution with an active BAA").
    ConstrainedWildcard(String),
}

/// The purpose of an `AuthorizationGrant` (§4.3.1). Research, AI, and federal scopes carry
/// distinct enforcement semantics (always require an explicit grant, §4.6 step 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrantPurpose {
    Treatment,
    Payment,
    Operations,
    PublicHealth,
    Research,
    AiTraining,
    AiInference,
    FederalProgram,
}

/// Use-mode constraint on a grant (§4.3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UseMode {
    ReadOnly,
    ReadAndRely,
    ReadAndExport,
}

/// Quantitative bounds on a grant (§4.3.1). All optional; absent = unbounded on that axis.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeConstraints {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_records: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_requests: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rate_per_hour: Option<u64>,
}
