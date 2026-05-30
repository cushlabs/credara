import { useMemo, useState } from 'react';
import { AppShell } from '@shared/components/AppShell';
import { Badge } from '@shared/components/Badge';
import { CodeCard } from '@shared/components/CodeCard';
import { DagLegend, EVENT_TYPE_COLORS, EventDag, TYPE_DESC, type DagNode, type EventType } from '@shared/components/EventDag';
import { Modal } from '@shared/components/Modal';
import { Section } from '@shared/components/Section';
import { SlideOver } from '@shared/components/SlideOver';
import { useToast } from '@shared/components/Toast';
import { getBridge, gossipToast } from '@shared/fhir/client';
import { classNames, confColor } from '@shared/lib/format';
import {
  CASES,
  consentMeta,
  effectiveConfidence,
  inertEventIds,
  isLinkBlocked,
  KIND_META,
  LINK_POLICY,
  parseClaim,
  type CaseAction,
  type CaseEvent,
  type StewardCase,
} from './fixtures';

import './steward.css';

export function StewardApp() {
  return (
    <AppShell
      persona="steward"
      brandContext="Identity Resolution"
      who="S. Okafor · Mercy General · Identity Steward"
      banner={
        <>
          <span>◑</span>
          <b>Operator view.</b>
          <span>
            Synthetic / test-tagged records are visible here and clearly flagged. You operate on tokenized values,
            never raw PHI.
          </span>
        </>
      }
      wrap={false}
    >
      <StewardConsole />
    </AppShell>
  );
}

function StewardConsole() {
  const [cases, setCases] = useState<StewardCase[]>(CASES);
  const [resolved, setResolved] = useState<Record<string, string>>({});
  const [caseId, setCaseId] = useState<string | null>('c1');
  const [selectedEvent, setSelectedEvent] = useState<string | null>(null);
  const [pending, setPending] = useState<{ caseId: string; action: CaseAction } | null>(null);
  const toast = useToast();
  const bridge = getBridge();

  const open = cases.filter((c) => !resolved[c.id]).length;
  const sel = cases.find((c) => c.id === caseId) ?? null;

  const commit = async () => {
    if (!pending) return;
    const { caseId: cid, action } = pending;
    const c = cases.find((x) => x.id === cid);
    if (!c) return;
    if (action.ev) {
      try {
        if (action.ev === 'Contest') {
          const link = c.events.find((e) => e.type === 'Link');
          await bridge.contest({ linkId: link?.id ?? c.id, reason: action.label });
        }
        // Attest / Amend / Tombstone / Link map to other $creda-* ops not yet wired in the
        // bridge — record the steward's action locally and surface the new node in the DAG.
        setCases((cs) =>
          cs.map((x) =>
            x.id !== cid
              ? x
              : {
                  ...x,
                  events: [...x.events, makeFreshEvent(x, action.ev!, action.label)],
                },
          ),
        );
      } catch (err) {
        toast.show(`Bridge error: ${(err as Error).message}`);
      }
    }
    setResolved((r) => ({ ...r, [cid]: action.label }));
    setPending(null);
    toast.show(
      action.ev
        ? gossipToast(action.ev === 'Link' ? 'Attest' : action.ev)
        : 'Case cleared',
    );
  };

  return (
    <div className="steward-layout">
      <Queue cases={cases} resolved={resolved} caseId={caseId} setCaseId={setCaseId} open={open} />
      <div className="detail">
        {!sel ? (
          <div className="empty">Select a case from the resolution queue.</div>
        ) : (
          <CaseDetail
            c={sel}
            resolved={resolved[sel.id]}
            onEvent={setSelectedEvent}
            onAct={(action) => setPending({ caseId: sel.id, action })}
          />
        )}
      </div>

      <SlideOver
        open={!!selectedEvent}
        onClose={() => setSelectedEvent(null)}
        header={
          sel && selectedEvent ? (
            (() => {
              const ev = sel.events.find((e) => e.id === selectedEvent);
              const color = EVENT_TYPE_COLORS[(ev?.type ?? 'Assert') as EventType];
              return (
                <span className="pill" style={{ background: color }}>
                  {ev?.type}
                </span>
              );
            })()
          ) : null
        }
      >
        {sel && selectedEvent && <EventDetail c={sel} ev={sel.events.find((e) => e.id === selectedEvent)!} />}
      </SlideOver>

      <Modal
        open={!!pending}
        onClose={() => setPending(null)}
        header={
          pending && (
            <>
              {pending.action.ev ? (
                <span className="pill" style={{ background: EVENT_TYPE_COLORS[pending.action.ev as EventType] }}>
                  {pending.action.ev}
                </span>
              ) : null}
              <b style={{ fontSize: 15 }}>{pending.action.ev ? 'Confirm this action' : 'No graph change'}</b>
            </>
          )
        }
        body={
          pending && (
            <>
              <div style={{ fontSize: 14, marginBottom: 6 }}>
                <b>{pending.action.label}</b>
              </div>
              <div className="muted" style={{ fontSize: 13 }}>
                {pending.action.note}
              </div>
              {pending.action.ev && (
                <CodeCard
                  header="A new signed event will be appended to the patient subgraph:"
                  lines={[
                    { key: 'event_type', value: `"${pending.action.ev}"` },
                    { key: 'signed_by', value: '"Mercy General · S. Okafor (steward)"' },
                    { key: 'references', value: '[ the events under review ]' },
                  ]}
                />
              )}
            </>
          )
        }
        confirm={
          pending
            ? {
                label: pending.action.ev ? `Sign & record ${pending.action.ev}` : 'Confirm',
                onClick: commit,
              }
            : undefined
        }
      />
    </div>
  );
}

