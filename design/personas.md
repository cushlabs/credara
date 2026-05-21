# Creda — Personas & Contexts

This document captures who Creda's interfaces serve and what each is allowed to see and do.
It is a design reference for the mockups in this folder, not a specification — the authoritative
source is [`docs/creda-technical-spec.md`](../docs/creda-technical-spec.md).

There are two intersecting axes:

- **Human personas** — who is at the screen, and why.
- **System contexts (views)** — the slice of the trust graph a session is permitted to see. A
  persona always operates *within* a context; the same person in two contexts sees different data.

Every action a persona takes resolves to a **signed event** appended to the DAG. The event types
(spec §3.4, §4.3) are the vocabulary all personas share:

| Action | Event type |
|---|---|
| Assert demographics for a patient | `Assert` |
| Claim two records are the same person | `Link` |
| Dispute a link | `Contest` |
| Record reliance on a chain (for a purpose) | `Attest` |
| Correct demographics (by the originating institution) | `Amend` |
| Scrub content / right to be forgotten | `Tombstone` |
| Grant access | `AuthorizationGrant` |
| Revoke access | `AuthorizationRevocation` |
| Record an authorized export | `ExportReceipt` |

Identity events are **advisory** (the consumer decides how much to trust the projection);
authorization events are **enforced** (§4.8).

---

## Human personas

### 1. Clinician (point of care)

- **Role.** A treating clinician viewing the identity of the patient in front of them, inside
  their EHR via the FHIR Bridge.
- **Context / view.** Clinical view — synthetic/test-tagged records are filtered out (§11.4.1);
  demographics are detokenized at the point of care. Read-mostly; trusts the projection.
- **Key actions.** Confirm/rely → `Attest`; flag a mismatch or "not the same person" → `Contest`;
  request a correction → `Amend` (takes effect only once the originating institution accepts it,
  §3.4.5); request access → an `AuthorizationGrant` flow.
- **Sees / doesn't see.** Sees the effective identity with per-field confidence and disagreement
  flags, and the provenance DAG. Never sees synthetic records or another institution's clinical
  payloads.
- **Status.** Mockup built — [`clinician-review-mockup.html`](clinician-review-mockup.html).

### 2. Identity steward / operator

- **Role.** An internal operator who resolves cross-institutional identity-resolution problems:
  low-confidence links, conflicting demographics, open contests, possible duplicates, and stale or
  anomalous records.
- **Context / view.** Operator view — sees everything, including synthetic/test-tagged events
  (clearly flagged), for triage and load testing.
- **Key actions.** The fuller graph-editing surface: create a `Link` (with confidence + method),
  `Contest` a link to sever an incorrect merge, review and accept an `Amend`, and (under legal
  basis) `Tombstone` content. Each action is signed by the operator's institution.
- **Sees / doesn't see.** Sees the merged subgraph (the DAG across institutions), per-field
  confidence (Fellegi–Sunter-style weighting, §5.3) and disagreement, link methods and scores,
  and the test-data tag. Still bound by tokenization — operates on tokens, not raw PHI.
- **Status.** Mockup built — [`steward-console-mockup.html`](steward-console-mockup.html).

### 3. Compliance / audit reviewer

- **Role.** Reviews authorization activity and provenance for who-accessed-what and policy
  adherence — grants, revocations, export receipts, and the integrity of provenance chains.
- **Context / view.** Operator-scoped, read-only and audit-oriented.
- **Key actions.** Read-only. Traverses `AuthorizationGrant` / `AuthorizationRevocation` /
  `ExportReceipt` events and the provenance DAG; verifies revocation latency and dual-control
  adherence. Does not author identity or authorization events.
- **Sees / doesn't see.** Sees the full authorization and provenance history (tokenized). Never
  clinical content.
- **Status.** Planned — not yet mocked up.

### 4. Patient (consent owner)

- **Role.** A patient managing their own authorization — granting and revoking access to their
  identity subgraph — typically from a patient-controlled **patient peer client**.
- **Context / view.** Patient-scoped: their own identity and authorization events only.
- **Key actions.** `AuthorizationGrant` and `AuthorizationRevocation`, signed by the patient's own
  key. Consent can originate from *any* peer, but the patient peer is the natural home for a
  patient to manage it. Revocations gossip like any event and are enforced locally by every
  responding peer (§4.6, §4.7).
- **Sees / doesn't see.** Sees who currently has access, active grants and their scope/expiry, and
  a revocation taking effect. Sees only tokenized identity + authorization events — never another
  party's clinical data.
- **Status.** Mockup built — [`patient-consent-mockup.html`](patient-consent-mockup.html); also
  represented in the federation view of
  [`reference-architecture.html`](reference-architecture.html).

---

## System contexts (views)

These gate what any persona's session may see. They are enforced below the UI.

- **Clinical view.** Test/synthetic data filtered out (§11.4.1). Backs clinician reads and any
  patient-facing clinical surface. Implemented as `clinical_view` in the conformance crate.
- **Operator view.** Everything, including synthetic events (flagged), for stewards, ops, and
  load testing. Implemented as `operator_view`.
- **Dual-control roles (enforcement, not UI).** The **Export Gate** evaluates authorization before
  any data leaves the source institution and emits a signed `ExportReceipt`; the **Verifier**
  independently re-checks authorization, identity continuity, and provenance on the relying side —
  offline, against its local copy (§4.5, §10.2, §10.3). Neither can unilaterally circumvent
  authorization.

---

## Mockups in this folder

| File | Persona / context |
|---|---|
| [`clinician-review-mockup.html`](clinician-review-mockup.html) | Clinician · clinical view |
| [`steward-console-mockup.html`](steward-console-mockup.html) | Identity steward · operator view |
| [`patient-consent-mockup.html`](patient-consent-mockup.html) | Patient · consent client |
| [`reference-architecture.html`](reference-architecture.html) | System map (incl. the patient peer) |

Planned: a compliance/audit reviewer surface.
