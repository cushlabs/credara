//! Demographics and verification metadata — spec §5.3.1 and §3.4.1.
//!
//! Demographic values are **tokenized** (privacy by structure, §3.2): Creda's event graph
//! carries tokens produced by an external tokenizer (TEFCA IAS, per Appendix C / open
//! question 13), not raw PII. This crate is agnostic to the tokenization scheme — it treats
//! a token as an opaque, comparable string — but the type names make the intent explicit so
//! raw PII is not accidentally placed in the graph.
//!
//! All fields are optional: an `Assert` carries only what the institution verified.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An opaque demographic token (e.g. a TEFCA IAS token for a name part or SSN fragment).
/// Equality/ordering are over the token bytes; this crate never sees raw PII.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokenizedString(pub String);

impl From<&str> for TokenizedString {
    fn from(s: &str) -> Self {
        TokenizedString(s.to_string())
    }
}

/// A tokenized date (e.g. date of birth), stored as an opaque token rather than a raw date.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokenizedDate(pub String);

/// FHIR administrative gender value set (§5.3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdministrativeGender {
    Male,
    Female,
    Other,
    Unknown,
}

/// A structured, normalized address. Components are tokenized; this crate is agnostic to the
/// normalizer (libpostal, per Appendix C §8.2.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredAddress {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub line1: Option<TokenizedString>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub line2: Option<TokenizedString>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub city: Option<TokenizedString>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub state: Option<TokenizedString>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub postal_code: Option<TokenizedString>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub country: Option<TokenizedString>,
}

/// An institutional identifier such as an MRN: the issuing institution plus the value.
/// A patient may have several (§3.4.1), so they are carried as an array on `Demographics`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstitutionalIdentifier {
    pub institution_id: TokenizedString,
    pub value: TokenizedString,
}

/// An insurance identifier: payer plus member id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsuranceIdentifier {
    pub payer_id: TokenizedString,
    pub member_id: TokenizedString,
}

/// Tokenized demographics (§5.3.1). All fields optional; absent fields are omitted from the
/// canonical encoding (not encoded as null). `extensions` uses a `BTreeMap` so its key order
/// is deterministic before the canonical pass even runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Demographics {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name_family: Option<Vec<TokenizedString>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name_given: Option<Vec<TokenizedString>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name_middle: Option<Vec<TokenizedString>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub date_of_birth: Option<TokenizedDate>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sex: Option<AdministrativeGender>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub address: Option<StructuredAddress>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ssn_last_four: Option<TokenizedString>,
    /// (institution_id, mrn_value) pairs. A patient may hold multiple MRNs.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mrns: Vec<InstitutionalIdentifier>,
    /// (payer_id, member_id) pairs.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub insurance_member_ids: Vec<InsuranceIdentifier>,
    /// Extensible, namespaced key → token bag (e.g. `us-va:veteran-id`). `BTreeMap` for
    /// deterministic ordering.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub extensions: BTreeMap<String, TokenizedString>,
}

impl Demographics {
    /// True if no demographic field is populated — e.g. after a tombstone scrubs content.
    pub fn is_empty(&self) -> bool {
        self.name_family.is_none()
            && self.name_given.is_none()
            && self.name_middle.is_none()
            && self.date_of_birth.is_none()
            && self.sex.is_none()
            && self.address.is_none()
            && self.ssn_last_four.is_none()
            && self.mrns.is_empty()
            && self.insurance_member_ids.is_empty()
            && self.extensions.is_empty()
    }
}

/// How an institution verified the demographics in an `Assert` (§3.4.1, §5.3.2). Feeds
/// confidence scoring downstream (computed in `creda-graph`, M3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationMethod {
    GovernmentPhotoId,
    BirthCertificate,
    VitalRecords,
    InsuranceCard,
    Biometric,
    SelfReport,
    /// Inherited from another institution's assertion; confidence is discounted downstream.
    ReferralInherited,
    Other,
}