function makeFreshEvent(c: StewardCase, type: NonNullable<CaseAction['ev']>, label: string): CaseEvent {
  const priorNew = c.events.filter((e) => e.fresh).length;
  const rightmost = Math.max(...c.events.map((e) => e.x));
  const parent = c.events.find((e) => e.type === 'Link') ?? c.events[c.events.length - 1];
  return {
    id: `n${priorNew + 1}`,
    type: type as EventType,
    inst: 'Mercy General (you)',
    when: 'just now',
    parents: parent ? [parent.id] : [],
    fresh: true,
    x: rightmost + 190,
    y: 70 + priorNew * 78,
    summary: `Recorded by the steward — ${label}.`,
  };
}

function Queue({
  cases,
  resolved,
  caseId,
  setCaseId,
  open,
}: {
  cases: StewardCase[];
  resolved: Record<string, string>;
  caseId: string | null;
  setCaseId: (id: string) => void;
  open: number;
}) {
  return (
    <div className="queue">
      <div className="qhd">
        <h2>Resolution queue</h2>
        <div className="sub">
          {open} open · {cases.length - open} resolved · operator view
        </div>
      </div>
      {cases.map((c) => {
        const k = KIND_META[c.kind];
        const done = resolved[c.id];
        const cm = consentMeta(c.consent);
        const flagged = c.consent.state === 'revoked' || c.consent.state === 'restricted';
        const blockedLink = c.events
          .filter(isLinkBlocked)
          .map((e) => ({ e, eff: effectiveConfidence(e.method, parseClaim(e.conf)) }))
          .sort((a, b) => a.eff - b.eff)[0];
        return (
          <button
            key={c.id}
            className={classNames('qitem', caseId === c.id && 'sel', done && 'resolved')}
            onClick={() => setCaseId(c.id)}
            data-testid={`case-${c.id}`}
          >
            <div className="top">
              <span className={`badge ${k.cls}`}>{k.tag}</span>
              {flagged && (
                <Badge style={{ background: cm.bg, color: cm.fg }}>
                  {cm.label}
                </Badge>
              )}
              {c.testData && <Badge variant="test">TEST DATA</Badge>}
              {done && <Badge variant="good">resolved</Badge>}
            </div>
            <div className="nm">{c.title}</div>
            <div className="sm">{c.insts.join('  ·  ')}</div>
            {c.kind === 'contest' ? null : blockedLink ? (
              <div className="meterrow">
                <span style={{ fontSize: 12, color: '#b91c1c', fontVariantNumeric: 'tabular-nums', width: 36 }}>
                  {blockedLink.eff}
                </span>
                <div className="meter" style={{ position: 'relative', width: 90 }}>
                  <span style={{ width: `${(blockedLink.eff / 10000) * 100}%`, background: '#b91c1c' }} />
                  <span
                    style={{
                      position: 'absolute',
                      top: -2,
                      bottom: -2,
                      left: `${(LINK_POLICY.min_link_confidence / 10000) * 100}%`,
                      width: 2,
                      background: '#0f1b2d',
                    }}
                  />
                </div>
                <span className="muted" style={{ fontSize: 11 }}>
                  eff. confidence · floor {LINK_POLICY.min_link_confidence}
                </span>
              </div>
            ) : (
              <div className="meterrow">
                <span style={{ fontSize: 12, color: confColor(c.conf), fontVariantNumeric: 'tabular-nums', width: 36 }}>
                  {c.conf}%
                </span>
                <div className="meter" style={{ width: 90 }}>
                  <span style={{ width: `${c.conf}%`, background: confColor(c.conf) }} />
                </div>
                <span className="muted" style={{ fontSize: 11 }}>
                  match confidence
                </span>
              </div>
            )}
          </button>
        );
      })}
    </div>
  );
}

