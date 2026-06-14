// Typed client over the Creda FHIR bridge. The bridge advertises FHIR R4 + the $creda-*
// operations declared in bridge/src/main/kotlin/health/creda/bridge/providers/*.kt.
//
// Transport selection — controlled by import.meta.env.VITE_FHIR_BASE:
//   - "mock"  → in-memory adapter backed by src/shared/mock fixtures (default for dev/tests).
//   - http(s) URL → real HTTP transport against a bridge instance.
//
// Both transports satisfy the same `FhirBridge` interface so the persona clients are
// transport-agnostic. The mock adapter mirrors the bridge response shape exactly; switching
// to a real bridge is a build-time env change, no code change.

import { mockBridge } from '../mock/bridge';
import type {
  AuthorizationDecision,
  ContestReasonCode,
  CredaAuthorization,
  CredaProvenance,
  GrantPurpose,
  GrantScope,
  UseMode,
} from './types';

export interface AttestRequest {
  patientId: string;
  purpose: string;
  references: string[];
  /** Free-text summary the steward / clinician entered. */
  summary?: string;
}

export interface AuthorizeRequest {
  patientId: string;
  audience: string;
  audienceKind: 'institution' | 'class';
  purpose: GrantPurpose;
  use: UseMode;
  scope: GrantScope;
  /** ISO-8601 expiration timestamp, or "No expiry". */
  expires: string;
}

export interface RevokeRequest {
  patientId: string;
  grantId: string;
}

/** An off-chain access request a provider has made (FHIR Task, ephemeral bridge state). */
export interface AccessRequest {
  id: string;
  patientId: string;
  requester: string;
  purpose: GrantPurpose;
  use: UseMode;
}

export interface RequestAccessArgs {
  patientId: string;
  requester: string;
  purpose: GrantPurpose;
  use: UseMode;
}

export interface AmendRequest {
  patientId: string;
  /** The Assert event being amended (must be a real event UUID). */
  targetEventId: string;
  /** Corrected DOB (token form against the real bridge). */
  dateOfBirth: string;
  reason: string;
}

/** One asserted value of a demographic field, with its Core-computed confidence (basis points). */
export interface EffectiveValue {
  value: string;
  confidence: number;
  /** Assert event ids backing this value — attest one of these to affirm it. */
  supporting: string[];
}

/** Core's effective projection of one demographic field (§5.3): values confidence-desc + dispute. */
export interface EffectiveField {
  key: string;
  disputed: boolean;
  values: EffectiveValue[];
}

export interface ContestRequest {
  /** The Provenance.id of the Link being contested. */
  linkId: string;
  /** ContestReason.code (§3.4.3) — why the link is wrong. */
  code: ContestReasonCode;
  /** Optional free-text elaboration (ContestReason.detail). */
  detail?: string;
}

