# Creda — Context & Decision History for Cowork

**Purpose:** This file gives Cowork the background it needs to build out the Creda project faithfully. It captures *how the specification came to be* and *why decisions were made* — context that lives in the originating conversation but is not restated inside the spec itself. Read this alongside `creda-technical-spec.md` (the authoritative artifact). Where this file and the spec disagree on a technical detail, the spec wins; this file exists to explain intent, not to override the spec.

**Status as of handoff:** The technical specification is complete through Sections 1–13 plus appendices (~3,360 lines, ~81 pages rendered). No code has been written yet. The next step is repository creation and component build-out per the build guide.

---

## 1. What Creda Is (the short version)

Creda is a decentralized, peer-to-peer substrate for **cross-institutional patient identity provenance and portable authorization** in US healthcare. Institutions run peers that form a vetted-but-uncoordinated network. A directed acyclic graph (DAG) of signed events records two co-primary things: **who a patient is** (identity continuity across institutions) and **what they have authorized** (portable, revocable, verifiable-at-point-of-use authorization). The graph replicates asynchronously via gossip and anti-entropy. FHIR R4 is the integration surface. There is admission control (a vetted trust framework) but no runtime coordinator — once admitted, peers operate directly with each other.

The problem it solves: today's patient-matching ecosystem (institutional MPIs, QHIN-mediated exchange, vendor identity like Epic Care Everywhere) can move data but cannot provide cross-institutional identity with cryptographic provenance, nor persistent/revocable authorization, without a central authority or vendor lock-in. Creda fills that gap as complementary infrastructure — it does not replace MPIs or EHRs.

---

## 2. How This Specification Was Developed

The spec was built section by section in a working conversation, using a co-authoring approach: brainstorm candidate points for each section, the human curates (keep/cut/combine), then draft. Key process facts Cowork should understand:

- **The architecture emerged from a progression.** The conversation started from first principles ("what is a directed acyclic graph") and built up: DAG → distributed DAG → replicated near-real-time DAG across thousands of k8s instances → decentralized health data fabric → patient identity provenance → identity management replacing top-down MPI → the full Creda design. The spec is the endpoint of that reasoning, but the reasoning explains why certain choices (decentralization, append-forward, DAG-as-audit-trail) are load-bearing rather than incidental.

- **The spec was deliberately built on existing standards.** A recurring instruction was "don't reinvent the wheel." Appendix C of the spec ("Build vs. Buy") is the contract: assemble from libp2p, HAPI FHIR, SPIRE, cert-manager, RocksDB/libgit2, ciborium, blake3, the `uuid` and `pqcrypto` crates, libpostal, and TEFCA tokenization. Write only the healthcare-domain layer (~8,000–15,000 lines of genuinely new code). **If Cowork finds itself building a gossip protocol, DHT, FHIR server, or crypto primitive from scratch, that is a mistake — the spec says to assemble those.**

- **Portable Authorization and dual-control were added late, deliberately.** The spec originally treated authorization as a simple Consent/RevokeConsent event pair. It was later restructured so that **Portable Authorization is a co-primary primitive** alongside identity (now Section 4), with a **dual-control enforcement model** (Export Gate at the source, Verifier at the relying party — both new components in Section 10). This was a conscious upgrade, not a draft artifact. See Section 5 below for the decision record.

---

## 3. Decision Record (the "why" behind specific choices)

These are decisions the human made explicitly during development. Cowork should treat them as settled and not re-litigate them.

