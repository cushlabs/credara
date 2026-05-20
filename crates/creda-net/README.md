# creda-net (M4)

Networking and replication — the single biggest "assemble, don't build".

**Governing spec sections:** §6 (Network Architecture), §7 (Replication and Consistency).

Will contain: the libp2p stack (gossipsub with bucketed topics, Kademlia DHT, Noise transport);
the gossip event-propagation path; anti-entropy with Merkle-root-over-UUID-set comparison;
snapshot generation/bootstrap; the `NetworkTransport` trait.

**Assemble:** rust-libp2p (gossipsub, Kademlia, Noise, NAT traversal). **Write:** the 1,024-topic
bucketing scheme, anti-entropy reconciliation, snapshot format, the trait boundary.

> DHT query-privacy is `TODO(open-question-13.3)` / §8.5 — scaffold the DHT, mark the privacy gap.

Not yet registered as a Cargo workspace member; added in M4.
