# Scenario: bridge-smoke

Verifies that the **HAPI FHIR Bridge starts and serves** — and that it reaches Core over the
**production Unix-domain-socket transport** (§10.5.1), not the TCP override the rest of the testbed
uses.

`make bridge-smoke`

## Why this exists — read this first

Two coverage gaps, found 2026-08-19 while planning the post-upgrade E2E pass.

**1. The release gate never started the Bridge.** `values-peer-a.yaml` and `values-peer-b.yaml`
both set `bridge.enabled: false`, and `ui-smoke` deploys the persona clients in *mock mode*. That
is a defensible trade for those scenarios — gossip/AE/partition exercise the protocol layer only,
and the Bridge costs ~3 GB of Java heap per peer — but the consequence was that the entire
eight-scenario gate could go green while the Bridge failed to start, failed to serve, or returned
garbage. The Bridge *image* was built by `make images`; nothing ever ran it.

**2. Nothing exercised the epoll UDS transport.** The chart's production default is
`grpcSocket: /var/run/creda/core.sock` — a Unix socket on the shared `uds` emptyDir. Every testbed
values file overrides it to `tcp://0.0.0.0:50051`, including `values-uat-peer.yaml`, because the
`seed`/`reset` Jobs run as separate pods and cannot reach a socket inside `peer-0`. Since
`statefulset.yaml` derives the Bridge's `CREDA_CORE_SOCKET` from that same key, the Bridge took the
TCP branch of `CredaCoreClient` everywhere — `eventLoopGroup` is `null` and
`EpollEventLoopGroup` / `EpollDomainSocketChannel` are never constructed. So the transport
production actually uses was exercised by the gate (Bridge off) and by real-mode UAT (Bridge on,
TCP) equally: not at all.

This scenario asserts over **HTTP against the Bridge**, never gRPC against Core, so it has no
reason to want TCP and can hold the production shape. That is what lets one scenario close both
gaps.

## What it exercises

- Spring Boot 4.1 / Spring Framework 7 / Tomcat 11 boot inside the shipped image
- HAPI FHIR 8.10.1 `RestfulServer` in Plain Server mode, provider registration, `/metadata`
  generation and the Creda CapabilityStatement interceptor
- Request routing, parameter validation, and `OperationOutcome` exception rendering
- `CredaCoreClient` dialing Core over the **netty 4.2 epoll domain-socket channel**
- Both the read path (`Patient/{id}`) and the search path (`AuditEvent?patient=`) round-tripping to
  Core and back

## Assertions

| # | Check | Proves |
|---|---|---|
| 1 | `helm install --wait` succeeds | The Bridge's readinessProbe (`GET /fhir/metadata`) passed — Spring booted, Tomcat bound, HAPI registered its providers |
| 2 | `GET /fhir/metadata` → 200, body is a `CapabilityStatement` | CapabilityStatement generation + interceptor |
| 3 | `GET /fhir/Patient/not-a-uuid` → 400 | Routing and exception rendering. No Core call |
| 4 | `GET /fhir/Patient/<unknown-uuid>` → **404, not 500** | **The UDS assertion.** 404 means the Bridge reached Core over the socket and Core answered "no events". 500 or a hang is what a broken epoll/UDS transport looks like |
| 5 | `GET /fhir/AuditEvent?patient=<unknown-uuid>` → 200 + empty `Bundle` | A second Core round-trip, through search rather than read, returning an honest empty ledger |
| 6 | Bridge log free of `NoSuchMethodError` / `NoClassDefFoundError` naming `springframework` | The HAPI-on-Spring-Framework-7 risk did not land. HAPI 8.10.1 CI-tests against Spring Framework 6.2.18; linkage errors can surface long after startup, so a Ready pod is not proof |

Assertion 1 is the cheapest detection of the highest-severity failure: if the framework stack
cannot come up together, the install times out before a single request is sent.

## Deliberately not seeded

The scenario asserts on empty-store responses precisely so it needs no seed data — which is what
frees it from the TCP requirement (see gap #2 above). **Do not add a peer-driver seeding step**; it
would force `grpcSocket` back to `tcp://` and silently re-open the gap this scenario exists to
close. If you need seeded data here, seed *through* the Bridge.

## What this scenario does NOT prove

- **No write path.** Every assertion is a read. `$creda-authorize`, `$creda-revoke`,
  `$creda-attest` and the rest are untouched, so the Bridge→Core *write* direction over the UDS is
  still only exercised by manual real-mode UAT (which runs on TCP). Closing that means either
  seeding through the Bridge or a second scenario.
- **No FHIR conformance.** It checks status codes and `resourceType`, not profile validity. US Core
  / CredaPatient conformance is `anchor creda`'s job.
- **No multi-peer behaviour.** Single isolated peer, `bootstrapPeers: []`. Nothing gossips.
- **No persona UI.** That is `ui-smoke` (mock) and real-mode UAT (real).

## Running

```
cd testbed
make up            # builds + loads creda-core, creda-bridge, peer-driver
make bridge-smoke
```

Or directly:

```
testbed/scenarios/bridge-smoke/run.sh creda-testbed
```

`KEEP_NAMESPACES=1` leaves `creda-bridge-smoke` up for manual inspection — useful for poking the
FHIR surface by hand:

```
kubectl --context kind-creda-testbed -n creda-bridge-smoke port-forward svc/peer-fhir 8080:8080
```

## What success looks like

```
==> generating Ed25519 keypair
==> creating namespace + secrets
==> installing peer (Core + Bridge, UDS transport)
    Bridge is Ready (readinessProbe GET /fhir/metadata passed)
==> asserting the FHIR surface
  ok  GET /metadata -> 200 CapabilityStatement
  ok  GET /Patient/not-a-uuid -> 400
  ok  GET /Patient/<unknown-uuid> -> 404 (Core round-trip over UDS succeeded)
  ok  GET /AuditEvent?patient=<unknown-uuid> -> 200 Bundle (empty ledger)
ALL_CHECKS_PASSED
==> checking the Bridge log for Spring/HAPI linkage errors
    clean
PASS: bridge smoke test (FHIR surface served over the production UDS transport)
```

## Prerequisites

1. **kind cluster up** (`make up`).
2. **`creda-core:testbed`, `creda-bridge:testbed`, `peer-driver:testbed` built and loaded.**
   `make up` handles all three; the scenario refuses to start if the Bridge or driver image is
   missing locally.
3. **`curl` in the peer-driver image.** The assertion Job overrides the peer-driver ENTRYPOINT and
   runs `curl` from that image rather than the driver binary — see
   `testbed/images/peer-driver.Dockerfile`.

## Known follow-ups

- **Write-path coverage** — one `$creda-authorize` through the Bridge, read back via
  `Consent?patient=`, would exercise the UDS in both directions and turn this into a genuine
  round-trip test.
- **Fold into `ui-smoke`** — once the persona clients can run against a real bridge, `ui-smoke`'s
  mock mode becomes the redundant one and these assertions could move there.
- **CI** — same `testbed-kind` matrix entry as the other scenarios, once that exists.
