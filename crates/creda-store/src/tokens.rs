//! Demographic token extraction for the token → entry-points index (spec §5.2.5).
//!
//! The index maps each tokenized demographic value carried by an event to that event's UUID,
//! so an institution can tokenize a patient's demographics at registration and look up
//! candidate subgraph entry points. Only events that carry demographics contribute tokens:
//! `Assert` (its `demographics`) and `Amend` (its `updated_demographics`). Every other event
//! type contributes none.

use std::collections::BTreeSet;

use creda_events::{Demographics, EventPayload, IdentityEventNode, StructuredAddress};

/// The set of demographic tokens an event contributes to the token index, sorted and
/// de-duplicated. Empty for event types that carry no demographics.
pub fn demographic_tokens(node: &IdentityEventNode) -> Vec<String> {
    let demographics = match &node.payload {
        EventPayload::Assert { demographics, .. } => Some(demographics),
        EventPayload::Amend {
            updated_demographics,
            ..
        } => Some(updated_demographics),
        _ => None,
    };

    let mut tokens: BTreeSet<String> = BTreeSet::new();
    if let Some(d) = demographics {
        collect_demographics(d, &mut tokens);
    }
    tokens.into_iter().collect()
}

fn collect_demographics(d: &Demographics, out: &mut BTreeSet<String>) {
    push_name_parts(&d.name_family, out);
    push_name_parts(&d.name_given, out);
    push_name_parts(&d.name_middle, out);
    if let Some(dob) = &d.date_of_birth {
        out.insert(dob.0.clone());
    }
    if let Some(ssn) = &d.ssn_last_four {
        out.insert(ssn.0.clone());
    }
    if let Some(addr) = &d.address {
        collect_address(addr, out);
    }
    for mrn in &d.mrns {
        out.insert(mrn.value.0.clone());
    }
    for ins in &d.insurance_member_ids {
        out.insert(ins.member_id.0.clone());
    }
    for value in d.extensions.values() {
        out.insert(value.0.clone());
    }
}

fn push_name_parts(parts: &Option<Vec<creda_events::TokenizedString>>, out: &mut BTreeSet<String>) {
    if let Some(parts) = parts {
        for p in parts {
            out.insert(p.0.clone());
        }
    }
}

fn collect_address(addr: &StructuredAddress, out: &mut BTreeSet<String>) {
    for field in [
        &addr.line1,
        &addr.line2,
        &addr.city,
        &addr.state,
        &addr.postal_code,
        &addr.country,
    ] {
        if let Some(v) = field {
            out.insert(v.0.clone());
        }
    }
}
