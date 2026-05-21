# creda-verifier (M6)

Relying-side half of dual-control — validates locally, including offline.

**Governing spec sections:** §4.5 (Dual-Control Enforcement), §10.3 (Verifier).

Will contain: an SDK/runtime that validates authorization + identity continuity + provenance
integrity against a local read-only DAG replica; offline operation; stale-state reporting; language bindings.

**Assemble:** reuse `creda-graph`'s authorization evaluation. **Write:** the local replica wiring
and stale-state reporting. `TODO(open-question-13.4.3)` Verifier stale-state policy.

## Status: implemented (M6), tests pending local run

`Verifier::verify` runs the three-part point-of-use check (§10.3.2) against a local `Store`
replica: **authorization validity** (reuses `creda_graph::evaluate`; the governing Grant must
cover), **identity continuity** (the Grant is present/bound in the patient's subgraph), and
**provenance integrity** (no missing parents). It is **offline by construction** — it only reads
the local store, never the source — and reports staleness (and the DAG view's age) against a
configurable threshold (`TODO(open-question-13.4.3)`); staleness is advisory, so `is_valid()`
covers only the substantive checks. Tests: valid→verifies, revoked→denied, missing-parent→
provenance broken, day-old view→stale (but still valid). Workspace member; verify with
`anchor creda` or `cargo test -p creda-verifier`. The language bindings and the
replication-fabric wiring of the read-only replica (§10.3.4) are follow-ups.
