# testbed/scenarios — Shared scenario library

Declarative, reusable scenarios exercised by BOTH the Compose and kind/k3d test-bed paths
and by the M9 conformance suite. Each scenario sets up a peer topology, drives events
(synthetic data only), and asserts expected end-state (convergence, repair, authorization
decisions, revocation latency). Keep scenarios runner-agnostic so the same definition runs
locally and in CI.
