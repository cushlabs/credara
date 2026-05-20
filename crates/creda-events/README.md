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

## Status: implemented (M1), tests pending local run

Source and tests are complete and registered as a workspace member. They were authored in an
environment without a Rust toolchain (see `docs/DESIGN_QUEUE.md` build notes / project memory),
so they have **not yet been compiled or run here**. Verify locally:

```sh
cargo test -p creda-events                    # default (includes PQC: ML-DSA-65, SLH-DSA, hybrid)
cargo test -p creda-events --no-default-features   # Ed25519-only fast path (no pqcrypto)
cargo fmt -p creda-events -- --check
cargo clippy -p creda-events --all-targets -- -D warnings
```

The PQC algorithms are behind the default-on `pqc` feature. The single most likely first-build
adjustment is a pqcrypto crate/module name or version — all such interaction is isolated in
`src/crypto/pqc.rs` (notably the SLH-DSA module path `sphincssha2256ssimple`).

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
