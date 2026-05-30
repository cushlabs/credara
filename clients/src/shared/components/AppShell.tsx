import { ReactNode, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { isMockBridge } from '../fhir/client';

export type Persona = 'clinician' | 'prior-auth' | 'steward' | 'patient' | 'audit';

export interface AppShellProps {
  persona: Persona;
  /** Shown in the dark app bar, after the brand mark. */
  brandContext: string;
  /** Who-is-signed-in line on the right side of the app bar. */
  who: string;
  /** Read-only chip (compliance/audit persona). */
  readOnly?: boolean;
  /** The pale banner under the app bar — clinical view / operator view / etc. */
  banner: ReactNode;
  /** Wrap children in the standard .wrap container. Set false for full-bleed layouts. */
  wrap?: boolean;
  children: ReactNode;
}

/**
 * Common page chrome for all five persona apps — dark app bar, persona-tinted banner,
 * optional content wrapper. Sets `data-persona` on <html> so tokens.css can swap the
 * accent palette.
 */
export function AppShell({
  persona,
  brandContext,
  who,
  readOnly,
  banner,
  wrap = true,
  children,
}: AppShellProps) {
  useEffect(() => {
    document.documentElement.setAttribute('data-persona', persona);
    return () => {
      document.documentElement.removeAttribute('data-persona');
    };
  }, [persona]);

  return (
    <>
      <div className="appbar" data-testid="appbar">
        <div className="brand">
          <span className="mark">C</span>
          Creda <span className="ctx">· {brandContext}</span>
        </div>
        <div className="navlinks" aria-label="Personas">
          <Link to="/">All personas</Link>
        </div>
        <div className="spacer" />
        <div className="who">
          {readOnly && <span className="ro">READ-ONLY</span>}
          {!readOnly && <span className="dot" />}
          {who}
        </div>
      </div>
      <div className="viewbanner" data-testid="viewbanner">
        {banner}
        {isMockBridge() && (
          <span
            className="mockchip"
            data-testid="mock-bridge-chip"
            title="VITE_FHIR_BASE is unset or 'mock'. Writes never leave this browser tab; no FHIR bridge, no creda-core, no peer gossip. See clients/README.md."
          >
            MOCK BRIDGE · no gossip
          </span>
        )}
      </div>
      {wrap ? <div className="wrap">{children}</div> : children}
    </>
  );
}
