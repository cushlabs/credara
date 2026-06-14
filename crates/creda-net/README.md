# creda-net (M4)

Networking and replication — the single biggest "assemble, don't build".

**Governing spec sections:** §6 (Network Architecture), §7 (Replication and Consistency).

Will contain: the libp2p stack (gossipsub with bucketed topics, Kademlia DHT, Noise transport);
the gossip event-propagation path; anti-entropy with Merkle-root-over-UUID-set comparison;
snapshot generation/bootstrap; the `NetworkTransport` trait.

**Assemble:** rust-libp2p (gossipsub, Kademlia, Noise, NAT traversal). **Write:** the 1,024-topic
bucketing scheme, anti-entropy reconciliation, snapshot format, the trait boundary.

> DHT query-privacy is `TODO(open-question-13.3)` / §8.5 — scaffold the DHT, mark the privacy gap.

## Status: M4 pure logic verified green ✓ (libp2p adapter is an opt-in scaffold)

### The approach: assemble, but quarantine the assembled part

libp2p is the networking layer (spec §6.2.1/§6.3.1) — we do **not** reimplement gossip, DHT, or
encryption. But libp2p is large and its API moves between versions, so this crate is split so
that libp2p can never destabilize the rest of the workspace:

- **Genuinely-new, libp2p-free logic — the default build, fully unit-tested without a network:**
  - `bucketing.rs` — DHT key derivation (§6.1.6) and the `Blake3(dht_key) mod 1024` topic-bucket
    scheme (§6.2.4).
  - `antientropy.rs` — Merkle root over the **UUID set** (not contents, so tombstoning doesn't
    diverge roots) and the reconciliation delta (§6.1.8).
  - `snapshot.rs` — the snapshot format: sorted canonical-CBOR events + manifest with a Blake3
    integrity hash and event count (§6.2.5/§6.3.2); Store load/unload.
  - `gossip.rs` — the batch envelope, batching policy, and bounded dedup (§6.1.4/§6.2.2).
  - `transport.rs` — the `NetworkTransport` trait, the boundary that lets libp2p be replaced
    (§6.3.1).
- **The assembled part — `libp2p_adapter.rs`, behind the OFF-BY-DEFAULT `libp2p` feature:** a
  thin `NetworkTransport` over a libp2p `Swarm`. This is the single isolation point for libp2p
  version churn; it compiles + clippy-cleanly against the pinned libp2p (0.56), and CI's
  `libp2p-adapter` job builds/lints it on every push, so a version bump that breaks the API fails
  there. The version-sensitive spots carry a `libp2p 0.56` note (where to re-check on a bump).

### Why off by default

`cargo build` / `make test` (default features) never compile libp2p, so the workspace stays
green and fast regardless of rust-libp2p API changes. Enable the adapter deliberately:

```sh
cargo build -p creda-net --features libp2p   # heavy compile; verified against libp2p 0.56
```

Core (M5) and the multi-peer test bed (DQ-3) turn the feature on. The full multi-peer
convergence / partition tests (the M4 "Done when") live in the test bed, since they need real
peers and Core to drive them.

Verify the pure logic now with `make test` or `cargo test -p creda-net` (no libp2p, no network).
Fifth workspace member.
