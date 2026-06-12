# Creda Clients (M-clients)

> ⚠️ **DEMONSTRATION CODE + the project's MANUAL END-TO-END HARNESS — not production software.**
> These persona apps are not shipped products; they serve two jobs: (1) showcase the Creda bridge
> surface, and (2) act as the hands-on **end-to-end test harness** — driven against a *real* bridge
> they exercise client → FHIR → bridge → gRPC → Core → DAG → gossip, the same path external clients
> will use. Run them as the pre-pilot E2E pass per **`docs/E2E.md`**.
>
> They run in **mock** mode (in-memory fixtures; global "MOCK BRIDGE" chip) and **real** mode
> (against a live peer). In real mode, any surface still backed by fixtures shows an amber
> **`DEMO DATA`** chip — which for testing purposes means *"this surface tests nothing yet"* (a
> coverage gap). See `docs/STATUS.md` for the authoritative real-vs-fixture map and `docs/HANDOFF.md`
> for the de-fixturing backlog. Don't treat these as a reference production EHR / patient / payer
> integration.

Five persona-specific front-end clients ported from `design/*-mockup.html` into
type-checked, FHIR-bridge-wired React apps:

| Route | Persona | Mockup source |
|---|---|---|
| `/clinician` | Treating clinician (clinical view) | `design/clinician-review-mockup.html` |
| `/prior-auth` | Clinician · Da Vinci CRD/DTR/PAS | `design/prior-auth-mockup.html` |
| `/steward` | Identity steward (operator view) | `design/steward-console-mockup.html` |
| `/patient` | Patient (consent client) | `design/patient-consent-mockup.html` |
| `/audit` | Compliance/audit reviewer | `design/compliance-audit-mockup.html` |

The clients are a **single Vite + React + TS app** with five route-mounted persona modules
that share a design system (`src/shared/components`) and a typed FHIR bridge client
(`src/shared/fhir`). One image, one nginx, five UIs. If individual personas later need
distinct deploy footprints, the per-persona modules already isolate their state and can be
split into separate Vite entries without touching the shared layer.

## Stack

- **React 18 + TypeScript** — strict mode, no `any`.
- **Vite 5** — fast dev server, library-free bundling.
- **React Router 6** — top-level persona routes + nested pages.
- **pnpm** — single workspace; no monorepo needed for one app.
- **Playwright** — e2e tests, runnable in-cluster as a Kubernetes Job (matches the testbed
  `gossip-convergence` / `anti-entropy-repair` execution model).

## Backend wiring

The clients talk to the **HAPI FHIR Bridge** (`bridge/`) over standard FHIR REST + the
`$creda-*` operations declared on each ResourceProvider. The shared FHIR client
(`src/shared/fhir/client.ts`) is configurable:

- `VITE_FHIR_BASE=http://bridge:8080/fhir` → real bridge.
- `VITE_FHIR_BASE=mock` → in-memory fixtures matching the bridge response shape (no network).

In `mock` mode the clients run end-to-end without any backend — the same code path is
exercised, only the transport adapter swaps. This is what the testbed `ui-smoke` scenario
asserts against until the bridge's M7 `TODO(bridge-verify)` stubs land.

## Quickstart (host-side dev)

```sh
cd clients
pnpm install
pnpm dev
```

The dev server serves the landing at `http://localhost:5173/` and each persona at
`/clinician`, `/prior-auth`, `/steward`, `/patient`, `/audit`. The other entry points:

```sh
pnpm build      # production bundle in dist/
pnpm test:e2e   # Playwright against the dev server
pnpm typecheck  # tsc --noEmit (strict)
```

Set `VITE_FHIR_BASE` in `.env.local` (or as a Vite mode flag) to point at a running
bridge:

```
echo 'VITE_FHIR_BASE=http://localhost:8080/fhir' > .env.local
pnpm dev
```

## Testbed integration

`testbed/scenarios/ui-smoke/` brings up the clients alongside a peer + bridge in the kind
cluster and runs Playwright as an in-cluster Job. See `testbed/scenarios/ui-smoke/README.md`.

```
cd testbed
make ui-smoke
```

## Layout

```
clients/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
├── nginx.conf
├── Dockerfile             — multi-stage: pnpm build → nginx serve
├── playwright.config.ts
├── e2e/
│   ├── clinician.spec.ts
│   ├── patient.spec.ts
│   ├── steward.spec.ts
│   ├── audit.spec.ts
│   └── prior-auth.spec.ts
└── src/
    ├── main.tsx
    ├── App.tsx
    ├── styles/
    │   ├── tokens.css     — CSS variables ported from the mockups
    │   └── globals.css
    ├── shared/
    │   ├── components/    — AppBar, ViewBanner, Badge, Modal, Toast, …
    │   ├── fhir/          — typed client + mock adapter
    │   ├── mock/          — fixture data shared across personas
    │   └── lib/           — initials, confColor, …
    ├── clinician/
    ├── prior-auth/
    ├── steward/
    ├── patient/
    └── audit/
```

## Relationship to the mockups

The mockups in `design/` are the **visual + interaction spec** for the clients. Visual
parity is enforced by the Playwright snapshot tests in `e2e/`. When a persona's flow
evolves, update the mockup first (it's small, the design surface is HTML+JS), then port
the change here.
