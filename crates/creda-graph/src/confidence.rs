//! Per-field confidence scoring (spec §5.3.2–§5.3.3).
//!
//! Confidence is computed **per demographic field/value**, never per patient (§5.3.2), as a
//! function of four signals: verification-method weight, institutional credibility,
//! independent-agreement amplification, and independent-attestation amplification — then
//! attenuated by temporal decay (§5.3.3). The result is a `u16` in basis points (0–10000 =
//! 0.00–100.00%), avoiding floating point so it serializes deterministically.
//!
//! The combiner is an additive-evidence model (the Fellegi–Sunter principle of summing
//! independent log-likelihood-style evidence, ported rather than reinvented; see Appendix C.1)
//! squashed by a saturating function `conf = 10000·T/(T+K)` that gives the diminishing returns
//! §5.3.2 requires — the tenth agreement adds less than the second. Independence is enforced by
//! counting each institution at most once (its strongest assertion).
//!
//! **Calibration follows the documented methodology** (`docs/matching-calibration.md`, §5.3.2): the
//! concrete weights, the saturation constant `K`, and the decay curves below are **bootstrap priors**,
//! not calibrated values — loaded as network configuration and re-estimated per deployment against
//! that population's data and a validation set. Same discipline as the `$match` scorer (a different
//! model — evidence reliability, not record linkage — but the same per-deployment, validated, auditable
//! process). `TODO(open-question-confidence-calibration)`: the *methodology* is resolved; the
//! calibrated numbers remain a per-deployment step.

use std::collections::{HashMap, HashSet};

use creda_events::{CertificateFingerprint, VerificationMethod};

/// How quickly a field's confidence decays with age (§5.3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldClass {
    /// Never decays (date of birth, sex, SSN).
    NonDecaying,
    /// Decays slowly (names — legal changes are infrequent).
    SlowDecaying,
    /// Decays quickly (address, insurance member id).
    FastDecaying,
}

/// Base weight per verification method (§5.3.2). Higher = more reliable for identity.
#[derive(Clone, Debug)]
pub struct MethodWeights {
    pub government_photo_id: u64,
    pub birth_certificate: u64,
    pub vital_records: u64,
    pub insurance_card: u64,
    pub biometric: u64,
    pub self_report: u64,
    pub referral_inherited: u64,
    pub other: u64,
}

impl Default for MethodWeights {
    fn default() -> Self {
        // TODO(open-question-confidence-calibration): placeholder weights, network-configurable.
        Self {
            government_photo_id: 100,
            birth_certificate: 100,
            vital_records: 100,
            insurance_card: 50,
            biometric: 90,
            self_report: 15,
            // Simplification: a referral inherits a discounted weight. Properly it should
            // inherit the *referenced* assertion's confidence, discounted — deferred.
            referral_inherited: 30,
            other: 20,
        }
    }
}

/// A single decay curve: full confidence until `full_secs`, then linear to `floor_pct` by
/// `floor_after_secs`, then flat at `floor_pct`.
#[derive(Clone, Copy, Debug)]
pub struct DecayCurve {
    pub full_secs: i64,
    pub floor_after_secs: i64,
    pub floor_pct: u64,
}

/// Configuration for the confidence engine. All values are network-level configuration and
/// locally overridable (§5.3.2); the defaults are uncalibrated placeholders.
#[derive(Clone, Debug)]
pub struct ConfidenceConfig {
    pub method_weights: MethodWeights,
    /// Per-institution credibility weight as a percentage (100 = neutral).
    pub institution_credibility: HashMap<CertificateFingerprint, u64>,
    pub default_credibility: u64,
    /// Evidence contributed per distinct independent attesting institution.
    pub attest_weight: u64,
    /// Saturation constant `K` in `conf = 10000·T/(T+K)`.
    pub saturation_k: u64,
    pub slow_decay: DecayCurve,
    pub fast_decay: DecayCurve,
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        const YEAR: i64 = 365 * 24 * 3600;
        Self {
            method_weights: MethodWeights::default(),
            institution_credibility: HashMap::new(),
            default_credibility: 100,
            attest_weight: 25,
            saturation_k: 100,
            // Names: full for 2y, linear to 30% by 7y.
            slow_decay: DecayCurve {
                full_secs: 2 * YEAR,
                floor_after_secs: 7 * YEAR,
                floor_pct: 30,
            },
            // Address/insurance: full for 0.5y, linear to 10% by 3.5y.
            fast_decay: DecayCurve {
                full_secs: YEAR / 2,
                floor_after_secs: 7 * YEAR / 2,
                floor_pct: 10,
            },
        }
    }
}

