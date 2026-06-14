//! Effective-identity projection (spec §5.2.4) with per-field disagreement flagging (§5.3.4).
//!
//! The effective identity is a **projection**, not a stored record: from a set of entry
//! points, traverse the subgraph and aggregate demographics per field, respecting amendments
//! (supersede), contestations (sever the contested Link), and tombstones (no demographics).
//! Each field reports every asserted value with its confidence; a field with more than one
//! distinct value is flagged `disputed` — the system surfaces disagreement rather than picking
//! a winner (§5.3.4) — UNLESS exactly one of the competing values carries an attestation, in
//! which case that recorded reliance resolves the disagreement in its favor (§5.3.2). This makes
//! a clinician's resolving attestation a durable, projection-level effect rather than transient
//! UI state.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use creda_events::{
    AdministrativeGender, CertificateFingerprint, ContentHash, Demographics, EventId, EventPayload,
    IdentityEventNode, IdentityEventType, StructuredAddress, TokenizedString,
};

use crate::confidence::{ConfidenceConfig, Contribution, FieldClass};
use crate::subgraph::Subgraph;
use crate::validation;

/// A demographic field of the effective identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldKey {
    NameFamily,
    NameGiven,
    NameMiddle,
    DateOfBirth,
    Sex,
    Address,
    SsnLastFour,
    Mrn,
    InsuranceMemberId,
    Extension(String),
}

fn field_class(key: &FieldKey) -> FieldClass {
    match key {
        FieldKey::DateOfBirth | FieldKey::Sex | FieldKey::SsnLastFour | FieldKey::Mrn => {
            FieldClass::NonDecaying
        }
        FieldKey::NameFamily
        | FieldKey::NameGiven
        | FieldKey::NameMiddle
        | FieldKey::Extension(_) => FieldClass::SlowDecaying,
        FieldKey::Address | FieldKey::InsuranceMemberId => FieldClass::FastDecaying,
    }
}

/// One asserted value for a field, with its confidence and the events supporting it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldValue {
    /// The tokenized value (an opaque string; this layer never sees raw PII).
    pub value: String,
    /// Confidence in basis points (0–10000 = 0.00–100.00%).
    pub confidence: u16,
    /// The Assert events supporting this value, sorted.
    pub supporting: Vec<EventId>,
}

/// All asserted values for one field. `disputed` is true when institutions assert conflicting
/// values (§5.3.4); values are sorted by confidence descending, then value ascending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldEntry {
    pub values: Vec<FieldValue>,
    pub disputed: bool,
}

/// The computed effective identity — a per-field projection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectiveIdentity {
    pub fields: BTreeMap<FieldKey, FieldEntry>,
}

impl EffectiveIdentity {
    /// Whether no demographic fields were derived.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The entry for a field, if any.
    pub fn field(&self, key: &FieldKey) -> Option<&FieldEntry> {
        self.fields.get(key)
    }

    /// Fields with conflicting asserted values.
    pub fn disputed_fields(&self) -> Vec<&FieldKey> {
        self.fields
            .iter()
            .filter(|(_, e)| e.disputed)
            .map(|(k, _)| k)
            .collect()
    }
}

#[derive(Default)]
struct Group {
    contributions: Vec<Contribution>,
    support: BTreeSet<EventId>,
    attesters: HashSet<CertificateFingerprint>,
}

