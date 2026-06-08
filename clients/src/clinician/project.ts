// Clinician projection — build a PatientProjection from a REAL subgraph (the bridge's
// `$creda-provenance` read, mapped to CredaProvenance[] at the transport boundary). This is the
// read path the handoff calls item 1: demographics, the provenance DAG, and DOB-conflict
// challenges whose Amend/Attest/Contest targets are real Core event ids — so a resolution
// written from here persists across `make -C testbed reset` (the tok:demo:* anchors are stable).
//
// The subgraph carries identity-critical facts (events, verification, asserted DOBs, link
// scores). Presentation-only fields the seed does not model — address, MRNs, a headline
// summary — come from a per-name overlay so the UI keeps mockup parity; everything that drives
// a write is derived from the live events, never the overlay.

import type { EventType } from '@shared/components/EventDag';
import type { CredaProvenance } from '@shared/fhir/types';
import type {
  Challenge,
  ChallengeOption,
  PatientField,
  PatientProjection,
  ProjectedEvent,
} from './fixtures';

/** Presentation-only overlay: fields the seed dataset doesn't carry, keyed by family name. */
export interface PresentationOverlay {
  /** Display name, when the caller wants the mockup's exact form (e.g. "Maria Elena Gonzalez"). */
  name?: string;
  sex?: string;
  mrns?: string[];
  address?: { value: string; conf: number; sources: string[]; stale?: boolean };
  summary?: string;
}

/** Demo tokens embed their display form (`tok:demo:1971-08-04`). Strip the namespace prefix. */
export function detokenize(token: string | undefined): string | undefined {
  if (!token) return undefined;
  const m = /^tok:[^:]+:(.+)$/.exec(token);
  return m ? m[1] : token;
}

/** Title-case a detokenized name fragment (`whitfield` -> `Whitfield`). */
function titleCase(s: string): string {
  return s.replace(/\b\w/g, (c) => c.toUpperCase());
}

/**
 * Lay the DAG out left-to-right by causal depth (longest parent chain) and top-to-bottom by
 * order within a depth column — the same visual grammar the static fixtures used, but computed
 * from real parent edges so any topology renders.
 */
function layout(events: CredaProvenance[]): Map<string, { x: number; y: number }> {
  const byId = new Map(events.map((e) => [e.id, e]));
  const depthMemo = new Map<string, number>();
  const depthOf = (id: string, seen: Set<string> = new Set()): number => {
    if (depthMemo.has(id)) return depthMemo.get(id)!;
    if (seen.has(id)) return 0; // cycle guard (shouldn't happen in a DAG)
    seen.add(id);
    const ev = byId.get(id);
    const parents = (ev?.parents ?? []).filter((p) => byId.has(p));
    const d = parents.length === 0 ? 0 : 1 + Math.max(...parents.map((p) => depthOf(p, seen)));
    depthMemo.set(id, d);
    return d;
  };

  const rowCursor = new Map<number, number>();
  const pos = new Map<string, { x: number; y: number }>();
  // Stable order: recorded time, so columns fill top-to-bottom predictably.
  const ordered = [...events].sort((a, b) => a.recorded.localeCompare(b.recorded));
  for (const e of ordered) {
    const d = depthOf(e.id);
    const row = rowCursor.get(d) ?? 0;
    rowCursor.set(d, row + 1);
    pos.set(e.id, { x: 120 + d * 240, y: 70 + row * 90 });
  }
  return pos;
}

function toProjectedEvent(e: CredaProvenance, pos: { x: number; y: number }): ProjectedEvent {
  const dob = detokenize(e.dateOfBirth);
  return {
    id: e.id,
    type: e.eventType as EventType,
    inst: e.institution,
    when: (e.recorded || '').slice(0, 10),
    vm: e.verificationMethod,
    dob,
    conf: e.matchScore,
    purpose: e.purpose,
    parents: e.parents,
    summary: e.summary ?? defaultSummary(e, dob),
    x: pos.x,
    y: pos.y,
  };
}

function defaultSummary(e: CredaProvenance, dob: string | undefined): string {
  switch (e.eventType) {
    case 'Assert':
      return dob ? `Asserted DOB ${dob} (${e.verificationMethod ?? 'unspecified'}).` : 'Demographic assertion.';
    case 'Link':
      return `${e.linkMethod ?? 'Algorithmic'} match${e.matchScore ? ` (${e.matchScore})` : ''}.`;
    case 'Attest':
      return `Reliance recorded${e.purpose ? ` for ${e.purpose}` : ''}.`;
    case 'Amend':
      return dob ? `DOB amended to ${dob}.` : 'Demographic amendment.';
    case 'Contest':
      return 'Link contested.';
    default:
      return e.eventType;
  }
}

/**
 * Build a clinician PatientProjection from a subgraph. Identity-critical fields (name, DOB,
 * verification, the DAG, conflict challenges with real targets) come from the events; `overlay`
 * supplies presentation-only fields the seed dataset omits.
 */