| Decision | Choice | Rationale / Notes |
|---|---|---|
| Project name | **Creda** | Latin "to believe/trust" — fits an identity-provenance system. |
| Primary audience of the spec | Engineering team building it | So the spec goes deep on architecture and skips the "why decentralization matters" pitch. |
| Core language | **Rust** for Core, Export Gate, Verifier | Performance + memory safety for the network/storage/crypto layer. |
| FHIR layer | **HAPI FHIR (Java/Kotlin)**, Plain Server mode | Maturity of HAPI; NOT JPA mode (no parallel relational store — the event store is the source of truth). |
| FHIR version | **R4** now; R5 deferred | R5 is open question 13.6.1, gated on US Core R5 adoption reaching majority. |
| Storage | `Store` trait; **libgit2** preferred, **RocksDB** alternate | libgit2-vs-RocksDB is open question 13.1 (Phase 0 trade study). Build RocksDB impl first (simplest), scaffold libgit2 behind the same trait. |
| Networking | **libp2p** (rust-libp2p) | gossipsub + Kademlia DHT + Noise transport. The single biggest "assemble, don't build." |
| Serialization | **Canonical CBOR** (ciborium) | Deterministic bytes for signature verification; rejected protobuf for non-deterministic map handling. |
| Hashing | **Blake3** | Speed + 128-bit post-quantum margin (meets NIST floor). Algorithm-agile so it can be upgraded. |
| Node IDs | **UUIDv7** | Time-ordered, primary key; chosen because tombstoning breaks content-addressing, so UUIDs (not content hashes) are the stable address. Content hash is an optional integrity check, voided after tombstone. |
| Signatures | **Algorithm-agile** (`CryptoSignature`) | Ed25519 default; ML-DSA-65 (FIPS 204) and SLH-DSA (FIPS 205) for PQC; hybrid mode for "harvest-now-decrypt-later" defense. Designed for decades of validity. PQC was an explicit requirement. |
| Confidence scoring | u16, 0–10000 (0.00–100.00%) | Avoids floating point in deterministic serialization. Per-field, not per-patient. |
| Topic gossip | **Bucketed**, 1,024 buckets | Reduces gossipsub topic cardinality vs. one-topic-per-patient; tunable. |
| Identity matching | **Out of scope** — left to institutions | Creda provides the provenance layer; institutions run their own matching (Verato/NextGate/etc.) and assert results as events. |
| Network admission | **Approval gate required** (NPA / BAA exchange) | Under HIPAA, peers exchanging PHI need BAA coverage. Vetted participation, peer-to-peer operations — explicitly modeled on DirectTrust. |
| Coordinator role | **Framework Steward / Legal Coordinator** | Administrative, not architectural — governs admission, does not mediate transactions or see PHI. Role is transferable across successors. |
| Patient Patient.id | **Random opaque UUID** at the peer; subgraph hash is a separate identifier slice | Conventional FHIR usage; aids provider-side caching. |
| Right to be forgotten | **Tombstone** scrubs PII content, preserves graph topology | Distinguished from authorization revocation. "Structural append-forward, content mutable by exception." Like `git filter-repo`, not `git rm`. |
| Authorization revocation vs. tombstone | **Distinct event types** | Deliberately NOT collapsed into one "Revocation" concept (this avoids a name-collision bug found in a sibling spec — see Section 6 below). |
| Default authorization posture | **Treatment-presumed (TPO)** recommended; deny-by-default available | Aligns with HIPAA TPO. Research/AI/federal scopes ALWAYS require explicit grant regardless of posture. |
| Break-the-glass | **Auditable but not preventable** | Patient safety supersedes consent in emergencies; fail-open with strict accountability. |
| Disambiguation (`$creda-disambiguate`) | **In v1**, with patient-direct `$creda-self-verify` preferred | Question-selection algorithm is open question 13.2.x. (Note: the sibling Fathom spec deferred this — Creda keeps it. See Section 6.) |
| Deployment | Helm chart primary; Docker Compose for laptop; Operator deferred | Must be deployable "with little to no oversight" on laptop, on-prem, or cloud. |
| Containers | **Distroless** for Core and Bridge | Minimal attack surface; trades debuggability for security. |
| License | **Apache 2.0** | Open source is a precondition (Section 12.2.2) for security review, standards-body acceptance, and avoiding lock-in. Compatible with HAPI FHIR's Apache 2.0. |
| Workflow orchestration | **k8s CronJobs** for simple tasks; **Argo Workflows** for multi-step pipelines | Argo is for operational tasks (bulk imports, anti-entropy sweeps), NOT for the patient-identity DAG. Conflating the two would be a category error. |

