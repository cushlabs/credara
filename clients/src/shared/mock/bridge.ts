// In-memory FHIR bridge — the offline-dev / testbed adapter behind getBridge().
//
// Shape-for-shape compatible with the real bridge's responses. Data is seeded from the
// fixtures below (derived from the mockup fixtures so the visual parity tests have something
// real to render). Mutating operations (attest/contest/authorize/revoke) update the in-memory
// store and emit a new CredaProvenance / CredaAuthorization just as the bridge would.

import type {
  AuthorizationDecision,
  CredaAuthorization,
  CredaEventType,
  CredaProvenance,
  GrantPurpose,
  GrantScope,
  UseMode,
} from '../fhir/types';
import type {
  AccessRequest,
  AmendRequest,
  AttestRequest,
  AuthorizeRequest,
  ContestRequest,
  EffectiveField,
  FhirBridge,
  RevokeRequest,
} from '../fhir/client';

let counter = 1000;
function nextId(prefix: string): string {
  counter += 1;
  return `${prefix}-${counter.toString(16)}`;
}

const initialProvenance: CredaProvenance[] = [
  // Maria Gonzalez (p1) — single consistent identity
  prov('p1-a1', 'p1', 'Assert', 'Mercy General', { verificationMethod: 'Government photo ID', dateOfBirth: '1984-03-12', nameFamily: 'gonzalez', nameGiven: 'maria', parents: [], summary: 'Initial registration at Mercy General.', recorded: '2021-06-02T00:00:00Z' }),
  prov('p1-a2', 'p1', 'Assert', 'Northside Clinic', { verificationMethod: 'Insurance card', dateOfBirth: '1984-03-12', nameFamily: 'gonzalez', nameGiven: 'maria', parents: [], summary: 'Registration at Northside Clinic.', recorded: '2022-11-18T00:00:00Z' }),
  prov('p1-l1', 'p1', 'Link', 'Mercy General', { matchScore: '94%', linkMethod: 'Algorithmic', parents: ['p1-a1', 'p1-a2'], summary: 'Algorithmic match linked the two records.', recorded: '2022-11-20T00:00:00Z' }),
  prov('p1-at1', 'p1', 'Attest', 'Mercy General', { purpose: 'Treatment', parents: ['p1-l1'], summary: 'Treatment reliance recorded.', recorded: '2023-02-10T00:00:00Z' }),

  // James Whitfield (p2) — DOB conflict
  prov('p2-a1', 'p2', 'Assert', 'Mercy General', { verificationMethod: 'Government photo ID', dateOfBirth: '1971-08-04', nameFamily: 'whitfield', nameGiven: 'james', parents: [], summary: 'DOB 1971-08-04 (photo ID).', recorded: '2020-01-09T00:00:00Z' }),
  prov('p2-a2', 'p2', 'Assert', 'Lakeside Hospital', { verificationMethod: 'Self-report', dateOfBirth: '1971-08-14', nameFamily: 'whitfield', nameGiven: 'james', parents: [], summary: 'DOB 1971-08-14 (self-reported).', recorded: '2023-07-22T00:00:00Z' }),
  prov('p2-l1', 'p2', 'Link', 'Lakeside Hospital', { matchScore: '82%', linkMethod: 'Algorithmic', parents: ['p2-a1', 'p2-a2'], summary: 'Records linked despite DOB mismatch.', recorded: '2023-07-23T00:00:00Z' }),

  // Robert Chen (p3) — possible duplicate
  prov('p3-a1', 'p3', 'Assert', 'Mercy General', { verificationMethod: 'Government photo ID', dateOfBirth: '1990-05-27', nameFamily: 'chen', nameGiven: 'robert', parents: [], summary: 'Robert Chen, Madison address.', recorded: '2019-09-14T00:00:00Z' }),
  prov('p3-a2', 'p3', 'Assert', 'Eastgate Urgent Care', { verificationMethod: 'Self-report', dateOfBirth: '1990-05-27', nameFamily: 'chen', nameGiven: 'robert', parents: [], summary: '"Bob" Chen, Clinton address.', recorded: '2024-03-30T00:00:00Z' }),
  prov('p3-l1', 'p3', 'Link', 'Eastgate Urgent Care', { matchScore: '63%', linkMethod: 'Algorithmic', parents: ['p3-a1', 'p3-a2'], summary: 'Probabilistic match (low confidence).', recorded: '2024-03-30T00:00:00Z' }),

  // Eleanor Petrova (p4) — stale verification
  prov('p4-a1', 'p4', 'Assert', 'Mercy General', { verificationMethod: 'Birth certificate', dateOfBirth: '1949-12-01', nameFamily: 'petrova', nameGiven: 'eleanor', parents: [], summary: 'Verified by birth certificate in 2019.', recorded: '2019-08-15T00:00:00Z' }),
];

