// FHIR R4 types — narrow surface only. We deliberately do not pull in the full @types/fhir
// package; only the shapes the Creda bridge actually returns/accepts are needed.
//
// The bridge's CredaPatient / CredaProvenance / CredaAuthorization profiles are documented at
// bridge/src/main/kotlin/health/creda/bridge/providers/*.kt and are TODO-tagged where the
// translator stubs still need to be filled in. These types describe the **target** shape so
// the clients can be written against the eventual bridge today; the mock adapter returns
// exactly the same shape.

export interface FhirReference {
  reference: string;
  display?: string;
}

export interface FhirCoding {
  system?: string;
  code?: string;
  display?: string;
}

export interface FhirCodeableConcept {
  coding?: FhirCoding[];
  text?: string;
}

export interface FhirExtension {
  url: string;
  valueString?: string;
  valueDecimal?: number;
  valueBoolean?: boolean;
  valueCodeableConcept?: FhirCodeableConcept;
}

export interface FhirHumanName {
  use?: 'official' | 'usual' | 'nickname';
  text?: string;
  family?: string;
  given?: string[];
}

export interface FhirIdentifier {
  system?: string;
  value?: string;
  type?: FhirCodeableConcept;
}

export interface FhirAddress {
  text?: string;
  city?: string;
  state?: string;
  postalCode?: string;
}

// ---- CredaPatient (US Core Patient + Creda extensions, §8.2.2) -------------------------------

export interface CredaPatient {
  resourceType: 'Patient';
  id: string;
  meta?: { profile?: string[] };
  name?: FhirHumanName[];
  birthDate?: string;
  gender?: 'male' | 'female' | 'other' | 'unknown';
  address?: FhirAddress[];
  identifier?: FhirIdentifier[];
  /** Per-field confidence + disputed-value extensions (§8.1.3-§8.1.4). */
  extension?: FhirExtension[];
}

// ---- CredaProvenance (one per identity event, §8.2.3) ----------------------------------------

export type CredaEventType =
  | 'Assert'
  | 'Attest'
  | 'Link'
  | 'Contest'
  | 'Amend'
  | 'Tombstone'
  | 'AuthorizationGrant'
  | 'AuthorizationRevocation'
  | 'ExportReceipt';

export interface CredaProvenance {
  resourceType: 'Provenance';
  id: string;
  meta?: { profile?: string[] };
  recorded: string; // ISO timestamp
  target: FhirReference[];
  /** Creda event_type — extension on Provenance.activity per §8.2.3. */
  eventType: CredaEventType;
  /** Originating institution (signer). */
  institution: string;
  /** Verification method for Assert events. */
  verificationMethod?: string;
  /** Match score for Link events (e.g. "94%" or 9400 / 10000). */
  matchScore?: string;
  /** Link method for Link events. */
  linkMethod?: 'InsuranceCrosswalk' | 'Referral' | 'Algorithmic' | 'Manual' | 'Other';
  /** Purpose for Attest / Grant. */
  purpose?: string;
  /** Parent provenance ids (DAG edges). */
  parents: string[];
  /** Human summary the projection rendered. */
  summary?: string;
  /** Signature verification — set by the verifier. */
  signature?: { algorithm: string; verified: boolean };
}

// ---- CredaAuthorization (Consent profile, §8.2.9) --------------------------------------------

export type GrantStatus = 'active' | 'revoked' | 'expired';
export type GrantPurpose =
  | 'Treatment'
  | 'Payment'
  | 'Operations'
  | 'Public health'
  | 'Research'
  | 'AI training'
  | 'AI inference'
  | 'Federal program';
export type UseMode = 'Read only' | 'Read & rely' | 'Read & export';
export type GrantScope = 'Identity only' | 'Identity + history' | 'Identity (de-identified)';

export interface CredaAuthorization {
  resourceType: 'Consent';
  id: string;
  status: GrantStatus;
  patient: FhirReference;
  audience: string;
  audienceKind: 'institution' | 'class';
  purpose: GrantPurpose;
  use: UseMode;
  scope: GrantScope;
  since: string;
  expires: string;
  signedBy?: string;
}

// ---- $creda-verify response ------------------------------------------------------------------

export interface AuthorizationDecision {
  decision: 'authorized' | 'denied-revoked' | 'denied-expired' | 'denied-no-grant';
  reason: string;
  /** The Grant id that governed the decision, if any. */
  governingGrant?: string;
  /** The age of the DAG view at evaluation time (the Verifier may be offline-stale, §10.3.3). */
  viewAgeMs?: number;
}