---

## 4. The Spec's Structure (so Cowork can navigate it)

The authoritative spec (`creda-technical-spec.md`) is organized as:

1. **Overview** — what Creda is, the problem, the architectural thesis.
2. **Design Principles** — system-level invariants (Verification-not-mediation, Decentralization, Provenance, Data Sovereignty, Standards-over-invention, Incremental Integration, Privacy-by-structure, Honest-about-tradeoffs, Operational Longevity).
3. **Identity Model** — principles, tenets, the seven identity event types, the trust/signature model.
4. **Portable Authorization** *(co-primary)* — authorization-as-verifiable-state, the three authorization event types, the Portable Authorization Artifact, dual-control enforcement, the seven-step evaluation algorithm, bounded revocation latency.
5. **Data Structures** — event node schema, PQC, payload schemas, subgraph computation, confidence model.
6. **Network Architecture** — peer identity, admission, gossip, DHT, anti-entropy (foundational/complementary/competitive/deferrable framing).
7. **Replication and Consistency** — eventual + causal consistency, conflict resolution, storage, tooling matrix (k8s-nativity ratings).
8. **FHIR Integration** — Patient mapping, the IG, custom operations (including the authorization ops and `$creda-disambiguate`), TEFCA/QHIN interop.
9. **Security and Access Control** — threat model, UDAP+SPIFFE, zero-trust insider-threat handling, DDOS, tokenization, authorization enforcement (security view), audit trail, future privacy (PSI/ZK).
10. **System Components** — Creda Core, **Export Gate**, **Verifier**, HAPI FHIR Bridge, Peer Daemon, container/k8s, Participant Registry service.
11. **Operations** — bootstrap, monitoring, failure modes, DR, integration testing with synthetic data.
12. **Migration and Adoption Path** — bottom-up phased adoption, critique of alternatives (NPI, federal MPI, blockchain, improved MPIs, vendor identity), international adaptation.
13. **Open Questions** — unresolved decisions with closure conditions (storage substrate, tokenization alignment, confidence calibration, disambiguation algorithm, pairwise identifiers, revocation bounds, Export Gate integration surface, Verifier stale-state policy, PQC finalization, etc.).
- **Appendix A** — Prior Art (stub). **Appendix B** — Glossary (stub). **Appendix C** — Build vs. Buy (the assemble-vs-write contract).

---

## 5. The Portable Authorization / Dual-Control Addition (decision record)

This was the most significant change made during development, so it gets its own section.

**What was added and why:** Authorization was promoted from a simple consent event to a co-primary primitive. The reasoning: identity provenance answers *who* a patient is, but cross-institutional exchange also needs to answer *what they authorized* — and crucially, to keep that answer verifiable after data has moved. Today, authorization is checked at the moment of transfer and then forgotten; if a patient revokes consent an hour later, downstream holders have no way to know. Portable Authorization makes the grant a signed, scoped, detachable artifact that travels with data references and is re-verifiable at any point of use without contacting the source.

**The dual-control model:** Enforcement happens at two independent points — the **Export Gate** (source-side: validates the authorization artifact before data leaves, emits an ExportReceipt) and the **Verifier** (relying-side: validates authorization + identity continuity + provenance integrity locally, including offline). Neither side can unilaterally circumvent authorization.

**Three new event types:** AuthorizationGrant (supersedes the old Consent), AuthorizationRevocation (withdraws a grant — kept DISTINCT from Tombstone), ExportReceipt (records release/receipt under a grant, creating chain of custody).