const initialAuth: CredaAuthorization[] = [
  auth('g-mercy', 'p1', 'Mercy General Hospital', 'institution', 'Treatment', 'Read & rely', 'Identity + history', '2023-02-10', 'No expiry', 'active'),
  auth('g-northside', 'p1', 'Northside Clinic', 'institution', 'Treatment', 'Read only', 'Identity only', '2024-06-01', '2025-06-01', 'active'),
  auth('g-apex', 'p1', 'Apex Research — study NCT-1142', 'institution', 'Research', 'Read & export', 'Identity (de-identified)', '2024-09-15', '2026-09-15', 'active'),
  auth('g-qhin', 'p1', 'Any TEFCA QHIN', 'class', 'Treatment', 'Read & rely', 'Identity only', '2023-01-01', 'No expiry', 'active'),
];

function prov(
  id: string,
  patientId: string,
  eventType: CredaEventType,
  institution: string,
  rest: {
    verificationMethod?: string;
    matchScore?: string;
    linkMethod?: CredaProvenance['linkMethod'];
    purpose?: string;
    dateOfBirth?: string;
    nameFamily?: string;
    nameGiven?: string;
    parents: string[];
    summary?: string;
    recorded: string;
  },
): CredaProvenance {
  return {
    resourceType: 'Provenance',
    id,
    recorded: rest.recorded,
    target: [{ reference: `Patient/${patientId}` }],
    eventType,
    institution,
    verificationMethod: rest.verificationMethod,
    matchScore: rest.matchScore,
    linkMethod: rest.linkMethod,
    purpose: rest.purpose,
    dateOfBirth: rest.dateOfBirth,
    nameFamily: rest.nameFamily,
    nameGiven: rest.nameGiven,
    parents: rest.parents,
    summary: rest.summary,
    signature: { algorithm: 'ed25519', verified: true },
  };
}

function auth(
  id: string,
  patientId: string,
  audience: string,
  audienceKind: CredaAuthorization['audienceKind'],
  purpose: GrantPurpose,
  use: UseMode,
  scope: GrantScope,
  since: string,
  expires: string,
  status: CredaAuthorization['status'],
): CredaAuthorization {
  return {
    resourceType: 'Consent',
    id,
    status,
    patient: { reference: `Patient/${patientId}` },
    audience,
    audienceKind,
    purpose,
    use,
    scope,
    since,
    expires,
    signedBy: patientId === 'p1' ? 'Maria Gonzalez' : 'patient',
  };
}