export function projectPatient(
  patientId: string,
  subgraph: CredaProvenance[],
  overlay: PresentationOverlay = {},
): PatientProjection {
  const asserts = subgraph.filter((e) => e.eventType === 'Assert');
  const amends = subgraph.filter((e) => e.eventType === 'Amend');
  const links = subgraph.filter((e) => e.eventType === 'Link');

  // ---- Name: from Assert demographic tokens (given + family), else the overlay's form. -----
  const named = asserts.find((a) => a.nameFamily || a.nameGiven);
  const nameFromTokens = (() => {
    const given = detokenize(named?.nameGiven);
    const family = detokenize(named?.nameFamily);
    const parts = [given, family].filter(Boolean).map((p) => titleCase(p as string));
    return parts.length ? parts.join(' ') : undefined;
  })();
  const displayName = overlay.name ?? nameFromTokens ?? patientId;

  // ---- DOB: the latest Amend wins; else agreement among Asserts; else a conflict. ----------
  const assertDobs = asserts
    .map((a) => ({ id: a.id, inst: a.institution, vm: a.verificationMethod, dob: detokenize(a.dateOfBirth) }))
    .filter((d): d is { id: string; inst: string; vm?: string; dob: string } => !!d.dob);
  const latestAmendDob = amends
    .slice()
    .sort((a, b) => a.recorded.localeCompare(b.recorded))
    .map((a) => detokenize(a.dateOfBirth))
    .filter(Boolean)
    .pop();
  const distinctDobs = [...new Set(assertDobs.map((d) => d.dob))];
  const dobResolved = latestAmendDob ?? (distinctDobs.length === 1 ? distinctDobs[0] : undefined);
  const dobConflict = !dobResolved && distinctDobs.length > 1;

  // ---- Confidence: a simple, honest proxy — top link score, penalized by an open conflict. -
  const topLinkBps = Math.max(0, ...links.map((l) => parseInt(l.matchScore ?? '0', 10) || 0));
  const confidence = dobConflict ? Math.min(topLinkBps, 64) : topLinkBps || 90;

  // ---- Effective-identity fields. -----------------------------------------------------------
  const fields: PatientField[] = [];
  const allInsts = [...new Set(subgraph.map((e) => e.institution).filter(Boolean))];
  if (displayName !== patientId) {
    fields.push({ key: 'Legal name', value: displayName, conf: 93, sources: allInsts });
  }
  if (dobConflict) {
    fields.push({
      key: 'Date of birth',
      disputed: true,
      options: assertDobs.map((d) => ({ inst: d.inst, v: d.dob, vm: d.vm ?? 'unspecified' })),
    });
  } else if (dobResolved) {
    fields.push({
      key: 'Date of birth',
      value: dobResolved,
      conf: latestAmendDob ? 96 : 90,
      sources: latestAmendDob ? ['Resolved by amendment'] : allInsts,
    });
  }
  if (overlay.sex) fields.push({ key: 'Sex', value: overlay.sex, conf: 99, sources: allInsts.slice(0, 1) });
  if (overlay.address) {
    fields.push({
      key: 'Address',
      value: overlay.address.value,
      conf: overlay.address.conf,
      sources: overlay.address.sources,
      stale: overlay.address.stale,
    });
  }

  // ---- Challenges: a DOB conflict becomes a resolvable challenge with REAL targets. --------
  const challenges: Challenge[] = [];
  if (dobConflict) {
    const govId = assertDobs.find((d) => (d.vm ?? '').toLowerCase().includes('photo'));
    const options: ChallengeOption[] = assertDobs.map((d) => ({
      label: `${d.dob} is correct`,
      // Affirming the photo-ID DOB is an Attest on that Assert; affirming the other value
      // requires an Amend to the conflicting Assert so the effective DOB changes.
      eventType: govId && d.id === govId.id ? 'Attest' : 'Amend',
      note:
        govId && d.id === govId.id
          ? `Records a treatment-purpose attestation affirming ${d.dob} (${d.vm}).`
          : `Amends the record so the effective DOB reflects ${d.dob}.`,
      targetEventId: d.id,
      amendDob: d.dob,
    }));
    // Always offer a no-assert escape that contests the Link instead.
    const link = links[0];
    options.push({
      label: 'Neither / unsure',
      eventType: link ? 'Contest' : null,
      note: link
        ? 'Flags the demographic conflict by contesting the link, without asserting a value.'
        : 'Routes to the identity team. No event is written.',
      targetEventId: link?.id,
    });
    challenges.push({
      id: 'dob-conflict',
      kind: 'dob',
      tag: 'Conflicting DOB',
      title: 'Which date of birth matches the patient in front of you?',
      prompt: `Institutions disagree: ${assertDobs
        .map((d) => `${d.inst} has ${d.dob} (${d.vm ?? 'unspecified'})`)
        .join('; ')}. Confirm against the patient or their ID.`,
      options,
    });
  }

  const pos = layout(subgraph);
  const events = subgraph
    .map((e) => toProjectedEvent(e, pos.get(e.id) ?? { x: 120, y: 70 }))
    .sort((a, b) => a.x - b.x || a.y - b.y);

  return {
    id: patientId,
    name: displayName,
    dob: dobResolved ?? '—',
    sex: overlay.sex ?? '—',
    mrns: overlay.mrns ?? allInsts.map((i) => `${i}`),
    confidence,
    summary: overlay.summary ?? (dobConflict ? 'Institutions disagree on date of birth — needs confirmation.' : 'Identity projected from the subgraph.'),
    needsReview: dobConflict,
    // Consent is read separately (Consent?patient=) and overlaid by the detail page; default to
    // the treatment-presumed posture until that read resolves.
    consent: { state: 'presumed', purpose: 'Treatment', use: 'Read & rely', source: 'HIPAA treatment posture', expires: '—' },
    fields,
    events,
    challenges,
  };
}