**Structural decision:** This was integrated as **co-primary** (the human's explicit choice), which required inserting Portable Authorization as Section 4 and renumbering everything below it (old Sections 4–12 became 5–13). The Export Gate and Verifier were added as **new components in Section 10** (System Components). All ~150 cross-references were updated accordingly.

---

## 6. Relationship to the "Fathom" Specification

During development, a separate spec called **Fathom** (131 pages, associated with an "ATTEST Program" / ARPA-H framing, implementer "Azorian Project," successor steward "HITEF") was compared against Creda. Cowork should understand this relationship:

- **Fathom is ~80% architecturally identical to Creda** — same DAG event model, same libp2p replication, same HAPI Bridge, same UDAP/SPIFFE credentials, same crypto choices, same confidence model, same design principles (verbatim in places). Strong evidence indicates Fathom was **derived from the Creda spec** (textual identity in the principles section, identical edge-case enumerations, "deferred from v1" / "replaced by" language indicating course corrections relative to a predecessor).

- **The two genuine architectural advances Fathom made were Portable Authorization and dual-control** — and those have now been **ported into Creda** (Section 5 above). 

- **Creda deliberately did NOT adopt some Fathom choices:**
  - Fathom has a **"Revocation Event" name-collision bug** (uses one event type for both authorization revocation and identity redaction). Creda keeps these as distinct event types. Do not reintroduce the collision.
  - Fathom **defers `$creda-disambiguate`** (its `$fathom-reconcile`); Creda keeps it in v1 with the question-selection algorithm flagged as an open question.
  - Fathom **rejected the globally-deterministic subject identifier** in favor of pairwise-scoped identifiers (for privacy/correlation reasons) but left that design unfinished. Creda currently uses a deterministic subgraph-identifier hash exposed as a FHIR identifier slice. This is a live divergence — see open question 13 / the diff analysis. If pairwise identifiers are pursued, treat it as a Phase 0 design item.

- **Fathom additions NOT yet ported to Creda** (available if wanted): ATTEST/ARPA-H program framing, EHI/PHI boundary classification appendix, Prior Authorization application profile, Patient Verification Wallet profile, Conformance Suite as a packaged deliverable, and a Risk Disposition Table. A full diff analysis exists (`creda-fathom-diff.md`) if deeper detail is needed.

**Practical implication for Cowork:** Build from the **Creda** spec. It now contains the best of both (Creda's well-developed operations sections + Fathom's authorization advances) and avoids Fathom's known defects.

---

## 7. What's Done vs. What's Next

**Done:**
- Full technical specification, Sections 1–13 + appendices.
- Portable Authorization + dual-control integrated as co-primary.
- Fathom comparison and diff analysis.

**Not yet done (the build):**
- No repository exists yet. **GitHub will be the source repository.**
- No code written.
- Appendices A (Prior Art) and B (Glossary) are stubs in the spec.
- A Cowork build guide was started but not completed in the originating conversation — if present in this package as `COWORK_BUILD_GUIDE.md`, follow it; if absent or partial, derive the build sequence from the dependency order: event model → storage → graph/computation → networking → Core → Export Gate/Verifier → FHIR Bridge → deployment/Helm → conformance suite.

**Open questions that must NOT be silently resolved** (scaffold the interface, mark `TODO(open-question-13.x)`, file an issue):
- 13.1 storage substrate (libgit2 vs RocksDB)
- 13.2 disambiguation question-selection algorithm
- pairwise vs. deterministic subject identifier
- 13.3 / Section 8.5 DHT query-privacy
- 13.4 revocation latency bounds 2 & 3, Export Gate integration patterns, Verifier stale-state policy

---

## 8. Build Principles Cowork Must Honor

1. **Spec-first.** Read the relevant spec section in full before generating any file. Cite the section in commit messages.
2. **Assemble, don't reinvent.** Appendix C is the contract. Only the healthcare-domain layer is new code.
3. **Open source from commit one** (Apache 2.0).
4. **Conformance-driven.** Every component ships with tests; "done" means tests pass.
5. **Incremental and verifiable.** Each milestone produces something runnable; no giant unreviewable commits.
6. **Honor open questions.** Don't pretend to resolve what the spec marks unresolved.
7. **Respect security boundaries.** Repository creation, visibility changes, and access-control settings are account-level actions — confirm with the human operator before making them. Never commit secrets, real PHI, or credentials. Use synthetic data only (Section 11 of the spec specifies a synthetic data generator and test-data tagging).

---

*This context file is a handoff artifact. The authoritative source of truth for all technical decisions is `creda-technical-spec.md`.*