/// One assertion contributing to a particular (field, value): how it was verified, who
/// asserted it, and how old the assertion is.
#[derive(Clone, Debug)]
pub struct Contribution {
    pub method: VerificationMethod,
    pub institution: CertificateFingerprint,
    /// Age of the assertion in seconds (negative ages are clamped to 0).
    pub age_secs: i64,
}

impl ConfidenceConfig {
    /// Base weight for a verification method.
    pub fn method_weight(&self, method: VerificationMethod) -> u64 {
        let w = &self.method_weights;
        match method {
            VerificationMethod::GovernmentPhotoId => w.government_photo_id,
            VerificationMethod::BirthCertificate => w.birth_certificate,
            VerificationMethod::VitalRecords => w.vital_records,
            VerificationMethod::InsuranceCard => w.insurance_card,
            VerificationMethod::Biometric => w.biometric,
            VerificationMethod::SelfReport => w.self_report,
            VerificationMethod::ReferralInherited => w.referral_inherited,
            VerificationMethod::Other => w.other,
        }
    }

    /// Credibility percentage for an institution (default if unconfigured).
    pub fn credibility(&self, institution: &CertificateFingerprint) -> u64 {
        self.institution_credibility
            .get(institution)
            .copied()
            .unwrap_or(self.default_credibility)
    }

    /// Decay percentage (0–100) for a field class at a given age.
    pub fn decay_pct(&self, class: FieldClass, age_secs: i64) -> u64 {
        let age = age_secs.max(0);
        let curve = match class {
            FieldClass::NonDecaying => return 100,
            FieldClass::SlowDecaying => self.slow_decay,
            FieldClass::FastDecaying => self.fast_decay,
        };
        if age <= curve.full_secs {
            100
        } else if age >= curve.floor_after_secs {
            curve.floor_pct
        } else {
            // Linear interpolation from 100% down to floor_pct.
            let span = (curve.floor_after_secs - curve.full_secs).max(1) as i128;
            let into = (age - curve.full_secs) as i128;
            let drop = (100 - curve.floor_pct as i128) * into / span;
            (100 - drop).max(curve.floor_pct as i128) as u64
        }
    }

    /// Score one (field, value): combine the contributing assertions (one per institution,
    /// strongest wins — independence) plus independent attestations, into 0–10000 basis points.
    pub fn score(
        &self,
        class: FieldClass,
        contributions: &[Contribution],
        attesting_institutions: &HashSet<CertificateFingerprint>,
    ) -> u16 {
        // Strongest evidence per distinct institution (independence: count each once).
        let mut best: HashMap<&CertificateFingerprint, u64> = HashMap::new();
        for c in contributions {
            let evidence = self.method_weight(c.method)
                * self.credibility(&c.institution)
                * self.decay_pct(class, c.age_secs)
                / 10_000; // divide out the two percentage scales
            let slot = best.entry(&c.institution).or_insert(0);
            if evidence > *slot {
                *slot = evidence;
            }
        }
        let t_assert: u64 = best.values().sum();
        let t_attest: u64 = self.attest_weight * attesting_institutions.len() as u64;
        let t = (t_assert + t_attest) as u128;

        let conf = 10_000u128 * t / (t + self.saturation_k as u128);
        conf.min(10_000) as u16
    }
}