/// Project the effective identity for the subgraph reachable from `entry_points`.
///
/// `now_unix_secs` is the reference time for temporal decay (§5.3.3); event ages are derived
/// from each contributing assertion's wall-clock timestamp.
pub fn project(
    subgraph: &Subgraph,
    entry_points: &[EventId],
    config: &ConfidenceConfig,
    now_unix_secs: i64,
) -> EffectiveIdentity {
    // §5.2.4 step 4: contested Links (by a *valid* Contest) are severed.
    let mut contested: HashSet<EventId> = HashSet::new();
    for contest in subgraph.nodes_of_type(IdentityEventType::Contest) {
        if validation::contest_is_valid(subgraph, contest) {
            if let EventPayload::Contest { target_link_id, .. } = &contest.payload {
                contested.insert(*target_link_id);
            }
        }
    }

    // Reachable component from the entry points, not crossing contested Links.
    let reachable = reachable_excluding(subgraph, entry_points, &contested);

    // §5.2.4 step 5: tombstoned nodes carry no demographics.
    let mut tombstoned: HashSet<EventId> = HashSet::new();
    for ts in subgraph.nodes_of_type(IdentityEventType::Tombstone) {
        if let EventPayload::Tombstone {
            target_event_ids, ..
        } = &ts.payload
        {
            tombstoned.extend(target_event_ids.iter().copied());
        }
    }

    // §5.2.4 step 3: valid amendments supersede their targets. Index amends by target.
    let mut amends_by_target: HashMap<EventId, Vec<&IdentityEventNode>> = HashMap::new();
    for amend in subgraph.nodes_of_type(IdentityEventType::Amend) {
        if !reachable.contains(&amend.id) || validation::validate_amend(subgraph, amend).is_err() {
            continue;
        }
        if let EventPayload::Amend {
            target_event_id, ..
        } = &amend.payload
        {
            amends_by_target
                .entry(*target_event_id)
                .or_default()
                .push(amend);
        }
    }

    // Attestation amplification index: assert id -> distinct attesting institutions (§5.3.2).
    let mut attesters: HashMap<EventId, HashSet<CertificateFingerprint>> = HashMap::new();
    for attest in subgraph.nodes_of_type(IdentityEventType::Attest) {
        if !reachable.contains(&attest.id) {
            continue;
        }
        if let EventPayload::Attest {
            target_event_ids, ..
        } = &attest.payload
        {
            for tid in target_event_ids {
                attesters
                    .entry(*tid)
                    .or_default()
                    .insert(attest.institution_id.clone());
            }
        }
    }

    // §5.2.4 step 6: collect uncontested, untombstoned Asserts (with their amendments).
    let mut groups: BTreeMap<FieldKey, BTreeMap<String, Group>> = BTreeMap::new();
    for assert in subgraph.nodes_of_type(IdentityEventType::Assert) {
        if !reachable.contains(&assert.id) || tombstoned.contains(&assert.id) {
            continue;
        }
        let EventPayload::Assert {
            verification_method,
            ..
        } = &assert.payload
        else {
            continue;
        };

        let (demographics, basis) = resolve_amend_chain(assert, &amends_by_target, &tombstoned);
        let age_secs =
            now_unix_secs - unix_secs(&basis.wall_clock_timestamp).unwrap_or(now_unix_secs);
        let assert_attesters = attesters.get(&assert.id).cloned().unwrap_or_default();

        for (key, value) in field_values(demographics) {
            let group = groups.entry(key).or_default().entry(value).or_default();
            group.contributions.push(Contribution {
                method: *verification_method,
                institution: assert.institution_id.clone(),
                age_secs,
            });
            group.support.insert(assert.id);
            group.attesters.extend(assert_attesters.iter().cloned());
        }
    }

    // §5.2.4 steps 7–8: per-field aggregation with confidence + disagreement.
    let mut fields: BTreeMap<FieldKey, FieldEntry> = BTreeMap::new();
    for (key, value_map) in groups {
        let class = field_class(&key);
        // A field with conflicting asserted values is disputed UNLESS exactly one of those values
        // carries an attestation (§5.3.4 + §5.3.2): an institution recording reliance on a value
        // is an explicit decision that resolves the disagreement in that value's favor (and the
        // amplification also lifts its confidence so it sorts first). Zero attestations leaves the
        // conflict open; attestations on *two or more* competing values is itself a disagreement,
        // so it stays disputed. This is what lets the clinician's "resolve DOB" attestation stick
        // across a refresh — the resolution is the persisted Attest, not transient client state.
        let attested_values = value_map
            .values()
            .filter(|g| !g.attesters.is_empty())
            .count();
        // Identifier bags are sets of valid identifiers, not competing values (§3.4.1) — a patient
        // legitimately holds several MRNs / member ids at once, so they never count as a dispute.
        let identifier_set = matches!(&key, FieldKey::Mrn | FieldKey::InsuranceMemberId);
        let disputed = !identifier_set && value_map.len() > 1 && attested_values != 1;
        let mut values: Vec<FieldValue> = value_map
            .into_iter()
            .map(|(value, group)| FieldValue {
                value,
                confidence: config.score(class, &group.contributions, &group.attesters),
                supporting: group.support.into_iter().collect(),
            })
            .collect();
        values.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(a.value.cmp(&b.value)));
        fields.insert(key, FieldEntry { values, disputed });
    }

    EffectiveIdentity { fields }
}

