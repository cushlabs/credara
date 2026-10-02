# Creda FHIR Bridge — REST API reference

The Bridge exposes one HTTP API: a **FHIR R4** server at **`/fhir`** (port `8080`, `bridge/src/main/resources/application.yml`). There are no other HTTP endpoints — no actuator, no custom controllers. The Kubernetes readiness probe calls `GET /fhir/metadata`.

This document describes **what the code serves today**, derived from the HAPI annotations in `bridge/src/main/kotlin/health/creda/bridge/providers/`. The design intent behind each operation lives in the technical spec (`docs/credara-technical-spec.md`, §8.2); where the two disagree, this file and the running server's `GET /fhir/metadata` are authoritative for behaviour, and §8.2 is authoritative for intent. Build status per endpoint is tracked in `docs/STATUS.md`.

Conventions used below:

- All paths are relative to `/fhir`.
- `{id}` on a Patient path is the patient's **subgraph entry-point event UUID** (§8.1.1). Clients resolve it via `Patient?_creda-token=`. A `urn:uuid:` prefix is tolerated; any non-UUID id is rejected (`400`), not coerced.
- Operations take a FHIR `Parameters` body on `POST`. Only operations marked **GET-invocable** may also be called with `GET` (HAPI `idempotent = true`).
- Operation parameter values are read as primitives (`valueString` / `valueCode` / `valueDateTime` / `valueBoolean`); a parameter named `grant`/`target` may also be a `valueReference` (`Consent/<uuid>`, `Provenance/<uuid>`), from which the last path segment is taken.
- Fingerprints are hex strings (optional `0x` prefix).
- Errors follow HAPI's standard `OperationOutcome` shape. Missing or malformed parameters raise `400`.

---

## 1. Server

| Method | Path | Returns |
|---|---|---|
| GET | `/metadata` | `CapabilityStatement`. HAPI generates it from the registered providers, so operations and the `_creda-token` search parameter are always advertised. `CredaCapabilityStatementInterceptor` adds `implementationGuide = http://credara.network/fhir/ig/v1`, `publisher = Credara`, and per-resource profiles: `Patient → …/StructureDefinition/CredaPatient`, `Provenance → …/CredaProvenance`, `Consent → …/CredaAuthorization`. `AuditEvent` is deliberately unprofiled. |

Every completed interaction is also recorded by `BridgeAccessAuditInterceptor` to the `AccessAuditSink` (read-side access log, §8.2.4) — separate from the on-chain disclosure ledger served by `AuditEvent?patient=`.

---

## 2. Patient — identity

Provider: `PatientResourceProvider` (resource provider) and `AuthorizationResourceProvider` (plain provider contributing Patient-typed operations; see §7).

### `GET /Patient/{id}`
The **CredaPatient projection** (§8.2.2). Returns a US Core Patient with the three Credara `mustSupport` extensions (subgraph identifier, root set, last-modified event), institutional MRN identifiers, and real `gender`. **`name`, `birthDate`, and `address` are masked** with `data-absent-reason` — cleartext is never held at the Bridge (§9.2); fetch it via `$creda-cleartext`.

Core calls: `GetSubgraphIdentity`, `EffectiveIdentity`.
Errors: `400` non-UUID id · `404` no events reachable from the entry point.

### `GET /Patient?_creda-token={token}[,{token}…]`
Search by demographic token (§8.2.11). Returns Patients carrying **only** an `id` (the entry-point UUID) — no demographics. Tokens may be repeated or comma-joined (`TokenAndListParam`); all values are passed to Core `MatchByTokens`.

### `POST /Patient/$match`
Scored identity matching (standard FHIR `$match`). The `resource` parameter is a query Patient carrying **tokenized** demographics:

- name tokens in `name[0].family` / `name[0].given[0]`
- other fields as identifiers with `system = http://credara.network/fhir/sid/match-token/<field>` (e.g. `…/date-of-birth`)

| Parameter | Type | Notes |
|---|---|---|
| `resource` | Patient | required; must yield at least one token or `400` |
| `count` | integer | optional cap on results |
| `onlyCertainMatches` | boolean | default `false`; when true, only grade `certain` is returned |

Returns a `searchset` Bundle of CredaPatients, best first, each entry with `search.score` (0–1, 3 dp) and the `http://hl7.org/fhir/StructureDefinition/match-grade` extension (`certain` / `probable` / `possible`; `certainly-not` is filtered out). Scoring (`PatientMatcher`) is a log-likelihood ratio over per-field token agreement against each candidate's effective identity; weights are **uncalibrated** (§5.3.2, `docs/matching-calibration.md`).

