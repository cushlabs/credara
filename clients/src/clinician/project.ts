// Clinician read rewiring (handoff item 1). The clinician worklist/detail are seeded by the
// static fixtures for presentation parity, but the identity-critical surfaces — the provenance
// DAG and the DOB-conflict challenge — are rewired here to a REAL subgraph (the bridge's
// `$creda-provenance` read, mapped to CredaProvenance[] at the transport boundary).
//
// `enrichWithSubgraph` overlays a fixture patient with live data: it replaces the DAG with the
// real events and rebuilds any DOB-conflict challenge so its Amend/Attest/Contest options carry
// REAL Core event ids. A resolution written from those options therefore lands on the actual
// Assert/Link in the patient's subgraph and persists across `make -C testbed reset` (the
// tok:demo:* anchors are stable). Link/stale challenges and presentation fields are left to the
// fixture — those are not in scope for this item and are not modeled by the seed dataset.

import type { EventType } from '@shared/components/EventDag';
import type { CredaProvenance } from '@shared/fhir/types';
import type { Challenge, ChallengeOption, PatientField, PatientProjection, ProjectedEvent } from './fixtures';

/** Demo tokens embed their display form (`tok:demo:1971-08-04`). Strip the namespace prefix. */
export function detokenize(token: string | undefined): string | undefined {
  if (!token) return undefined;
  const m = /^tok:[^:]+:(.+)$/.exec(token);
  return m ? m[1] : token;
}

/**
 * Lay the DAG out left-to-right by causal depth (longest parent chain) and top-to-bottom within
 * a column — the visual grammar the fixtures used, computed from real parent edges so any
 * topology renders.
 */
function layout(events: CredaProvenance[]): Map<string, { x: number; y: number }> {
  const byId = new Map(events.map((e) => [e.id, e]));
  const depthMemo = new Map<string, number>();
  const depthOf = (id: string, seen: Set<string> = new Set()): number => {
    if (depthMemo.has(id)) return depthMemo.get(id)!;
    if (seen.has(id)) return 0;
    seen.add(id);
    const parents = (byId.get(id)?.parents ?? []).filter((p) => byId.has(p));
    const d = parents.length === 0 ? 0 : 1 + Math.max(...parents.map((p) => depthOf(p, seen)));
    depthMemo.set(id, d);
    return d;
  };
  const rowCursor = new Map<number, number>();
  const pos = new Map<string, { x: number; y: number }>();
  const ordered = [...events].sort((a, b) => a.recorded.localeCompare(b.recorded));
  for (const e of ordered) {
    const d = depthOf(e.id);
    const row = rowCursor.get(d) ?? 0;
    rowCursor.set(d, row + 1);
    pos.set(e.id, { x: 120 + d * 240, y: 70 + row * 90 });
  }
  return pos;
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

/** Map the real subgraph to the UI's ProjectedEvent[] (DAG nodes), laid out by causal depth. */
export function projectEvents(subgraph: CredaProvenance[]): ProjectedEvent[] {
  const pos = layout(subgraph);
  return subgraph
    .map((e) => {
      const dob = detokenize(e.dateOfBirth);
      const p = pos.get(e.id) ?? { x: 120, y: 70 };
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
        x: p.x,
        y: p.y,
      } satisfies ProjectedEvent;
    })
    .sort((a, b) => a.x - b.x || a.y - b.y);
}

interface AssertDob {
  id: string;
  inst: string;
  vm?: string;
  /** Detokenized DOB for display. */
  dob: string;
  /** Raw DOB token as asserted, carried onto an Amend so it round-trips like the seed. */
  dobToken: string;
}

function conflictingAssertDobs(subgraph: CredaProvenance[]): AssertDob[] {
  const dobs: AssertDob[] = [];
  for (const a of subgraph) {
    if (a.eventType !== 'Assert') continue;
    const dob = detokenize(a.dateOfBirth);
    if (!dob || !a.dateOfBirth) continue;
    dobs.push({ id: a.id, inst: a.institution, vm: a.verificationMethod, dob, dobToken: a.dateOfBirth });
  }
  const distinct = new Set(dobs.map((d) => d.dob));
  return distinct.size > 1 ? dobs : [];
}

/**
 * Build a DOB-conflict challenge whose options reference REAL events: affirming the photo-ID DOB
 * is an Attest on that Assert; affirming the other value is an Amend to the conflicting Assert;
 * "neither" contests the Link. Returns null when the subgraph has no DOB disagreement.
 */
export function projectDobChallenge(subgraph: CredaProvenance[]): Challenge | null {
  const dobs = conflictingAssertDobs(subgraph);
  if (dobs.length === 0) return null;
  const govId = dobs.find((d) => (d.vm ?? '').toLowerCase().includes('photo'));
  const link = subgraph.find((e) => e.eventType === 'Link');
  const options: ChallengeOption[] = dobs.map((d) => {
    const isGov = govId && d.id === govId.id;
    return {
      label: `${d.dob} is correct`,
      eventType: isGov ? 'Attest' : 'Amend',
      note: isGov
        ? `Records a treatment-purpose attestation affirming ${d.dob} (${d.vm}).`
        : `Amends the record so the effective DOB reflects ${d.dob}.`,
      targetEventId: d.id,
      amendDob: d.dobToken,
    };
  });
  options.push({
    label: 'Neither / unsure',
    eventType: link ? 'Contest' : null,
    note: link
      ? 'Flags the demographic conflict by contesting the link, without asserting a value.'
      : 'Routes to the identity team. No event is written.',
    targetEventId: link?.id,
  });
  return {
    id: 'dob-conflict',
    kind: 'dob',
    tag: 'Conflicting DOB',
    title: 'Which date of birth matches the patient in front of you?',
    prompt: `${dobs
      .map((d) => `${d.inst} has ${d.dob} (${d.vm ?? 'unspecified'})`)
      .join('; ')}. Confirm against the patient or their ID.`,
    options,
  };
}

/**
 * Overlay a fixture patient with live subgraph data. Replaces the DAG with the real events and,
 * where the subgraph shows a DOB disagreement, rebuilds the patient's DOB challenge + disputed
 * field so the write targets are real Core ids. Everything else (presentation, link/stale
 * challenges, consent) is left to the fixture. An empty subgraph returns the fixture unchanged.
 */
export function enrichWithSubgraph(fixture: PatientProjection, subgraph: CredaProvenance[]): PatientProjection {
  if (subgraph.length === 0) return fixture;

  const events = projectEvents(subgraph);
  const dobChallenge = projectDobChallenge(subgraph);

  // Swap the fixture's DOB challenge for the real-target one; keep any other challenges as-is.
  const otherChallenges = fixture.challenges.filter((c) => c.kind !== 'dob');
  const challenges = dobChallenge ? [dobChallenge, ...otherChallenges] : otherChallenges;

  // Rebuild the disputed DOB field from the real conflicting Asserts (display only).
  const dobs = conflictingAssertDobs(subgraph);
  const fields: PatientField[] = fixture.fields.map((f) => {
    if (f.key === 'Date of birth' && dobs.length > 0) {
      return { key: 'Date of birth', disputed: true, options: dobs.map((d) => ({ inst: d.inst, v: d.dob, vm: d.vm ?? 'unspecified' })) };
    }
    return f;
  });

  return { ...fixture, events, challenges, fields, needsReview: dobChallenge ? true : fixture.needsReview };
}
