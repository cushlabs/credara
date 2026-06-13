import { useCallback, useEffect, useState } from 'react';
import { AppShell } from '@shared/components/AppShell';
import { CodeCard } from '@shared/components/CodeCard';
import { Modal } from '@shared/components/Modal';
import { useToast } from '@shared/components/Toast';
import { getBridge, gossipToast } from '@shared/fhir/client';
import type { AccessRequest } from '@shared/fhir/client';
import type { CredaAuthorization, CredaProvenance, GrantPurpose, GrantScope, UseMode } from '@shared/fhir/types';
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
  /** Sort key (epoch ms). Optimistic entries use Date.now(); event-sourced ones use `recorded`. */
  ts?: number;
}

const EV_COLOR: Record<ActivityEntry['ev'], string> = {
  grant: 'var(--grant)',
  revoke: 'var(--revoke)',
  access: 'var(--access)',
};
const EV_LETTER: Record<ActivityEntry['ev'], string> = { grant: '+', revoke: '–', access: '↧' };

function fmtWhen(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso || '—';
  return d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

/**
 * Build the activity feed from the real event DAG (`$creda-provenance`), not from authorization
 * *state*. Each AuthorizationGrant, AuthorizationRevocation, and ExportReceipt is its own node, so
 * a grant that was later revoked still appears as a distinct "Granted …" entry — the feed survives
 * a page reload because it reflects events, not the collapsed active/revoked status. Grant payloads
 * don't carry the audience (it's not in the Provenance projection), so we recover it by joining on
 * the grant id from `Consent?patient=`; a revocation's parent is the grant it targets.
 */
function buildActivity(events: CredaProvenance[], auths: CredaAuthorization[]): ActivityEntry[] {
  const audienceById = new Map(auths.map((a) => [a.id, a.audience]));
  const purposeById = new Map(auths.map((a) => [a.id, a.purpose]));
  const out: ActivityEntry[] = [];
  for (const e of events) {
    const ts = Date.parse(e.recorded) || 0;
    if (e.eventType === 'AuthorizationGrant') {
      const audience = audienceById.get(e.id) ?? 'an institution';
      const purpose = purposeById.get(e.id) ?? e.purpose ?? 'access';
      out.push({ ev: 'grant', text: `Granted ${purpose} access to ${audience}`, when: fmtWhen(e.recorded), ts });
    } else if (e.eventType === 'AuthorizationRevocation') {
      const audience = audienceById.get(e.parents[0] ?? '') ?? 'an institution';
      out.push({ ev: 'revoke', text: `Stopped sharing with ${audience}`, when: fmtWhen(e.recorded), ts });
    } else if (e.eventType === 'ExportReceipt') {
      out.push({ ev: 'access', text: `${e.institution || 'An institution'} used an access you granted`, when: fmtWhen(e.recorded), ts });
    }
  }
  return out.sort((a, b) => (b.ts ?? 0) - (a.ts ?? 0)).slice(0, 50);
}

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
  // Event-sourced from `$creda-provenance` on every refresh (see below) — each grant, revocation,
  // and export receipt is its own timeline entry, so the feed survives a page reload and no longer
  // collapses a revoked grant into a single "stopped" row.
  const [activity, setActivity] = useState<ActivityEntry[]>([]);

  // Institutions known to the network — `GET /Organization` (Core's ListInstitutions): the real,
  // network-wide list of institutions seen in grants, for the share datalist. Fetched once.
  const [institutions, setInstitutions] = useState<string[]>([]);
  // Pending access requests from providers (off-chain FHIR Task inbox). The patient answers each
  // with an on-chain grant (Approve) or dismisses it.
  const [requests, setRequests] = useState<AccessRequest[]>([]);

  const refresh = useCallback(async () => {
    // Real reads, joined: `Consent?patient={id}` (§8.2.9 read-back) gives the current grant list
    // (audience/purpose/status) for the access tab; `$creda-provenance` gives the full event DAG
    // for the activity feed; `Task?patient=` gives pending access requests. The mock implements all
    // three. Authorization provenance doesn't carry the audience, so buildActivity joins it back.
    const id = await patientId(bridge);
    const [list, events, reqs] = await Promise.all([
      bridge.listAuthorizations(id),
      // A provenance hiccup shouldn't blank the access list — degrade the feed to empty instead.
      bridge.readSubgraph(id).catch(() => [] as CredaProvenance[]),
      bridge.listAccessRequests(id).catch(() => [] as AccessRequest[]),
    ]);
    setGrants(list);
    setActivity(buildActivity(events, list));
    setRequests(reqs);
  }, [bridge]);

  useEffect(() => {
    refresh();
    // Network institutions are independent of the patient; load once and tolerate failure.
    bridge.listInstitutions().then(setInstitutions).catch(() => setInstitutions([]));
  }, [refresh, bridge]);

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
            requests={requests}
            onApprove={async (r) => {
              try {
                const grant = await bridge.authorize({
                  patientId: await patientId(bridge),
                  audience: r.requester,
                  audienceKind: 'institution',
                  purpose: r.purpose,
                  use: r.use,
                  scope: 'Identity only',
                  expires: 'No expiry',
                });
                await bridge.resolveAccessRequest(r.id);
                setActivity((a) => [{ ev: 'grant', text: `Granted ${grant.purpose} access to ${grant.audience}`, when: 'Just now', ts: Date.now() }, ...a]);
                await refresh();
                toast.show(gossipToast('Grant'));
              } catch (err) {
                toast.show(`Bridge error: ${(err as Error).message}`);
              }
            }}
            onDismiss={async (r) => {
              try {
                await bridge.resolveAccessRequest(r.id);
                await refresh();
                toast.show('Request dismissed');
              } catch (err) {
                toast.show(`Bridge error: ${(err as Error).message}`);
              }
            }}
            onRevoke={async (g) => {
              try {
                await bridge.revoke({ patientId: await patientId(bridge), grantId: g.id });
                // Optimistic entry for instant feedback; refresh() then replaces the whole feed
                // with the authoritative event-sourced list, so there's no duplicate.
                setActivity((a) => [{ ev: 'revoke', text: `Stopped sharing with ${g.audience}`, when: 'Just now', ts: Date.now() }, ...a]);
                await refresh();
                toast.show(gossipToast('Revocation'));
              } catch (err) {
                toast.show(`Bridge error: ${(err as Error).message}`);
              }
            }}
          />
        )}
        {tab === 'share' && (
          <ShareTab
            suggestions={Array.from(
              new Set([
                // Network-wide institutions (GET /Organization) first, then any this patient has
                // shared with that aren't in that list yet.
                ...institutions,
                ...grants.filter((g) => g.audienceKind === 'institution').map((g) => g.audience),
              ]),
            ).sort()}
            onAuthorized={async (g) => {
              setActivity((a) => [{ ev: 'grant', text: `Granted ${g.purpose} access to ${g.audience}`, when: 'Just now', ts: Date.now() }, ...a]);
              setGrants((prev) => [g, ...prev]);
              setTab('access');
              toast.show(gossipToast('Grant'));
              await refresh();
            }}
          />
        )}
        {tab === 'activity' && <ActivityTab activity={activity} />}
      </div>
    </div>
  );
}

