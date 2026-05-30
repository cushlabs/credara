// Test orders + DTR questionnaires for the prior-auth flow.
// Faithful port of design/prior-auth-mockup.html ORDERS.

export type DtrFieldKind = 'text' | 'select' | 'textarea';

export interface DtrField {
  id: string;
  key: string;
  preset?: string;
  placeholder?: string;
  kind?: DtrFieldKind;
  options?: string[];
  required: boolean;
  /** True for fields the clinician must fill in (vs. pre-populated from the EHR). */
  gap: boolean;
  /** Source caption shown under the field key. */
  sourceKind: string;
}

export interface DtrQuestionnaire {
  title: string;
  fields: DtrField[];
}

export interface CrdResult {
  rule: string;
  latencyMs: number;
  note: string;
}

export interface AuthDecision {
  kind: 'approved' | 'denied' | 'info';
  title: string;
  sub: string;
  conditions: string[];
  rationale: string;
}

export interface Order {
  id: string;
  name: string;
  code: string;
  needsAuth: boolean;
  crd: CrdResult;
  dtr?: DtrQuestionnaire;
  decision?: AuthDecision;
}

export const ORDERS: Record<string, Order> = {
  mri: {
    id: 'mri',
    name: 'MRI Lumbar Spine without contrast',
    code: 'CPT 72148',
    needsAuth: true,
    crd: {
      rule: 'BlueChoice medical-necessity policy MR-LS-101',
      latencyMs: 400,
      note: 'Prior authorization required. Documentation template available (DTR).',
    },
    dtr: {
      title: 'BlueChoice PPO — MRI Lumbar Spine documentation',
      fields: [
        { id: 'dx', key: 'Indicated diagnosis', preset: 'M54.50 — Low back pain', sourceKind: 'EHR · Condition since 2024-08-12', required: true, gap: false },
        { id: 'dur', key: 'Pain duration ≥ 6 weeks', preset: 'Yes — symptom onset 2024-07-28', sourceKind: 'EHR · Encounter note 2024-09-04', required: true, gap: false },
        { id: 'tx', key: 'Conservative therapy tried', preset: 'Physical therapy × 8 weeks; ibuprofen 600 mg TID × 4 weeks', sourceKind: 'EHR · MedicationStatement + Encounter notes', required: true, gap: false },
        { id: 'imhx', key: 'Lumbar imaging in last 12 months', preset: 'None on file', sourceKind: 'EHR · ImagingStudy (none in window)', required: true, gap: false },
        {
          id: 'red',
          key: 'Red-flag symptoms',
          placeholder: 'Select one',
          kind: 'select',
          options: [
            'None',
            'Sciatica without weakness',
            'Sciatica with weakness',
            'Saddle anesthesia / bowel-bladder dysfunction',
            'Suspected malignancy / fracture / infection',
          ],
          sourceKind: 'Gap — clinician confirms',
          required: true,
          gap: true,
        },
        {
          id: 'just',
          key: 'Provider clinical justification',
          kind: 'textarea',
          placeholder: 'Briefly state why imaging is medically necessary now.',
          sourceKind: 'Gap — free-text justification',
          required: true,
          gap: true,
        },
      ],
    },
    decision: {
      kind: 'approved',
      title: 'APPROVED',
      sub: 'BlueChoice PPO · auth #BC-PA-441089',
      conditions: [
        'Valid for 30 days from today',
        'In-network facility only (BlueChoice radiology network)',
        'Single study (no repeat without new authorization)',
        'Notify within 24h if MR contrast becomes necessary',
      ],
      rationale:
        'Documentation establishes ≥ 6-week duration, prior conservative therapy, and clinical red-flag screening — meeting policy MR-LS-101.',
    },
  },
  specialty: {
    id: 'specialty',
    name: 'Ustekinumab 90 mg SubQ injection',
    code: 'HCPCS J3357',
    needsAuth: true,
    crd: {
      rule: 'BlueChoice specialty drug policy SP-D-227',
      latencyMs: 520,
      note: 'Step-therapy / specialty drug — prior authorization required.',
    },
    dtr: {
      title: 'BlueChoice PPO — Specialty drug authorization (Ustekinumab)',
      fields: [
        { id: 'dx', key: 'Indicated diagnosis', preset: 'L40.50 — Arthropathic psoriasis (Plaque psoriasis with PsA)', sourceKind: 'EHR · Condition list', required: true, gap: false },
        { id: 'step', key: 'Step-therapy: methotrexate tried', preset: 'Yes — methotrexate 15 mg/wk × 12 weeks, partial response', sourceKind: 'EHR · MedicationStatement', required: true, gap: false },
        {
          id: 'tnf',
          key: 'TNF-α inhibitor tried',
          placeholder: 'Yes / No',
          kind: 'select',
          options: ['No — clinical contraindication', 'Yes — failed adalimumab', 'Yes — failed etanercept'],
          sourceKind: 'Gap — clinician confirms',
          required: true,
          gap: true,
        },
        { id: 'tb', key: 'Latent TB screening', preset: 'Negative QuantiFERON 2024-11-02', sourceKind: 'EHR · Observation', required: true, gap: false },
        {
          id: 'just',
          key: 'Provider justification',
          kind: 'textarea',
          placeholder: 'Briefly justify step-up therapy.',
          sourceKind: 'Gap — free-text justification',
          required: true,
          gap: true,
        },
      ],
    },
    decision: {
      kind: 'approved',
      title: 'APPROVED — with quantity limit',
      sub: 'BlueChoice PPO · auth #BC-PA-441146',
      conditions: [
        'Authorized for 6 months',
        'Quantity limit: 1 syringe per 8 weeks (after induction)',
        'Specialty pharmacy dispensing only',
      ],
      rationale: 'Step-therapy criteria met; safety screening complete. Approved under SP-D-227 §3.',
    },
  },
  generic: {
    id: 'generic',
    name: 'Atorvastatin 40 mg tablets',
    code: 'RxNorm 617318',
    needsAuth: false,
    crd: { rule: 'Formulary tier 1 — no prior authorization required', latencyMs: 240, note: 'Covered under standard formulary.' },
  },
};

export const PATIENT_CTX = {
  name: 'Maria Gonzalez',
  dob: '1984-03-12',
  sex: 'Female',
  mrn: 'Mercy General · MRN 5582019',
  coverage: { payer: 'BlueChoice PPO', memberId: 'BC-5582-019', plan: 'Choice Plus Network' },
  confidence: 96,
};