function CaseDetail({
  c,
  resolved,
  onEvent,
  onAct,
}: {
  c: StewardCase;
  resolved: string | undefined;
  onEvent: (id: string) => void;
  onAct: (a: CaseAction) => void;
}) {
  const k = KIND_META[c.kind];
  const cm = consentMeta(c.consent);
  const inert = useMemo(() => inertEventIds(c.events), [c.events]);
  const dagNodes: DagNode[] = c.events.map((e) => ({
    id: e.id,
    type: e.type,
    inst: e.inst,
    sub: e.conf ? `match ${e.conf}` : e.method ?? e.vm ?? e.when,
    x: e.x,
    y: e.y,
    parents: e.parents,
    blocked: isLinkBlocked(e),
    inert: inert.has(e.id),
    fresh: e.fresh,
  }));
  const hasBlocked = c.events.some(isLinkBlocked);
  const hasInert = inert.size > 0;

  return (
    <>
      <div className="chead">
        <div style={{ flex: 1 }}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <span className={`badge ${k.cls}`}>{k.tag}</span>
            {c.testData && <span className="badge b-test">TEST DATA — clinical-invisible</span>}
            {c.kind !== 'contest' && (
              <span className="badge" style={{ background: '#f3f0fb', color: '#6d28d9' }}>
                match {c.conf}%
              </span>
            )}
          </div>
          <h1 style={{ marginTop: 8 }}>{c.title}</h1>
          <div className="csummary">{c.summary}</div>
          <div className="insts">
            {c.insts.map((i) => (
              <span className="chip" key={i}>
                {i}
              </span>
            ))}
          </div>
        </div>
      </div>

      <Section title="Field comparison" aside={<span className="muted" style={{ fontSize: 12 }}>effective identity across institutions · §5.2.4</span>}>
        <table className="cmp">
          <thead>
            <tr>
              <th>Field</th>
              <th>{c.insts[0] ?? 'A'}</th>
              <th>{c.insts[1] ?? 'B'}</th>
              <th>Agreement</th>
            </tr>
          </thead>
          <tbody>
            {c.cmp.map((r, i) => (
              <tr key={i}>
                <td className="muted">{r.key}</td>
                <td className={r.agree === 'conflict' ? 'val-conflict' : ''}>{r.a}</td>
                <td className={r.agree === 'conflict' ? 'val-conflict' : ''}>{r.b}</td>
                <td>
                  <span className={`agree a-${r.agree}`}>{r.agree}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Section>

      <Section title="Provenance graph" aside={<span className="muted" style={{ fontSize: 12 }}>tap a node for detail</span>}>
        <DagLegend />
        {(hasBlocked || hasInert) && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 12, padding: '6px 16px 0', fontSize: 12, color: 'var(--ink-2)' }}>
            {hasBlocked && <span>↳ blocked Link is in the DAG but no merge took.</span>}
            {hasInert && <span>↳ inert events depend on a blocked Link.</span>}
          </div>
        )}
        <div style={{ padding: 8 }}>
          <EventDag nodes={dagNodes} onNodeClick={onEvent} />
        </div>
      </Section>

      <Section title="Match evidence" aside={<span className="muted" style={{ fontSize: 12 }}>Fellegi–Sunter-style weighting · §5.3</span>}>
        <div className="ev">
          {c.evidence.map((e, i) => (
            <div className="row" key={i}>
              <span className="k">{e.k}</span>
              <span className={`wt ${e.sign}`}>{e.wt === 'note' ? '·' : e.wt}</span>
              <span className="desc">{e.desc}</span>
            </div>
          ))}
        </div>
      </Section>

      {c.linkChain && <LinkChainSection steps={c.linkChain} />}
      <LinkPolicySection />

      <Section
        title="Patient consent context"
        aside={
          <span className="badge" style={{ background: cm.bg, color: cm.fg }}>
            <span className="d" style={{ background: cm.dot, width: 7, height: 7, borderRadius: '50%', display: 'inline-block', marginRight: 5 }} />
            {cm.label}
          </span>
        }
      >
        <div className="muted" style={{ fontSize: 13 }}>
          {c.consent.note}
        </div>
      </Section>

      <Section
        title="Resolve"
        aside={resolved ? <span className="badge b-good">resolved · {resolved}</span> : null}
      >
        {resolved ? (
          <div className="muted" style={{ fontSize: 13 }}>
            This case is resolved.
          </div>
        ) : (
          <div className="acts">
            {c.actions.map((a, i) => (
              <button key={i} className={`btn ${a.cls}`} onClick={() => onAct(a)} data-testid={`steward-action-${i}`}>
                {a.label}
              </button>
            ))}
          </div>
        )}
      </Section>
    </>
  );
}

