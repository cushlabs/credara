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

export interface ContestRequest {
  /** The Provenance.id of the Link being contested. */
  linkId: string;
  reason: string;
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
      entry?: { resource: CredaProvenance }[];
    }
    const bundle = await this.req<Bundle>(
      `/Patient/${encodeURIComponent(patientId)}/$creda-provenance`,
    );
    return (bundle.entry ?? []).map((e) => e.resource);
  }

  readProvenance(id: string): Promise<CredaProvenance> {
    return this.req<CredaProvenance>(`/Provenance/${encodeURIComponent(id)}`);
  }

  attest(req: AttestRequest): Promise<CredaProvenance> {
    return this.req<CredaProvenance>(`/Patient/${encodeURIComponent(req.patientId)}/$creda-attest`, {
      method: 'POST',
      body: JSON.stringify(parametersOf({ ...req })),
    });
  }

  contest(req: ContestRequest): Promise<CredaProvenance> {
    return this.req<CredaProvenance>(`/Provenance/${encodeURIComponent(req.linkId)}/$creda-contest`, {
      method: 'POST',
      body: JSON.stringify(parametersOf({ reason: req.reason })),
    });
  }

  authorize(req: AuthorizeRequest): Promise<CredaAuthorization> {
    return this.req<CredaAuthorization>(`/Patient/${encodeURIComponent(req.patientId)}/$creda-authorize`, {
      method: 'POST',
      body: JSON.stringify(parametersOf({ ...req })),
    });
  }

  revoke(req: RevokeRequest): Promise<CredaAuthorization> {
    return this.req<CredaAuthorization>(`/Patient/${encodeURIComponent(req.patientId)}/$creda-revoke`, {
      method: 'POST',
      body: JSON.stringify(parametersOf({ grantId: req.grantId })),
    });
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
      body: JSON.stringify(parametersOf({ ...args })),
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

let _bridge: FhirBridge | null = null;
let _isMock = true;

function bridgeBase(): string | undefined {
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
