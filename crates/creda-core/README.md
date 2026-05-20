# creda-core (M5)

The runnable peer daemon — composes M1–M4.

**Governing spec section:** §10.1 (Creda Core).

Will contain: the binary that wires creda-events + creda-store + creda-graph + creda-net into a
peer; the gRPC API (CreateEvent, GetEvent, GetSubgraph, GetEffectiveIdentity, MatchByTokens,
EvaluateAuthorization, Subscribe, GetMetrics, plus scaffolded disambiguation RPCs); CLI mode
(`creda init`, `creda snapshot`, …); the tokio runtime; hierarchical config (TOML + env + flags).

**Assemble:** tonic (gRPC), tokio. **Write:** the composition, gRPC service definitions, CLI, config.

Not yet registered as a Cargo workspace member; added in M5.
