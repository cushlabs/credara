import { useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import { Badge } from '@shared/components/Badge';
import { ConfidenceMeter } from '@shared/components/ConfidenceMeter';
import { avatarColor, initials } from '@shared/lib/format';
import { useClinicianState } from './state';
import { consentMeta } from './consent';
import type { PatientProjection } from './fixtures';

import './clinician.css';

export function WorklistPage() {
  const { patients, actionLog } = useClinicianState();
  const [q, setQ] = useState('');

  const rows = useMemo(() => {
    const f = q.trim().toLowerCase();
    return patients.filter((p) => !f || p.name.toLowerCase().includes(f) || p.mrns.join(' ').toLowerCase().includes(f));
  }, [patients, q]);

  const needsReview = patients.filter((p) => p.needsReview).length;

  return (
    <>
      <h1>Patient identity worklist</h1>
      <div className="muted" style={{ marginBottom: 16 }}>
        {patients.length} patients in view ·{' '}
        <b style={{ color: 'var(--warn)' }}>{needsReview} need confirmation</b>
      </div>
      <div className="searchrow">
        <input
          aria-label="Search patients"
          placeholder="Search by name or MRN…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          autoComplete="off"
          autoFocus
        />
      </div>
      <div className="plist">
        {rows.length === 0 && <div className="empty">No matching patients.</div>}
        {rows.map((p) => (
          <PatientCard key={p.id} p={p} actions={actionLog[p.id]?.length ?? 0} />
        ))}
      </div>
    </>
  );
}

function PatientCard({ p, actions }: { p: PatientProjection; actions: number }) {
  const cm = consentMeta(p.consent);
  return (
    <Link to={`patients/${p.id}`} className="pcard" data-testid={`patient-card-${p.id}`}>
      <div className="avatar" style={{ background: avatarColor(p.name) }}>
        {initials(p.name)}
      </div>
      <div>
        <div className="nm">{p.name}</div>
        <div className="sub">
          DOB {p.dob} · {p.sex} · {p.mrns.length} MRN{p.mrns.length > 1 ? 's' : ''}
        </div>
        <div className="sub" style={{ marginTop: 3 }}>
          {p.summary}
        </div>
      </div>
      <div className="right">
        <span className="badge" style={{ background: cm.bg, color: cm.fg }} data-testid={`consent-${p.id}`}>
          <span className="d" style={{ background: cm.dot }} />
          {cm.ok ? 'Authorized' : 'Needs access'}
        </span>
        {p.needsReview ? (
          <Badge variant="warn" dot="var(--warn)">
            Needs confirmation
          </Badge>
        ) : (
          <Badge variant="good" dot="var(--good)">
            Confirmed
          </Badge>
        )}
        <ConfidenceMeter percent={p.confidence} />
        {actions > 0 && (
          <Badge variant="info">
            {actions} action{actions > 1 ? 's' : ''} taken
          </Badge>
        )}
      </div>
    </Link>
  );
}