/// Follow the amendment chain from an Assert to its latest non-tombstoned amendment, returning
/// the effective demographics and the node they came from (used for the decay age basis).
fn resolve_amend_chain<'a>(
    assert: &'a IdentityEventNode,
    amends_by_target: &HashMap<EventId, Vec<&'a IdentityEventNode>>,
    tombstoned: &HashSet<EventId>,
) -> (&'a Demographics, &'a IdentityEventNode) {
    let mut cur = assert;
    loop {
        let next = amends_by_target.get(&cur.id).and_then(|amends| {
            amends
                .iter()
                .filter(|a| !tombstoned.contains(&a.id))
                .copied()
                .max_by_key(|a| (a.logical_clock, a.id))
        });
        match next {
            Some(a) => cur = a,
            None => break,
        }
    }
    let demographics = match &cur.payload {
        EventPayload::Assert { demographics, .. } => demographics,
        EventPayload::Amend {
            updated_demographics,
            ..
        } => updated_demographics,
        // resolve_amend_chain only ever lands on an Assert or an Amend.
        _ => match &assert.payload {
            EventPayload::Assert { demographics, .. } => demographics,
            _ => unreachable!("resolve_amend_chain started from a non-Assert"),
        },
    };
    (demographics, cur)
}

/// Reachable component from `entry_points` over undirected parent edges, never visiting an
/// `excluded` node (the contested Links).
fn reachable_excluding(
    subgraph: &Subgraph,
    entry_points: &[EventId],
    excluded: &HashSet<EventId>,
) -> HashSet<EventId> {
    let mut seen: HashSet<EventId> = HashSet::new();
    let mut queue: VecDeque<EventId> = VecDeque::new();
    for e in entry_points {
        if subgraph.contains(e) && !excluded.contains(e) && seen.insert(*e) {
            queue.push_back(*e);
        }
    }
    while let Some(id) = queue.pop_front() {
        let Some(node) = subgraph.get(&id) else {
            continue;
        };
        let mut neighbors: Vec<EventId> = node.parent_ids.clone();
        neighbors.extend(subgraph.children_of(&id));
        for n in neighbors {
            if !excluded.contains(&n) && subgraph.contains(&n) && seen.insert(n) {
                queue.push_back(n);
            }
        }
    }
    seen
}

