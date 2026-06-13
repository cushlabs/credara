# Pre-Launch Checklist

Tracked list of placeholders, maintainer notes, and operational TODOs to resolve before the
repository goes public. This covers **launch-blocking** items only — the deliberate, CI-gated
`TODO(open-question-13.x)` / `TODO(libp2p-verify)` / `TODO(bridge-verify)` markers documented in
[`STATUS.md`](STATUS.md) are part of the spec-first methodology and are **not** in scope here.

Last audited: 2026-06-13.

## Public-facing contacts & governance

- [x] **Security contact** — `SECURITY.md` now lists `security@cushlabs.com` (was a maintainer-note placeholder).
- [x] **Code of Conduct enforcement contact** — `CODE_OF_CONDUCT.md` now lists `security@cushlabs.com`.
- [ ] **CODEOWNERS teams** — `.github/CODEOWNERS` references `@cushlabs/maintainers`, `@cushlabs/security`,
      `@cushlabs/docs`, `@cushlabs/spec-owners`, and `@cushlabs/m5-core`. Create these GitHub teams (or
      swap in real usernames). If they don't exist, CODEOWNERS enforcement — including the two-approval
      rule on the spec — silently does nothing. (`.github/CODEOWNERS:4` flags this.)
- [ ] **Branch protection** — confirm the `main` branch protection rule requiring two approvals on
      `docs/credara-technical-spec.md` actually exists on `cushlabs/credara` (referenced by CODEOWNERS).

## Specification (now v1.0.0 / Released)

- [x] **Appendix A — Prior Art & References** — written (standards, protocols, academic citations, and
      implementation components, grouped A.1–A.8). Cite-check versions before final publication.
- [x] **Appendix B — Glossary** — written (~45 terms, sourced from spec usage with section references).
- [x] **Coordinator runbook** — written as §11.5 (Legal Coordinator Operations Runbook): roles &
      separation of duties, genesis, admission, revocation, salt rotation, key management, Registry
      service ops, incident response, succession, and audit/reporting. (Emergency key-transition
      thresholds + tabletop remain tracked under open question §13.5.3.)

## Deployment hardening

- [ ] **NetworkPolicy egress is open** — `deploy/helm/creda/templates/networkpolicy.yaml:31` uses
      `- {}` (allow all egress) with `TODO: restrict to S3/MinIO CIDR`. Scope egress down per environment
      before any real deployment.
- [ ] **Pin container base + app image digests (DQ-4)** — `deploy/helm/creda/values.yaml:12,16`,
      `deploy/docker/core.Dockerfile`, `deploy/docker/bridge.Dockerfile`: Hummingbird FIPS base images
      and the `ghcr.io/cushlabs/creda-core` / `creda-bridge` app images are unpinned placeholders.
- [ ] **Snapshot CronJob trigger** — `deploy/helm/creda/templates/cronjob-snapshot.yaml:6`: wire the
      gRPC trigger to the peer instead of mounting the RWO PVC.
- [ ] See `deploy/README.md` → "Known reconciliation items (TODO)" for the consolidated deployment list.

## Housekeeping

- [x] **README build badges** — CI status badges (ci-rust, ci-java, ci-conformance, ci-docs, gitleaks)
      added to `README.md`.

## Decided — no action

- **Container image names stay `creda-*`.** `creda-core` / `creda-bridge` match the Rust crate/binary
  names (`Cargo.toml`, StatefulSet container names, `cargo build -p creda-core`), which were deliberately
  kept as `creda-*` code identifiers during the Credara rename. Image names follow the binaries they
  package; renaming them to `credara-*` would create a binary/image-name mismatch. Only digest pinning
  (above) is outstanding.

## Out of scope here (tracked elsewhere)

- All `TODO(open-question-13.x)`, `TODO(libp2p-verify)`, `TODO(bridge-verify)`, `TODO(grpc-verify)`,
  `TODO(open-question-confidence-calibration)`, `TODO(trust-layer)`, and the `GitStore` `Unimplemented`
  stub — see [`STATUS.md`](STATUS.md). CI rejects *untracked* `TODO`/`FIXME`.
- Spec open questions that self-flag go-live prerequisites: **§13.5.3** (coordinator key-compromise
  runbook + tabletop "before network launch") and **§13.6.3** (FAST Consent IG at STU 1 ballot).
