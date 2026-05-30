import { useEffect } from 'react';
import { Link } from 'react-router-dom';

interface PersonaLink {
  path: string;
  name: string;
  description: string;
}

const PERSONAS: PersonaLink[] = [
  {
    path: '/clinician',
    name: 'Clinician',
    description:
      'Treating clinician at point of care. Clinical view; synthetic / test-tagged records are filtered. Identity review + attest/contest/amend.',
  },
  {
    path: '/prior-auth',
    name: 'Prior authorization',
    description: 'Da Vinci CRD / DTR / PAS in-workflow prior auth; the submission is recorded as a signed Attest.',
  },
  {
    path: '/steward',
    name: 'Identity steward',
    description: 'Operator view of the merged subgraph; resolves duplicates, conflicts, contests; Link / Contest / Amend / Tombstone.',
  },
  {
    path: '/patient',
    name: 'Patient consent',
    description: 'Patient-controlled grants and revocations. Every choice is signed by the patient key on their own device.',
  },
  {
    path: '/audit',
    name: 'Compliance & audit',
    description:
      'Read-only review of authorization activity, provenance integrity, and §4.6 step 5.5 link-chain decisions.',
  },
];

export function Landing() {
  useEffect(() => {
    document.documentElement.removeAttribute('data-persona');
  }, []);
  return (
    <div className="landing">
      <h1>Creda clients</h1>
      <p className="lead">
        Five persona-specific UIs, ported from the design mockups onto a typed FHIR bridge client. Mock mode is on
        unless <code>VITE_FHIR_BASE</code> points at a live bridge.
      </p>
      <div className="personagrid">
        {PERSONAS.map((p) => (
          <Link key={p.path} to={p.path} className="personacard" data-testid={`persona-${p.path.slice(1)}`}>
            <div className="nm">{p.name}</div>
            <div className="ds">{p.description}</div>
            <span className="rt">Open →</span>
          </Link>
        ))}
      </div>
    </div>
  );
}