### `POST /Patient` → `405`
Patient is a projection, not a writable resource (§8.3.3). Always `MethodNotAllowed`.
> The error text says to "use `$creda-attest` / `$creda-link` / `$creda-authorize`". **`$creda-link` is not implemented** (see §9); following that hint 404s.

### `DELETE /Patient/{id}` → `405`
Deletion is `$creda-tombstone`.

### `POST /Patient/{id}/$creda-attest`
Record an **Attest** event (§8.2.6) affirming reliance on existing events.

| Parameter | Type | Notes |
|---|---|---|
| `references` | string / Reference, repeatable | target event UUIDs. Parsed tolerantly: every UUID found in each value is used (plain, `Provenance/<uuid>`, or a JSON-stringified array). |
| `purpose` | code | default `treatment` |

Targets are both the attest's targets and its parents, so it lands inside the patient's subgraph. With no `references`, the Attest targets the entry-point event named by `{id}` (must exist, else `404`).
Returns the new event as `Provenance`. Core: `GetEvent`, `CreateEvent`.

### `POST /Patient/{id}/$creda-tombstone`
Right-to-be-forgotten (§3.4.6, §8.2.8). Records a signed **Tombstone** over the target events; Core then scrubs their stored demographic content to husks. Irreversible.

| Parameter | Type | Notes |
|---|---|---|
| `references` | string / Reference, repeatable | target event UUIDs; same tolerant parsing as `$creda-attest`. Falls back to `{id}` (must exist, else `404`). |
| `legal-basis` | code | `right-to-be-forgotten` (default) · `state-law` · `court-order` · `other` |

Returns the Tombstone as `Provenance`.

### `POST /Patient/{id}/$creda-amend`
Amend a prior Assert's demographics (§3.4.5) — currently **date of birth only**.

| Parameter | Type | Notes |
|---|---|---|
| `target` | Reference / string | required; UUID of the Assert being amended (becomes the parent) |
| `dateOfBirth` | string | required |
| `reason` | string | required |

Returns the Amend as `Provenance`. Originating-institution rule is enforced in Core.

### `GET|POST /Patient/{id}/$creda-provenance` — GET-invocable
The patient's full provenance chain (§8.2.5): every subgraph event projected as `CredaProvenance`, in logical-clock order (Core sorts). Returns a `searchset` Bundle. Core: `GetSubgraphEvents` with no type filter.

### `GET|POST /Patient/{id}/$creda-effective-identity` — GET-invocable
The computed effective identity (§5.2.4 / §5.3) as `Parameters`:

```
field (repeated)
  key        string   kebab field key, e.g. date-of-birth
  disputed   boolean
  value (repeated)
    token       string   tokenized value
    confidence  integer
    support     string   (repeated) supporting Assert event UUIDs — attest one to affirm this value
```

Core: `EffectiveIdentity`.

---

## 3. Patient — authorization and disclosure

Provider: `AuthorizationResourceProvider` (plain provider, `typeName = "Patient"`). Enumerations shared by several operations:

- **purpose** — `treatment` · `payment` · `operations` · `public-health` · `research` · `ai-training` · `ai-inference` · `federal-program` (§4.3.1)
- **useMode** — `read-only` · `read-and-rely` · `read-and-export`
- **scope** — absent or `full-subgraph` ⇒ whole subgraph; any other string ⇒ a single data category
- **requester / recipient / requestingInstitution** — institution certificate fingerprint, hex

### `POST /Patient/{id}/$creda-authorize`
Create an **AuthorizationGrant** (§8.2.9). Parent = the patient's entry-point event.

| Parameter | Type | Notes |
|---|---|---|
| `purpose` | code | required, from the purpose list |
| `useMode` | code | required, from the useMode list |
| `audience` | string | required. `id:<hex>` → a specific institution; `wildcard:<pattern>` → constrained wildcard; any other value → institution class (e.g. `any-tefca-qhin`) |
| `scope` | string | optional |
| `expiration` | dateTime | optional |

Returns the Grant as `Consent` (status `active`, profile CredaAuthorization).

### `POST /Patient/{id}/$creda-revoke`
Create an **AuthorizationRevocation**. Parent = the revoked Grant.