export function mockBridge(): FhirBridge {
  const provenance: CredaProvenance[] = [...initialProvenance];
  const authorizations: CredaAuthorization[] = [...initialAuth];
  // Ephemeral access-request inbox (mirrors the bridge's in-memory Task store).
  const accessRequests: AccessRequest[] = [];

  const delay = <T>(v: T): Promise<T> =>
    new Promise((resolve) => {
      // 30 ms emulates a local bridge round-trip — long enough for the toasts and pending
      // states to be visible, short enough not to feel laggy in dev.
      window.setTimeout(() => resolve(v), 30);
    });

  return {
    async searchPatientsByToken(tokens: string[]): Promise<string[]> {
      // Mirror the real bridge's MatchByTokens: a family token resolves to that patient's id.
      // Demo families map to the fixture ids so one resolution path serves both modes; an
      // empty/unknown query returns the full set (the patient app passes a single family token).
      const FAMILY_TO_ID: Record<string, string> = {
        'tok:demo:gonzalez': 'p1',
        'tok:demo:whitfield': 'p2',
        'tok:demo:chen': 'p3',
        'tok:demo:petrova': 'p4',
      };
      const matched = tokens.map((t) => FAMILY_TO_ID[t]).filter(Boolean);
      return delay(matched.length ? matched : ['p1', 'p2', 'p3', 'p4']);
    },

    async readPatient(id) {
      return delay({ resourceType: 'Patient', id });
    },

    async readSubgraph(patientId) {
      return delay(provenance.filter((p) => p.target.some((t) => t.reference === `Patient/${patientId}`)));
    },

    async readProvenance(id) {
      const found = provenance.find((p) => p.id === id);
      if (!found) throw new Error(`Provenance/${id} not found`);
      return delay(found);
    },

    async attest(req: AttestRequest) {
      const ev = prov(
        nextId('a'),
        req.patientId,
        'Attest',
        'Mercy General (you)',
        {
          purpose: req.purpose,
          parents: req.references,
          summary: req.summary,
          recorded: new Date().toISOString(),
        },
      );
      provenance.push(ev);
      return delay(ev);
    },

    async contest(req: ContestRequest) {
      const ev = prov(nextId('c'), 'unknown', 'Contest', 'Mercy General (you)', {
        parents: [req.linkId],
        summary: req.detail ? `${req.code}: ${req.detail}` : req.code,
        recorded: new Date().toISOString(),
      });
      provenance.push(ev);
      return delay(ev);
    },

    async amend(req: AmendRequest) {
      const ev = prov(nextId('am'), req.patientId, 'Amend', 'Mercy General (you)', {
        parents: [req.targetEventId],
        summary: `DOB amended to ${req.dateOfBirth}: ${req.reason}`,
        recorded: new Date().toISOString(),
      });
      provenance.push(ev);
      return delay(ev);
    },

    async authorize(req: AuthorizeRequest) {
      const a = auth(
        nextId('g'),
        req.patientId,
        req.audience,
        req.audienceKind,
        req.purpose,
        req.use,
        req.scope,
        new Date().toISOString().slice(0, 10),
        req.expires,
        'active',
      );
      authorizations.unshift(a);
      provenance.push(
        prov(nextId('gp'), req.patientId, 'AuthorizationGrant', 'patient', {
          parents: [],
          summary: `Granted ${req.purpose} access to ${req.audience}.`,
          recorded: new Date().toISOString(),
        }),
      );
      return delay(a);
    },

    async revoke(req: RevokeRequest) {
      const grant = authorizations.find((a) => a.id === req.grantId);
      if (!grant) throw new Error(`Consent/${req.grantId} not found`);
      grant.status = 'revoked';
      provenance.push(
        prov(nextId('rv'), req.patientId, 'AuthorizationRevocation', 'patient', {
          parents: [],
          summary: `Stopped sharing with ${grant.audience}.`,
          recorded: new Date().toISOString(),
        }),
      );
      return delay(grant);
    },

    async verifyAuthorization({ patientId, requester, purpose }): Promise<AuthorizationDecision> {
      const grant = authorizations.find(
        (a) =>
          a.patient.reference === `Patient/${patientId}` &&
          a.audience === requester &&
          a.purpose === purpose &&
          a.status === 'active',
      );
      if (grant) return delay({ decision: 'authorized', reason: 'covered by active grant', governingGrant: grant.id });
      return delay({ decision: 'denied-no-grant', reason: 'no active grant covers this requester+purpose' });
    },

    async listAuthorizationEvents() {
      return delay(
        provenance.filter((p) =>
          (['AuthorizationGrant', 'AuthorizationRevocation', 'ExportReceipt'] as CredaEventType[]).includes(p.eventType),
        ),
      );
    },

    async listAuthorizations(patientId: string) {
      // Mirrors the real bridge's `Consent?patient={id}` search over the in-memory state.
      return delay(authorizations.filter((a) => a.patient.reference === `Patient/${patientId}`));
    },

    async listInstitutions() {
      // Mirrors the real bridge's `GET /Organization`: distinct institution audiences seen in
      // grants across the in-memory state.
      const names = Array.from(
        new Set(
          authorizations
            .filter((a) => a.audienceKind === 'institution')
            .map((a) => a.audience),
        ),
      ).sort();
      return delay(names);
    },

    async requestAccess(req) {
      // Mirrors the bridge's ephemeral Task inbox over in-memory state.
      const created = { id: nextId('req'), patientId: req.patientId, requester: req.requester, purpose: req.purpose, use: req.use };
      accessRequests.push(created);
      return delay(created);
    },

    async listAccessRequests(patientId: string) {
      return delay(accessRequests.filter((r) => r.patientId === patientId));
    },

    async resolveAccessRequest(id: string) {
      const i = accessRequests.findIndex((r) => r.id === id);
      if (i >= 0) accessRequests.splice(i, 1);
      return delay(undefined);
    },

    async effectiveIdentity(patientId: string): Promise<EffectiveField[]> {
      // Mirrors Core's per-field projection over the mock asserts. James (p2) carries the DOB
      // disagreement (photo-ID 08-04 outweighs self-report 08-14); others resolve cleanly.
      const dob = (value: string, confidence: number, supporting: string[]) => ({ value, confidence, supporting });
      const byPatient: Record<string, EffectiveField[]> = {
        p1: [{ key: 'date-of-birth', disputed: false, values: [dob('tok:demo:1984-03-12', 9800, ['p1-a1', 'p1-a2'])] }],
        p2: [{
          key: 'date-of-birth',
          disputed: true,
          values: [dob('tok:demo:1971-08-04', 9000, ['p2-a1']), dob('tok:demo:1971-08-14', 6000, ['p2-a2'])],
        }],
      };
      return delay(byPatient[patientId] ?? []);
    },
  };
}
