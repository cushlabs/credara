import { useEffect, useState } from 'react';
import { AppShell } from '@shared/components/AppShell';
import { Badge } from '@shared/components/Badge';
import { CodeCard } from '@shared/components/CodeCard';
import { DemoData } from '@shared/components/DemoData';
import { Section } from '@shared/components/Section';
import { useToast } from '@shared/components/Toast';
import { getBridge } from '@shared/fhir/client';
import type { CredaProvenance } from '@shared/fhir/types';
import { ORDERS, PATIENT_CTX, type DtrField, type Order } from './orders';

import './prior-auth.css';

type Step = 'crd' | 'dtr' | 'pas' | 'decision';

export function PriorAuthApp() {
  return (
    <AppShell
      persona="prior-auth"
      brandContext="Clinical Workflow · Prior Authorization"
      who="Dr. A. Reyes · Mercy General"
      banner={
        <>
          <span>⚕</span>
          <b>In-workflow prior auth.</b>
          <span>
            FHIR-based, real-time, pre-filled from the chart — following the Da Vinci CRD / DTR / PAS implementation
            guides. No separate payer portal.
          </span>
        </>
      }
      wrap={false}
    >
      <div className="pa-wrap">
        <PriorAuthFlow />
      </div>
    </AppShell>
  );
}

function PriorAuthFlow() {
  const toast = useToast();
  const bridge = getBridge();
  const [orderId, setOrderId] = useState<string>('mri');
  const [step, setStep] = useState<Step>('crd');
  const [fields, setFields] = useState<Record<string, string>>({});
  const [attested, setAttested] = useState(false);
  const [receipt, setReceipt] = useState<CredaProvenance | null>(null);

  const o = ORDERS[orderId];
  if (!o) return null;

  const onPick = (id: string) => {
    setOrderId(id);
    setStep('crd');
    setFields({});
    setAttested(false);
    setReceipt(null);
  };

  const onSubmit = async () => {
    try {
      const ev = await bridge.attest({
        patientId: 'p1',
        purpose: 'Prior authorization submission',
        references: ['patient-subgraph-head', o.code],
        summary: `${o.name} (${o.code}) — submitted to ${PATIENT_CTX.coverage.payer}.`,
      });
      setReceipt(ev);
      setStep('pas');
      toast.show('Bundle signed and submitted to BlueChoice PPO');
      window.setTimeout(() => setStep('decision'), 2200);
    } catch (err) {
      toast.show(`Bridge error: ${(err as Error).message}`);
    }
  };

  return (
    <>
      <Header order={o} onPick={onPick} orderId={orderId} />
      <Stepper order={o} step={step} />
      {o.needsAuth ? (
        <>
          <CrdCard order={o} step={step} onAdvance={() => setStep('dtr')} />
          {step === 'dtr' && o.dtr && (
            <DtrCard
              order={o}
              fields={fields}
              setFields={setFields}
              attested={attested}
              setAttested={setAttested}
              onSubmit={onSubmit}
            />
          )}
          {(step === 'pas' || step === 'decision') && <PasCard step={step} />}
          {step === 'decision' && o.decision && <DecisionCard decision={o.decision} />}
          {step === 'decision' && <ReceiptCard order={o} receipt={receipt} />}
        </>
      ) : (
        <NotRequiredCard order={o} />
      )}
      <DaVinciFooter />
    </>
  );
}

