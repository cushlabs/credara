import { useEffect, useMemo, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Badge } from '@shared/components/Badge';
import { CodeCard } from '@shared/components/CodeCard';
import { ConfidenceMeter } from '@shared/components/ConfidenceMeter';
import { DagLegend, EVENT_TYPE_COLORS, EventDag, TYPE_DESC, type DagNode, type EventType } from '@shared/components/EventDag';
import { DemoData } from '@shared/components/DemoData';
import { Modal } from '@shared/components/Modal';
import { Section } from '@shared/components/Section';
import { SlideOver } from '@shared/components/SlideOver';
import { useToast } from '@shared/components/Toast';
import { getBridge, gossipToast } from '@shared/fhir/client';
import { avatarColor, confColor, initials } from '@shared/lib/format';
import type { ActionLogEntry, ChallengeOption, Challenge, PatientField, PatientProjection, ProjectedEvent } from './fixtures';
import { useClinicianState } from './state';
import { consentMeta } from './consent';

import './clinician.css';

export function PatientDetailPage() {
  const { patientId = '' } = useParams<{ patientId: string }>();
  const { patients, actionLog, resolved, accessRequested, resolveChallenge, appendAction, requestAccess } =
    useClinicianState();
  const toast = useToast();
  const bridge = getBridge();

  const patient = patients.find((p) => p.id === patientId);
  const [selectedEvent, setSelectedEvent] = useState<string | null>(null);
  const [pending, setPending] = useState<{ challengeId: string; option: ChallengeOption } | null>(null);

  // Live consent state from the bridge (`Consent?patient=`, §8.2.9 read-back). The fixture
  // consent.state is only the fallback when no real authorization data exists — a grant or
  // revocation made in the patient app shows here because both views read the same DAG. The
  // real patient is resolved by demographic token (stable across `make -C testbed reset`).
  const [liveConsent, setLiveConsent] = useState<PatientProjection['consent'] | null>(null);
  // The patient's real subgraph entry-point id (resolved by demographic token), so an access
  // request targets the same id the patient app reads — not the fixture id.
  const [realPatientId, setRealPatientId] = useState<string | null>(null);
  useEffect(() => {
    if (!patient) return;
    let cancelled = false;
    (async () => {
      try {
        const family = patient.name.split(' ').pop()?.toLowerCase() ?? '';
        const ids = await bridge.searchPatientsByToken([`tok:demo:${family}`]);
        if (!cancelled && ids[0]) setRealPatientId(ids[0]);
        const grants = await bridge.listAuthorizations(ids[0] ?? patient.id);
        if (cancelled || grants.length === 0) return;
        const active = grants.find((g) => g.status === 'active');
        const revoked = grants.find((g) => g.status === 'revoked');
        if (active) {
          setLiveConsent({
            state: 'granted',
            purpose: active.purpose,
            use: active.use,
            source: `Patient grant to ${active.audience}`,
            expires: active.expires,
          });
        } else if (revoked) {
          setLiveConsent({
            state: 'restricted',
            purpose: revoked.purpose,
            use: '—',
            source: `Patient revoked access (${revoked.audience})`,
            expires: '—',
          });
        }
      } catch {
        // Bridge read unavailable — keep the fixture fallback rather than blanking the panel.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bridge, patient]);

  // Just-written-this-session actions (optimistic; not yet re-read into the DAG). These are the
  // only ones rendered as "fresh" extra DAG nodes — they become real graph nodes on next refresh.
  const sessionLog = actionLog[patientId] ?? [];
  // Identity actions on this record, EVENT-SOURCED from the real DAG (Attest/Contest/Amend), so the
  // log survives a refresh instead of being session-only memory. Combined with the session writes
  // above for instant feedback before the next read.
  const derivedActions: ActionLogEntry[] = patient
    ? patient.events
        .filter((e) => e.type === 'Attest' || e.type === 'Contest' || e.type === 'Amend')
        .map((e) => ({
          eventType: e.type as ActionLogEntry['eventType'],
          summary: e.summary,
          when: `${e.when} · ${e.inst}`,
          receipt: null,
        }))
    : [];
  const logEntries = [...derivedActions, ...sessionLog];

  /** Combined DAG: projection events + freshly-recorded actions from this session. */
  const dagNodes: DagNode[] = useMemo(() => {
    if (!patient) return [];
    const base: DagNode[] = patient.events.map((e) => ({
      id: e.id,
      type: e.type,
      inst: e.inst,
      sub: e.conf ? `match ${e.conf}` : e.purpose ?? e.vm ?? e.inst,
      x: e.x,
      y: e.y,
      parents: e.parents,
    }));
    const rightmost = base.length ? Math.max(...base.map((n) => n.x)) : 120;
    const extras: DagNode[] = sessionLog.map((entry, i) => ({
      id: `x-${i}`,
      type: entry.eventType as EventType,
      inst: 'Mercy General (you)',
      sub: 'just now',
      x: rightmost + 190,
      y: 40 + i * 70,
      parents: [],
      fresh: true,
    }));
    return [...base, ...extras];
  }, [patient, sessionLog]);

  if (!patient) {
    return (
      <div className="empty">
        Patient not found. <Link to="..">Back to worklist</Link>
      </div>
    );
  }

  // Effective view: live bridge consent overrides the fixture when present.
  const patientView: PatientProjection = liveConsent ? { ...patient, consent: liveConsent } : patient;
  const cm = consentMeta(patientView.consent);
  const accessRequestedFlag = !!accessRequested[patient.id];

  const onChallenge = (challenge: Challenge, option: ChallengeOption) => {
    setPending({ challengeId: challenge.id, option });
  };

  const onCommit = async () => {
    if (!pending) return;
    const { challengeId, option } = pending;
    resolveChallenge(patient.id, challengeId, option.label);
    if (option.eventType) {
      try {
        let receipt = null;
        if (option.eventType === 'Attest') {
          // Prefer the challenge's real target (the Assert being affirmed); fall back to the
          // subgraph head for the static fixtures that carry no target.
          const target = option.targetEventId ?? patient.events[patient.events.length - 1]?.id ?? patient.id;
          receipt = await bridge.attest({
            patientId: patient.id,
            purpose: 'Treatment',
            references: [target],
            summary: option.label,
          });
        } else if (option.eventType === 'Contest') {
          const link = option.targetEventId ?? patient.events.find((e) => e.type === 'Link')?.id ?? patient.id;
          receipt = await bridge.contest({ linkId: link, code: option.contestCode ?? 'other', detail: option.label });
        } else if (option.eventType === 'Amend') {
          // Wired to $creda-amend (handoff item 1): the corrected DOB is written as a real
          // Amend against the conflicting Assert, so the resolution persists past a reseed.
          // Only the projected challenge carries a real target; the static fixtures don't, so
          // those still record locally.
          if (option.targetEventId) {
            receipt = await bridge.amend({
              patientId: patient.id,
              targetEventId: option.targetEventId,
              dateOfBirth: option.amendDob ?? '',
              reason: option.label,
            });
          }
        }
        const entry: ActionLogEntry = {
          eventType: option.eventType,
          summary: option.label,
          when: 'just now · pending replication',
          receipt,
        };
        appendAction(patient.id, entry);
        toast.show(gossipToast(option.eventType));
      } catch (err) {
        toast.show(`Bridge error: ${(err as Error).message}`);
      }
    } else {
      toast.show('Routed to identity team');
    }
    setPending(null);
  };

  return (
    <>
      <div className="crumbs">
        <Link to="..">Worklist</Link> ⟩ <span>{patient.name}</span>
      </div>
      <div className="phead">
        <div className="avatar" style={{ background: avatarColor(patient.name) }}>
          {initials(patient.name)}
        </div>
        <div>
          <div className="nm" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            {patient.name}
            {patientView.demo ? (
              <DemoData detail="This patient isn't in your seeded network — shown from fixtures." />
            ) : (
              <DemoData what="Confidence/sex demo" detail="Identity is live from Core (legal name, DOB, address, MRNs, consent, links); the headline confidence score and sex are still fixture." />
            )}
          </div>
          <div className="meta">
            DOB <b>{patient.dob}</b> · {patient.sex}
          </div>
          <div className="meta" style={{ marginTop: 4 }}>
            {patient.mrns.map((m) => (
              <Badge key={m} variant="neutral" style={{ marginRight: 6 }}>
                {m}
              </Badge>
            ))}
          </div>
        </div>
        <div className="score">
          <div className="lab">Identity confidence</div>
          <div className="val" style={{ color: confColor(patient.confidence) }}>
            {patient.confidence}%
          </div>
          <ConfidenceMeter percent={patient.confidence} align="right" showLabel={false} />
        </div>
      </div>

      <div className="grid2">
        <div style={{ display: 'grid', gap: 16 }}>
          <ConsentCard
            patient={patientView}
            requested={accessRequestedFlag}
            onRequestAccess={async () => {
              try {
                // Off-chain request (FHIR Task) — the patient answers it with an on-chain grant.
                await bridge.requestAccess({
                  patientId: realPatientId ?? patient.id,
                  requester: 'Mercy General',
                  purpose: 'Treatment',
                  use: 'Read & rely',
                });
                requestAccess(patient.id); // local button-state flag
                toast.show('Access request sent to the patient');
              } catch (err) {
                toast.show(`Bridge error: ${(err as Error).message}`);
              }
            }}
          />
          <Section title="Effective identity" aside={<span className="muted" style={{ fontSize: 12 }}>projected from the DAG · §5.2.4</span>}>
            {cm.ok ? patient.fields.map((f, i) => <FieldRow key={i} f={f} />) : <LockedIdentity />}
          </Section>
          <Section title="Identity actions on this record" aside={<span className="muted" style={{ fontSize: 12 }}>from the DAG · Attest / Contest / Amend</span>}>
            <div className="log">
              {logEntries.length === 0 ? (
                <div className="empty">No identity actions on this record yet.</div>
              ) : (
                logEntries.map((r, i) => (
                  <div className="row" key={i}>
                    <span className="ic" style={{ background: EVENT_TYPE_COLORS[r.eventType as EventType] }}>
                      {r.eventType[0]}
                    </span>
                    <div>
                      <b>{r.eventType}</b> · {r.summary}
                      <div className="muted" style={{ fontSize: 12 }}>
                        {r.when}
                      </div>
                    </div>
                  </div>
                ))
              )}
            </div>
          </Section>
        </div>

        <div style={{ display: 'grid', gap: 16 }}>
          <Section
            title="Challenge questions"
            aside={
              patient.challenges.length ? (
                <Badge variant="warn">
                  {patient.challenges.filter((c) => !resolved[`${patient.id}/${c.id}`]).length} open
                </Badge>
              ) : null
            }
          >
            {patient.challenges.length === 0 ? (
              <div className="empty">No open questions. Identity looks consistent.</div>
            ) : (
              patient.challenges.map((c) => (
                <ChallengeCard
                  key={c.id}
                  challenge={c}
                  resolved={resolved[`${patient.id}/${c.id}`]}
                  onAct={(opt) => onChallenge(c, opt)}
                />
              ))
            )}
          </Section>
          <Section title="Provenance graph" aside={<span className="muted" style={{ fontSize: 12 }}>tap a node for detail</span>}>
            <DagLegend />
            <div style={{ padding: 6 }}>
              <EventDag nodes={dagNodes} onNodeClick={setSelectedEvent} />
            </div>
          </Section>
        </div>
      </div>

      <SlideOver
        open={!!selectedEvent}
        onClose={() => setSelectedEvent(null)}
        header={<SlideOverHeader patient={patient} eventId={selectedEvent} />}
      >
        {selectedEvent && <EventDetail patient={patient} eventId={selectedEvent} logEntries={sessionLog} />}
      </SlideOver>

      <Modal
        open={!!pending}
        onClose={() => setPending(null)}
        header={<PendingHeader pending={pending} />}
        body={pending && <PendingBody pending={pending} />}
        confirm={
          pending
            ? {
                label: pending.option.eventType ? `Sign & record ${pending.option.eventType}` : 'Defer',
                onClick: onCommit,
              }
            : undefined
        }
      />
    </>
  );
}

function FieldRow({ f }: { f: PatientField }) {
  if (f.disputed) {
    return (
      <div className="field disputed">
        <div className="top">
          <span className="key">{f.key}</span>
          <Badge variant="warn" dot="var(--warn)">
            Disputed
          </Badge>
        </div>
        <div className="conflict">
          {f.options?.map((o, i) => (
            <div className="opt" key={i}>
              <span className="v">{o.v}</span>
              <span className="muted">
                — {o.inst} · {o.vm}
              </span>
            </div>
          ))}
        </div>
        <div className="srcs">Resolve via the challenge question on the right.</div>
      </div>
    );
  }
  return (
    <div className="field">
      <div className="top">
        <span className="key">{f.key}</span>
        <ConfidenceMeter percent={f.conf ?? 0} width={90} />
      </div>
      <div className="value">
        {f.value}
        {f.stale && (
          <Badge variant="warn" style={{ marginLeft: 6 }}>
            stale
          </Badge>
        )}
      </div>
      <div className="srcs">Asserted by {(f.sources ?? []).join(', ')}</div>
    </div>
  );
}

function ConsentCard({
  patient,
  requested,
  onRequestAccess,
}: {
  patient: PatientProjection;
  requested: boolean;
  onRequestAccess: () => void;
}) {
  const cm = consentMeta(patient.consent);
  return (
    <Section
      title="Consent & authorization"
      aside={
        <Badge style={{ background: cm.bg, color: cm.fg }} dot={cm.dot}>
          {cm.label}
        </Badge>
      }
    >
      <div className="meta" style={{ marginTop: 2 }}>
        Purpose <b>{patient.consent.purpose ?? '—'}</b> · Access <b>{patient.consent.use ?? '—'}</b> · Basis{' '}
        {patient.consent.source ?? '—'} · Expires {patient.consent.expires ?? '—'}
      </div>
      {cm.ok ? (
        <div className="srcs" style={{ marginTop: 10 }}>
          Access is permitted for treatment. Your reliance on this identity is recorded as an <b>Attest</b> under
          this authorization (§4.6).
        </div>
      ) : (
        <>
          <div className="srcs" style={{ marginTop: 10 }}>
            The patient controls this — access is granted or revoked from their consent client, not here.
          </div>
          <div style={{ marginTop: 12 }}>
            {requested ? (
              <Badge variant="info">Access request sent — awaiting patient</Badge>
            ) : (
              <button className="btn primary" onClick={onRequestAccess} data-testid="request-access">
                Request access
              </button>
            )}
          </div>
        </>
      )}
    </Section>
  );
}

function LockedIdentity() {
  return (
    <div className="empty" style={{ textAlign: 'left' }}>
      🔒 Identity details are withheld until the patient authorizes Mercy General for treatment — request access
      above to view.
    </div>
  );
}

function ChallengeCard({
  challenge,
  resolved,
  onAct,
}: {
  challenge: Challenge;
  resolved: string | undefined;
  onAct: (o: ChallengeOption) => void;
}) {
  return (
    <div className={['chq', resolved && 'resolved'].filter(Boolean).join(' ')}>
      <div className="qt">
        <span className="chip-tag" style={{ background: '#fdf1e3', color: 'var(--warn)' }}>
          {challenge.tag}
        </span>
        {challenge.title}
      </div>
      <div className="qp">{challenge.prompt}</div>
      {resolved ? (
        <Badge variant="good" dot="var(--good)">
          Resolved · {resolved}
        </Badge>
      ) : (
        <div className="acts">
          {challenge.options.map((o, i) => {
            const cls =
              o.eventType === 'Attest'
                ? 'attest'
                : o.eventType === 'Contest' || o.eventType === null
                  ? 'contest'
                  : o.eventType === 'Amend'
                    ? 'amend'
                    : 'ghost';
            return (
              <button key={i} className={`btn ${cls}`} onClick={() => onAct(o)} data-testid={`challenge-opt-${i}`}>
                {o.label}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function SlideOverHeader({ patient, eventId }: { patient: PatientProjection; eventId: string | null }) {
  if (!eventId) return null;
  const ev = findEvent(patient.events, eventId);
  const type = (ev?.type ?? 'Assert') as EventType;
  return (
    <span className="pill" style={{ background: EVENT_TYPE_COLORS[type] }}>
      {type}
    </span>
  );
}

function EventDetail({
  patient,
  eventId,
  logEntries,
}: {
  patient: PatientProjection;
  eventId: string;
  logEntries: ActionLogEntry[];
}) {
  const e: ProjectedEvent | null = findEvent(patient.events, eventId);
  let actionEntry: ActionLogEntry | null = null;
  if (!e && eventId.startsWith('x-')) {
    const idx = Number.parseInt(eventId.slice(2), 10);
    actionEntry = logEntries[idx] ?? null;
  }

  if (e) {
    return (
      <>
        <div className="muted" style={{ fontSize: 13, marginBottom: 10 }}>
          {TYPE_DESC[e.type]}
        </div>
        <div className="kv">
          <div className="k">Originating</div>
          <div className="v">{e.inst}</div>
          <div className="k">Recorded</div>
          <div className="v">{e.when}</div>
          {e.vm && (
            <>
              <div className="k">Verification</div>
              <div className="v">{e.vm}</div>
            </>
          )}
          {e.dob && (
            <>
              <div className="k">Asserted DOB</div>
              <div className="v">{e.dob}</div>
            </>
          )}
          {e.conf && (
            <>
              <div className="k">Match score</div>
              <div className="v">{e.conf}</div>
            </>
          )}
          {e.purpose && (
            <>
              <div className="k">Purpose</div>
              <div className="v">{e.purpose}</div>
            </>
          )}
          <div className="k">Parents</div>
          <div className="v">{e.parents.length ? e.parents.join(', ') : 'root (none)'}</div>
        </div>
        <div style={{ marginTop: 14, fontSize: 13 }}>{e.summary}</div>
        <CodeCard
          lines={[
            { key: 'event_type', value: `"${e.type}"` },
            { key: 'institution', value: `"${e.inst}"` },
            { key: 'signature', value: 'ed25519:verified ✓' },
            { key: 'content_hash', value: `blake3:…${hashSuffix(eventId + patient.id)}` },
          ]}
        />
      </>
    );
  }
  if (actionEntry) {
    return (
      <>
        <div className="muted" style={{ fontSize: 13, marginBottom: 10 }}>
          {TYPE_DESC[actionEntry.eventType as EventType] ?? ''}
        </div>
        <div className="kv">
          <div className="k">Originating</div>
          <div className="v">Mercy General (you)</div>
          <div className="k">Recorded</div>
          <div className="v">{actionEntry.when}</div>
          <div className="k">Summary</div>
          <div className="v">{actionEntry.summary}</div>
        </div>
        {actionEntry.receipt && (
          <CodeCard
            lines={[
              { key: 'event_type', value: `"${actionEntry.receipt.eventType}"` },
              { key: 'id', value: `"${actionEntry.receipt.id}"` },
              { key: 'recorded', value: actionEntry.receipt.recorded },
              { key: 'signature', value: actionEntry.receipt.signature?.verified ? 'ed25519:verified ✓' : 'unverified' },
            ]}
          />
        )}
      </>
    );
  }
  return <div className="empty">Event not found.</div>;
}

function PendingHeader({ pending }: { pending: { option: ChallengeOption } | null }) {
  if (!pending) return null;
  if (!pending.option.eventType) return <b style={{ fontSize: 15 }}>Defer to identity team</b>;
  const col = EVENT_TYPE_COLORS[pending.option.eventType as EventType];
  return (
    <>
      <span className="pill" style={{ background: col }}>
        {pending.option.eventType}
      </span>
      <b style={{ fontSize: 15 }}>Confirm this action</b>
    </>
  );
}

function PendingBody({ pending }: { pending: { option: ChallengeOption } }) {
  const willWrite = !!pending.option.eventType;
  return (
    <>
      <div style={{ fontSize: 14, marginBottom: 6 }}>
        <b>{pending.option.label}</b>
      </div>
      <div className="muted" style={{ fontSize: 13 }}>
        {pending.option.note}
      </div>
      {willWrite && (
        <>
          <CodeCard
            header="A new signed event will be written to the patient subgraph:"
            lines={[
              { key: 'event_type', value: `"${pending.option.eventType}"` },
              { key: 'signed_by', value: '"Mercy General · Dr. A. Reyes"' },
              { key: 'purpose', value: '"treatment"' },
              { key: 'references', value: '[ subgraph head ]' },
            ]}
          />
          <div className="muted" style={{ fontSize: 12, marginTop: 10 }}>
            This is advisory for identity; it does not alter the originating institution&apos;s record.
            {pending.option.eventType === 'Amend' && ' An Amend must be accepted by the originating institution to take effect (§3.4.5).'}
          </div>
        </>
      )}
      {!willWrite && (
        <div className="muted" style={{ fontSize: 12, marginTop: 10 }}>
          No event is written. The question is routed to the identity team for follow-up.
        </div>
      )}
    </>
  );
}

function findEvent(events: ProjectedEvent[], id: string): ProjectedEvent | null {
  return events.find((e) => e.id === id) ?? null;
}

function hashSuffix(s: string): string {
  return s.split('').reverse().join('').slice(0, 8);
}