function LinkChainSection({ steps }: { steps: NonNullable<StewardCase['linkChain']> }) {
  const anyFail = steps.some((r) => r.status === 'fail');
  return (
    <Section
      title={
        <>
          Link-chain evaluation{' '}
          <span className="muted" style={{ fontWeight: 500, fontSize: 12, marginLeft: 6 }}>
            §4.6 step 5.5
          </span>
        </>
      }
      aside={anyFail ? <span className="badge b-blocked">chain blocked</span> : <span className="badge b-good">chain clears floor</span>}
    >
      <div className="chain">
        {steps.map((r, i) => {
          const eff = effectiveConfidence(r.method, r.claimed);
          const ceiling = LINK_POLICY.ceilings[r.method];
          return (
            <div key={i} className={`step ${r.status}`}>
              <span className="dot" />
              <div className="det">
                <div className="nm">
                  {r.from} → {r.to}
                </div>
                <div className="sub-d">
                  {r.method} · claimed {r.claimed}, ceiling {ceiling} → effective <b>{eff}</b>
                </div>
              </div>
              <div className="conf">{eff}</div>
              <div className="status">
                {r.status === 'pass' ? 'PASS' : 'BLOCKED'}
                <div style={{ fontWeight: 500, fontSize: 10.5, color: 'var(--ink-3)', marginTop: 2 }}>
                  {r.reason}
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </Section>
  );
}

function LinkPolicySection() {
  const floor = LINK_POLICY.min_link_confidence;
  const floorPct = floor / 100;
  return (
    <Section
      title={
        <>
          Link policy{' '}
          <span className="muted" style={{ fontWeight: 500, fontSize: 12, marginLeft: 6 }}>
            §4.6 step 5.5 · §5.3.5
          </span>
        </>
      }
      aside={<span className="badge b-policy">{LINK_POLICY.posture}</span>}
    >
      <div style={{ fontSize: 12.5, color: 'var(--ink-2)', marginBottom: 10 }}>
        Cross-institutional Grants reached through Link traversal must clear every Link&apos;s floor after per-method
        ceilings are applied.
      </div>
      <div className="floorbar">
        <div>0</div>
        <div className="track">
          <div className="lane warn" />
          <div className="lane ok" />
          <div className="marker" style={{ left: `${floorPct}%` }} />
          <div className="tag" style={{ left: `${floorPct}%` }}>
            floor {floor}
          </div>
        </div>
        <div>10000</div>
      </div>
      <div className="lp" style={{ marginTop: 24 }}>
        <div className="col">
          <h3>Per-method ceilings</h3>
          {Object.entries(LINK_POLICY.ceilings).map(([m, v]) => (
            <div className="row" key={m}>
              <span className="nm">{m}</span>
              <span className="mini">
                <span style={{ width: `${(v as number) / 100}%`, background: (v as number) >= floor ? 'var(--good)' : 'var(--bad)' }} />
              </span>
              <span className="num">{v as number}</span>
            </div>
          ))}
        </div>
        <div className="col">
          <h3>Other policy</h3>
          <div className="row">
            <span className="nm">min_link_confidence (floor)</span>
            <span className="num">{floor}</span>
          </div>
          <div className="row">
            <span className="nm">require_author_standing</span>
            <span className="num">{LINK_POLICY.require_author_standing ? 'ON' : 'OFF'}</span>
          </div>
          <div className="row">
            <span className="nm">applies to</span>
            <span className="num" style={{ fontWeight: 600, fontSize: 12, color: 'var(--ink-2)' }}>
              cross-institutional Grants only
            </span>
          </div>
        </div>
      </div>
    </Section>
  );
}

function EventDetail({ c, ev }: { c: StewardCase; ev: CaseEvent }) {
  const color = EVENT_TYPE_COLORS[ev.type];
  let linkPolicyDetail: JSX.Element | null = null;
  if (ev.type === 'Link' && ev.method) {
    const claimed = parseClaim(ev.conf);
    const ceiling = LINK_POLICY.ceilings[ev.method] ?? LINK_POLICY.ceilings.Other;
    const eff = effectiveConfidence(ev.method, claimed);
    const pass = eff >= LINK_POLICY.min_link_confidence;
    linkPolicyDetail = (
      <div
        style={{
          marginTop: 14,
          padding: '10px 12px',
          borderRadius: 9,
          background: pass ? '#eef9f1' : '#fdeeee',
          border: `1px solid ${pass ? '#cae3d3' : '#f4cccc'}`,
        }}
      >
        <div style={{ fontWeight: 700, fontSize: 12, color: pass ? '#15803d' : '#b91c1c', marginBottom: 6 }}>
          {pass ? '✓ Link clears floor' : '✗ Link blocked by step 5.5'}
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 60px', gap: '4px 12px', fontSize: 12.5 }}>
          <div className="muted">Claimed by signer</div>
          <div style={{ textAlign: 'right', fontWeight: 600 }}>{claimed}</div>
          <div className="muted">{ev.method} ceiling</div>
          <div style={{ textAlign: 'right', fontWeight: 600 }}>{ceiling}</div>
          <div className="muted">Effective</div>
          <div style={{ textAlign: 'right', fontWeight: 700, color: pass ? '#15803d' : '#b91c1c' }}>{eff}</div>
          <div className="muted">Responder floor</div>
          <div style={{ textAlign: 'right', fontWeight: 600 }}>{LINK_POLICY.min_link_confidence}</div>
        </div>
      </div>
    );
  }
  void color;
  void c;
  return (
    <>
      <div className="muted" style={{ fontSize: 13, marginBottom: 10 }}>
        {TYPE_DESC[ev.type]}
      </div>
      <div className="kv">
        <div className="k">Originating</div>
        <div className="v">{ev.inst}</div>
        <div className="k">Recorded</div>
        <div className="v">{ev.when}</div>
        {ev.vm && (
          <>
            <div className="k">Verification</div>
            <div className="v">{ev.vm}</div>
          </>
        )}
        {ev.conf && (
          <>
            <div className="k">Match score</div>
            <div className="v">{ev.conf}</div>
          </>
        )}
        {ev.method && (
          <>
            <div className="k">Link method</div>
            <div className="v">{ev.method}</div>
          </>
        )}
        <div className="k">Parents</div>
        <div className="v">{ev.parents.length ? ev.parents.join(', ') : 'root (none)'}</div>
      </div>
      <div style={{ marginTop: 14, fontSize: 13 }}>{ev.summary}</div>
      {linkPolicyDetail}
      <CodeCard
        lines={[
          { key: 'event_type', value: `"${ev.type}"` },
          { key: 'institution', value: `"${ev.inst}"` },
          { key: 'signature', value: 'ed25519:verified ✓' },
        ]}
      />
    </>
  );
}
