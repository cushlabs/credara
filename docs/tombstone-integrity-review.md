# Tombstone content-integrity review (§13.1.2)

**Status:** Review package — the open item is governance sign-off, not engineering. The tombstone
mechanism is implemented and stable (§3.4.6). This question (§13.1.2) closes when three groups have
reviewed the tradeoff below: (a) privacy counsel (HIPAA / GDPR / state right-to-be-forgotten),
(b) institutional security architects from founding institutions, and (c) the HL7 Security work
group during IG ballot. This document is the package for that review and records the recommended
posture. **It is not legal advice; the legal determinations are the reviewers' to make.**

## 1. The question

Tombstoning destroys an event's content. Because the event's signature was computed over that
content, the signature no longer verifies once the content is scrubbed, and the content hash is
voided. So an auditor can no longer **cryptographically verify what a tombstoned event originally
contained**. Is that acceptable, and should the system additionally retain a *non-PHI* attestation
of the destroyed content so that "what was tombstoned" stays verifiable?

This is narrow on purpose. It is not "is tombstoning safe" in general — it is specifically the
content-attestation gap.

## 2. What the system does today (the facts under review)

- **Content is actually destroyed (§3.4.6).** A tombstone reduces its target events to husks: the
  demographic payload is stripped to empty, the content hash is voided (`verify_content_hash`
  returns "no hash," never a mismatch), and the demographic-token index is rebuilt so the value is
  no longer findable by token. The structural envelope (id, type, parents, timestamps, institution)
  remains. The scrub is enforced on create and on ingest, is idempotent against a re-received
  original, and is re-applied on boot — and in the decentralized model each peer scrubs its own
  copy. The PHI is gone, not hidden.

- **The scrub *action* is fully and immutably attested.** The `Tombstone` is itself a signed event
  recording **who** (institution certificate fingerprint), **when** (timestamp), **why**
  (`legal_basis` ∈ right-to-be-forgotten | state-law | court-order | other), and **what**
  (the target event ids). That record is tamper-evident and is retained permanently.

- **The graph shape stays auditable.** Anti-entropy is a Merkle root over the *sorted set of event
  UUIDs* — deliberately content-agnostic — so tombstoning never diverges two peers, and the
  existence, type, and lineage of every node (including husks) remain verifiable.

- **What is lost** is exactly one thing: cryptographic verification of the destroyed *content*.
  You can prove a node existed, was tombstoned, by whom, when, and why — but you can no longer
  prove "node X originally contained demographic set Y."

## 3. What is already settled (please do not relitigate)

- **Destruction is mandatory.** Right-to-be-forgotten under GDPR Art. 17, the HIPAA
  restriction/amendment workflows, and state right-to-be-forgotten laws require the PHI itself to be
  destroyed. A tombstone that merely concealed content would not satisfy them. Content destruction
  is therefore not a tunable.
- **The action-audit is complete.** Who/when/why/what is signed and retained (§2 above).

So the only open question is the narrow one in §1: whether to *additionally* retain a non-PHI
content attestation, and if so, how — given that a digest of low-entropy demographics can itself be
a re-identification vector.

## 4. Options for the content-attestation gap

| | What is retained | Restores content verifiability? | Re-identification surface |
|---|---|---|---|
| **A — nothing extra** (current default) | Only the signed action-record (§2) | No | None. Cleanest RTBF posture. |
| **B — bare content hash** | The original Blake3 content hash, recorded in the signed `Tombstone` | Yes — verify a lawfully-produced original against the hash | **Confirmation oracle.** Demographic payloads are low-entropy (a DOB is ~tens of thousands of values; sex is a few). Anyone who can recompute candidate tokens can brute-force the hash to *confirm* a guessed identity. Safe only if the token salt is secret and unrecoverable — which is exactly what an attacker with store access may not lack. |
| **C — keyed HMAC** | An HMAC of the original content under an institution-held key, recorded in the `Tombstone` | Yes — same as B, for holders of the key | Not brute-forceable without the key. Destroying the key collapses C to A (and can be a documented retention-expiry control). Cost: key management; verification needs the key holder's cooperation. |

A digest derived from PHI may itself be treated as retained personal data under some
interpretations (esp. GDPR) — a question for privacy counsel, and a reason the default should not
silently retain one.

## 5. Recommended posture (for the reviewers to ratify or amend)

1. **Keep the signed action-attestation** (who/when/why/what). Already implemented; it carries most
   of the audit value and creates no re-identification surface.
2. **Default to Option A** — retain no content digest. This is the cleanest legal posture and has
   zero re-identification surface; it is what ships today.
3. **Offer Option C (HMAC), off by default, per-deployment, reviewer-approved**, for institutions
   whose auditors require content-level attestation. Prefer HMAC over a bare hash (B) precisely to
   defeat the confirmation-oracle risk, with the key institution-held and destroyable. Do **not**
   offer Option B as a default; if an institution insists on a bare hash, that is an explicit,
   counsel-approved local choice.

The intent is that the conservative posture is the default and the audit-preserving option is an
opt-in an institution turns on only with its own privacy counsel's approval.

## 6. Questions for each reviewer group

**Privacy counsel (HIPAA / GDPR / state RTBF).**
- Does destroying the content while retaining the signed *action*-record satisfy the destruction
  requirement under each applicable regime?
- Would retaining a content **hash** (Option B) or **HMAC** (Option C) count as retained personal
  data, and under what conditions is it permissible?
- Is the confirmation-oracle risk on low-entropy demographics acceptable given the token-salt model,
  or does it argue for HMAC-only (C) or nothing (A)?

**Institutional security architects.**
- Is the loss of content-signature verifiability on tombstoned nodes acceptable for your audit,
  incident-response, and forensics needs?
- Does the signed action-attestation suffice, or do your auditors require content attestation
  (Option C)? If so, who holds and rotates the HMAC key?

**HL7 Security work group (IG ballot).**
- Does the husk model (destroy content, void the content hash, retain a signed action-record) meet
  the IG's integrity expectations for a conformant implementation?
- Should the IG specify the content-attestation option and its safeguards, or leave it deployment-local?

## 7. Engineering readiness

Options B/C are a small, isolated change: the `Tombstone` payload optionally carries a
`target_content_attestation` (hash or HMAC) captured at scrub time; the husk path is unchanged, and
nothing about content destruction changes. This is **deliberately not built ahead of the review**,
because the choice among A/B/C — and especially whether to retain any PHI-derived digest at all — is
the reviewers' call, not an engineering default. On a decision, implementation is straightforward.

## 8. Decision record

| Reviewer group | Disposition | Date | Notes |
|---|---|---|---|
| Privacy counsel | _pending_ | | |
| Institutional security architects | _pending_ | | |
| HL7 Security WG (IG ballot) | _pending_ | | |

§13.1.2 remains **open** until these are recorded. Closing it should update spec §13.1.2 with the
ratified posture and note any required adjustments here.
