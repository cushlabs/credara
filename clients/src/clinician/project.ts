// Clinician read rewiring. The worklist/detail are seeded by static fixtures for presentation
// parity, but the identity-critical surfaces are rewired to REAL data: the provenance DAG comes
// from the bridge's `$creda-provenance` read, and the DOB field + conflict challenge come from
// Core's EFFECTIVE IDENTITY (`$creda-effective-identity`, §5.2.4/§5.3) — the confidence-weighted,
// attestation-amplified, disagreement-flagged projection. Identity reasoning lives in Core, not
// here (§8.3.2): the client renders Core's numbers and never recomputes which DOB "wins".
//
// Resolving the DOB challenge Attests the chosen value's supporting Assert, which raises that
// value's confidence on re-projection — a real, persisted effect (the Attest survives refresh;
// `make -C testbed reset` restores the baseline conflict). Link/stale challenges and the other
// presentation fields are left to the fixture (not modeled by the seed dataset).

import type { EventType } from '@shared/components/EventDag';
import type { EffectiveField } from '@shared/fhir/client';
import type { CredaProvenance } from '@shared/fhir/types';
import type { Challenge, ChallengeOption, PatientProjection, ProjectedEvent } from './fixtures';

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

/** Core's effective date-of-birth field, if the projection carries one. */
function dobField(identity: EffectiveField[]): EffectiveField | undefined {
  return identity.find((f) => f.key === 'date-of-birth');
}

const confPct = (bp: number): number => Math.round(bp / 100);

/** Title-case a detokenized name part (demo tokens are lowercase, e.g. `tok:demo:whitfield`). */
function titleCase(s: string): string {
  return s.replace(/\b\w/g, (c) => c.toUpperCase());
}

/** Core's top-confidence value for a name field, title-cased, or undefined if absent. */
function namePart(identity: EffectiveField[], key: 'name-given' | 'name-middle' | 'name-family'): { value: string; conf: number } | undefined {
  const f = identity.find((x) => x.key === key);
  if (!f || f.values.length === 0) return undefined;
  const v = detokenize(f.values[0].value);
  if (!v) return undefined;
  return { value: titleCase(v), conf: confPct(f.values[0].confidence) };
}

/**
 * Per-institution MRNs from Core's effective identity. Each `mrn` value is `institution\u{1f}id`
 * (the issuing institution travels in the identifier payload), rendered as "Institution · MRN id".
 */
function mrnList(identity: EffectiveField[]): string[] {
  const f = identity.find((x) => x.key === 'mrn');
  if (!f) return [];
  return f.values.map((v) => {
    const [inst, id] = v.value.split('\u{1f}');
    return `${titleCase(detokenize(inst) ?? inst)} · MRN ${detokenize(id) ?? id}`;
  });
}

/** Address from Core's effective identity — unit-separated tokenized components, detokenized + joined. */
function addressValue(identity: EffectiveField[]): { value: string; conf: number } | undefined {
  const f = identity.find((x) => x.key === 'address');
  if (!f || f.values.length === 0) return undefined;
  const parts = f.values[0].value
    .split('\u{1f}')
    .map((p) => detokenize(p) ?? '')
    .filter(Boolean);
  if (parts.length === 0) return undefined;
  return { value: parts.join(', '), conf: confPct(f.values[0].confidence) };
}

/**
 * Build the legal-name PatientField from Core's effective identity (given + middle + family),
 * title-cased from the demo tokens. Confidence is the lowest across the present parts (a name is
 * only as trustworthy as its weakest component). Returns undefined when Core carries no name.
 */
function legalNameField(identity: EffectiveField[]): { value: string; conf: number } | undefined {
  const parts = (['name-given', 'name-middle', 'name-family'] as const)
    .map((k) => namePart(identity, k))
    .filter((p): p is { value: string; conf: number } => !!p);
  if (parts.length === 0) return undefined;
  return {
    value: parts.map((p) => p.value).join(' '),
    conf: Math.min(...parts.map((p) => p.conf)),
  };
}

/**
 * Build the DOB-conflict challenge from Core's effective identity (§5.2.4/§5.3) — never from
 * client-side reasoning (§8.3.2). Each option affirms one asserted value by Attesting its
 * supporting Assert (which raises that value's confidence on re-projection); "neither" contests
 * the Link. Returns null unless Core reports the date-of-birth field disputed.
 */
export function projectDobChallenge(identity: EffectiveField[], subgraph: CredaProvenance[]): Challenge | null {
  const field = dobField(identity);
  if (!field || !field.disputed || field.values.length < 2) return null;

  const link = subgraph.find((e) => e.eventType === 'Link');
  const asserts = new Map(subgraph.filter((e) => e.eventType === 'Assert').map((a) => [a.id, a]));

  const options: ChallengeOption[] = field.values
    .filter((v) => v.supporting.length > 0)
    .map((v) => {
      const dob = detokenize(v.value) ?? v.value;
      const src = asserts.get(v.supporting[0]);
      return {
        label: `${dob} is correct`,
        eventType: 'Attest' as const,
        note: `Attests reliance on the ${src?.verificationMethod ?? 'asserting institution'}'s record (${dob}, ${confPct(v.confidence)}% confidence). Raises its weight in the effective identity.`,
        targetEventId: v.supporting[0],
      };
    });
  options.push({
    label: 'Neither / unsure',
    eventType: link ? 'Contest' : null,
    note: link
      ? 'Flags the demographic conflict by contesting the link, without asserting a value.'
      : 'Routes to the identity team. No event is written.',
    targetEventId: link?.id,
    contestCode: 'demographic-conflict',
  });

  return {
    id: 'dob-conflict',
    kind: 'dob',
    tag: 'Conflicting DOB',
    title: 'Which date of birth matches the patient in front of you?',
    prompt: `Core reports conflicting DOBs: ${field.values
      .map((v) => `${detokenize(v.value)} (${confPct(v.confidence)}%)`)
      .join(' vs ')}. Confirm against the patient or their ID.`,
    options,
  };
}

