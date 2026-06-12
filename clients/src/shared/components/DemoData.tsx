import { isMockBridge } from '../fhir/client';

/**
 * Visible data-provenance marker (front-end de-fixturing plan, #1). When the app is connected to a
 * REAL bridge, any surface still backed by fixtures / local state renders this chip so demo data
 * never silently impersonates live network data — the failure mode that drove the whack-a-mole
 * bug hunt (consent badge, prior-auth decision, audit ledger, etc. all looked real but weren't).
 *
 * In MOCK mode the global "MOCK BRIDGE · no gossip" chip in AppShell already says everything is
 * local, so per-surface chips would be redundant noise — suppressed here. The chip is therefore a
 * precise signal: "you are on a real bridge, but THIS is not from it yet."
 */
export function DemoData({ what = 'Demo data', detail }: { what?: string; detail?: string }) {
  if (isMockBridge()) return null;
  return (
    <span
      data-testid="demo-data"
      title={detail ?? 'Not from your Creda network — fixture/demo data, pending real wiring.'}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        fontSize: 11,
        fontWeight: 700,
        letterSpacing: 0.3,
        color: '#92400e',
        background: '#fef3c7',
        border: '1px solid #fcd34d',
        borderRadius: 6,
        padding: '2px 7px',
        textTransform: 'uppercase',
        whiteSpace: 'nowrap',
      }}
    >
      ⚠ {what}
    </span>
  );
}
