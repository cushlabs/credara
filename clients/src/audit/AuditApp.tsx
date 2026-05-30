import { useMemo, useState } from 'react';
import { AppShell } from '@shared/components/AppShell';
import { Modal } from '@shared/components/Modal';
import { useToast } from '@shared/components/Toast';
import { classNames } from '@shared/lib/format';
import {
  AUDIT_EVENTS,
  effectiveConfidence,
  LINK_POLICY,
  type AuditEvent,
  type ChainEntry,
  type ChainEntryType,
} from './fixtures';

import './audit.css';

type Filter = 'all' | 'grant' | 'revoke' | 'export' | 'linkdecision' | 'findings';

const META = {
  grant: { label: 'AuthorizationGrant', color: 'var(--grant)', letter: '+', badge: 'b-grant', tag: 'Grant' },
  revoke: { label: 'AuthorizationRevocation', color: 'var(--revoke)', letter: '–', badge: 'b-revoke', tag: 'Revocation' },
  export: { label: 'ExportReceipt', color: 'var(--export)', letter: '↧', badge: 'b-export', tag: 'Export' },
  linkdecision: { label: 'Link-chain decision', color: '#6d28d9', letter: '⟂', badge: 'b-grant', tag: '§4.6 step 5.5' },
} as const;

const PILL: Record<ChainEntryType, string> = {
  Assert: 'var(--grant)',
  Link: '#7c3aed',
  Grant: 'var(--grant)',
  Revocation: 'var(--revoke)',
  Export: 'var(--export)',
  Attest: '#15803d',
};

const FINDING_META = {
  pass: { cls: 'pass', badge: 'b-pass', word: 'PASS' },
  warn: { cls: 'warn', badge: 'b-warn', word: 'WARN' },
  violation: { cls: 'violation', badge: 'b-violation', word: 'VIOLATION' },
} as const;

