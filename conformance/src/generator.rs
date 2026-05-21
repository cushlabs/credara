//! Synthetic data generator (spec §11.4.2).
//!
//! Produces synthetic patient subgraphs whose **content is deterministic** from a seed (the same
//! seed yields the same demographics and structure), with realistic demographics from small
//! public-domain corpora and realistic event chains. Every event is tagged as test data
//! (§11.4.1) so it propagates and replicates like a real event but is filtered from clinical
//! responses (see [`crate::filter`]).
//!
//! Note: event UUIDs and signing keys are inherently random/time-based (UUIDv7, §5.1.4), so the
//! *identifiers* differ run to run; the seed governs the synthetic **content** (names, DOBs,
//! scenario shape), which is what "reproducible scenario" means here.

use creda_events::{
    AuthorizationScope, AttestPurpose, CertificateFingerprint, Demographics, EventPayload,
    GrantAudience, GrantPurpose, IdentityEventNode, LinkMethod, SignatureAlgorithm, SigningKey,
    StructuredAddress, TestDataTag, TokenizedDate, TokenizedString, UseMode, VerificationMethod,
};
use creda_store::Store;

const WALL: &str = "2026-01-01T00:00:00Z";

// Small public-domain corpora (common surnames / given names / US places — facts, not
// copyrightable). Tokens below are synthetic stand-ins for real TEFCA-tokenized values.
const FAMILY: &[&str] = &["smith", "johnson", "williams", "brown", "jones", "garcia", "miller", "davis"];
const GIVEN: &[&str] = &["james", "mary", "robert", "patricia", "john", "jennifer", "michael", "linda"];
const CITY: &[&str] = &["springfield", "franklin", "clinton", "georgetown", "madison", "salem"];
const STATE: &[&str] = &["ca", "tx", "ny", "fl", "il", "pa"];

/// A synthetic patient scenario (§11.4.2). Extensible; M9 implements the core set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// A single institution asserts the patient (one root).
    Simple,
    /// Two institutions assert conflicting demographics (a disputed field).
    Disagreement,
    /// One assert plus an AuthorizationGrant to the conformance requester and an attestation.
    Authorized,
}

/// The fixed requesting-institution fingerprint that [`Scenario::Authorized`] grants to, so tests
/// can query authorization deterministically.
pub fn conformance_requester() -> CertificateFingerprint {
    CertificateFingerprint::from_public_key_bytes(b"creda-conformance-requester")
}

/// Deterministic synthetic data generator.
pub struct Generator {
    state: u64,
    test_id: String,
    clock: u64,
}

impl Generator {
    /// New generator with a content seed and a test-plan identifier (recorded on every event's
    /// test-data tag).
    pub fn new(seed: u64, test_id: impl Into<String>) -> Self {
        Self { state: seed, test_id: test_id.into(), clock: 0 }
    }

