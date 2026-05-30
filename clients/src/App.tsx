import { Route, Routes, Navigate } from 'react-router-dom';
import { ToastProvider } from './shared/components/Toast';
import { Landing } from './Landing';
import { ClinicianApp } from './clinician/ClinicianApp';
import { PriorAuthApp } from './prior-auth/PriorAuthApp';
import { StewardApp } from './steward/StewardApp';
import { PatientApp } from './patient/PatientApp';
import { AuditApp } from './audit/AuditApp';

export function App() {
  return (
    <ToastProvider>
      <Routes>
        <Route path="/" element={<Landing />} />
        <Route path="/clinician/*" element={<ClinicianApp />} />
        <Route path="/prior-auth/*" element={<PriorAuthApp />} />
        <Route path="/steward/*" element={<StewardApp />} />
        <Route path="/patient/*" element={<PatientApp />} />
        <Route path="/audit/*" element={<AuditApp />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </ToastProvider>
  );
}
