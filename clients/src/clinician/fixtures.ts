// Per-persona projection fixtures. Mirrors the patient list from
// design/clinician-review-mockup.html so the client renders with parity in mock mode.
// These are *projection* shapes that the bridge would normally derive from the DAG; we
// hold them client-side here for now and overlay just-written events from the FHIR mock.

import type { EventType } from '@shared/components/EventDag';
import type { CredaProvenance } from '@shared/fhir/types';

export type FieldOption = { inst: string; v: string; vm: string };

export interface PatientField {
  key: string;
  value?: string;
  conf?: number;
  sources?: string[];
  stale?: boolean;
  /** When two institutions disagree the field is rendered as a conflict prompt. */
  disputed?: boolean;
  options?: FieldOption[];
}

export interface Challenge {
  id: string;
  kind: 'dob' | 'link' | 'stale';
  tag: string;
  title: string;
  prompt: string;
  options: ChallengeOption[];
}

export interface ChallengeOption {
  label: string;
  /** Maps to the FHIR bridge operation; null = defer (no event). */
  eventType: 'Attest' | 'Contest' | 'Amend' | null;
  /** Steward note shown in the confirm dialog. */
  note: string;
  /**
   * The real subgraph event this action targets — set when the challenge is projected from a
   * live subgraph (Amend/Attest → the Assert id; Contest → the Link id). Absent for the static
   * fixtures, where onCommit falls back to a heuristic head/Link lookup.
   */
  targetEventId?: string;
  /** Corrected DOB token to carry on an Amend (the value being affirmed). */
  amendDob?: string;
}

export interface PatientProjection {
  id: string;
  name: string;
  dob: string;
  sex: string;
  mrns: string[];
  confidence: number;
  summary: string;
  needsReview?: boolean;
  /**
   * True when this projection is NOT backed by a live bridge read — a fixture standing in because
   * the patient isn't seeded, the token didn't resolve, or the bridge read failed. Drives the
   * DemoData chip so fixtures never silently impersonate real data (front-end de-fixturing #1).
   * `enrichWithSubgraph` sets it false when it overlays real events/identity.
   */
  demo?: boolean;
  consent: {
    state: 'granted' | 'presumed' | 'restricted' | 'expired';
    purpose?: string;
    use?: string;
    source?: string;
    expires?: string;
    requested?: boolean;
  };
  fields: PatientField[];
  /** Provenance projection — what the clinician sees in the DAG. */
  events: ProjectedEvent[];
  challenges: Challenge[];
}

export interface ProjectedEvent {
  id: string;
  type: EventType;
  inst: string;
  when: string;
  /** Verification method for Assert events. */
  vm?: string;
  /** Asserted DOB for Assert events (debug). */
  dob?: string;
  /** Match confidence for Link events ("63%"). */
  conf?: string;
  /** Purpose for Attest events. */
  purpose?: string;
  parents: string[];
  summary: string;
  /** Layout — x/y in the SVG. */
  x: number;
  y: number;
}

