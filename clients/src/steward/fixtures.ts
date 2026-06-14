// Steward / operator console fixtures — ported from design/steward-console-mockup.html.
// Each case is a triage item: a possible duplicate, demographic conflict, contest, blocked
// link, or synthetic record. The steward acts via signed graph events (Link / Contest /
// Amend / Tombstone) — each option here maps to a bridge `$creda-*` operation.

import type { EventType } from '@shared/components/EventDag';
import type { ContestReasonCode } from '@shared/fhir/types';

export type CaseKind = 'duplicate' | 'conflict' | 'contest' | 'synthetic' | 'stale' | 'blockedLink';

export interface ConsentNote {
  state: 'permitted' | 'presumed' | 'restricted' | 'revoked' | 'na';
  note: string;
}

export interface CmpRow {
  key: string;
  a: string;
  b: string;
  agree: 'match' | 'partial' | 'conflict';
}

export interface EvidenceRow {
  k: string;
  wt: string | 'note';
  sign: 'pos' | 'neg' | '';
  desc: string;
}

export interface CaseEvent {
  id: string;
  type: EventType;
  inst: string;
  when: string;
  vm?: string;
  conf?: string;
  method?: LinkMethod;
  x: number;
  y: number;
  parents: string[];
  summary: string;
  /** Steward-just-added marker — gets a dashed light border. */
  fresh?: boolean;
}

export type LinkMethod = 'InsuranceCrosswalk' | 'Referral' | 'Algorithmic' | 'Manual' | 'Other';

export interface LinkChainStep {
  linkId: string;
  from: string;
  to: string;
  method: LinkMethod;
  claimed: number;
  status: 'pass' | 'fail';
  reason: string;
}

export interface CaseAction {
  label: string;
  /** Visual style for the button. */
  cls: 'attest' | 'contest' | 'amend' | 'tomb' | 'ghost';
  /** Maps to the bridge operation. null = no graph change (defer / out-of-band). */
  ev: 'Attest' | 'Contest' | 'Amend' | 'Tombstone' | 'Link' | null;
  note: string;
  /** ContestReason.code carried on a Contest (§3.4.3). Defaults to 'other' if unset. */
  contestCode?: ContestReasonCode;
}

