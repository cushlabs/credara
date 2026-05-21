# conformance — Conformance Suite + Synthetic Data (M9)

**Governing spec sections:** §11.4 (Integration Testing in Production), §11 (Operations).

The `creda-conformance` crate provides the synthetic data generator and the in-process
conformance suite. Synthetic events are tagged as test data (§11.4.1) via
`IdentityEventNode::create_test_data`, so they propagate and replicate like real events but are
filtered from clinical responses while remaining visible to operator-scoped queries.

## What's here

- `src/generator.rs` — the synthetic data generator (§11.4.2). Content is **deterministic from a
  seed** (same seed → same demographics and structure), with realistic demographics from small
  public-domain corpora (common surnames / given names / US places — facts, not copyrightable)
  and realistic event chains. Scenarios: `Simple` (one Assert), `Disagreement` (two institutions
  asserting conflicting demographics), `Authorized` (Assert + AuthorizationGrant to a fixed
  conformance requester + Attest). Scale is configurable from one patient to millions (the
  load-test path). Demographic values are synthetic `tok:`-prefixed stand-ins for real
  TEFCA-tokenized values.
- `src/filter.rs` — the clinical-vs-operator views: `clinical_view` excludes test-data events
  (§11.4.1); `operator_view` returns everything.
- `tests/conformance.rs` — drives generated data through the store and graph and asserts the
  system's contracts: provenance preservation, authorization + revocation enforcement,
  disagreement surfacing, data-category handling (demographics tokenized; clinical cause-of-death
  is a flag only, never the cause), test-data filtering, configurable scale, and seed determinism.

## Run

```sh
conformance/run.sh          # nextest if present, else cargo test
# or, as part of the whole workspace:
anchor creda                # rolled-up nextest summary across all crates
```

## Out of scope here (lives in the test bed, DQ-3)

The deployment / multi-peer parts of conformance require real peers and a network and live in
`testbed/` (kind/k3d + Compose): `helm install` conformance, gossip convergence, anti-entropy
repair, partition/rejoin, and the Bound-1 revocation-latency check (§4.7). They run once the
libp2p transport and in-daemon gRPC serve path are wired. Scenario definitions are shared via
`testbed/scenarios/` so the same setup drives both the in-process suite and the multi-peer bed.

**Assembled:** standard test frameworks; public-domain name/address corpora.
**Written:** the conformance harness, the synthetic generator, the test-data tagging/filtering.