function Header({ order, orderId, onPick }: { order: Order; orderId: string; onPick: (id: string) => void }) {
  return (
    <div className="hdr">
      <div className="col">
        <div className="lab">Patient</div>
        <div className="val">{PATIENT_CTX.name}</div>
        <div className="sub">
          DOB {PATIENT_CTX.dob} · {PATIENT_CTX.sex} · {PATIENT_CTX.mrn}
        </div>
        <div style={{ marginTop: 5 }}>
          <span className="verifiedchip">
            <span className="d" />
            Identity verified · {PATIENT_CTX.confidence}% · via Creda
          </span>
        </div>
      </div>
      <div className="col">
        <div className="lab">Order</div>
        <div className="pickrow">
          <select aria-label="Order" value={orderId} onChange={(e) => onPick(e.target.value)} data-testid="order-pick">
            {Object.values(ORDERS).map((x) => (
              <option key={x.id} value={x.id}>
                {x.name}
              </option>
            ))}
          </select>
          <Badge variant="info">{order.code}</Badge>
        </div>
        <div className="lab" style={{ marginTop: 9 }}>
          Insurance
        </div>
        <div className="val" style={{ fontSize: 14 }}>
          {PATIENT_CTX.coverage.payer}
        </div>
        <div className="sub">
          Member {PATIENT_CTX.coverage.memberId} · {PATIENT_CTX.coverage.plan}
        </div>
      </div>
    </div>
  );
}

function Stepper({ order, step }: { order: Order; step: Step }) {
  if (!order.needsAuth) {
    return (
      <div className="stepper">
        <StepChip n={1} label="Coverage check" done active={false} skipped={false} />
        <StepChip n={2} label="Documentation" done={false} active={false} skipped />
        <StepChip n={3} label="Submission" done={false} active={false} skipped />
        <StepChip n={4} label="Decision" done={false} active={false} skipped last />
      </div>
    );
  }
  const order2: Step[] = ['crd', 'dtr', 'pas', 'decision'];
  const labels = ['Coverage check (CRD)', 'Documentation (DTR)', 'Submission (PAS)', 'Decision'];
  const idx = order2.indexOf(step);
  return (
    <div className="stepper">
      {order2.map((s, i) => (
        <StepChip key={s} n={i + 1} label={labels[i]!} done={i < idx} active={i === idx} skipped={false} last={i === 3} />
      ))}
    </div>
  );
}

function StepChip({ n, label, done, active, skipped, last = false }: { n: number; label: string; done: boolean; active: boolean; skipped: boolean; last?: boolean }) {
  const cls = active ? 'step active' : done ? 'step done' : skipped ? 'step skipped' : 'step';
  return (
    <>
      <span className={cls}>
        <span className="num">{done ? '✓' : n}</span>
        {label}
      </span>
      {!last && <span className="stepArrow">›</span>}
    </>
  );
}

function CrdCard({ order, step, onAdvance }: { order: Order; step: Step; onAdvance: () => void }) {
  return (
    <Section
      title="Coverage requirements (CRD)"
      aside={
        <>
          <span className="badge b-required">Prior authorization REQUIRED</span>
          <span className="badge b-rt">real-time · {order.crd.latencyMs} ms</span>
          <span className="badge b-davinci">Da Vinci CRD</span>
        </>
      }
    >
      <div className="lead">{order.crd.note}</div>
      <div className="ruleref">
        <span>📜</span> Payer rule: <code>{order.crd.rule}</code>
      </div>
      {step === 'crd' && (
        <div style={{ marginTop: 14, display: 'flex', gap: 9 }}>
          <button className="btn primary" onClick={onAdvance} data-testid="start-dtr">
            Start documentation →
          </button>
        </div>
      )}
    </Section>
  );
}

