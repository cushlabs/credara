import { createContext, ReactNode, useCallback, useContext, useMemo, useState } from 'react';
import type { ActionLogEntry, PatientProjection } from './fixtures';
import { PATIENTS } from './fixtures';

interface ClinicianState {
  patients: PatientProjection[];
  /** Per-session resolution of challenge questions: key = `${patientId}/${challengeId}`. */
  resolved: Record<string, string>;
  /** Per-session action log per patient. */
  actionLog: Record<string, ActionLogEntry[]>;
  /** Pending access requests per patient. */
  accessRequested: Record<string, boolean>;
  resolveChallenge: (patientId: string, challengeId: string, label: string) => void;
  appendAction: (patientId: string, entry: ActionLogEntry) => void;
  requestAccess: (patientId: string) => void;
}

const Ctx = createContext<ClinicianState | null>(null);

export function ClinicianStateProvider({ children }: { children: ReactNode }) {
  const [patients, setPatients] = useState<PatientProjection[]>(PATIENTS);
  const [resolved, setResolved] = useState<Record<string, string>>({});
  const [actionLog, setActionLog] = useState<Record<string, ActionLogEntry[]>>(
    () => Object.fromEntries(PATIENTS.map((p) => [p.id, [] as ActionLogEntry[]])),
  );
  const [accessRequested, setAccessRequested] = useState<Record<string, boolean>>({});

  const resolveChallenge = useCallback((patientId: string, challengeId: string, label: string) => {
    setResolved((r) => ({ ...r, [`${patientId}/${challengeId}`]: label }));
    setPatients((ps) =>
      ps.map((p) => {
        if (p.id !== patientId) return p;
        // If every challenge is now resolved, drop the needsReview flag.
        const stillOpen = p.challenges.some((c) => `${patientId}/${c.id}` !== `${patientId}/${challengeId}` && !resolved[`${patientId}/${c.id}`]);
        return stillOpen ? p : { ...p, needsReview: false };
      }),
    );
  }, [resolved]);

  const appendAction = useCallback((patientId: string, entry: ActionLogEntry) => {
    setActionLog((m) => ({ ...m, [patientId]: [...(m[patientId] ?? []), entry] }));
  }, []);

  const requestAccess = useCallback((patientId: string) => {
    setAccessRequested((m) => ({ ...m, [patientId]: true }));
  }, []);

  const value = useMemo<ClinicianState>(
    () => ({ patients, resolved, actionLog, accessRequested, resolveChallenge, appendAction, requestAccess }),
    [patients, resolved, actionLog, accessRequested, resolveChallenge, appendAction, requestAccess],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useClinicianState(): ClinicianState {
  const v = useContext(Ctx);
  if (!v) throw new Error('useClinicianState used outside ClinicianStateProvider');
  return v;
}
