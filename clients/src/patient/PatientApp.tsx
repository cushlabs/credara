import { useCallback, useEffect, useState } from 'react';
import { AppShell } from '@shared/components/AppShell';
import { CodeCard } from '@shared/components/CodeCard';
import { Modal } from '@shared/components/Modal';
import { useToast } from '@shared/components/Toast';
import { getBridge, gossipToast } from '@shared/fhir/client';
import type { CredaAuthorization, GrantPurpose, GrantScope, UseMode } from '@shared/fhir/types';
import { avatarColor, classNames, initials } from '@shared/lib/format';

import './patient.css';

// Resolve the demo patient the way a real Creda client would: demographic-token lookup
// (`Patient?_creda-token=`, §8.2.11) against the seeded dataset — never a hardcoded id, because
// `make -C testbed reset` reseeds with fresh event ids while the tok:demo:* tokens stay stable.
// The mock's token search returns its fixture ids, so one path serves both modes. Cached after
// the first resolution.
let resolvedPatientId: string | null = null;
async function patientId(bridge: ReturnType<typeof getBridge>): Promise<string> {
  if (!resolvedPatientId) {
    const ids = await bridge.searchPatientsByToken(['tok:demo:gonzalez']);
    resolvedPatientId = ids[0] ?? 'p1';
  }
  return resolvedPatientId;
}
const PATIENT_NAME = 'Maria Gonzalez';
const PURPOSES: GrantPurpose[] = ['Treatment', 'Payment', 'Operations', 'Public health', 'Research', 'AI training', 'AI inference', 'Federal program'];
const USES: UseMode[] = ['Read only', 'Read & rely', 'Read & export'];
const SCOPES: GrantScope[] = ['Identity only', 'Identity + history', 'Identity (de-identified)'];

type Tab = 'access' | 'share' | 'activity';

interface ActivityEntry {
  ev: 'grant' | 'revoke' | 'access';
  text: string;
  when: string;
}

const EV_COLOR: Record<ActivityEntry['ev'], string> = {
  grant: 'var(--grant)',
  revoke: 'var(--revoke)',
  access: 'var(--access)',
};
const EV_LETTER: Record<ActivityEntry['ev'], string> = { grant: '+', revoke: '–', access: '↧' };

export function PatientApp() {
  return (
    <AppShell
      persona="patient"
      brandContext="My Health Consent"
      who="Maria Gonzalez · patient peer"
      banner={
        <>
          <span>🔑</span>
          <b>Patient consent client.</b>
          <span>Every grant or revocation is signed by your key on this device and propagates across the network.</span>
        </>
      }
      wrap={false}
    >
      <ConsentApp />
    </AppShell>
  );
}