function DtrCard({
  order,
  fields,
  setFields,
  attested,
  setAttested,
  onSubmit,
}: {
  order: Order;
  fields: Record<string, string>;
  setFields: (f: Record<string, string>) => void;
  attested: boolean;
  setAttested: (v: boolean) => void;
  onSubmit: () => void;
}) {
  const dtr = order.dtr!;
  const gapsRemaining = dtr.fields.filter((f) => f.gap && !fields[f.id]).length;
  const update = (id: string, v: string) => setFields({ ...fields, [id]: v });
  return (
    <Section title={dtr.title} aside={<span className="badge b-davinci">Da Vinci DTR</span>}>
      <div className="lead">
        A FHIR Questionnaire from the payer — pre-populated from your patient&apos;s chart. Complete any gaps and
        attest.
      </div>
      <div style={{ marginTop: 12 }}>
        {dtr.fields.map((f) => (
          <DtrFieldRow key={f.id} field={f} value={fields[f.id] ?? ''} onChange={(v) => update(f.id, v)} />
        ))}
      </div>
      <label className="attest-row">
        <input
          type="checkbox"
          checked={attested}
          onChange={(e) => setAttested(e.target.checked)}
          data-testid="attest-box"
        />
        <span>
          I attest that the documentation above accurately reflects the patient&apos;s clinical picture and supports
          medical necessity.
        </span>
      </label>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 14, gap: 10, flexWrap: 'wrap' }}>
        <div className="muted" style={{ fontSize: 12.5 }}>
          {gapsRemaining ? `${gapsRemaining} gap${gapsRemaining > 1 ? 's' : ''} remain` : 'All gaps complete.'}
        </div>
        <button
          className="btn primary"
          disabled={gapsRemaining > 0 || !attested}
          onClick={onSubmit}
          data-testid="submit-pas"
        >
          Sign &amp; submit prior authorization
        </button>
      </div>
    </Section>
  );
}

function DtrFieldRow({ field, value, onChange }: { field: DtrField; value: string; onChange: (v: string) => void }) {
  return (
    <div className="dtr-field">
      <div className="top">
        <span className="key">{field.key}</span>
        {field.required && <span className="req">REQUIRED</span>}
        <span className={['src', field.gap && 'gap'].filter(Boolean).join(' ')}>
          <span className="d" />
          {field.sourceKind}
        </span>
      </div>
      {field.gap ? (
        <div className="gapinput">
          {field.kind === 'select' ? (
            <select value={value} onChange={(e) => onChange(e.target.value)} data-testid={`gap-${field.id}`}>
              <option value="">{field.placeholder ?? 'Choose…'}</option>
              {field.options?.map((o) => (
                <option key={o}>{o}</option>
              ))}
            </select>
          ) : field.kind === 'textarea' ? (
            <textarea
              placeholder={field.placeholder ?? ''}
              value={value}
              onChange={(e) => onChange(e.target.value)}
              data-testid={`gap-${field.id}`}
            />
          ) : (
            <input
              placeholder={field.placeholder ?? ''}
              value={value}
              onChange={(e) => onChange(e.target.value)}
              data-testid={`gap-${field.id}`}
            />
          )}
        </div>
      ) : (
        <div className="pre">
          <b>{field.preset}</b>
        </div>
      )}
    </div>
  );
}

function PasCard({ step }: { step: Step }) {
  const decided = step === 'decision';
  return (
    <Section title="Submission (PAS)" aside={<span className="badge b-davinci">Da Vinci PAS</span>}>
      <div className="tl">
        <TlRow s="done" title="Bundle assembled" when="FHIR Claim + Patient + Coverage + QuestionnaireResponse + supporting Observations · signed by submitter" />
        <TlRow s={decided ? 'done' : 'active'} title="Sent to BlueChoice PPO" when="Routed via FHIR PAS endpoint; converted to X12 278 by the intermediary" />
        <TlRow s={decided ? 'done' : 'pending'} title="Payer reviewing" when={decided ? 'Decision received' : 'Typical response: 5–60 seconds for automated rules'} />
        <TlRow s={decided ? 'done' : 'pending'} title="Decision" when={decided ? 'See below' : 'Pending'} />
      </div>
    </Section>
  );
}

function TlRow({ s, title, when }: { s: 'done' | 'active' | 'pending'; title: string; when: string }) {
  const dot = s === 'done' ? '✓' : s === 'active' ? '•' : '·';
  return (
    <div className="row">
      <div className={`dot ${s}`}>{dot}</div>
      <div>
        <div className="t">{title}</div>
        <div className="w">{when}</div>
      </div>
    </div>
  );
}

