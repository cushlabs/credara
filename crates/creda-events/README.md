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

Not yet registered as a Cargo workspace member; added in M1.
