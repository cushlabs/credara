import type { CredaAuthorization } from '@shared/fhir/types';

/**
 * Local mirror of the mock bridge's seed authorization list — used to hydrate the patient
 * "who has access" view without needing a separate bridge endpoint. The real bridge will
 * expose Consent?patient=... and this helper goes away.
 */
export function mockSeedAuthorizations(): CredaAuthorization[] {
  return [
    auth('g-mercy', 'Mercy General Hospital', 'institution', 'Treatment', 'Read & rely', 'Identity + history', '2023-02-10', 'No expiry'),
    auth('g-northside', 'Northside Clinic', 'institution', 'Treatment', 'Read only', 'Identity only', '2024-06-01', '2025-06-01'),
    auth('g-apex', 'Apex Research — study NCT-1142', 'institution', 'Research', 'Read & export', 'Identity (de-identified)', '2024-09-15', '2026-09-15'),
    auth('g-qhin', 'Any TEFCA QHIN', 'class', 'Treatment', 'Read & rely', 'Identity only', '2023-01-01', 'No expiry'),
  ];
}

function auth(
  id: string,
  audience: string,
  audienceKind: 'institution' | 'class',
  purpose: CredaAuthorization['purpose'],
  use: CredaAuthorization['use'],
  scope: CredaAuthorization['scope'],
  since: string,
  expires: string,
): CredaAuthorization {
  return {
    resourceType: 'Consent',
    id,
    status: 'active',
    patient: { reference: 'Patient/p1' },
    audience,
    audienceKind,
    purpose,
    use,
    scope,
    since,
    expires,
    signedBy: 'Maria Gonzalez',
  };
}