/** A few synthetic patients matching design/clinician-review-mockup.html. */
export const PATIENTS: PatientProjection[] = [
  {
    id: 'p1',
    name: 'Maria Gonzalez',
    dob: '1984-03-12',
    sex: 'Female',
    mrns: ['Mercy General · MRN 5582019', 'Northside Clinic · MRN A-7741'],
    confidence: 96,
    summary: 'Single consistent identity across two institutions.',
    consent: {
      state: 'granted',
      purpose: 'Treatment',
      use: 'Read & rely',
      source: 'Explicit patient grant',
      expires: 'No expiry',
    },
    fields: [
      { key: 'Legal name', value: 'Maria Elena Gonzalez', conf: 97, sources: ['Mercy General', 'Northside Clinic'] },
      { key: 'Date of birth', value: '1984-03-12', conf: 98, sources: ['Mercy General', 'Northside Clinic'] },
      { key: 'Sex', value: 'Female', conf: 99, sources: ['Mercy General'] },
      { key: 'Address', value: '418 Larkspur Ave, Madison', conf: 88, sources: ['Northside Clinic'] },
    ],
    events: [
      { id: 'e1', type: 'Assert', inst: 'Mercy General', when: '2021-06-02', vm: 'Government photo ID', x: 120, y: 70, parents: [], summary: 'Initial registration at Mercy General.' },
      { id: 'e2', type: 'Assert', inst: 'Northside Clinic', when: '2022-11-18', vm: 'Insurance card', x: 120, y: 230, parents: [], summary: 'Registration at Northside Clinic.' },
      { id: 'e3', type: 'Link', inst: 'Mercy General', when: '2022-11-20', conf: '94%', x: 360, y: 150, parents: ['e1', 'e2'], summary: 'Algorithmic match linked the two records.' },
      { id: 'e4', type: 'Attest', inst: 'Mercy General', when: '2023-02-10', purpose: 'Treatment', x: 600, y: 150, parents: ['e3'], summary: 'Treatment reliance recorded.' },
    ],
    challenges: [],
  },
  {
    id: 'p2',
    name: 'James Whitfield',
    dob: '—',
    sex: 'Male',
    mrns: ['Mercy General · MRN 6610042', 'Lakeside Hospital · MRN LH-3098'],
    confidence: 64,
    summary: 'Two institutions disagree on date of birth — needs point-of-care confirmation.',
    needsReview: true,
    consent: { state: 'presumed', purpose: 'Treatment', use: 'Read & rely', source: 'HIPAA treatment posture — no patient restriction', expires: '—' },
    fields: [
      { key: 'Legal name', value: 'James R. Whitfield', conf: 93, sources: ['Mercy General', 'Lakeside Hospital'] },
      {
        key: 'Date of birth',
        disputed: true,
        options: [
          { inst: 'Mercy General', v: '1971-08-04', vm: 'Government photo ID' },
          { inst: 'Lakeside Hospital', v: '1971-08-14', vm: 'Self-reported' },
        ],
      },
      { key: 'Sex', value: 'Male', conf: 99, sources: ['Mercy General', 'Lakeside Hospital'] },
      { key: 'Address', value: '92 Birchwood Ct, Franklin', conf: 81, sources: ['Mercy General'] },
    ],
    events: [
      { id: 'e1', type: 'Assert', inst: 'Mercy General', when: '2020-01-09', vm: 'Government photo ID', dob: '1971-08-04', x: 120, y: 70, parents: [], summary: 'DOB 1971-08-04 (photo ID).' },
      { id: 'e2', type: 'Assert', inst: 'Lakeside Hospital', when: '2023-07-22', vm: 'Self-reported', dob: '1971-08-14', x: 120, y: 230, parents: [], summary: 'DOB 1971-08-14, self-reported at intake.' },
      { id: 'e3', type: 'Link', inst: 'Lakeside Hospital', when: '2023-07-23', conf: '82%', x: 360, y: 150, parents: ['e1', 'e2'], summary: 'Records linked despite DOB mismatch.' },
    ],
    challenges: [
      {
        id: 'c1',
        kind: 'dob',
        tag: 'Conflicting DOB',
        title: 'Which date of birth matches the patient in front of you?',
        prompt:
          'Mercy General has 1971-08-04 (photo ID). Lakeside Hospital has 1971-08-14 (self-reported). Confirm against the patient or their ID.',
        options: [
          { label: '1971-08-04 is correct', eventType: 'Attest', note: 'Records a treatment-purpose attestation affirming the photo-ID-verified DOB.' },
          { label: '1971-08-14 is correct', eventType: 'Amend', note: 'Requests an amendment so the effective DOB reflects 1971-08-14.' },
          { label: 'Neither / unsure', eventType: 'Contest', note: 'Flags the demographic conflict for identity review without asserting a value.' },
        ],
      },
    ],
  },
  {
    id: 'p3',
    name: 'Robert Chen',
    dob: '1990-05-27',
    sex: 'Male',
    mrns: ['Mercy General · MRN 7700318', 'Eastgate Urgent Care · MRN EG-1188'],
    confidence: 71,
    summary: 'A low-confidence link suggests these may be the same person — confirm before relying.',
    needsReview: true,
    consent: { state: 'restricted', purpose: 'Treatment', use: '—', source: 'Patient revoked access for Mercy General', expires: '—' },
    fields: [
      { key: 'Legal name', value: 'Robert Chen / "Bob" Chen', conf: 70, sources: ['Mercy General', 'Eastgate Urgent Care'] },
      { key: 'Date of birth', value: '1990-05-27', conf: 90, sources: ['Mercy General', 'Eastgate Urgent Care'] },
      { key: 'Sex', value: 'Male', conf: 99, sources: ['Mercy General'] },
      { key: 'Address', value: '2 records differ (Madison / Clinton)', conf: 55, sources: ['Mercy General', 'Eastgate Urgent Care'] },
    ],
    events: [
      { id: 'e1', type: 'Assert', inst: 'Mercy General', when: '2019-09-14', vm: 'Government photo ID', x: 120, y: 70, parents: [], summary: 'Robert Chen, Madison address.' },
      { id: 'e2', type: 'Assert', inst: 'Eastgate Urgent Care', when: '2024-03-30', vm: 'Self-reported', x: 120, y: 230, parents: [], summary: '"Bob" Chen, Clinton address.' },
      { id: 'e3', type: 'Link', inst: 'Eastgate Urgent Care', when: '2024-03-30', conf: '63%', x: 360, y: 150, parents: ['e1', 'e2'], summary: 'Probabilistic match (low confidence).' },
    ],
    challenges: [
      {
        id: 'c1',
        kind: 'link',
        tag: 'Possible duplicate',
        title: 'Are these the same person?',
        prompt:
          'A 63% match links a Mercy General record (Robert Chen, Madison) with an Eastgate record ("Bob" Chen, Clinton). Confirm identity with the patient before relying on the merged history.',
        options: [
          { label: 'Yes — same person', eventType: 'Attest', note: 'Records a treatment attestation affirming the link; downstream confidence rises.' },
          { label: 'No — different people', eventType: 'Contest', note: 'Contests the link; the projection severs it.' },
          { label: 'Need more info', eventType: null, note: 'Leaves the link unresolved and routes to the identity team. No event is written.' },
        ],
      },
    ],
  },
  {
    id: 'p4',
    name: 'Eleanor Petrova',
    dob: '1949-12-01',
    sex: 'Female',
    mrns: ['Mercy General · MRN 4471290'],
    confidence: 79,
    summary: 'Identity is consistent but verification is 4+ years old.',
    needsReview: true,
    consent: { state: 'expired', purpose: 'Treatment', use: 'Read & rely', source: 'Patient grant expired 2024-08-15', expires: '2024-08-15' },
    fields: [
      { key: 'Legal name', value: 'Eleanor Petrova', conf: 88, sources: ['Mercy General'] },
      { key: 'Date of birth', value: '1949-12-01', conf: 84, sources: ['Mercy General'] },
      { key: 'Sex', value: 'Female', conf: 99, sources: ['Mercy General'] },
      { key: 'Address', value: '17 Cedar Hollow, Salem', conf: 62, sources: ['Mercy General'], stale: true },
    ],
    events: [
      { id: 'e1', type: 'Assert', inst: 'Mercy General', when: '2019-08-15', vm: 'Birth certificate', x: 120, y: 150, parents: [], summary: 'Verified by birth certificate in 2019; not re-verified since.' },
    ],
    challenges: [
      {
        id: 'c1',
        kind: 'stale',
        tag: 'Stale verification',
        title: 'Re-verify identity?',
        prompt: 'The last verification event is from 2019-08-15 (over 4 years ago). Confirm the patient’s identity to refresh confidence.',
        options: [
          { label: 'Re-confirm now', eventType: 'Attest', note: 'Records a fresh attestation, refreshing the confidence decay clock.' },
          { label: 'Address has changed', eventType: 'Amend', note: 'Requests an amendment to update the address on record.' },
        ],
      },
    ],
  },
];

/** Used by the action log to show recently-recorded events alongside the projection ones. */
export interface ActionLogEntry {
  eventType: 'Attest' | 'Contest' | 'Amend';
  summary: string;
  when: string;
  /** The CredaProvenance returned by the bridge — kept so the receipt can verify the signature. */
  receipt: CredaProvenance | null;
}