export interface FhirBridge {
  /** Patient search by tokenized demographic (`Patient?_creda-token=...`). */
  searchPatientsByToken(tokens: string[]): Promise<string[]>;
  /** Fetch the patient projection — minimal CredaPatient today (bridge TODO). */
  readPatient(id: string): Promise<unknown>;
  /** Read the subgraph as a stream of CredaProvenance resources. */
  readSubgraph(patientId: string): Promise<CredaProvenance[]>;
  /** Read a single Provenance event by id. */
  readProvenance(id: string): Promise<CredaProvenance>;
  /** `$creda-attest` — record an Attest event on the patient's chain. */
  attest(req: AttestRequest): Promise<CredaProvenance>;
  /** `$creda-contest` — contest a Link Provenance. */
  contest(req: ContestRequest): Promise<CredaProvenance>;
  /** `$creda-amend` — amend a prior Assert's demographics (DOB-resolution flow, §3.4.5). */
  amend(req: AmendRequest): Promise<CredaProvenance>;
  /**
   * `$creda-effective-identity` — Core's computed per-field projection (§5.2.4 / §5.3):
   * confidence-weighted, attestation-amplified, disagreement-flagged. The client renders this;
   * it does NOT recompute identity (§8.3.2).
   */
  effectiveIdentity(patientId: string): Promise<EffectiveField[]>;
  /** `$creda-authorize` — create an AuthorizationGrant. */
  authorize(req: AuthorizeRequest): Promise<CredaAuthorization>;
  /** `$creda-revoke` — revoke a prior Grant. */
  revoke(req: RevokeRequest): Promise<CredaAuthorization>;
  /** `$creda-verify` — evaluate authorization for a requesting institution. */
  verifyAuthorization(args: {
    patientId: string;
    requester: string;
    purpose: GrantPurpose;
    use: UseMode;
  }): Promise<AuthorizationDecision>;
  /** Listing for the audit reviewer — all grants/revocations/exports in a window. */
  listAuthorizationEvents(): Promise<CredaProvenance[]>;
  /**
   * The patient's authorizations — `GET /Consent?patient={id}` (§8.2.9 read-back). Grants a
   * revocation references come back with status `revoked`.
   */
  listAuthorizations(patientId: string): Promise<CredaAuthorization[]>;
  /**
   * Institutions known to the network — `GET /Organization`, the distinct audience names seen in
   * AuthorizationGrants store-wide (Core's ListInstitutions). A discovery surface for "share with
   * an institution that already exists" rather than a full directory.
   */
  listInstitutions(): Promise<string[]>;
  /**
   * Hybrid access-request workflow (§4.3 design note) — the OFF-CHAIN half. A provider requests
   * access by creating an ephemeral FHIR Task; the patient lists pending requests and answers with
   * an on-chain `authorize`, then resolves the Task. Not a DAG event; not persisted.
   */
  requestAccess(req: RequestAccessArgs): Promise<AccessRequest>;
  listAccessRequests(patientId: string): Promise<AccessRequest[]>;
  resolveAccessRequest(id: string): Promise<void>;
}

class HttpBridge implements FhirBridge {
  constructor(private readonly base: string) {}