/**
 * Build a link-confirmation challenge from a real, uncontested, not-yet-attested Link in the
 * subgraph (§3.4 / §4): a clinician confirms two records are the same person (Attest the Link) or
 * flags them as wrongly merged (Contest the Link). Returns null when there's no such Link — a Link
 * that's already attested (someone relied on it) or already contested is considered resolved, so we
 * don't re-prompt. Derived from real DAG edges, not a fixture.
 */
export function projectLinkChallenge(subgraph: CredaProvenance[]): Challenge | null {
  const contested = new Set(
    subgraph.filter((e) => e.eventType === 'Contest').flatMap((c) => c.parents),
  );
  const attested = new Set(
    subgraph.filter((e) => e.eventType === 'Attest').flatMap((a) => a.parents),
  );
  const link = subgraph.find(
    (e) => e.eventType === 'Link' && !contested.has(e.id) && !attested.has(e.id),
  );
  if (!link) return null;

  return {
    id: 'link-confirm',
    kind: 'link',
    tag: 'Unconfirmed link',
    title: 'Are these two records the same patient?',
    prompt: `Core linked records from ${link.institution || 'two institutions'}${
      link.matchScore ? ` at ${link.matchScore} match` : ''
    }, but no one has confirmed it. Confirm against the patient in front of you.`,
    options: [
      {
        label: 'Yes — same person',
        eventType: 'Attest' as const,
        note: 'Attests reliance on the link; downstream confidence rises and the link is treated as confirmed.',
        targetEventId: link.id,
      },
      {
        label: 'No — wrongly merged',
        eventType: 'Contest' as const,
        note: 'Contests the link, severing the merge in the effective identity (§5.2.4 step 4).',
        targetEventId: link.id,
        contestCode: 'distinct-patients',
      },
    ],
  };
}

/**
 * Overlay a fixture patient with live data: the DAG from the real subgraph, and the DOB field +
 * conflict challenge from Core's effective identity. The displayed DOB is Core's top-confidence
 * value; the disputed field lists each asserted value with its source + confidence. Everything
 * else (other presentation fields, link/stale challenges, consent) is left to the fixture. An
 * empty read returns the fixture unchanged.
 */
export function enrichWithSubgraph(
  fixture: PatientProjection,
  subgraph: CredaProvenance[],
  identity: EffectiveField[],
): PatientProjection {
  if (subgraph.length === 0 && identity.length === 0) return { ...fixture, demo: true };

  const events = projectEvents(subgraph);
  const dobChallenge = projectDobChallenge(identity, subgraph);
  const linkChallenge = projectLinkChallenge(subgraph);

  // In real mode the DOB and link challenges are derived from live data; drop the fixture
  // equivalents (dob/link) so we never mix fabricated challenges with real ones. The fixture
  // 'stale' challenge is also dropped: a stale (time-decayed) assert can't be synthesized in the
  // testbed (Core stamps wall-clock at creation, so seeded events are never old) — staleness needs
  // real elapsed time, tracked as a testbed limitation rather than faked.
  const otherChallenges = fixture.challenges.filter(
    (c) => c.kind !== 'dob' && c.kind !== 'link' && c.kind !== 'stale',
  );
  const challenges = [dobChallenge, linkChallenge, ...otherChallenges].filter(
    (c): c is Challenge => c !== null,
  );

  // DOB field + header from Core's effective identity (top-confidence value leads).
  const field = dobField(identity);
  const asserts = new Map(subgraph.filter((e) => e.eventType === 'Assert').map((a) => [a.id, a]));
  let dob = fixture.dob;
  let fields = fixture.fields;

  // Legal name from Core's effective identity (real, title-cased from demo tokens), replacing the
  // fixture value. Sources stay from the fixture for now (institution display names aren't on the
  // name projection); MRNs/address remain fixture until Core projects them.
  const name = legalNameField(identity);
  if (name) {
    fields = fields.map((f) => (f.key === 'Legal name' ? { ...f, value: name.value, conf: name.conf } : f));
  }

  // Address from Core's effective identity, replacing the fixture value when present.
  const address = addressValue(identity);
  if (address) {
    fields = fields.map((f) => (f.key === 'Address' ? { ...f, value: address.value, conf: address.conf } : f));
  }

  // Per-institution MRNs from Core, replacing the fixture badges when present.
  const mrns = mrnList(identity);
  if (field && field.values.length > 0) {
    dob = detokenize(field.values[0].value) ?? fixture.dob;
    fields = fixture.fields.map((f) => {
      if (f.key !== 'Date of birth') return f;
      if (!field.disputed) {
        return { key: 'Date of birth', value: dob, conf: confPct(field.values[0].confidence) };
      }
      return {
        key: 'Date of birth',
        disputed: true,
        options: field.values.map((v) => {
          const src = asserts.get(v.supporting[0] ?? '');
          return {
            inst: src?.institution ?? 'unknown source',
            v: detokenize(v.value) ?? v.value,
            vm: src?.verificationMethod ?? `${confPct(v.confidence)}% confidence`,
          };
        }),
      };
    });
  }

  return {
    ...fixture,
    dob,
    events,
    challenges,
    fields,
    mrns: mrns.length > 0 ? mrns : fixture.mrns,
    demo: false,
    needsReview: dobChallenge || linkChallenge ? true : fixture.needsReview,
  };
}
