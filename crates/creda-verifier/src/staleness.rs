//! Per-use-type stale-state policy for the Verifier (spec §13.4.3, §10.3.3).
//!
//! The Verifier runs offline against a local DAG replica that may lag the network. How much lag is
//! tolerable is **not** universal: a routine point-of-care read tolerates far more than a fresh
//! authorization check just before a bulk export, and institutions differ in risk tolerance — so
//! the relying institution keeps override authority by constructing the [`StalenessPolicy`]. This
//! module classifies a verification request into a [`UseClass`] and maps it to a staleness
//! threshold. The numbers in [`StalenessPolicy::recommended`] are **bootstrap defaults** to be
//! refined per deployment with pilot data (see `docs/staleness-policy.md`); the *structure* —
//! per-use-type thresholds with institutional override — is the resolution of open question §13.4.3.

use std::collections::BTreeSet;

use creda_events::{GrantPurpose, UseMode};
use creda_graph::AuthorizationQuery;

/// The staleness-relevant class of a use, **most-protective first**. Classification returns the
/// first matching class, so a tighter class always wins — an export of sensitive data is
/// [`PreExport`](UseClass::PreExport), not [`SensitiveRead`](UseClass::SensitiveRead).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseClass {
    /// Data is being released out of the institution (`UseMode::ReadAndExport`). Tightest: a near
    /// fresh view is wanted so a just-issued revocation is caught before data leaves.
    PreExport,
    /// A read touching a sensitive data category (42 CFR Part 2, behavioral health, HIV,
    /// reproductive, genetic, …). Tight.
    SensitiveRead,
    /// Research or AI use (`Research`, `AiTraining`, `AiInference`). Moderate — the data tolerates
    /// staleness, but consent-revocation freshness still matters.
    Research,
    /// Everything else — routine point-of-care / operations reads. Most tolerant (availability).
    RoutineRead,
}

impl UseClass {
    /// Stable lower-case label for reports and logs.
    pub fn label(self) -> &'static str {
        match self {
            UseClass::PreExport => "pre-export",
            UseClass::SensitiveRead => "sensitive-read",
            UseClass::Research => "research",
            UseClass::RoutineRead => "routine-read",
        }
    }
}

/// Per-use-type staleness thresholds (seconds) plus the data-category labels treated as sensitive.
/// The relying institution constructs this; overriding the defaults is its §13.4.3 authority.
/// [`recommended`](Self::recommended) supplies bootstrap defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StalenessPolicy {
    pub pre_export_secs: i64,
    pub sensitive_read_secs: i64,
    pub research_secs: i64,
    pub routine_read_secs: i64,
    /// Data-category labels that make a read `SensitiveRead`, stored lower-cased and matched
    /// case-insensitively against the query's `requested_data_categories`.
    pub sensitive_categories: BTreeSet<String>,
}

impl StalenessPolicy {
    /// Recommended **bootstrap** thresholds (NOT calibrated). See `docs/staleness-policy.md`; refine
    /// per deployment with pilot data. The relying institution may override any field.
    pub fn recommended() -> Self {
        Self {
            pre_export_secs: 5 * 60,         // 5 minutes — fresh auth before data leaves
            sensitive_read_secs: 60 * 60,    // 1 hour
            research_secs: 12 * 60 * 60,     // 12 hours
            routine_read_secs: 24 * 60 * 60, // 24 hours
            sensitive_categories: default_sensitive_categories(),
        }
    }

    /// A uniform policy: every use class uses the same threshold (the pre-§13.4.3 single-threshold
    /// behavior, and a simple starting point). No category is treated as sensitive.
    pub fn uniform(secs: i64) -> Self {
        Self {
            pre_export_secs: secs,
            sensitive_read_secs: secs,
            research_secs: secs,
            routine_read_secs: secs,
            sensitive_categories: BTreeSet::new(),
        }
    }

    /// Classify a request into its [`UseClass`], most-protective first.
    pub fn classify(&self, query: &AuthorizationQuery) -> UseClass {
        if query.use_mode == UseMode::ReadAndExport {
            return UseClass::PreExport;
        }
        if self.touches_sensitive(query) {
            return UseClass::SensitiveRead;
        }
        if matches!(
            query.purpose,
            GrantPurpose::Research | GrantPurpose::AiTraining | GrantPurpose::AiInference
        ) {
            return UseClass::Research;
        }
        UseClass::RoutineRead
    }

    /// The staleness threshold (seconds) for a class.
    pub fn threshold_secs(&self, class: UseClass) -> i64 {
        match class {
            UseClass::PreExport => self.pre_export_secs,
            UseClass::SensitiveRead => self.sensitive_read_secs,
            UseClass::Research => self.research_secs,
            UseClass::RoutineRead => self.routine_read_secs,
        }
    }

    /// Classify and resolve the threshold for a request in one step.
    pub fn threshold_for(&self, query: &AuthorizationQuery) -> (UseClass, i64) {
        let class = self.classify(query);
        (class, self.threshold_secs(class))
    }

    fn touches_sensitive(&self, query: &AuthorizationQuery) -> bool {
        query
            .requested_data_categories
            .iter()
            .any(|c| self.sensitive_categories.contains(&c.to_ascii_lowercase()))
    }
}

impl Default for StalenessPolicy {
    fn default() -> Self {
        Self::recommended()
    }
}

/// Recommended default sensitive-category labels (lower-case). Institutions map their own
/// data-category vocabulary; this is a starting set of commonly-regulated categories.
fn default_sensitive_categories() -> BTreeSet<String> {
    [
        "behavioral-health",
        "mental-health",
        "psychotherapy-notes",
        "substance-use",
        "substance-use-disorder",
        "part2",
        "hiv",
        "aids",
        "reproductive-health",
        "sexual-health",
        "genetic",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
