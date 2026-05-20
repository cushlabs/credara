# creda-graph (M3)

Graph traversal and computation over the event DAG.

**Governing spec sections:** §5.2.4 (Effective Identity Computation), §4.6 (Authorization
Evaluation Algorithm), §5.3 (Confidence and Trust Metadata).

Will contain: subgraph materialization (transitive closure from entry points); root discovery;
fork/split semantics; the effective-identity projection (respecting Amend/Contest/Tombstone);
the seven-step authorization evaluation algorithm; the Confidence Signals engine (per-field,
Fellegi-Sunter, with verification-method weight, institutional credibility, reliance/agreement
amplification, temporal decay).

**Assemble:** the Fellegi-Sunter record-linkage math (port from published references — do not
invent). **Write:** traversal, projection, authorization evaluation, per-field confidence.

Not yet registered as a Cargo workspace member; added in M3.