export function AuditApp() {
  const toast = useToast();
  const [filter, setFilter] = useState<Filter>('all');
  const [sel, setSel] = useState<string | null>('x1');
  const [reportOpen, setReportOpen] = useState(false);

  const filtered = useMemo(() => {
    if (filter === 'findings') return AUDIT_EVENTS.filter((e) => e.finding && e.finding.level !== 'pass');
    if (filter === 'all') return AUDIT_EVENTS;
    return AUDIT_EVENTS.filter((e) => e.type === filter);
  }, [filter]);

  const counts = useMemo(
    () => ({
      total: 1284,
      exports: AUDIT_EVENTS.filter((e) => e.type === 'export').length,
      revocations: AUDIT_EVENTS.filter((e) => e.type === 'revoke').length,
      linkDenied: AUDIT_EVENTS.filter((e) => e.type === 'linkdecision' && e.decision === 'denied').length,
      findings: AUDIT_EVENTS.filter((e) => e.finding && e.finding.level !== 'pass').length,
    }),
    [],
  );

  const selected = AUDIT_EVENTS.find((e) => e.id === sel) ?? null;

  return (
    <AppShell
      persona="audit"
      brandContext="Compliance & Audit"
      who="R. Mensah · Compliance Officer"
      readOnly
      banner={
        <>
          <span>⊜</span>
          <b>Read-only audit view.</b>
          <span>
            Authorization here originates from <b>patient consent</b>. Nothing is changed here; the audit log is
            itself the tamper-evident DAG (§9). Patient identifiers are tokenized.
          </span>
        </>
      }
      wrap={false}
    >
      <div className="audit-kpis">
        <Kpi v={counts.total.toLocaleString()} l="events in window" />
        <Kpi v={counts.exports} l="export receipts" />
        <Kpi v={counts.linkDenied} l="§5.5 link-chain denials" />
        <Kpi v={counts.findings} l="open findings" flag />
      </div>
      <div className="audit-toolbar">
        <div className="seg" role="tablist">
          {(
            [
              ['all', 'All events'],
              ['grant', 'Grants'],
              ['revoke', 'Revocations'],
              ['export', 'Exports'],
              ['linkdecision', 'Link decisions'],
              ['findings', 'Findings'],
            ] as [Filter, string][]
          ).map(([k, label]) => (
            <button
              key={k}
              className={classNames(filter === k && 'on')}
              onClick={() => setFilter(k)}
              data-testid={`filter-${k}`}
            >
              {label}
            </button>
          ))}
        </div>
        <div style={{ flex: 1 }} />
        <button className="btn primary" onClick={() => setReportOpen(true)} data-testid="generate-report">
          Generate report
        </button>
      </div>

      <div className="audit-layout">
        <div className="ledger">
          <div className="lh">
            Audit ledger <span className="muted" style={{ fontWeight: 500 }}>· {filtered.length} event{filtered.length !== 1 ? 's' : ''}</span>
          </div>
          {filtered.length === 0 ? (
            <div className="empty" style={{ padding: 24, textAlign: 'center' }}>
              No events match this filter.
            </div>
          ) : (
            filtered.map((e) => (
              <button
                key={e.id}
                className={classNames('lrow', sel === e.id && 'sel')}
                onClick={() => setSel(e.id)}
                data-testid={`audit-row-${e.id}`}
              >
                <div className="ic" style={{ background: META[e.type].color }}>
                  {META[e.type].letter}
                </div>
                <div>
                  <div className="nm">
                    {META[e.type].tag}: {e.who}
                  </div>
                  <div className="sm">
                    {e.patientToken} · {e.purpose}
                  </div>
                </div>
                <div className="rt">
                  {e.finding && <span className={`badge ${FINDING_META[e.finding.level].badge}`}>{FINDING_META[e.finding.level].word}</span>}
                  <div style={{ marginTop: 4 }}>{e.when}</div>
                </div>
              </button>
            ))
          )}
        </div>

        <div className="audit-detail">
          {selected ? <AuditDetail e={selected} /> : <div className="empty">Select an event from the audit ledger.</div>}
        </div>
      </div>

      <Modal
        open={reportOpen}
        onClose={() => setReportOpen(false)}
        header={<b style={{ fontSize: 15 }}>Compliance report — review window</b>}
        body={
          <>
            <div className="report">
              <ReportLine k="Events reviewed" v={counts.total.toLocaleString()} />
              <ReportLine k="Export receipts" v={counts.exports.toString()} />
              <ReportLine k="Revocations" v={counts.revocations.toString()} />
              <ReportLine k="Violations" v={String(AUDIT_EVENTS.filter((e) => e.finding?.level === 'violation').length)} color="var(--violation)" />
              <ReportLine k="Warnings" v={String(AUDIT_EVENTS.filter((e) => e.finding?.level === 'warn').length)} color="var(--warn)" />
              <ReportLine k="Provenance integrity" v="all chains intact" color="var(--pass)" />
            </div>
            <div className="muted" style={{ fontSize: 12.5, marginTop: 12 }}>
              The report is derived from the signed DAG — every line is independently re-verifiable against the
              events.
            </div>
          </>
        }
        confirm={{
          label: 'Download PDF',
          onClick: () => {
            setReportOpen(false);
            toast.show('Report generated (mockup) — would export a signed PDF');
          },
        }}
        cancelLabel="Close"
      />
    </AppShell>
  );
}

function Kpi({ v, l, flag = false }: { v: string | number; l: string; flag?: boolean }) {
  return (
    <div className={classNames('kpi', flag && 'flag')}>
      <div className="v">{v}</div>
      <div className="l">{l}</div>
    </div>
  );
}

function ReportLine({ k, v, color }: { k: string; v: string; color?: string }) {
  return (
    <div
      className="line"
      style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13.5, padding: '7px 0', borderBottom: '1px dashed var(--line)' }}
    >
      <span>{k}</span>
      <b style={color ? { color } : undefined}>{v}</b>
    </div>
  );
}