| Parameter | Type | Notes |
|---|---|---|
| `grant` (alias `target`) | Reference / string | required; UUID of the Grant to revoke |

Returns a `Consent` with status `inactive`.

### `POST /Patient/{id}/$creda-verify`
Run Core's authorization evaluation (`EvaluateAuthorization`) for a requester. The Verifier is local (§10.3.3) and may answer from stale state.

| Parameter | Type | Notes |
|---|---|---|
| `requester` | string | required, hex fingerprint |
| `purpose` | code | required |
| `useMode` | code | required |

Returns `Parameters`: `decision` (`authorized` / `denied`), `reason` (string), and zero or more `governingGrant` (`Reference` to `Consent/<uuid>`).

### `POST /Patient/{id}/$creda-export`
Record an **ExportReceipt** — data released under a Grant (§8.2.9; FAST `$record-disclosure` shape, §8.5.3). Typically called by the Export Gate, not a user. Parent = the governing Grant.

| Parameter | Type | Notes |
|---|---|---|
| `grant` | Reference / string | required; governing Grant UUID |
| `requestingInstitution` | string | required, hex fingerprint |
| `scope` | string | optional |

Returns the receipt as `AuditEvent`.

### `POST /Patient/{id}/$creda-tpo-disclose`
Record a **TPODisclosure** — the grant-less sibling of `$creda-export` for presumptive HIPAA treatment/payment/operations disclosures (§4.3.5). Parent = the patient's entry-point event.

| Parameter | Type | Notes |
|---|---|---|
| `recipient` | string | required, hex fingerprint |
| `purpose` | code | required; **only** `treatment` · `payment` · `operations` |
| `scope` | string | optional |
| `dataReference` | string | optional |

Returns the disclosure as `AuditEvent`.

### `POST /Patient/{id}/$creda-cleartext`
Consent-gated fetch of the cleartext demographics that `GET /Patient/{id}` masks (§9.2.4). Two fail-closed stages:

1. **Consent gate** — `EvaluateAuthorization` with the requester's fingerprint, purpose, and useMode (same inputs as `$creda-verify`). Not authorized ⇒ **`403`**.
2. **Source** — the deployment's `CleartextProvider` bean (SPI; cleartext lives in the institution's EHR/MPI, never in Credara). No bean registered ⇒ **`501`**; bean has no record for this patient ⇒ **`404`**.

| Parameter | Type | Notes |
|---|---|---|
| `requester` | string | required, hex fingerprint |
| `purpose` | code | required |
| `useMode` | code | required |
| `field` | code, repeatable | `name` · `birthDate` · `address`; absent ⇒ everything the provider returns |

Returns an ordinary `Patient` with **real** values. Served by the *originating* institution's bridge; the Bridge↔Bridge P2P transport that reaches it is not yet built (`docs/STATUS.md`).

---

## 4. Provenance

Provider: `ProvenanceResourceProvider`. Each Creda identity event is a `CredaProvenance` (§8.2.3).

### `GET /Provenance/{id}`
One event by UUID, mapped by `ProvenanceMapper`. `404` if the id is not a UUID or the event is unknown. Core: `GetEvent`.

### `POST /Provenance/{id}/$creda-contest`
Contest a **Link** event (§8.2.7). `{id}` is the Link's UUID and becomes the Contest's parent. Party-of-subgraph is enforced in Core.

| Parameter | Type | Notes |
|---|---|---|
| `code` | code | `distinct-patients` · `demographic-conflict` · `duplicate-record` · `other` (§3.4.3). Default `other`. Unknown value ⇒ `400`. |
| `detail` | string | optional free text |
| `reason` | string | **legacy**: if `code` is absent, `reason` is used as `detail` with `code = other` |

Returns the Contest as `Provenance`.

---

## 5. Consent and AuditEvent (read-back)

### `GET /Consent?patient={id}`
Provider: `ConsentResourceProvider`. The patient's AuthorizationGrants projected as CredaAuthorization `Consent`s (§8.2.9). A Grant referenced by any stored AuthorizationRevocation is returned with status `inactive`; the rest are `active`. `patient` is **required**; `400` if not a UUID. Core: `GetSubgraphEvents(types = AuthorizationGrant, AuthorizationRevocation)`.

### `GET /AuditEvent?patient={id}`
Provider: `AuditEventResourceProvider`. The on-chain **disclosure ledger** (§4.3.3, §8.2.4): the patient's `ExportReceipt` and `TPODisclosure` events as FHIR `AuditEvent`, newest first, each tied to the patient whose data moved. An empty result is the honest answer when nothing has been disclosed. `patient` is **required**; `400` if not a UUID.