export interface StewardCase {
  id: string;
  kind: CaseKind;
  conf: number;
  testData: boolean;
  title: string;
  summary: string;
  insts: string[];
  consent: ConsentNote;
  cmp: CmpRow[];
  evidence: EvidenceRow[];
  linkChain?: LinkChainStep[];
  events: CaseEvent[];
  actions: CaseAction[];
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

export function effectiveConfidence(method: LinkMethod | undefined, claimed: number): number {
  if (!method) return claimed;
  return Math.min(claimed, LINK_POLICY.ceilings[method] ?? LINK_POLICY.ceilings.Other);
}

export function parseClaim(c: string | undefined | null): number {
  if (typeof c === 'number') return c;
  if (typeof c !== 'string') return 0;
  if (c.endsWith('%')) return Math.round(parseFloat(c) * 100);
  return parseInt(c, 10) || 0;
}

export const KIND_META: Record<CaseKind, { tag: string; cls: string }> = {
  duplicate: { tag: 'Possible duplicate', cls: 'b-dup' },
  conflict: { tag: 'Demographic conflict', cls: 'b-conflict' },
  contest: { tag: 'Open contest', cls: 'b-contest' },
  synthetic: { tag: 'Synthetic record', cls: 'b-synthetic' },
  stale: { tag: 'Stale verification', cls: 'b-stale' },
  blockedLink: { tag: 'Link blocked by policy', cls: 'b-blocked' },
};

export interface ConsentVisual {
  bg: string;
  fg: string;
  dot: string;
  label: string;
}

export function consentMeta(c: ConsentNote): ConsentVisual {
  const M: Record<ConsentNote['state'], ConsentVisual> = {
    permitted: { bg: '#e7f6ec', fg: '#15803d', dot: '#15803d', label: 'Patient permits linking' },
    presumed: { bg: '#e7f6ec', fg: '#15803d', dot: '#15803d', label: 'Operations posture — no restriction' },
    restricted: { bg: '#fdf1e3', fg: '#b45309', dot: '#b45309', label: 'Patient restricted access' },
    revoked: { bg: '#fde7e7', fg: '#b91c1c', dot: '#b91c1c', label: 'Patient revoked access' },
    na: { bg: '#eef2f6', fg: '#475569', dot: '#7c8aa0', label: 'Synthetic — no consent' },
  };
  return M[c.state];
}

export const CASES: StewardCase[] = [
  {
    id: 'c1',
    kind: 'duplicate',
    conf: 63,
    testData: false,
    title: 'Robert Chen / "Bob" Chen',
    summary:
      'A 63% probabilistic match links a Mercy General record with an Eastgate Urgent Care record — same DOB and similar name, but different city. Below the auto-merge threshold; needs a human decision.',
    insts: ['Mercy General', 'Eastgate Urgent Care'],
    consent: {
      state: 'revoked',
      note:
        'The patient has revoked Mercy General’s access. Resolving this duplicate is advisory: confirming the link does not restore access the patient withdrew.',
    },
    cmp: [
      { key: 'Name', a: 'Robert Chen', b: '"Bob" Chen', agree: 'partial' },
      { key: 'Date of birth', a: '1990-05-27', b: '1990-05-27', agree: 'match' },
      { key: 'Sex', a: 'Male', b: 'Male', agree: 'match' },
      { key: 'Address', a: 'Madison', b: 'Clinton', agree: 'conflict' },
    ],
    evidence: [
      { k: 'DOB', wt: '+4.1', sign: 'pos', desc: 'exact match on a high-entropy field' },
      { k: 'Name', wt: '+1.2', sign: 'pos', desc: 'given-name nickname, family match' },
      { k: 'Sex', wt: '+0.3', sign: 'pos', desc: 'low-entropy agreement' },
      { k: 'Address', wt: '−2.0', sign: 'neg', desc: 'different city — evidence against' },
    ],
    events: [
      { id: 'a1', type: 'Assert', inst: 'Mercy General', when: '2019-09-14', vm: 'Government photo ID', x: 70, y: 55, parents: [], summary: 'Robert Chen, Madison address.' },
      { id: 'a2', type: 'Assert', inst: 'Eastgate Urgent Care', when: '2024-03-30', vm: 'Self-report', x: 70, y: 205, parents: [], summary: '"Bob" Chen, Clinton address.' },
      { id: 'l1', type: 'Link', inst: 'Eastgate Urgent Care', when: '2024-03-30', conf: '63%', method: 'Algorithmic', x: 320, y: 130, parents: ['a1', 'a2'], summary: 'Probabilistic match; low confidence.' },
    ],
    actions: [
      { label: 'Confirm — same person', cls: 'attest', ev: 'Attest', note: 'Records a treatment-purpose attestation affirming the link.' },
      { label: 'Reject — different people', cls: 'contest', ev: 'Contest', note: 'Contests the link. The projection severs it.', contestCode: 'distinct-patients' },
      { label: 'Defer', cls: 'ghost', ev: null, note: 'No event written; the case stays open for more evidence.' },
    ],
  },
  {
    id: 'c2',
    kind: 'conflict',
    conf: 48,
    testData: false,
    title: 'James R. Whitfield — conflicting DOB',
    summary: 'Two institutions assert different dates of birth ten days apart. Mercy General verified by photo ID; Lakeside is self-reported.',
    insts: ['Mercy General', 'Lakeside Hospital'],
    consent: { state: 'presumed', note: 'Treatment-presumed posture — the patient has placed no restriction on cross-institution use.' },
    cmp: [
      { key: 'Name', a: 'James R. Whitfield', b: 'James Whitfield', agree: 'match' },
      { key: 'Date of birth', a: '1971-08-04 (photo ID)', b: '1971-08-14 (self-report)', agree: 'conflict' },
      { key: 'Sex', a: 'Male', b: 'Male', agree: 'match' },
    ],
    evidence: [
      { k: 'Name', wt: '+3.0', sign: 'pos', desc: 'strong family + given match' },
      { k: 'DOB', wt: '−1.6', sign: 'neg', desc: '10-day disagreement; one source is self-reported' },
      { k: 'Verify', wt: 'note', sign: '', desc: 'photo ID outranks self-report for DOB' },
    ],
    events: [
      { id: 'a1', type: 'Assert', inst: 'Mercy General', when: '2020-01-09', vm: 'Government photo ID', x: 70, y: 55, parents: [], summary: 'DOB 1971-08-04 (photo ID).' },
      { id: 'a2', type: 'Assert', inst: 'Lakeside Hospital', when: '2023-07-22', vm: 'Self-report', x: 70, y: 205, parents: [], summary: 'DOB 1971-08-14 (self-reported).' },
      { id: 'l1', type: 'Link', inst: 'Lakeside Hospital', when: '2023-07-23', conf: '82%', method: 'Algorithmic', x: 320, y: 130, parents: ['a1', 'a2'], summary: 'Linked on name + address despite the DOB mismatch.' },
    ],
    actions: [
      { label: 'Request amendment from Lakeside', cls: 'amend', ev: 'Amend', note: 'Requests that Lakeside correct its DOB to the photo-ID-verified value. An Amend only takes effect once that institution signs it (§3.4.5).' },
      { label: 'Mark as distinct patients', cls: 'contest', ev: 'Contest', note: 'Contests the link if these are not the same person.', contestCode: 'distinct-patients' },
      { label: 'Defer', cls: 'ghost', ev: null, note: 'Hold for outreach; no event written.' },
    ],
  },
  {
    id: 'c5',
    kind: 'blockedLink',
    conf: 0,
    testData: false,
    title: 'Link from BluePeak Family Care — blocked by step 5.5',
    summary:
      'A clinic Mercy has never interacted with published a Manual Link to an existing Mercy patient subgraph, claiming 9800/10000 confidence. The §4.6 step 5.5 check rejected it because Manual’s effective confidence caps at 5000 — below Mercy’s 6000 floor.',
    insts: ['Mercy General', 'BluePeak Family Care'],
    consent: { state: 'presumed', note: 'No consent restriction at play here. The Link itself did not extract data — step 5.5 stopped the request that depended on it.' },
    cmp: [
      { key: 'Name', a: 'Anita Park', b: 'Anita J Park', agree: 'partial' },
      { key: 'Date of birth', a: '1986-04-09', b: '1986-04-09', agree: 'match' },
      { key: 'Sex', a: 'Female', b: 'Female', agree: 'match' },
      { key: 'Address', a: 'Madison', b: '(absent)', agree: 'partial' },
    ],
    evidence: [
      { k: 'Method', wt: 'note', sign: '', desc: 'BluePeak claimed Manual at 9800/10000 — capped to 5000 by responder ceiling' },
      { k: 'Floor', wt: '−10', sign: 'neg', desc: 'effective 5000 < min_link_confidence (6000) → Link blocked' },
      { k: 'Standing', wt: 'note', sign: '', desc: 'BluePeak has no prior Assert/Attest in Mercy’s view' },
    ],
    linkChain: [
      { linkId: 'rl1', from: 'BluePeak Assert', to: 'Mercy Assert (anchor)', method: 'Manual', claimed: 9800, status: 'fail', reason: 'effective 5000 < floor 6000' },
    ],
    events: [
      { id: 'a1', type: 'Assert', inst: 'Mercy General', when: '2022-05-15', vm: 'Government photo ID', x: 60, y: 50, parents: [], summary: 'Anita Park, Madison address — Mercy’s own record.' },
      { id: 'a2', type: 'Assert', inst: 'BluePeak Family Care', when: '2026-05-28', vm: 'Self-report', x: 60, y: 200, parents: [], summary: 'Anita J Park — new from BluePeak.' },
      { id: 'l1', type: 'Link', inst: 'BluePeak Family Care', when: '2026-05-28', conf: '98%', method: 'Manual', x: 300, y: 125, parents: ['a1', 'a2'], summary: 'Self-issued Manual link at 9800/10000 — blocked.' },
      { id: 'g1', type: 'Attest', inst: 'BluePeak Family Care', when: '2026-05-28', x: 540, y: 55, parents: ['l1'], summary: 'Self-issued Grant naming BluePeak as audience. Inert.' },
    ],
    actions: [
      { label: 'Contest the link', cls: 'contest', ev: 'Contest', note: 'Records a signed Contest by Mercy on the BluePeak Link.', contestCode: 'other' },
      { label: 'Leave standing for review', cls: 'ghost', ev: null, note: 'No event written. The Link stays in the DAG, blocked by step 5.5.' },
      { label: 'Reach BluePeak out-of-band', cls: 'amend', ev: null, note: 'No graph change. Triggers an operator workflow.' },
    ],
  },
  {
    id: 'c4',
    kind: 'synthetic',
    conf: 90,
    testData: true,
    title: 'Synthetic patient surfaced in triage',
    summary: 'A load-test record (test-data tagged) appears in the operator view. This is expected — synthetic events propagate like real ones but are filtered from every clinical view (§11.4.1).',
    insts: ['conformance/load-test'],
    consent: { state: 'na', note: 'Synthetic test data — no patient or consent is involved.' },
    cmp: [
      { key: 'Name', a: 'tok:smith / tok:james', b: 'tok:smith / tok:james', agree: 'match' },
      { key: 'Date of birth', a: 'tok:1980-01-01', b: 'tok:1980-01-01', agree: 'match' },
      { key: 'Origin', a: 'integration-testing', b: 'integration-testing', agree: 'match' },
    ],
    evidence: [
      { k: 'test_data', wt: 'note', sign: '', desc: 'tag present: purpose=integration-testing' },
      { k: 'Visibility', wt: 'note', sign: '', desc: 'visible to operators, invisible to clinical FHIR queries' },
    ],
    events: [
      { id: 'a1', type: 'Assert', inst: 'conformance generator', when: 'just now', vm: 'synthetic', x: 150, y: 130, parents: [], summary: 'Synthetic Assert, test-data tagged.' },
    ],
    actions: [
      { label: 'Acknowledge (no change)', cls: 'ghost', ev: null, note: 'Confirms this is synthetic and removes it from the queue. No graph change.' },
      { label: 'Tombstone expired test data', cls: 'tomb', ev: 'Tombstone', note: 'For test data past its expiration, scrub the content (topology preserved).' },
    ],
  },
];

export function isLinkBlocked(e: CaseEvent): boolean {
  if (e.type !== 'Link' || !e.method || !e.conf) return false;
  return effectiveConfidence(e.method, parseClaim(e.conf)) < LINK_POLICY.min_link_confidence;
}

/** Returns the set of event ids whose only path to an anchor crosses a blocked Link. */
export function inertEventIds(events: CaseEvent[]): Set<string> {
  const blocked = events.filter(isLinkBlocked).map((e) => e.id);
  if (!blocked.length) return new Set();
  const childMap: Record<string, string[]> = {};
  for (const e of events) for (const p of e.parents) (childMap[p] ??= []).push(e.id);
  const inert = new Set<string>();
  const queue = [...blocked];
  while (queue.length) {
    const id = queue.shift()!;
    for (const cid of childMap[id] ?? []) {
      if (!inert.has(cid) && !blocked.includes(cid)) {
        inert.add(cid);
        queue.push(cid);
      }
    }
  }
  return inert;
}
