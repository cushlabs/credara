# Scenario: UI Smoke

End-to-end smoke test for the persona front-end clients in `clients/`. Brings up the clients
container in the testbed kind cluster, then runs the Playwright e2e specs as a Kubernetes Job
inside the same cluster — exact same execution model as `gossip-convergence` (peer-driver
Job) and `anti-entropy-repair`.

## What it exercises

For each persona (clinician / prior-auth / steward / patient / audit), the smoke spec asserts
the primary flow:

- **Landing** — all five persona cards render.
- **Clinician** — worklist renders, patient detail loads, attest action records to the action
  log.
- **Prior auth** — CRD → DTR → PAS → Decision advances; provenance receipt reveals.
- **Steward** — queue + detail render, link policy shows, blocked-link case shows the BLOCKED
  stamp, contest action appends a fresh DAG node.
- **Patient** — active grants list, revoke flow, share-tab grant flow.
- **Audit** — KPIs + ledger render, filters work, link-decision shows §5.5 evaluation, report
  modal opens.

The clients are deployed in **mock mode** (no bridge wiring) so the spec runs deterministically
against the in-memory fixtures shared with the bridge response shape. Once the bridge's M7
`TODO(bridge-verify)` stubs land, a second variant of this scenario will run the clients
against a real bridge (`values.fhirBase=http://creda-bridge:8080/fhir`).

## Running

```
cd testbed
make ui-smoke
```

Or directly:

```
testbed/scenarios/ui-smoke/run.sh creda-testbed
```

Set `KEEP_NAMESPACES=1` to leave the namespace in place for manual inspection.

## Execution model

Two images are loaded by `make images`:

- `creda-clients:testbed` — multi-stage build (pnpm + vite → nginx). Built from
  `clients/Dockerfile`. Serves the SPA on `:8080`.
- `creda-clients-e2e:testbed` — Playwright runner. Built from `clients/e2e.Dockerfile`.
  Runs the e2e specs against `CLIENTS_URL=http://creda-clients:8080`.

The scenario:

1. Creates the `creda-ui-smoke` namespace (restricted PSS) and installs the clients chart
   (`testbed/helm/clients/`) with a single replica.
2. Waits for the clients Pod to be Ready.
3. Submits a Playwright kubectl Job whose container runs `pnpm test:e2e`. The Job's stdout is
   tailed back to the scenario script and printed on failure (matching the diagnostics block
   in `gossip-convergence/run.sh`).
4. Tears the namespace down (unless `KEEP_NAMESPACES=1`).

The Job inherits the same `securityContext` block the peer-driver Job uses — non-root, all
capabilities dropped, seccomp RuntimeDefault, runAsUser 1000 (the `pwuser` UID baked into the
Playwright image).
