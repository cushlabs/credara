# creda-store (M2)

The storage layer behind a clean `Store` trait.

**Governing spec sections:** §5.2 (subgraph as query result), §7.3 (storage architecture),
Appendix C.1/C.3.

Will contain: the `Store` trait; a RocksDB-backed implementation; the secondary indexes from
§5.2.5 (demographic-token→entry-points, institution→events, event-UUID→node, parent→children);
index rebuild-on-startup.

**Assemble:** rust-rocksdb. **Scaffold:** a libgit2-backed `Store` impl behind the same trait —
`TODO(open-question-13.1)`, the storage-substrate trade study is unresolved.

Not yet registered as a Cargo workspace member; added in M2.
