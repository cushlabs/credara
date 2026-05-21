# creda-core (M5)

The runnable peer daemon — composes M1–M4.

**Governing spec section:** §10.1 (Creda Core).

Will contain: the binary that wires creda-events + creda-store + creda-graph + creda-net into a
peer; the gRPC API (CreateEvent, GetEvent, GetSubgraph, GetEffectiveIdentity, MatchByTokens,
EvaluateAuthorization, Subscribe, GetMetrics, plus scaffolded disambiguation RPCs); CLI mode
(`creda init`, `creda snapshot`, …); the tokio runtime; hierarchical config (TOML + env + flags).

**Assemble:** tonic (gRPC), tokio. **Write:** the composition, gRPC service definitions, CLI, config.

## Status: implemented (M5 engine/config/CLI verified-ready; gRPC + libp2p opt-in)

Completes the M1→M5 spine. Same isolation discipline as the rest of the workspace.

### Default build (verifiable, no heavy/unverifiable deps)
- `engine.rs` — `CredaCore`: the synchronous composition of store + graph. Implements
  CreateEvent, GetEvent, GetSubgraph, GetEffectiveIdentity (§5.2.4), MatchByTokens (§5.2.5),
  EvaluateAuthorization (§4.6), snapshot/load (§6.2.5). Unit-tested with `MemoryStore` — no gRPC,
  no network, no RocksDB.
- `config.rs` — hierarchical config: defaults → TOML → env (`CREDA_*`) → CLI flags, validated at
  startup, fail-loud (§10.1.6).
- `signer.rs` — the `Signer` abstraction + in-memory implementation (§10.1.4).
- `src/main.rs` — the `creda` binary: `init`, `snapshot`, `inspect` (and `serve`, which needs the
  `grpc` feature).

### Opt-in adapters (heavy / write-blind-risky, behind features)
- `grpc` — tonic server (`proto/creda.proto` + `grpc.rs`) wrapping the engine (§10.1.3).
  Needs `protoc`; `build.rs` runs codegen only with this feature. Documented scaffold;
  version-sensitive spots marked `TODO(grpc-verify)` (notably the Unix-socket serve wiring and
  the `EvaluateAuthorization` reply shape).
- `libp2p` — turns on `creda-net`'s libp2p transport so it builds from Core (§10.1.5). Wiring
  replication into the engine + the multi-peer harness lands in `testbed/` (DQ-3).

### Verify
`cargo test -p creda-core` (or `anchor creda`) exercises the engine, config, and signer — the
default build pulls neither tonic nor libp2p. Build the gRPC server with
`cargo build -p creda-core --features grpc` (requires protoc; provisioned in the dev image).
Sixth and final spine workspace member.
