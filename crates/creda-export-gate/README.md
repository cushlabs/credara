# creda-export-gate (M6)

Source-side half of dual-control authorization enforcement.

**Governing spec sections:** §4.5 (Dual-Control Enforcement), §10.2 (Export Gate).

Will contain: validation of a Portable Authorization Artifact before data egress; emission of an
ExportReceipt (chain of custody); the egress-hook integration points.

**Assemble:** reuse `creda-graph`'s authorization evaluation — do NOT reimplement it.
**Write:** the egress-hook integration surface. `TODO(open-question-13.4)` Export Gate integration patterns.

## Status: implemented (M6), tests pending local run

`ExportGate::authorize_export` materializes the local subgraph, reuses `creda_graph::evaluate`
under a **deny-by-default** posture (egress needs an explicit covering artifact, §10.2.2), and on
success emits a signed `ExportReceipt` (governing grant, requester, released scope). Tests:
permit valid (emits receipt), refuse expired / revoked / audience-mismatch / no-grant. Workspace
member; verify with `anchor creda` or `cargo test -p creda-export-gate` (uses MemoryStore, no
RocksDB/network). The egress-hook integration surface (FHIR interceptor / sidecar, §10.2.3) is a
follow-up; the validate-and-emit core is here. The Bound-1 revocation-latency timing check (§4.7)
lives in the test bed (DQ-3, needs a network).