function AccessTab({
  grants,
  requests,
  onRevoke,
  onApprove,
  onDismiss,
}: {
  grants: CredaAuthorization[];
  requests: AccessRequest[];
  onRevoke: (g: CredaAuthorization) => Promise<void>;
  onApprove: (r: AccessRequest) => Promise<void>;
  onDismiss: (r: AccessRequest) => Promise<void>;
}) {
  const active = grants.filter((g) => g.status === 'active');
  const revoked = grants.filter((g) => g.status === 'revoked');
  const [confirm, setConfirm] = useState<CredaAuthorization | null>(null);
  return (
    <>
      <div className="lead-card">
        You control who can use your identity records. Each choice is <b>signed by your key</b> on this device and
        takes effect across the network within seconds — and so does stopping.
      </div>
      {requests.length > 0 && (
        <>
          <h2 className="sec">{requests.length} request{requests.length > 1 ? 's' : ''} for access</h2>
          {requests.map((r) => (
            <div className="gcard" key={r.id} data-testid={`request-${r.id}`} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div className="ch">
                <div className="ic" style={{ background: avatarColor(r.requester) }}>{initials(r.requester)}</div>
                <div style={{ flex: 1 }}>
                  <div className="nm">{r.requester}</div>
                  <div className="meta">wants {r.purpose} access · {r.use}</div>
                </div>
              </div>
              <div className="row2">
                <button className="linky" onClick={() => onDismiss(r)} data-testid={`request-dismiss-${r.id}`}>
                  Dismiss
                </button>
                <button className="btn primary" onClick={() => onApprove(r)} data-testid={`request-approve-${r.id}`}>
                  Approve &amp; share
                </button>
              </div>
            </div>
          ))}
        </>
      )}
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

function ShareTab({
  onAuthorized,
  suggestions,
}: {
  onAuthorized: (g: CredaAuthorization) => void;
  /** Institutions on the network (+ any this patient has shared with) — datalist hints. */
  suggestions: string[];
}) {
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
            <>
              {/* Free-text input backed by a datalist of institutions on the network (GET
                  /Organization). Type any institution, or pick an existing one. Native datalist
                  keeps it a real text field — faster repeat testing without a fixed list. */}
              <input
                list="known-institutions"
                placeholder="e.g. Lakeside Hospital"
                value={form.who}
                onChange={(e) => setForm({ ...form, who: e.target.value })}
                data-testid="share-who"
              />
              <datalist id="known-institutions">
                {suggestions.map((s) => (
                  <option key={s} value={s} />
                ))}
              </datalist>
              {suggestions.length > 0 && (
                <div className="small" style={{ marginTop: 4 }}>
                  Suggestions are institutions already on the network — or type a new name.
                </div>
              )}
            </>
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
