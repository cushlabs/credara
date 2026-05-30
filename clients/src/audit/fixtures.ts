// Audit ledger fixtures — ported from design/compliance-audit-mockup.html. These describe
// the *enriched* audit-view shape, not raw FHIR; the bridge's $creda-audit-stream operation
// will return this shape once implemented.

export type AuditType = 'grant' | 'revoke' | 'export' | 'linkdecision';

export interface AuditFinding {
  level: 'pass' | 'warn' | 'violation';
  title: string;
  note: string;
  meta: string;
}

export type ChainEntryType = 'Assert' | 'Link' | 'Grant' | 'Revocation' | 'Export' | 'Attest';

export interface ChainEntry {
  type: ChainEntryType;
  label?: string;
  blocked?: boolean;
  inert?: boolean;
}

export interface LinkChainStep {
  from: string;
  to: string;
  method: 'InsuranceCrosswalk' | 'Referral' | 'Algorithmic' | 'Manual' | 'Other';
  claimed: number;
  status: 'pass' | 'fail';
  reason: string;
}

export interface AuditEvent {
  id: string;
  type: AuditType;
  who: string;
  patientToken: string;
  purpose: string;
  when: string;
  requester: string;
  grant: string;
  scope: string;
  chain: (ChainEntryType | ChainEntry)[];
  intact: boolean;
  linkChain?: LinkChainStep[];
  decision?: 'admitted' | 'denied';
  finding?: AuditFinding;
}

export const LINK_POLICY = {
  posture: 'Deny-by-default',
  min_link_confidence: 6000,
  require_author_standing: false,
  ceilings: {
    InsuranceCrosswalk: 9500,
    Referral: 9000,
    Algorithmic: 7000,
    Manual: 5000,
    Other: 3000,
  } as const,
};

export function effectiveConfidence(method: LinkChainStep['method'], claimed: number): number {
  return Math.min(claimed, LINK_POLICY.ceilings[method] ?? LINK_POLICY.ceilings.Other);
}