  private async req<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${this.base}${path}`, {
      ...init,
      headers: { Accept: 'application/fhir+json', 'Content-Type': 'application/fhir+json', ...(init?.headers ?? {}) },
    });
    if (!res.ok) {
      throw new Error(`bridge ${path} -> HTTP ${res.status}`);
    }
    return (await res.json()) as T;
  }

  async searchPatientsByToken(tokens: string[]): Promise<string[]> {
    const q = tokens.map((t) => `_creda-token=${encodeURIComponent(t)}`).join('&');
    interface Bundle {
      entry?: { resource: { id: string } }[];
    }
    const bundle = await this.req<Bundle>(`/Patient?${q}`);
    return (bundle.entry ?? []).map((e) => e.resource.id);
  }

  readPatient(id: string): Promise<unknown> {
    return this.req(`/Patient/${encodeURIComponent(id)}`);
  }

  async readSubgraph(patientId: string): Promise<CredaProvenance[]> {
    interface Bundle {
      entry?: { resource: FhirProvenanceResource }[];
    }
    const bundle = await this.req<Bundle>(
      `/Patient/${encodeURIComponent(patientId)}/$creda-provenance`,
    );
    // Real FHIR Provenance -> UI shape at the transport boundary, like every other read.
    return (bundle.entry ?? []).map((e) => provenanceFromFhir(e.resource));
  }

  readProvenance(id: string): Promise<CredaProvenance> {
    return this.req<CredaProvenance>(`/Provenance/${encodeURIComponent(id)}`);
  }

  async attest(req: AttestRequest): Promise<CredaProvenance> {
    // Clean, conformant params: a `references` part per target (not a JSON-stringified array),
    // so the bridge attests the real Assert/Link rather than a stub.
    const parameter: FhirParam[] = [{ name: 'purpose', valueString: req.purpose }];
    req.references.forEach((r) => parameter.push({ name: 'references', valueString: r }));
    if (req.summary) parameter.push({ name: 'summary', valueString: req.summary });
    const res = await this.req<FhirProvenanceResource>(
      `/Patient/${encodeURIComponent(req.patientId)}/$creda-attest`,
      { method: 'POST', body: JSON.stringify(fhirParameters(parameter)) },
    );
    return provenanceFromFhir(res);
  }

  async contest(req: ContestRequest): Promise<CredaProvenance> {
    // ContestReason {code, detail?} — code is required; detail omitted when absent.
    const parts: Record<string, string> = { code: req.code };
    if (req.detail) parts.detail = req.detail;
    const res = await this.req<FhirProvenanceResource>(
      `/Provenance/${encodeURIComponent(req.linkId)}/$creda-contest`,
      { method: 'POST', body: JSON.stringify(parametersOf(parts)) },
    );
    return provenanceFromFhir(res);
  }

  async amend(req: AmendRequest): Promise<CredaProvenance> {
    const res = await this.req<FhirProvenanceResource>(
      `/Patient/${encodeURIComponent(req.patientId)}/$creda-amend`,
      {
        method: 'POST',
        body: JSON.stringify(
          fhirParameters([
            { name: 'target', valueReference: { reference: `Provenance/${req.targetEventId}` } },
            { name: 'dateOfBirth', valueString: req.dateOfBirth },
            { name: 'reason', valueString: req.reason },
          ]),
        ),
      },
    );
    return provenanceFromFhir(res);
  }

  async authorize(req: AuthorizeRequest): Promise<CredaAuthorization> {
    // Conform to the bridge contract (§8.2.9) in BOTH directions: requests carry spec parameter
    // names + FHIR codes; the response is a real FHIR R4 Consent, translated back to the UI's
    // display model here at the transport boundary (never in components).
    const parameter: FhirParam[] = [
      { name: 'audience', valueString: req.audience },
      { name: 'purpose', valueCode: PURPOSE_CODE[req.purpose] },
      { name: 'useMode', valueCode: USE_CODE[req.use] },
      { name: 'scope', valueCode: SCOPE_CODE[req.scope] },
    ];
    if (req.expires && req.expires !== 'No expiry') {
      parameter.push({ name: 'expiration', valueDateTime: req.expires });
    }
    const res = await this.req<FhirConsentResource>(
      `/Patient/${encodeURIComponent(req.patientId)}/$creda-authorize`,
      { method: 'POST', body: JSON.stringify(fhirParameters(parameter)) },
    );
    return consentToAuthorization(res, req);
  }

  async revoke(req: RevokeRequest): Promise<CredaAuthorization> {
    const res = await this.req<FhirConsentResource>(
      `/Patient/${encodeURIComponent(req.patientId)}/$creda-revoke`,
      {
        method: 'POST',
        body: JSON.stringify(
          fhirParameters([{ name: 'grant', valueReference: { reference: `Consent/${req.grantId}` } }]),
        ),
      },
    );
    return consentToAuthorization(res);
  }

  async verifyAuthorization(args: {
    patientId: string;
    requester: string;
    purpose: GrantPurpose;
    use: UseMode;
  }): Promise<AuthorizationDecision> {
    interface Parameters {
      parameter?: { name: string; valueString?: string; valueCode?: string }[];
    }
    const raw = await this.req<Parameters>(`/Patient/${encodeURIComponent(args.patientId)}/$creda-verify`, {
      method: 'POST',
      body: JSON.stringify(
        fhirParameters([
          { name: 'requester', valueString: args.requester },
          { name: 'purpose', valueCode: PURPOSE_CODE[args.purpose] },
          { name: 'useMode', valueCode: USE_CODE[args.use] },
        ]),
      ),
    });
    const decision = raw.parameter?.find((p) => p.name === 'decision')?.valueCode ?? 'denied-no-grant';
    const reason = raw.parameter?.find((p) => p.name === 'reason')?.valueString ?? '';
    return { decision: decision as AuthorizationDecision['decision'], reason };
  }

  async listAuthorizationEvents(): Promise<CredaProvenance[]> {
    interface Bundle {
      entry?: { resource: CredaProvenance }[];
    }
    const bundle = await this.req<Bundle>(`/Provenance?_creda-eventType=AuthorizationGrant,AuthorizationRevocation,ExportReceipt`);
    return (bundle.entry ?? []).map((e) => e.resource);
  }

  async listAuthorizations(patientId: string): Promise<CredaAuthorization[]> {
    interface Bundle {
      entry?: { resource: FhirConsentResource }[];
    }
    const bundle = await this.req<Bundle>(`/Consent?patient=${encodeURIComponent(patientId)}`);
    return (bundle.entry ?? []).map((e) => consentToAuthorization(e.resource));
  }

  async listInstitutions(): Promise<string[]> {
    interface Bundle {
      entry?: { resource: { name?: string } }[];
    }
    const bundle = await this.req<Bundle>(`/Organization`);
    return (bundle.entry ?? []).map((e) => e.resource.name ?? '').filter(Boolean);
  }

  async requestAccess(req: RequestAccessArgs): Promise<AccessRequest> {
    const task = {
      resourceType: 'Task',
      status: 'requested',
      intent: 'order',
      for: { reference: `Patient/${req.patientId}` },
      requester: { display: req.requester },
      // The bridge stores `purpose|useMode`; the patient client splits it to pre-fill the grant.
      description: `${req.purpose}|${req.use}`,
    };
    const res = await this.req<FhirTaskResource>(`/Task`, { method: 'POST', body: JSON.stringify(task) });
    return taskToAccessRequest(res);
  }

  async listAccessRequests(patientId: string): Promise<AccessRequest[]> {
    interface Bundle {
      entry?: { resource: FhirTaskResource }[];
    }
    const bundle = await this.req<Bundle>(`/Task?patient=${encodeURIComponent(patientId)}`);
    return (bundle.entry ?? []).map((e) => taskToAccessRequest(e.resource));
  }

  async resolveAccessRequest(id: string): Promise<void> {
    await this.req(`/Task/${encodeURIComponent(id)}/$creda-resolve-request`, {
      method: 'POST',
      body: JSON.stringify({ resourceType: 'Parameters' }),
    });
  }

  async effectiveIdentity(patientId: string): Promise<EffectiveField[]> {
    // $creda-effective-identity returns a Parameters resource: one `field` part per field, each
    // with `key`, `disputed`, and a `value` part (token + confidence) per asserted value.
    interface Part {
      name?: string;
      valueString?: string;
      valueBoolean?: boolean;
      valueInteger?: number;
      part?: Part[];
    }
    const res = await this.req<{ parameter?: Part[] }>(
      `/Patient/${encodeURIComponent(patientId)}/$creda-effective-identity`,
    );
    const pick = (parts: Part[] | undefined, name: string) => (parts ?? []).find((p) => p.name === name);
    return (res.parameter ?? [])
      .filter((f) => f.name === 'field')
      .map((f) => ({
        key: pick(f.part, 'key')?.valueString ?? '',
        disputed: pick(f.part, 'disputed')?.valueBoolean ?? false,
        values: (f.part ?? [])
          .filter((p) => p.name === 'value')
          .map((v) => ({
            value: pick(v.part, 'token')?.valueString ?? '',
            confidence: pick(v.part, 'confidence')?.valueInteger ?? 0,
            supporting: (v.part ?? []).filter((p) => p.name === 'support').map((p) => p.valueString ?? ''),
          })),
      }));
  }
}

/** Wrap a value object into a FHIR Parameters resource (one parameter per key). */
function parametersOf(obj: object): {
  resourceType: 'Parameters';
  parameter: { name: string; valueString: string }[];
} {
  return {
    resourceType: 'Parameters',
    parameter: Object.entries(obj).map(([name, value]) => ({
      name,
      valueString: typeof value === 'string' ? value : JSON.stringify(value),
    })),
  };
}

/** A typed FHIR Parameters.parameter entry — one of the value[x] forms the bridge accepts. */
type FhirParam =
  | { name: string; valueString: string }
  | { name: string; valueCode: string }
  | { name: string; valueDateTime: string }
  | { name: string; valueReference: { reference: string } };

/** Build a FHIR Parameters resource from typed entries (used for the authorization operations). */
function fhirParameters(parameter: FhirParam[]): { resourceType: 'Parameters'; parameter: FhirParam[] } {
  return { resourceType: 'Parameters', parameter };
}

// UI display label -> FHIR code (the §4.3.1 / Rust-enum kebab-case codes the bridge validates).
const PURPOSE_CODE: Record<GrantPurpose, string> = {
  Treatment: 'treatment',
  Payment: 'payment',
  Operations: 'operations',
  'Public health': 'public-health',
  Research: 'research',
  'AI training': 'ai-training',
  'AI inference': 'ai-inference',
  'Federal program': 'federal-program',
};
const USE_CODE: Record<UseMode, string> = {
  'Read only': 'read-only',
  'Read & rely': 'read-and-rely',
  'Read & export': 'read-and-export',
};
const SCOPE_CODE: Record<GrantScope, string> = {
  'Identity only': 'identity-only',
  'Identity + history': 'identity-history',
  'Identity (de-identified)': 'identity-deidentified',
};

// Inverse maps: FHIR code -> UI display label, for translating bridge responses back.
const CODE_TO_PURPOSE = Object.fromEntries(
  Object.entries(PURPOSE_CODE).map(([label, code]) => [code, label]),
) as Record<string, GrantPurpose>;
const CODE_TO_USE = Object.fromEntries(
  Object.entries(USE_CODE).map(([label, code]) => [code, label]),
) as Record<string, UseMode>;

/** The slice of a real FHIR R4 Provenance (the bridge's CredaProvenance projection) the UI reads. */
interface FhirProvenanceResource {
  resourceType: 'Provenance';
  id?: string;
  recorded?: string;
  target?: { reference?: string }[];
  activity?: { coding?: { code?: string }[] };
  agent?: { who?: { reference?: string } }[];
  entity?: { what?: { reference?: string } }[];
  extension?: {
    url?: string;
    extension?: { url?: string; valueCode?: string; valueString?: string; valueUnsignedInt?: number }[];
  }[];
}

// Payload-extension code -> UI display label maps (the bridge sends the Rust kebab-case
// discriminants; components render the human labels the mock fixtures established).
const VM_LABEL: Record<string, string> = {
  'government-photo-id': 'Government photo ID',
  'birth-certificate': 'Birth certificate',
  'vital-records': 'Vital records',
  'insurance-card': 'Insurance card',
  biometric: 'Biometric',
  'self-report': 'Self-reported',
  'referral-inherited': 'Referral (inherited)',
  other: 'Other',
};
const LINK_METHOD_LABEL: Record<string, NonNullable<CredaProvenance['linkMethod']>> = {
  manual: 'Manual',
  algorithmic: 'Algorithmic',
  referral: 'Referral',
  'insurance-crosswalk': 'InsuranceCrosswalk',
  other: 'Other',
};
const PURPOSE_LABEL: Record<string, string> = {
  treatment: 'Treatment',
  payment: 'Payment',
  operations: 'Operations',
  'public-health': 'Public health',
  research: 'Research',
  other: 'Other',
};

/**
 * Translate the bridge's FHIR Provenance (§8.2.3 mapping) into the UI display shape. Transport
 * owns this — components never see raw FHIR. The event-signature extension's presence means the
 * node carries a signature Core verified at ingest (§3.6).
 */
function provenanceFromFhir(res: FhirProvenanceResource): CredaProvenance {
  const sigExt = res.extension?.find((e) => e.url?.endsWith('/event-signature'));
  const algorithm = sigExt?.extension?.find((e) => e.url === 'algorithm')?.valueCode ?? 'Ed25519';
  const who = res.agent?.[0]?.who?.reference ?? '';

  // event-payload extension: the type-specific fields the clinician projection reads. Sub-
  // extensions are present only when the variant carries the field (see ProvenanceMapper).
  const payloadExt = res.extension?.find((e) => e.url?.endsWith('/event-payload'))?.extension ?? [];
  const sub = (name: string) => payloadExt.find((e) => e.url === name);
  const vmCode = sub('verificationMethod')?.valueCode;
  const linkCode = sub('linkMethod')?.valueCode;
  const purposeCode = sub('purpose')?.valueCode;
  const bps = sub('confidenceScore')?.valueUnsignedInt;

  return {
    resourceType: 'Provenance',
    id: res.id ?? '',
    recorded: res.recorded ?? '',
    target: (res.target ?? []).map((t) => ({ reference: t.reference ?? '' })),
    eventType: (res.activity?.coding?.[0]?.code ?? 'Assert') as CredaProvenance['eventType'],
    institution: who.replace('Organization/', '').slice(0, 16),
    parents: (res.entity ?? [])
      .map((e) => e.what?.reference?.replace('Provenance/', '') ?? '')
      .filter(Boolean),
    verificationMethod: vmCode ? (VM_LABEL[vmCode] ?? vmCode) : undefined,
    matchScore: typeof bps === 'number' ? `${Math.round(bps / 100)}%` : undefined,
    linkMethod: linkCode ? LINK_METHOD_LABEL[linkCode] : undefined,
    purpose: purposeCode ? (PURPOSE_LABEL[purposeCode] ?? purposeCode) : undefined,
    dateOfBirth: sub('dateOfBirth')?.valueString,
    nameFamily: sub('nameFamily')?.valueString,
    nameGiven: sub('nameGiven')?.valueString,
    summary: sub('amendmentReason')?.valueString ?? sub('contestReason')?.valueString,
    signature: sigExt ? { algorithm, verified: true } : undefined,
  };
}

/** The slice of a FHIR R4 Task (the bridge's ephemeral access-request) the UI reads. */
interface FhirTaskResource {
  resourceType: 'Task';
  id?: string;
  for?: { reference?: string };
  requester?: { display?: string };
  /** `purpose|useMode` display labels, as the bridge stored them. */
  description?: string;
}

/** Translate the bridge's FHIR Task into the UI's AccessRequest shape (transport owns the mapping). */
function taskToAccessRequest(res: FhirTaskResource): AccessRequest {
  const [purpose, use] = (res.description ?? 'Treatment|Read & rely').split('|');
  return {
    id: res.id ?? '',
    patientId: (res.for?.reference ?? '').replace('Patient/', ''),
    requester: res.requester?.display ?? 'Unknown requester',
    purpose: (CODE_TO_PURPOSE[purpose] ?? (purpose as GrantPurpose)) || 'Treatment',
    use: (CODE_TO_USE[use] ?? (use as UseMode)) || 'Read & rely',
  };
}

/** The slice of a FHIR R4 Consent (the bridge's CredaAuthorization projection) the UI reads. */
interface FhirConsentResource {
  resourceType: 'Consent';
  id?: string;
  status?: string;
  patient?: { reference?: string };
  dateTime?: string;
  provision?: {
    period?: { end?: string };
    purpose?: { code?: string }[];
    actor?: { reference?: { display?: string; identifier?: { value?: string } } }[];
    extension?: { url?: string; valueCode?: string }[];
  };
}

/**
 * Translate the bridge's FHIR Consent into the UI display model. The transport owns this mapping
 * (components never see raw FHIR). Fields the projection doesn't carry yet (scope, audienceKind)
 * fall back to the request that produced the Consent when available.
 */
function consentToAuthorization(
  res: FhirConsentResource,
  requested?: Pick<AuthorizeRequest, 'audience' | 'audienceKind' | 'purpose' | 'use' | 'scope'>,
): CredaAuthorization {
  const actorRef = res.provision?.actor?.[0]?.reference;
  const purposeCode = res.provision?.purpose?.[0]?.code;
  const useCode = res.provision?.extension?.find((e) => e.url?.endsWith('/use-mode'))?.valueCode;
  return {
    resourceType: 'Consent',
    id: res.id ?? '',
    status: res.status === 'active' ? 'active' : 'revoked',
    patient: { reference: res.patient?.reference ?? '' },
    audience: actorRef?.display ?? actorRef?.identifier?.value ?? requested?.audience ?? 'Unknown institution',
    audienceKind: requested?.audienceKind ?? 'institution',
    purpose: (purposeCode ? CODE_TO_PURPOSE[purposeCode] : undefined) ?? requested?.purpose ?? 'Treatment',
    use: (useCode ? CODE_TO_USE[useCode] : undefined) ?? requested?.use ?? 'Read only',
    scope: requested?.scope ?? 'Identity only',
    since: (res.dateTime ?? '').slice(0, 10),
    expires: res.provision?.period?.end ?? 'No expiry',
  };
}

let _bridge: FhirBridge | null = null;
let _isMock = true;

function bridgeBase(): string | undefined {
  // Priority order:
  //   1. window.__CREDA_FHIR_BASE__ — runtime-injected by the docker entrypoint's sed step.
  //      Lets one built image switch between mock and real mode based on the FHIR_BASE env
  //      var the chart passes in. If the entrypoint did not replace the placeholder (e.g.
  //      `pnpm dev` serves the raw index.html), the value will be the literal sentinel
  //      "__CREDA_FHIR_BASE_PLACEHOLDER__" and we fall through to (2).
  //   2. import.meta.env.VITE_FHIR_BASE — build-time default for `pnpm dev` / host-side work.
  const runtime = (globalThis as unknown as { __CREDA_FHIR_BASE__?: string }).__CREDA_FHIR_BASE__;
  if (runtime && runtime !== '__CREDA_FHIR_BASE_PLACEHOLDER__') return runtime;
  return (import.meta as ImportMeta & { env: { VITE_FHIR_BASE?: string } }).env.VITE_FHIR_BASE;
}

/** Singleton bridge selected from VITE_FHIR_BASE. Defaults to the in-memory mock. */
export function getBridge(): FhirBridge {
  if (_bridge) return _bridge;
  const base = bridgeBase();
  if (!base || base === 'mock') {
    _bridge = mockBridge();
    _isMock = true;
  } else {
    _bridge = new HttpBridge(base.replace(/\/$/, ''));
    _isMock = false;
  }
  return _bridge;
}

/**
 * True when the active bridge is the in-memory mock. Persona UIs use this to surface a
 * MOCK BRIDGE chip and to phrase action toasts honestly — in mock mode nothing leaves the
 * browser tab, no peer gossip happens, and the UI should say so rather than claiming
 * "propagating to peers."
 */
export function isMockBridge(): boolean {
  // getBridge() initializes _isMock as a side effect; cheap to call at every render.
  getBridge();
  return _isMock;
}

/** For tests — swap the bridge implementation. */
export function setBridge(b: FhirBridge, mock = false): void {
  _bridge = b;
  _isMock = mock;
}

/**
 * Phrase a write-action toast honestly based on the active bridge. In mock mode the action
 * never leaves the browser; in real mode it actually goes through the bridge → core → peers.
 */
export function gossipToast(action: 'Attest' | 'Contest' | 'Amend' | 'Tombstone' | 'Grant' | 'Revocation'): string {
  if (isMockBridge()) {
    return `${action} recorded · mock bridge (no peer gossip in this mode)`;
  }
  return `${action} recorded — propagating to peers`;
}
