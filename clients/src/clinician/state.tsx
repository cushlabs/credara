import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { getBridge } from '@shared/fhir/client';
import type { ActionLogEntry, PatientProjection } from './fixtures';
import { PATIENTS } from './fixtures';
import { enrichWithSubgraph } from './project';

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

  // Rewire the DAG + DOB-conflict challenge to the live subgraph (handoff item 1). Each fixture
  // patient is resolved by its stable family token (`tok:demo:<family>`, never a hardcoded id —
  // reset reseeds with fresh ids) and overlaid with real events; presentation fields stay from
  // the fixture. Patients the seed doesn't carry (no token match) keep the fixture untouched.
  // Fixtures render first so the worklist is never blank and the read is purely enriching.
  useEffect(() => {
    const bridge = getBridge();
    let cancelled = false;
    (async () => {
      const enriched = await Promise.all(
        PATIENTS.map(async (p) => {
          try {
            const family = p.name.split(' ').pop()?.toLowerCase() ?? '';
            const ids = await bridge.searchPatientsByToken([`tok:demo:${family}`]);
            const realId = ids[0];
            if (!realId) return p;
            const subgraph = await bridge.readSubgraph(realId);
            // Keep the fixture's stable id for routing/testids; overlay live events + challenge.
            return enrichWithSubgraph(p, subgraph);
          } catch {
            return p; // bridge read unavailable — keep the fixture rather than blanking a row.
          }
        }),
      );
      if (!cancelled) setPatients(enriched);
    })();
    return () => {
      cancelled = true;
    };
  }, []);
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