export const AUDIT_EVENTS: AuditEvent[] = [
  {
    id: 'x1',
    type: 'export',
    who: 'Apex Research',
    patientToken: 'tok:7f3a…c2',
    purpose: 'Research',
    when: '2025-01-14 09:42',
    requester: 'Apex Research — study NCT-1142',
    grant: 'g-apex (revoked 2025-01-14 05:10)',
    scope: 'Identity (de-identified)',
    chain: ['Assert', 'Link', 'Grant', 'Revocation', 'Export'],
    intact: true,
    finding: {
      level: 'violation',
      title: 'Export after revocation',
      note: 'An ExportReceipt was recorded ~4.5 h after the governing grant was revoked by the patient. The export should have been denied at the Export Gate.',
      meta: 'Governing grant g-apex revoked 05:10 · export 09:42 · §4.6 step 2',
    },
  },
  {
    id: 'r1',
    type: 'revoke',
    who: 'Northside Clinic',
    patientToken: 'tok:91b0…7e',
    purpose: 'Treatment',
    when: '2025-01-13 18:20',
    requester: '—',
    grant: 'g-northside',
    scope: 'Identity only',
    chain: ['Grant', 'Revocation'],
    intact: true,
    finding: {
      level: 'warn',
      title: 'Revocation propagation latency',
      note: 'One peer enforced this revocation 9.2 s after publication, exceeding the Bound-1 target. Within tolerance but worth monitoring.',
      meta: 'Target ≈ 5 s · observed 9.2 s · §4.7',
    },
  },
  {
    id: 'x2',
    type: 'export',
    who: 'Lakeside Hospital',
    patientToken: 'tok:5cc1…a9',
    purpose: 'Treatment',
    when: '2025-01-13 14:05',
    requester: 'Lakeside Hospital',
    grant: 'g-lakeside (expired 2025-01-01)',
    scope: 'Identity + history',
    chain: ['Assert', 'Grant', 'Export'],
    intact: true,
    finding: {
      level: 'violation',
      title: 'Export under expired grant',
      note: 'The governing grant expired 12 days before this export. The Export Gate should have refused it on the expiration check.',
      meta: 'Grant expired 2025-01-01 · export 2025-01-13 · §4.6 step 4',
    },
  },
  {
    id: 'x3',
    type: 'export',
    who: 'Mercy General Hospital',
    patientToken: 'tok:7f3a…c2',
    purpose: 'Treatment',
    when: '2025-01-13 11:31',
    requester: 'Mercy General Hospital',
    grant: 'g-mercy (active)',
    scope: 'Identity + history',
    chain: ['Assert', 'Link', 'Grant', 'Export'],
    intact: true,
    finding: {
      level: 'pass',
      title: 'Authorized · dual control intact',
      note: 'Within an active grant for the stated purpose. Matching ExportReceipt at the source and an independent Verifier check at the relying party.',
      meta: '§4.5 · §4.6',
    },
  },
  {
    id: 'g1',
    type: 'grant',
    who: 'Mercy General Hospital',
    patientToken: 'tok:7f3a…c2',
    purpose: 'Treatment',
    when: '2023-02-10 08:00',
    requester: '—',
    grant: 'g-mercy',
    scope: 'Identity + history',
    chain: ['Assert', 'Grant'],
    intact: true,
    finding: {
      level: 'pass',
      title: 'Patient-signed grant',
      note: 'Signed by the patient key, bound to the patient subgraph (non-transferable). Purpose and scope well-formed.',
      meta: '§4.3',
    },
  },
  {
    id: 'ld1',
    type: 'linkdecision',
    who: 'BluePeak Family Care',
    patientToken: 'tok:7f3a…c2',
    purpose: 'Treatment',
    when: '2025-01-13 17:08',
    requester: 'BluePeak Family Care',
    grant: 'g-bluepeak (self-issued)',
    scope: 'Identity + history',
    chain: [
      { type: 'Assert', label: 'Assert(BluePeak)' },
      { type: 'Link', label: 'Link(Manual)', blocked: true },
      { type: 'Grant', label: 'Grant(self)', inert: true },
      { type: 'Revocation', label: 'Request blocked' },
    ],
    intact: true,
    linkChain: [
      { from: 'BluePeak Assert', to: 'Mercy Assert (anchor)', method: 'Manual', claimed: 9800, status: 'fail', reason: 'effective 5000 < floor 6000' },
    ],
    decision: 'denied',
    finding: {
      level: 'pass',
      title: 'Step 5.5 blocked self-issued Grant via Manual link',
      note: 'BluePeak (no prior relationship to Mercy or the patient) published a Manual Link at 9800/10000 confidence and a self-issued Grant. Effective confidence after method ceiling was 5000, below the 6000 floor. Authorization denied. No data was released.',
      meta: '§4.6 step 5.5 · Manual ceiling 5000 · floor 6000',
    },
  },
  {
    id: 'ld2',
    type: 'linkdecision',
    who: 'Lakeside Hospital',
    patientToken: 'tok:5cc1…a9',
    purpose: 'Treatment',
    when: '2025-01-13 14:00',
    requester: 'Lakeside Hospital',
    grant: 'g-lakeside',
    scope: 'Identity only',
    chain: [
      { type: 'Assert', label: 'Assert(Lakeside)' },
      { type: 'Link', label: 'Link(InsuranceCrosswalk)' },
      { type: 'Grant', label: 'Grant' },
      { type: 'Export', label: 'Access granted' },
    ],
    intact: true,
    linkChain: [
      { from: 'Lakeside Assert', to: 'Mercy Assert (anchor)', method: 'InsuranceCrosswalk', claimed: 9000, status: 'pass', reason: 'effective 9000 ≥ floor 6000' },
    ],
    decision: 'admitted',
    finding: {
      level: 'pass',
      title: 'Step 5.5 admitted InsuranceCrosswalk link',
      note: 'Lakeside used the InsuranceCrosswalk method, which caps at 9500. Effective confidence 9000 cleared the 6000 floor.',
      meta: '§4.6 step 5.5 · InsuranceCrosswalk ceiling 9500 · floor 6000',
    },
  },
];
