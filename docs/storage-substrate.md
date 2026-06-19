# Storage substrate decision — RocksDB (resolves open question §13.1.1)

**Status:** Decided. RocksDB is Credara Core's storage substrate. The libgit2 alternative is
retired; the scaffold (`crates/creda-store/src/git.rs`, the `libgit2` feature, the `git2`
dependency) has been removed. This document records the decision and the reasoning, so the choice
is auditable rather than implicit.

This resolves **§13.1.1** (libgit2 vs. RocksDB as the storage foundation). It does **not** touch
**§13.1.2** (the tombstone content-integrity governance review), which remains open — that is a
review with privacy counsel and auditors, not a substrate choice.

## The question

Credara Core stores a DAG of signed `IdentityEventNode`s, keyed by event UUID (UUIDv7), with the
four secondary indexes of §5.2.5, content-hashed with Blake3, and reconciled between peers by
anti-entropy. The open question was whether to build that on **libgit2** (using Git's data model
directly — Git is itself a signed DAG with parent references, so the appeal was storing the event
DAG natively, "one repo per institution, patient subgraphs as refs," and inheriting Git's hardening
and pack-protocol replication) or on **RocksDB** with our own Merkle-DAG primitives.

## Decision: RocksDB

RocksDB is the substrate. It is also already the complete, tested backend (`RocksdbStore`, one
column family per index); the libgit2 path never advanced past a stub whose `Store` methods all
returned `Unimplemented`. The decision is made on architecture, not micro-benchmarks, because the
libgit2 mismatches below are **structural** — they are not the kind of thing a performance bake-off
would reverse.

### Why not libgit2

1. **Right-to-be-forgotten fights Git's immutability — this is the disqualifier.** Git objects are
   content-addressed and immutable: you cannot scrub a blob in place, because changing its bytes
   changes its object id, and the original blob survives in packs until a history rewrite/repack.
   Credara's §3.4.6 tombstone requires *actual* content destruction. On RocksDB that is a point
   overwrite — exactly what the engine does today: replace the target event with its husk via
   `put_event` and rebuild the demographic-token index so the value is unfindable. On libgit2 the
   same operation becomes a filter/repack of the object database. A storage layer whose core data
   model resists the one legally mandatory destructive operation is the wrong foundation for PHI.

2. **The "subgraphs as refs" mapping hits Git's known scaling wall, and libgit2 lacks the fix.**
   Millions of patient subgraphs means millions of refs. Git's loose-ref (a file per ref) and
   `packed-refs` (one rewritten/scanned file) formats do not scale to that; the ecosystem's answer
   is the **reftable** backend (in Git core since 2.45). As of June 2026, **libgit2 still does not
   implement reftable**. So the one mitigation for the ref-explosion problem is precisely what the
   libgit2 backend cannot offer. Avoiding the mapping (subgraph heads tracked in a side index
   instead of refs) is possible — but then Git is just a dumb object store and most of the
   "native DAG for free" rationale is gone.

3. **Write amplification.** Millions of patients times several events each is tens to hundreds of
   millions of small immutable objects. Until `git gc`, each is a loose file (256-way fan-out, slow
   past a few hundred thousand); after, you repack constantly, and delta compression buys little on
   already-compact CBOR + signatures. Per-object fsync and per-ref lock contention are the opposite
   of RocksDB's batched WAL/LSM, and this is a write-heavy workload.

4. **Anti-entropy is not actually reusable.** Git's pack protocol negotiates by *reachability*;
   Credara's anti-entropy is a deliberately content-agnostic Merkle root over the *sorted set of
   event UUIDs* (so that tombstoning never makes two peers diverge). Reachability-based transfer
   would also tend to move or resurrect tombstoned objects. The privacy-preserving reconciliation
   would have to be rebuilt on top of Git rather than inherited from it.

5. **The "free" wins carry a tax.** Credara addresses by UUIDv7 and content-hashes with Blake3;
   Git addresses by its own object id. Storing "natively" still requires a UUID→OID translation
   index, so the secondary-index machinery is not shed — only duplicated.

### Why RocksDB fits

The workload is a high write rate of small, independent, signed records with custom secondary
indexes, content addressing that is not Git's, anti-entropy that is content-agnostic by design, and
a mandatory in-place destructive operation for right-to-be-forgotten. That is an embedded
ordered-KV profile: point overwrite and delete, column families per index, prefix scans, batched
durable writes, tunable compaction. RocksDB matches it directly, and the `Store` trait already
isolates it (§7.4.1) so a future substrate change would touch no other crate — the decision is not
a one-way door, even though libgit2 specifically is retired.

## Amendment to the §13.1.1 closure condition

The spec's original closure asked for two working prototypes benchmarked on lines of code,
throughput, and recovery characteristics. We are instead closing on architectural grounds: the
immutability-vs-mandatory-scrub conflict (1) and the missing reftable backend in libgit2 (2) are
disqualifying independent of any throughput measurement, and RocksDB is already implemented and
passing. Should a future substrate question reopen (e.g., a different embedded engine), it would be
evaluated against this same record behind the unchanged `Store` trait.