/// The populated (FieldKey, tokenized-value-string) pairs of a demographics record.
///
/// Covers the scalar demographic fields and the extension bag. The identifier bags (`mrns`,
/// `insurance_member_ids`) are intentionally not folded into the disagreement model here —
/// they are sets of valid identifiers, not competing values — and are a documented follow-up.
fn field_values(d: &Demographics) -> Vec<(FieldKey, String)> {
    let mut out = Vec::new();
    if let Some(v) = &d.name_family {
        out.push((FieldKey::NameFamily, join_tokens(v)));
    }
    if let Some(v) = &d.name_given {
        out.push((FieldKey::NameGiven, join_tokens(v)));
    }
    if let Some(v) = &d.name_middle {
        out.push((FieldKey::NameMiddle, join_tokens(v)));
    }
    if let Some(dob) = &d.date_of_birth {
        out.push((FieldKey::DateOfBirth, dob.0.clone()));
    }
    if let Some(sex) = &d.sex {
        out.push((FieldKey::Sex, gender_str(*sex).to_string()));
    }
    if let Some(addr) = &d.address {
        out.push((FieldKey::Address, address_string(addr)));
    }
    if let Some(ssn) = &d.ssn_last_four {
        out.push((FieldKey::SsnLastFour, ssn.0.clone()));
    }
    // Identifier bags (§3.4.1): each MRN / insurance id is a *valid identifier*, not a competing
    // value, so they are emitted as distinct values of an identifier-set field that never disputes
    // (see the disputed computation). The issuing institution / payer is carried in the value, unit-
    // separated from the identifier, so a reader can show "institution · id" without the signer.
    for mrn in &d.mrns {
        out.push((
            FieldKey::Mrn,
            format!("{}\u{1f}{}", mrn.institution_id.0, mrn.value.0),
        ));
    }
    for ins in &d.insurance_member_ids {
        out.push((
            FieldKey::InsuranceMemberId,
            format!("{}\u{1f}{}", ins.payer_id.0, ins.member_id.0),
        ));
    }
    for (k, v) in &d.extensions {
        out.push((FieldKey::Extension(k.clone()), v.0.clone()));
    }
    out
}

fn join_tokens(tokens: &[TokenizedString]) -> String {
    tokens
        .iter()
        .map(|t| t.0.as_str())
        .collect::<Vec<_>>()
        .join("\u{1f}") // unit separator — tokens are opaque
}

fn gender_str(g: AdministrativeGender) -> &'static str {
    match g {
        AdministrativeGender::Male => "male",
        AdministrativeGender::Female => "female",
        AdministrativeGender::Other => "other",
        AdministrativeGender::Unknown => "unknown",
    }
}

fn address_string(a: &StructuredAddress) -> String {
    [
        a.line1.as_ref(),
        a.line2.as_ref(),
        a.city.as_ref(),
        a.state.as_ref(),
        a.postal_code.as_ref(),
        a.country.as_ref(),
    ]
    .into_iter()
    .map(|f| f.map(|t| t.0.as_str()).unwrap_or(""))
    .collect::<Vec<_>>()
    .join("\u{1f}")
}

fn unix_secs(rfc3339: &str) -> Option<i64> {
    time::OffsetDateTime::parse(rfc3339, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp())
}

/// The §8.2.2 CredaPatient identity envelope for a subgraph: a deterministic, **peer-identical**
/// identifier plus the data the FHIR projection's `mustSupport` extensions need. Every peer holding
/// the same subgraph computes the same values, which is what makes the subgraph identifier a stable
/// cross-institution key (and why it lives here in shared graph logic, not at any one Bridge).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubgraphIdentity {
    /// Blake3 over the canonical (sorted, concatenated 16-byte) root-set ids (§8.2.2). 32 bytes.
    pub subgraph_id: Vec<u8>,
    /// The root set: the sorted ids of the subgraph's parentless root Asserts (§3.4.1).
    pub root_set: Vec<EventId>,
    /// The most recently authored event (max logical clock, ties broken by id) — backs the
    /// last-modified-event extension. `None` only for an empty subgraph.
    pub last_modified_event: Option<EventId>,
}

/// Compute the deterministic identity envelope for a subgraph (§8.2.2). The subgraph identifier is
/// `Blake3` over the sorted root-set ids concatenated as raw 16-byte values — order-independent
/// because the set is sorted first, so two peers with the same roots agree byte-for-byte.
pub fn subgraph_identity(subgraph: &Subgraph) -> SubgraphIdentity {
    let mut root_set = subgraph.roots();
    root_set.sort();
    let mut buf = Vec::with_capacity(root_set.len() * 16);
    for id in &root_set {
        buf.extend_from_slice(id.as_bytes());
    }
    let subgraph_id = ContentHash::blake3(&buf).digest;
    let last_modified_event = subgraph
        .nodes()
        .max_by_key(|n| (n.logical_clock, n.id))
        .map(|n| n.id);
    SubgraphIdentity {
        subgraph_id,
        root_set,
        last_modified_event,
    }
}