    /// splitmix64 — a tiny, dependency-free, deterministic PRNG.
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() as usize) % items.len()]
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn tag(&self) -> TestDataTag {
        TestDataTag {
            purpose: "integration-testing".to_string(),
            originating_test: self.test_id.clone(),
            expiration_time: None,
        }
    }

    /// A fresh institutional signing key. Institution identity is not part of the deterministic
    /// content (keys are random); the demographics/structure are what's reproducible.
    fn signer(&self) -> SigningKey {
        SigningKey::generate(SignatureAlgorithm::Ed25519).expect("ed25519 keygen")
    }

    fn demographics(&mut self) -> Demographics {
        let family = (*self.pick(FAMILY)).to_string();
        let given = (*self.pick(GIVEN)).to_string();
        let year = 1940 + (self.next() % 70);
        let month = 1 + (self.next() % 12);
        let day = 1 + (self.next() % 28);
        let sex = *self.pick(&[
            creda_events::AdministrativeGender::Male,
            creda_events::AdministrativeGender::Female,
            creda_events::AdministrativeGender::Other,
        ]);
        let city = (*self.pick(CITY)).to_string();
        let state = (*self.pick(STATE)).to_string();
        Demographics {
            name_family: Some(vec![tok(&family)]),
            name_given: Some(vec![tok(&given)]),
            date_of_birth: Some(TokenizedDate(format!("tok:{year:04}-{month:02}-{day:02}"))),
            sex: Some(sex),
            address: Some(StructuredAddress {
                city: Some(tok(&city)),
                state: Some(tok(&state)),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn assert_event(&mut self, key: &SigningKey, demographics: Demographics) -> IdentityEventNode {
        let clock = self.tick();
        IdentityEventNode::create_test_data(
            EventPayload::Assert { demographics, verification_method: VerificationMethod::GovernmentPhotoId },
            vec![],
            key,
            clock,
            WALL,
            None,
            self.tag(),
        )
        .expect("valid synthetic Assert")
    }

    /// Generate the events for one synthetic patient under a scenario.
    pub fn patient(&mut self, scenario: Scenario) -> Vec<IdentityEventNode> {
        match scenario {
            Scenario::Simple => {
                let key = self.signer();
                let demo = self.demographics();
                vec![self.assert_event(&key, demo)]
            }
            Scenario::Disagreement => {
                let ka = self.signer();
                let kb = self.signer();
                let mut a_demo = self.demographics();
                let mut b_demo = self.demographics();
                // Force a conflicting date of birth across the two institutions.
                a_demo.date_of_birth = Some(TokenizedDate("tok:1980-01-01".into()));
                b_demo.date_of_birth = Some(TokenizedDate("tok:1990-12-31".into()));
                vec![self.assert_event(&ka, a_demo), self.assert_event(&kb, b_demo)]
            }
            Scenario::Authorized => {
                let ka = self.signer();
                let kb = self.signer();
                let demo = self.demographics();
                let assert = self.assert_event(&ka, demo);

                let grant_clock = self.tick();
                let grant = IdentityEventNode::create_test_data(
                    EventPayload::AuthorizationGrant {
                        scope: AuthorizationScope::default(),
                        audience: GrantAudience::InstitutionId(conformance_requester()),
                        purpose: GrantPurpose::Treatment,
                        expiration: None,
                        volume_constraints: None,
                        use_mode: UseMode::ReadAndRely,
                    },
                    vec![assert.id],
                    &ka,
                    grant_clock,
                    WALL,
                    None,
                    self.tag(),
                )
                .expect("valid synthetic Grant");

                let attest_clock = self.tick();
                let attest = IdentityEventNode::create_test_data(
                    EventPayload::Attest {
                        target_event_ids: vec![assert.id],
                        purpose: AttestPurpose::Treatment,
                    },
                    vec![assert.id],
                    &kb,
                    attest_clock,
                    WALL,
                    None,
                    self.tag(),
                )
                .expect("valid synthetic Attest");

                vec![assert, grant, attest]
            }
        }
    }

    /// Generate `num_patients` patients under the same scenario. Scale is configurable from a
    /// single patient to millions for load testing (§11.4.2).
    pub fn generate(&mut self, num_patients: usize, scenario: Scenario) -> Vec<IdentityEventNode> {
        (0..num_patients).flat_map(|_| self.patient(scenario)).collect()
    }

    /// Optionally a Link connecting the head events of two patients (for link/contest scenarios).
    pub fn link(&mut self, signer: &SigningKey, head_a: creda_events::EventId, head_b: creda_events::EventId) -> IdentityEventNode {
        let clock = self.tick();
        IdentityEventNode::create_test_data(
            EventPayload::Link {
                target_subgraph_heads: (head_a, head_b),
                confidence_score: 9000,
                method: LinkMethod::Algorithmic,
            },
            vec![head_a, head_b],
            signer,
            clock,
            WALL,
            None,
            self.tag(),
        )
        .expect("valid synthetic Link")
    }

    /// Load events into a store.
    pub fn populate(store: &dyn Store, events: &[IdentityEventNode]) -> creda_store::Result<()> {
        for e in events {
            store.put_event(e)?;
        }
        Ok(())
    }
}

fn tok(value: &str) -> TokenizedString {
    TokenizedString(format!("tok:{value}"))
}
