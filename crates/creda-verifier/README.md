# creda-verifier (M6)

Relying-side half of dual-control — validates locally, including offline.

**Governing spec sections:** §4.5 (Dual-Control Enforcement), §10.3 (Verifier).

Will contain: an SDK/runtime that validates authorization + identity continuity + provenance
integrity against a local read-only DAG replica; offline operation; stale-state reporting; language bindings.

**Assemble:** reuse `creda-graph`'s authorization evaluation. **Write:** the local replica wiring
and stale-state reporting. `TODO(open-question-13.4.3)` Verifier stale-state policy.

Not yet registered as a Cargo workspace member; added in M6.