Read-side access logging ("who queried which subgraph") is **not** here — see `BridgeAccessAuditInterceptor`.

---

## 6. Organization and Task

### `GET /Organization`
Provider: `OrganizationResourceProvider`. Institution discovery: every distinct grant audience in the local store (Core `ListInstitutions`), one `Organization` per name. Only `Organization.name` is populated; `id` is the unsigned hash of the name. No search parameters. Richer directory data is a Participant Registry concern (spec Appendix C).

### `POST /Task`
Provider: `TaskResourceProvider`. The **off-chain** half of the hybrid access-request workflow (§4.3.4): a requesting institution asks for access; the patient answers on-chain with `$creda-authorize`, then resolves the Task.

Request body is a `Task` with:

| Element | Required | Notes |
|---|---|---|
| `for.reference` | yes | `Patient/<id>` |
| `requester.display` | yes | the requesting institution's name |
| `description` | no | `"<purpose>|<useMode>"`, default `"Treatment|Read & rely"` (free text, split on `|`) |

Returns `201` with the stored Task: `status = requested`, `intent = order`, `authoredOn`, and the same `description`.

### `GET /Task?patient={id}`
The patient's pending access requests. `patient` is **required**.

### `POST /Task/{id}/$creda-resolve-request`
Remove a request once the patient has granted or dismissed it. Returns an empty `OperationOutcome`; unknown ids are silently ignored.

**State caveat:** Tasks are the Bridge's one piece of mutable state — an in-memory `ConcurrentHashMap`. They are **not DAG events, not persisted, and lost on restart**, and are delivered only when requester and patient share one bridge. This is deliberate (an access request is coordination, not identity, and must not be gossiped to every peer, §13.3); cross-peer delivery is a tracked real-PHI design item.

---

## 7. Registration notes (why the paths look the way they do)

`CredaRestfulServer` (`FhirServerConfig.kt`) registers six **resource providers** — Patient, Provenance, Consent, Organization, Task, AuditEvent — plus `AuthorizationResourceProvider` as a HAPI **plain provider**. The authorization operations attach to `/Patient/{id}/…` via `@Operation(typeName = "Patient")` because HAPI forbids two resource providers for one type and a Consent-typed provider would have put them under `/Consent/{id}/…`. `initialize()` is guarded with an `AtomicBoolean` because Spring's lazy servlet init can call it twice under load, and `setResourceProviders` adds rather than replaces.

The spec's §10.4.2 describes `AuthorizationResourceProvider` as a resource provider with its own create/read/search/delete; that was the plan, not what shipped. There is no `/Authorization` resource — grants are read via `Consent?patient=` and written via the Patient operations above.

---

## 8. Error summary

| Status | When |
|---|---|
| `400` | Non-UUID patient/event id; missing or invalid required parameter; unknown enum value; `$match` with no tokens |
| `403` | `$creda-cleartext` with no covering grant |
| `404` | Unknown event / patient; `$creda-cleartext` when the institution holds no cleartext for the patient |
| `405` | `POST /Patient`, `DELETE /Patient/{id}` |
| `501` | `$creda-cleartext` with no `CleartextProvider` bean registered |

---

## 9. Specified but not implemented

These appear in the spec (§8.2) or the bridge README but are **not registered**; calling them returns HAPI's `400 "Unknown operation"` / `404`:

| Operation / feature | Spec | Notes |
|---|---|---|
| `$creda-link` | §8.2.7 | Links are created in Core today; no FHIR face. Still named in the `POST /Patient` error message. |
| `$creda-disambiguate` | §8.2.10 | |
| `$creda-self-verify` | §8.2.10.4 | |
| `Patient/$export` (Bulk Data) | §8.2.14 | |
| `Subscription` → gossip | §8.2.13 | |
| Provenance search / history, Patient history | §10.4.2 | Only `read` exists on Provenance; no `_history` anywhere |
| FASTConsent-conformant Consent projection (F1) | §8.5.6 | Current projection is the minimal CredaAuthorization shape |
| SMART on FHIR scope enforcement | §10.4.4 | |

---

*Generated from the provider sources on 2026-10-02. When adding or changing an endpoint, update this file, `docs/STATUS.md`, and — if intent changes — the spec section it cites.*
