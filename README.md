# Creda

**Creda** is a decentralized, peer-to-peer substrate for **cross-institutional patient
identity provenance** and **portable authorization** in US healthcare.

Institutions run peers that form a vetted-but-uncoordinated network. A directed acyclic
graph (DAG) of signed events records two co-primary things: **who a patient is** —
identity continuity across institutions — and **what they have authorized** — portable,
revocable, verifiable-at-point-of-use authorization. The graph replicates asynchronously
via gossip and anti-entropy. **FHIR R4** is the integration surface. There is admission
control (a vetted trust framework, modeled on DirectTrust) but **no runtime coordinator**:
once admitted, peers operate directly with one another.

Creda is complementary infrastructure. It does **not** replace institutional Master
Patient Indexes (MPIs), EHRs, or QHIN-mediated exchange. It fills a gap those systems
leave open: cross-institutional identity with cryptographic provenance, plus persistent,
revocable authorization that stays verifiable after data has moved — without a central
authority or vendor lock-in.

> The name *Creda* derives from the Latin for "to believe / to trust" — fitting for an
> identity-provenance system.

## Status

> **Pre-launch — scaffolding.** The [technical specification](docs/creda-technical-spec.md)
> (Sections 1–13 + appendices, ~81 pages) is complete and authoritative. Component code
> is being built out milestone by milestone per the
> [build guide](docs/COWORK_BUILD_GUIDE.md). This repository currently contains the M0
> foundation: licensing, documentation, directory skeleton, and CI scaffolding.

<!-- Build-status badges go here once CI is wired to the remote. -->

## Architectural thesis

- **Verification, not mediation.** Creda verifies identity and authorization claims; it
  does not sit in the data path or broker transactions. There is no central node that
  sees PHI.
- **Provenance by structure.** Every assertion is a signed event with parent references.
  The DAG *is* the audit trail — tamper-evident by construction.
- **Append-forward, content-mutable by exception.** History is structurally append-only;
  the right to be forgotten is honored by *tombstoning* (scrubbing PII content while
  preserving graph topology), distinct from authorization revocation.
- **Portable authorization is co-primary.** A grant is a signed, scoped, detachable
  artifact that travels with data references and is re-verifiable at any point of use —
  enforced by **dual control**: an Export Gate at the source and a Verifier at the
  relying party, neither able to unilaterally circumvent authorization.
- **Standards over invention.** The system is assembled from mature components (libp2p,
  HAPI FHIR, RocksDB/libgit2, the `pqcrypto` family, SPIRE, cert-manager, TEFCA
  tokenization). Only the healthcare-domain layer is new code. See spec **Appendix C**.

## Technology at a glance

| Layer | Choice |
|---|---|
| Core / Export Gate / Verifier | **Rust** |
| FHIR Bridge | **Java/Kotlin** — HAPI FHIR, Plain Server mode (not JPA) |
| FHIR version | **R4** (R5 deferred — open question 13.6.1) |
| Networking | **libp2p** — gossipsub, Kademlia DHT, Noise transport |
| Storage | `Store` trait — **RocksDB** impl first, **libgit2** scaffolded (open question 13.1) |
| Serialization | **Canonical CBOR** (ciborium, RFC 8949 deterministic encoding) |
| Hashing | **Blake3** |
| Node IDs | **UUIDv7** |
| Signatures | **Algorithm-agile** — Ed25519 default; ML-DSA-65 (FIPS 204) and SLH-DSA (FIPS 205) for PQC; hybrid mode |
| Identity | **UDAP** (institutional) + **SPIFFE/SPIRE** (workload), cert-manager rotation |
| Deployment | **Helm** chart primary; **Docker Compose** for laptop; Operator deferred |
| License | **Apache 2.0** |

## Repository layout

See [`REPO_STRUCTURE.md`](REPO_STRUCTURE.md) for the full map. In brief: the Rust
workspace lives in `crates/`, the FHIR Bridge in `bridge/`, deployment artifacts in
`deploy/`, the conformance suite and synthetic-data generator in `conformance/`, and all
specification documents in `docs/`.

## Build milestones

The build proceeds in strict dependency order (full detail in
[`docs/COWORK_BUILD_GUIDE.md`](docs/COWORK_BUILD_GUIDE.md)):

| Milestone | Component | Spec sections |
|---|---|---|
| M0 | Repo init + CI | §12.2.2 |
| M1 | Event model (`creda-events`) | §3, §4, §5 |
| M2 | Storage (`creda-store`) | §5.2, §7.3, App. C |
| M3 | Graph / computation (`creda-graph`) | §5.2.4, §4.6, §5.3 |
| M4 | Networking (`creda-net`) | §6, §7 |
| M5 | Creda Core (`creda-core`) | §10.1 |
| M6 | Export Gate + Verifier | §4.5, §10.2, §10.3 |
| M7 | FHIR Bridge (`bridge/`) | §8, §10.4 |
| M8 | Deployment (`deploy/`) | §10.5, §10.6, §11 |
| M9 | Conformance + synthetic data (`conformance/`) | §11.4 |

## Building (once components land)

The Rust workspace builds with a standard toolchain:

```sh
cargo build --workspace
cargo test  --workspace
```

The FHIR Bridge (from M7) builds via Gradle under `bridge/`. Local multi-peer
development uses Docker Compose under `deploy/compose/` (from M8). At M0 the workspace
is intentionally empty, so these commands succeed trivially.

## Security and data handling

This is healthcare infrastructure. **Never commit secrets, credentials, or real PHI** —
all testing uses the synthetic data generator (M9) with `test-data` tagging so synthetic
events are provably invisible to clinical FHIR queries. Vulnerability reports route to a
private channel; see [`SECURITY.md`](SECURITY.md). The security model (UDAP + SPIFFE dual
credential, mandatory signature verification on replication, authorization enforcement at
the responding peer, and dual-control) is load-bearing — see spec §9.

## Contributing

Contributions are welcome under the spec-first, conformance-driven model described in
[`CONTRIBUTING.md`](CONTRIBUTING.md). Read the relevant specification section before
writing code for a component, and cite it in your commits.

## License

Licensed under the [Apache License 2.0](LICENSE). Open source is a precondition of the
design (spec §12.2.2): it enables independent security review, standards-body acceptance,
and freedom from vendor lock-in.
