# creda-events (M1)

The event model — the heart of Creda. Pure data + cryptography, no network or storage.

**Governing spec sections:** §3 (Identity Model), §4 (Portable Authorization), §5 (Data Structures).

Will contain: the event node schema; the `IdentityEventType` enum (Assert, Link, Contest,
Attest, Amend, Tombstone, DeceasedDeclaration, AuthorizationGrant, AuthorizationRevocation,
ExportReceipt); the per-type `EventPayload` tagged union; the `Demographics` struct; canonical
CBOR serialization (ciborium); Blake3 content hashing; UUIDv7 generation; and the
algorithm-agile `CryptoSignature` (Ed25519 + ML-DSA-65 + SLH-DSA + hybrid).

**Assemble:** ciborium, blake3, uuid (v7), ed25519-dalek, pqcrypto-mldsa, pqcrypto-sphincsplus.
**Write:** the event schema, the enum, payload validation, per-event-type invariants.

> AuthorizationRevocation is a DISTINCT event type from Tombstone — never collapse them.

## Status: implemented and verified (M1) ✓

Source and tests are complete, registered as a workspace member, and **passing** (verified
locally with no source changes needed; exact dependency versions pinned in the workspace
`Cargo.lock`). Re-run anytime:

```sh
make test          # full suite incl. PQC (Docker-only; see docs/DEVELOPMENT.md)
make test-fast     # Ed25519-only path
# or natively, if you maintain your own toolchain:
cargo test -p creda-events
```

The PQC algorithms are behind the default-on `pqc` feature, isolated in `src/crypto/pqc.rs`.
Verified-good versions (pinned in `Cargo.lock`): `pqcrypto-mldsa 0.1.2`,
`pqcrypto-sphincsplus 0.7.2`, `ed25519-dalek 2.2.0`, `ciborium 0.2.2`.

### Module map

- `event.rs` — `IdentityEventNode`, `IdentityEventType` (10 types), builder/sign/verify, the
  in-isolation structural invariants, `RedistributionPolicy`.
- `payload.rs` — `EventPayload` tagged union and the per-type enums/structs.
- `demographics.rs` — `Demographics` and tokenized field types.
- `crypto.rs` (+ `crypto/pqc.rs`) — algorithm-agile signatures (Ed25519 / ML-DSA-65 /
  SLH-DSA-256s / hybrid).
- `canonical.rs` — RFC 8949 deterministic CBOR encoding.
- `hash.rs` — Blake3 content hash (agility-ready, voidable on tombstone).
- `ids.rs` — UUIDv7 generation and the certificate fingerprint type.

### Graph-dependent invariants deferred to M3

The `Contest` party-of-the-subgraph rule (§3.4.3) and the `Amend` originating-institution rule
(§3.4.5) need traversal context and are enforced in `creda-graph` (M3), not here.