function DecisionCard({ decision }: { decision: NonNullable<Order['decision']> }) {
  const seal = decision.kind === 'approved' ? '✓' : decision.kind === 'denied' ? '×' : '?';
  return (
    <Section
      title="Decision"
      aside={
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
          <DemoData what="Demo decision" detail="This authorization decision is fixture data. It is NOT computed by Creda's authorization evaluation ($creda-verify / EvaluateAuthorization)." />
          <span className={`badge ${decision.kind === 'approved' ? 'b-good' : decision.kind === 'denied' ? 'b-warn' : 'b-info'}`}>{decision.title}</span>
        </span>
      }
    >
      <div className="decision">
        <div className={`seal ${decision.kind}`}>{seal}</div>
        <div>
          <h3>{decision.title}</h3>
          <div className="muted" style={{ fontSize: 13, marginTop: 2 }}>
            {decision.sub}
          </div>
          <ul className="conds">
            {decision.conditions.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ul>
          <div className="muted" style={{ fontSize: 12.5, marginTop: 9 }}>
            <b>Payer rationale.</b> {decision.rationale}
          </div>
        </div>
      </div>
    </Section>
  );
}

function ReceiptCard({ order, receipt }: { order: Order; receipt: CredaProvenance | null }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="section receipt">
      <div className="hd">
        <h2>Provenance receipt</h2>
        <span className="badge b-good" style={{ marginLeft: 'auto' }}>
          Signed
        </span>
      </div>
      <div className="bd">
        <div className="lead">
          This authorization request is recorded as a signed <b>Attest</b> event in the patient&apos;s identity DAG
          (§3.4.4). Compliance can audit it independently — see the <i>compliance &amp; audit</i> view.
        </div>
        <button
          style={{ border: 0, background: 'transparent', color: 'var(--primary)', fontWeight: 650, fontSize: 12.5, padding: '6px 0', cursor: 'pointer' }}
          onClick={() => setOpen((o) => !o)}
          data-testid="toggle-receipt"
        >
          {open ? 'Hide signed record ⌃' : 'View signed record ›'}
        </button>
        {open && (
          <CodeCard
            lines={[
              { key: 'event_type', value: '"Attest"' },
              { key: 'attesting_party', value: '"Mercy General · Dr. A. Reyes"' },
              { key: 'purpose', value: '"Prior authorization submission"' },
              { key: 'references', value: `[ patient-subgraph-head, "${order.code}" ]` },
              { key: 'payer_decision', value: `"${order.decision?.title ?? ''}"` },
              { key: 'provenance_id', value: receipt ? `"${receipt.id}"` : '(pending)' },
              { key: 'signature', value: 'ed25519:verified ✓' },
            ]}
          />
        )}
      </div>
    </div>
  );
}

function NotRequiredCard({ order }: { order: Order }) {
  return (
    <Section
      title="Coverage requirements (CRD)"
      aside={
        <>
          <span className="badge b-notreq">No prior authorization required</span>
          <span className="badge b-rt">real-time · {order.crd.latencyMs} ms</span>
          <span className="badge b-davinci">Da Vinci CRD</span>
        </>
      }
    >
      <div className="lead">{order.crd.note}</div>
      <div className="ruleref">
        <span>📜</span> Payer rule: <code>{order.crd.rule}</code>
      </div>
      <div className="muted" style={{ fontSize: 12.5, marginTop: 12 }}>
        The DTR / PAS steps are <i>skipped</i> when CRD returns &quot;no auth required.&quot; The clinician can
        proceed with the order; no Questionnaire is fetched.
      </div>
    </Section>
  );
}

function DaVinciFooter() {
  return (
    <div className="davinci-foot">
      <b>Following the Da Vinci IGs:</b>
      <span className="badge b-davinci">CRD · Coverage Requirements Discovery</span>
      <span className="badge b-davinci">DTR · Documentation Templates &amp; Rules</span>
      <span className="badge b-davinci">PAS · Prior Authorization Support</span>
      <span style={{ marginLeft: 'auto' }}>FHIR R4 · CDS Hooks at order-sign</span>
    </div>
  );
}

// Reduce unused-eslint noise: the local `useEffect` is reserved for future bridge subscriptions.
void useEffect;
