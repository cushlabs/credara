import type { PatientProjection } from './fixtures';

export interface ConsentMeta {
  ok: boolean;
  bg: string;
  fg: string;
  dot: string;
  label: string;
}

const TABLE: Record<PatientProjection['consent']['state'], ConsentMeta> = {
  granted: { ok: true, bg: '#e7f6ec', fg: '#15803d', dot: '#15803d', label: 'Authorized · patient grant' },
  presumed: { ok: true, bg: '#e7f6ec', fg: '#15803d', dot: '#15803d', label: 'Treatment-presumed (HIPAA TPO)' },
  restricted: { ok: false, bg: '#fde7e7', fg: '#b91c1c', dot: '#b91c1c', label: 'Access revoked by patient' },
  expired: { ok: false, bg: '#fdf1e3', fg: '#b45309', dot: '#b45309', label: 'Grant expired' },
};

export function consentMeta(c: PatientProjection['consent']): ConsentMeta {
  return TABLE[c.state] ?? TABLE.expired;
}