function AuditDetail({ e }: { e: AuditEvent }) {
  const m = META[e.type];
  const f = e.finding;
  const decisionChip =
    e.type === 'linkdecision' ? (
      <span className={`badge ${e.decision === 'admitted' ? 'b-linkpass' : 'b-linkdeny'}`}>
        {e.decision === 'admitted' ? 'admitted by §5.5' : 'denied by §5.5'}
      </span>
    ) : null;
  return (
    <>
      <div className="dh">
        <div style={{ display: 'flex', alignItems: 'center', gap: 9, flexWrap: 'wrap' }}>
          <span className={`badge ${m.badge}`}>{m.label}</span>
          {decisionChip}
          {f && <span className={`badge ${FINDING_META[f.level].badge}`}>{FINDING_META[f.level].word}</span>}
        </div>
        <h2 style={{ marginTop: 8 }}>
          {m.tag}: {e.who}
        </h2>
      </div>
      <div className="db">
        <div className="kv-audit">
          <div className="k">Patient (tokenized)</div>
          <div className="v mono">{e.patientToken}</div>
          <div className="k">Purpose</div>
          <div className="v">{e.purpose}</div>
          <div className="k">Requester</div>
          <div className="v">{e.requester}</div>
          <div className="k">Governing grant</div>
          <div className="v">{e.grant}</div>
          <div className="k">Released scope</div>
          <div className="v">{e.scope}</div>
          <div className="k">Recorded</div>
          <div className="v">{e.when}</div>
        </div>

        <div className="sub-audit">
          <h3>Provenance chain</h3>
          <div className="chain">
            <Chain entries={e.chain} />
          </div>
          <div className="intact">
            {e.intact ? (
              <>
                <span style={{ color: 'var(--pass)' }}>✓</span> chain intact — every parent resolves, signatures verified
              </>
            ) : (
              <>
                <span style={{ color: 'var(--violation)' }}>✕</span> chain broken
              </>
            )}
          </div>
        </div>

        {e.linkChain && e.linkChain.length > 0 && <LinkChain steps={e.linkChain} />}

        <div className="sub-audit">
          <h3>Compliance findings</h3>
          {f ? (
            <div className={`finding ${FINDING_META[f.level].cls}`}>
              <div className="ft">
                <span className={`badge ${FINDING_META[f.level].badge}`}>{FINDING_META[f.level].word}</span> {f.title}
              </div>
              <div className="fn">{f.note}</div>
              <div className="fmeta mono">{f.meta}</div>
            </div>
          ) : (
            <div className="muted" style={{ fontSize: 13 }}>
              No findings.
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function Chain({ entries }: { entries: (ChainEntryType | ChainEntry)[] }) {
  const norm = entries.map<ChainEntry>((p) =>
    typeof p === 'string' ? { type: p, label: p } : { type: p.type, label: p.label ?? p.type, blocked: !!p.blocked, inert: !!p.inert },
  );
  return (
    <>
      {norm.map((p, i) => {
        const col = PILL[p.type] ?? 'var(--ink-3)';
        const cls = p.blocked ? 'pill-audit blocked' : p.inert ? 'pill-audit inert' : 'pill-audit';
        const arrowCls = i > 0 && (p.blocked || norm[i - 1]?.blocked) ? 'arrow blocked-arrow' : 'arrow';
        return (
          <span key={i} style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
            {i > 0 && <span className={arrowCls}>{p.blocked || norm[i - 1]?.blocked ? '⇢' : '→'}</span>}
            <span className={cls} style={p.blocked ? { color: col } : { background: col }}>
              {p.label}
            </span>
          </span>
        );
      })}
    </>
  );
}

function LinkChain({ steps }: { steps: NonNullable<AuditEvent['linkChain']> }) {
  const anyFail = steps.some((r) => r.status === 'fail');
  return (
    <div className="sub-audit">
      <h3>
        Link-chain evaluation{' '}
        <span style={{ textTransform: 'none', letterSpacing: 0, color: 'var(--ink-3)', fontWeight: 500, marginLeft: 6 }}>
          §4.6 step 5.5
        </span>
      </h3>
      <div style={{ fontSize: 12.5, marginBottom: 6 }}>
        <span className="policychip">posture: {LINK_POLICY.posture}</span>
        <span className="policychip">floor: {LINK_POLICY.min_link_confidence}</span>
        <span className="policychip">standing required: {LINK_POLICY.require_author_standing ? 'yes' : 'no'}</span>
      </div>
      <div className="chainsteps">
        {steps.map((r, i) => {
          const eff = effectiveConfidence(r.method, r.claimed);
          const ceiling = LINK_POLICY.ceilings[r.method];
          return (
            <div key={i} className={`step-audit ${r.status}`}>
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
                <div style={{ fontWeight: 500, fontSize: 10.5, color: 'var(--ink-3)', marginTop: 2 }}>{r.reason}</div>
              </div>
            </div>
          );
        })}
      </div>
      {anyFail && (
        <div className="muted" style={{ fontSize: 12, marginTop: 8, lineHeight: 1.5 }}>
          Cross-institutional honor was denied. The Link still sits in the DAG as evidence.
        </div>
      )}
    </div>
  );
}
