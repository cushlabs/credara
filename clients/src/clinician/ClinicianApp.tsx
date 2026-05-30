import { Navigate, Route, Routes } from 'react-router-dom';
import { AppShell } from '@shared/components/AppShell';
import { WorklistPage } from './WorklistPage';
import { PatientDetailPage } from './PatientDetailPage';
import { ClinicianStateProvider } from './state';

export function ClinicianApp() {
  return (
    <ClinicianStateProvider>
      <AppShell
        persona="clinician"
        brandContext="Clinical Identity"
        who="Dr. A. Reyes · Mercy General (signing institution)"
        banner={
          <>
            <span>🔒</span>
            <b>Clinical view.</b>
            <span>
              Synthetic / test-tagged records are hidden (spec §11.4). Demographics shown are detokenized at point
              of care via the FHIR bridge.
            </span>
          </>
        }
      >
        <Routes>
          <Route index element={<WorklistPage />} />
          <Route path="patients/:patientId" element={<PatientDetailPage />} />
          <Route path="*" element={<Navigate to="." replace />} />
        </Routes>
      </AppShell>
    </ClinicianStateProvider>
  );
}
