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

## Status: implemented and verified (M3) ✓

Registered as a workspace member; suite passes. Reads from a `creda_store::Store` (depends on
`creda-store` with `default-features = false`, so the graph crate **never compiles RocksDB** —
its tests use the `MemoryStore` and stay fast: `cargo test -p creda-graph` needs no C/RocksDB
build). Re-run with `make test` or `cargo test -p creda-graph`.

### Modules
- `subgraph.rs` — `Subgraph::materialize` (transitive closure from entry points), roots, leaves,
  within-set children.
- `validation.rs` — the graph-dependent invariants deferred from M1: Contest party-of-subgraph
  (§3.4.3) and Amend originating-institution (§3.4.5).
- `identity.rs` — `project()` → `EffectiveIdentity`: per-field aggregation respecting Amend
  (supersede), Contest (sever a valid-contested Link), Tombstone (no demographics), with
  disagreement flagging (§5.3.4).
- `confidence.rs` — `ConfidenceConfig` + `score()`: method weight × institutional credibility ×
  temporal decay, combined as independence-aware additive evidence (F-S principle) squashed by
  `10000·T/(T+K)` for diminishing returns. **Calibration constants are an open question.**
- `authorization.rs` — `evaluate()`: the seven-step algorithm (§4.6). Step 6 redistribution is
  the per-event `responder_may_serve()` helper.

### Deliberate boundaries / open items
- Audience class/wildcard membership and Grant-volume utilization are **inputs** here (the
  Participant Registry and per-Grant counters live in Core, M5), keeping evaluation pure.
- Revocation "validated" = parent references resolved locally; **signature verification is done
  at ingest**, not re-done here.
- Identifier bags (`mrns`, `insurance_member_ids`) are not yet folded into the per-field
  disagreement model (they are sets of valid ids, not competing values) — documented follow-up.
- Confidence calibration: `TODO(open-question-confidence-calibration)`.
