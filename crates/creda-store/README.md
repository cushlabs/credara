# creda-store (M2)

The storage layer behind a clean `Store` trait.

**Governing spec sections:** §5.2 (subgraph as query result), §7.3 (storage architecture),
Appendix C.1/C.3.

Will contain: the `Store` trait; a RocksDB-backed implementation; the secondary indexes from
§5.2.5 (demographic-token→entry-points, institution→events, event-UUID→node, parent→children);
index rebuild-on-startup.

**Assemble:** rust-rocksdb. The storage-substrate question (§13.1.1) is **resolved — RocksDB**
(see `docs/storage-substrate.md`); the libgit2 alternative was retired without going past a scaffold.

## Status: implemented and verified (M2) ✓

Registered as a workspace member; full suite (incl. the RocksDB backend) passes. Re-run with
`make test` (Docker-only) or `cargo test -p creda-store`. RocksDB compiles from source and is
memory-hungry — on a constrained Docker VM use `make test JOBS=1` or raise Docker's memory
(see `docs/DEVELOPMENT.md`). Verified versions: `rocksdb 0.22.0` (`librocksdb-sys 0.16.0+8.10.0`).

### Backends
- `MemoryStore` — always available; in-memory, for tests and for downstream crates that don't
  want the RocksDB compile (depend with `default-features = false`).
- `RocksdbStore` (feature `rocksdb`, **default**) — embedded, one column family per index;
  composite-key prefix scans (no read-modify-write of serialized sets).

### The four secondary indexes (§5.2.5)
1. demographic token → entry points (`entry_points_by_token`)
2. institution → events (`events_by_institution`)
3. event UUID → node (`get_event`, primary)
4. parent → children (`children_of`)

`rebuild_indexes` reconstructs indexes 1, 2, 4 from the primary event store (bootstrap /
corruption recovery). Index keys for institution and token are hashed to a fixed 32-byte
prefix so prefix scans are length-safe; the trailing 16 bytes are always the event UUID.

### Build note
RocksDB's `librocksdb-sys` compiles RocksDB from source and runs bindgen, so the build needs
a C++ compiler + libclang — already provisioned in the dev container (`.devcontainer/Dockerfile`).

### Module map
`store.rs` (trait) · `memory.rs` · `rocks.rs` · `tokens.rs`
(demographic token extraction) · `error.rs`.
