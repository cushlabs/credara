# testbed/compose — Fast multi-peer path (DQ-3)

Docker Compose bring-up of 2–3+ Creda peers for fast local iteration. Reuses the dev
image; optimized for speed and quick log/inspect cycles. Runs the shared `../scenarios/`.
Even here, containers run as non-root (DQ-1).