function ConsentApp() {
  const toast = useToast();
  const bridge = getBridge();
  const [tab, setTab] = useState<Tab>('access');
  const [grants, setGrants] = useState<CredaAuthorization[]>([]);
  // Seeded from real grants on load (see refresh); starts empty rather than with a fabricated
  // "export receipt" entry. The feed reflects grants/revocations, not a real ExportReceipt stream
  // yet — that's the remaining ⚠️ in the FE audit (HANDOFF), to be read from events later.
  const [activity, setActivity] = useState<ActivityEntry[]>([]);

  const refresh = useCallback(async () => {
    // Real data both ways: the bridge's `Consent?patient={id}` search (§8.2.9 read-back) returns
    // the patient's grants with revoked ones marked; the mock implements the same method over its
    // seed state. The access list now survives a page refresh against the real peer.
    const list = await bridge.listAuthorizations(await patientId(bridge));
    setGrants(list);
    // Seed the activity feed from the authorization state on first load.
    setActivity((prev) => {
      if (prev.length > 1) return prev;
      const seeded: ActivityEntry[] = list.map((g) => ({
        ev: g.status === 'revoked' ? ('revoke' as const) : ('grant' as const),
        text:
          g.status === 'revoked'
            ? `Stopped sharing with ${g.audience}`
            : `Granted ${g.purpose} access to ${g.audience}`,
        when: g.since || '—',
      }));
      return [...prev, ...seeded].slice(0, 12);
    });
  }, [bridge]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return (
    <div className="patient-app">
      <div className="topbar">
        <div className="who">
          <div className="avatar">{initials(PATIENT_NAME)}</div>
          <div>
            <div className="nm">{PATIENT_NAME}</div>
            <div className="sub">🔑 your patient peer · choices signed on this device</div>
          </div>
        </div>
      </div>

      <div className="tabs" role="tablist">
        <button role="tab" aria-selected={tab === 'access'} className={classNames(tab === 'access' && 'on')} onClick={() => setTab('access')} data-testid="tab-access">
          Who has access
        </button>
        <button role="tab" aria-selected={tab === 'share'} className={classNames(tab === 'share' && 'on')} onClick={() => setTab('share')} data-testid="tab-share">
          Share access
        </button>
        <button role="tab" aria-selected={tab === 'activity'} className={classNames(tab === 'activity' && 'on')} onClick={() => setTab('activity')} data-testid="tab-activity">
          Activity
        </button>
      </div>

      <div className="body">
        {tab === 'access' && (
          <AccessTab
            grants={grants}
            onRevoke={async (g) => {
              try {
                await bridge.revoke({ patientId: await patientId(bridge), grantId: g.id });
                await refresh();
                setActivity((a) => [{ ev: 'revoke', text: `Stopped sharing with ${g.audience}`, when: 'Just now' }, ...a]);
                toast.show(gossipToast('Revocation'));
              } catch (err) {
                toast.show(`Bridge error: ${(err as Error).message}`);
              }
            }}
          />
        )}
        {tab === 'share' && (
          <ShareTab
            onAuthorized={(g) => {
              setActivity((a) => [{ ev: 'grant', text: `Granted ${g.purpose} access to ${g.audience}`, when: 'Just now' }, ...a]);
              setGrants((prev) => [g, ...prev]);
              setTab('access');
              toast.show(gossipToast('Grant'));
            }}
          />
        )}
        {tab === 'activity' && <ActivityTab activity={activity} />}
      </div>
    </div>
  );
}

function AccessTab({ grants, onRevoke }: { grants: CredaAuthorization[]; onRevoke: (g: CredaAuthorization) => Promise<void> }) {
  const active = grants.filter((g) => g.status === 'active');
  const revoked = grants.filter((g) => g.status === 'revoked');
  const [confirm, setConfirm] = useState<CredaAuthorization | null>(null);
  return (
    <>
      <div className="lead-card">
        You control who can use your identity records. Each choice is <b>signed by your key</b> on this device and
        takes effect across the network within seconds — and so does stopping.
      </div>
      <h2 className="sec">{active.length} sharing now</h2>
      {active.length === 0 && <div className="muted small" style={{ padding: 4 }}>You are not sharing with anyone.</div>}
      {active.map((g) => (
        <GrantCard key={g.id} g={g} onAskRevoke={() => setConfirm(g)} />
      ))}
      {revoked.length > 0 && (
        <>
          <h2 className="sec">Stopped</h2>
          {revoked.map((g) => (
            <GrantCard key={g.id} g={g} />
          ))}
        </>
      )}

      <Modal
        open={!!confirm}
        onClose={() => setConfirm(null)}
        header={<b style={{ fontSize: 15 }}>Stop sharing with {confirm?.audience}?</b>}
        body={
          confirm && (
            <>
              <div className="muted" style={{ fontSize: 13.5 }}>
                Access ends right away. A signed revocation propagates to the network and every peer enforces it
                locally within seconds (§4.7).
              </div>
              <CodeCard
                lines={[
                  { key: 'event_type', value: '"AuthorizationRevocation"' },
                  { key: 'target_grant', value: `"${confirm.id}"` },
                  { key: 'audience', value: `"${confirm.audience}"` },
                  { key: 'signed_by', value: `"${PATIENT_NAME} (your key)"` },
                ]}
              />
            </>
          )
        }
        confirm={
          confirm
            ? {
                label: 'Stop sharing',
                className: 'btn danger',
                onClick: async () => {
                  const g = confirm;
                  setConfirm(null);
                  await onRevoke(g);
                },
              }
            : undefined
        }
      />
    </>
  );
}

function GrantCard({ g, onAskRevoke }: { g: CredaAuthorization; onAskRevoke?: () => void }) {
  const [open, setOpen] = useState(false);
  const revoked = g.status === 'revoked';
  return (
    <div className={['gcard', revoked && 'rev'].filter(Boolean).join(' ')} data-testid={`grant-${g.id}`}>
      <div className="ch">
        <div className="ic" style={{ background: avatarColor(g.audience) }}>
          {initials(g.audience)}
        </div>
        <div style={{ flex: 1 }}>
          <div className="nm">{g.audience}</div>
          <div className="meta">
            {g.audienceKind === 'class' ? 'institution class' : 'institution'} · sharing since {g.since}
          </div>
        </div>
        {revoked ? (
          <span className="badge b-revoked">stopped</span>
        ) : g.expires === 'No expiry' ? (
          <span className="badge b-active">● Active</span>
        ) : (
          <span className="badge b-expires">expires {g.expires}</span>
        )}
      </div>
      <div className="chips">
        <span className="chip purpose">{g.purpose}</span>
        <span className="chip">can: {g.use}</span>
        <span className="chip">covers: {g.scope}</span>
      </div>
      <div className="row2">
        <button className="linky" onClick={() => setOpen((o) => !o)} data-testid={`grant-toggle-${g.id}`}>
          {open ? 'Hide signed record' : 'View signed record ›'}
        </button>
        {!revoked && onAskRevoke && (
          <button className="btn danger" onClick={onAskRevoke} data-testid={`grant-stop-${g.id}`}>
            Stop sharing
          </button>
        )}
      </div>
      {open && (
        <div className="reveal">
          <div className="kv-grant">
            <div className="k">event_type</div>
            <div className="v">AuthorizationGrant</div>
            <div className="k">audience</div>
            <div className="v">{g.audience}</div>
            <div className="k">purpose</div>
            <div className="v">{g.purpose}</div>
            <div className="k">use_mode</div>
            <div className="v">{g.use}</div>
            <div className="k">scope</div>
            <div className="v">{g.scope}</div>
            <div className="k">expiration</div>
            <div className="v">{g.expires}</div>
            <div className="k">signed_by</div>
            <div className="v">{PATIENT_NAME} (your key)</div>
            <div className="k">status</div>
            <div className="v">{g.status}</div>
          </div>
        </div>
      )}
    </div>
  );
}

function ShareTab({ onAuthorized }: { onAuthorized: (g: CredaAuthorization) => void }) {
  const toast = useToast();
  const bridge = getBridge();
  const [form, setForm] = useState({
    kind: 'institution' as 'institution' | 'class',
    who: '',
    purpose: 'Treatment' as GrantPurpose,
    use: 'Read & rely' as UseMode,
    scope: 'Identity only' as GrantScope,
    expires: 'No expiry',
  });
  const [confirm, setConfirm] = useState(false);

  const ask = () => {
    if (!form.who.trim()) {
      toast.show('Choose who you are sharing with first');
      return;
    }
    setConfirm(true);
  };

  const commit = async () => {
    try {
      const grant = await bridge.authorize({
        patientId: await patientId(bridge),
        audience: form.who,
        audienceKind: form.kind,
        purpose: form.purpose,
        use: form.use,
        scope: form.scope,
        expires: form.expires,
      });
      setConfirm(false);
      onAuthorized(grant);
    } catch (err) {
      toast.show(`Bridge error: ${(err as Error).message}`);
    }
  };

  return (
    <>
      <div className="lead-card">
        Grant someone access to your identity records. You choose <b>who</b>, <b>why</b>, <b>what</b>, and{' '}
        <b>for how long</b> — and you can stop any time.
      </div>
      <div className="gcard" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <div className="formfield">
          <label>Who are you sharing with?</label>
          <div className="seg2">
            <button
              type="button"
              className={form.kind === 'institution' ? 'on' : ''}
              onClick={() => setForm({ ...form, kind: 'institution', who: '' })}
            >
              A specific institution
            </button>
            <button
              type="button"
              className={form.kind === 'class' ? 'on' : ''}
              onClick={() => setForm({ ...form, kind: 'class', who: '' })}
            >
              A class of providers
            </button>
          </div>
        </div>
        <div className="formfield">
          <label>{form.kind === 'institution' ? 'Institution name' : 'Provider class'}</label>
          {form.kind === 'institution' ? (
            <input
              placeholder="e.g. Lakeside Hospital"
              value={form.who}
              onChange={(e) => setForm({ ...form, who: e.target.value })}
              data-testid="share-who"
            />
          ) : (
            <select value={form.who} onChange={(e) => setForm({ ...form, who: e.target.value })} data-testid="share-who">
              <option value="">Choose a class…</option>
              <option>Any TEFCA QHIN</option>
              <option>Any provider with an active BAA</option>
              <option>Any treating provider</option>
            </select>
          )}
        </div>
        <div className="formfield">
          <label>Purpose</label>
          <select value={form.purpose} onChange={(e) => setForm({ ...form, purpose: e.target.value as GrantPurpose })}>
            {PURPOSES.map((p) => (
              <option key={p}>{p}</option>
            ))}
          </select>
        </div>
        <div className="formfield">
          <label>What they can do</label>
          <select value={form.use} onChange={(e) => setForm({ ...form, use: e.target.value as UseMode })}>
            {USES.map((u) => (
              <option key={u}>{u}</option>
            ))}
          </select>
        </div>
        <div className="formfield">
          <label>What it covers</label>
          <select value={form.scope} onChange={(e) => setForm({ ...form, scope: e.target.value as GrantScope })}>
            {SCOPES.map((s) => (
              <option key={s}>{s}</option>
            ))}
          </select>
        </div>
        <div className="formfield">
          <label>Expires</label>
          <select value={form.expires} onChange={(e) => setForm({ ...form, expires: e.target.value })}>
            {['No expiry', 'In 1 year', 'In 90 days'].map((e) => (
              <option key={e}>{e}</option>
            ))}
          </select>
        </div>
        <button className="btn primary share-btn" onClick={ask} data-testid="share-authorize">
          Review &amp; authorize
        </button>
        <div className="small" style={{ textAlign: 'center' }}>
          A research or AI purpose always requires this explicit grant — it is never presumed.
        </div>
      </div>

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        header={<b style={{ fontSize: 15 }}>Authorize access?</b>}
        body={
          <>
            <div className="muted" style={{ fontSize: 13.5 }}>
              You are granting <b>{form.who}</b> {form.use.toLowerCase()} access to <b>{form.scope.toLowerCase()}</b>{' '}
              for <b>{form.purpose}</b>. This is signed by your key.
            </div>
            <CodeCard
              lines={[
                { key: 'event_type', value: '"AuthorizationGrant"' },
                { key: 'audience', value: `"${form.who}"` },
                { key: 'purpose', value: `"${form.purpose}"` },
                { key: 'use_mode', value: `"${form.use}"` },
                { key: 'scope', value: `"${form.scope}"` },
                { key: 'expiration', value: `"${form.expires}"` },
                { key: 'signed_by', value: `"${PATIENT_NAME} (your key)"` },
              ]}
            />
          </>
        }
        confirm={{ label: 'Authorize', onClick: commit }}
      />
    </>
  );
}

function ActivityTab({ activity }: { activity: ActivityEntry[] }) {
  return (
    <>
      <div className="lead-card">
        Your consent history — grants and revocations you have made, and when someone used an access you granted.
      </div>
      <div className="gcard">
        <div className="tl-events">
          {activity.map((a, i) => (
            <div className="item" key={i}>
              <div className="dot" style={{ background: EV_COLOR[a.ev] }}>
                {EV_LETTER[a.ev]}
              </div>
              <div>
                <div className="t">{a.text}</div>
                <div className="w">
                  {a.when}
                  {a.ev !== 'access' && ' · signed by you'}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
