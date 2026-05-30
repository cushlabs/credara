import { truncate } from '../lib/format';

export type EventType =
  | 'Assert'
  | 'Attest'
  | 'Link'
  | 'Contest'
  | 'Amend'
  | 'Tombstone'
  | 'AuthorizationGrant'
  | 'AuthorizationRevocation'
  | 'ExportReceipt';

export interface DagNode {
  id: string;
  type: EventType;
  inst: string;
  when?: string;
  /** Display sub-text on the right side of the node (vm / method / purpose / etc.). */
  sub?: string;
  /** Layout position. */
  x: number;
  y: number;
  parents: string[];
  /** Render as blocked (hatched + dashed). */
  blocked?: boolean;
  /** Render as inert (faded with dashed border) — depends on a blocked Link. */
  inert?: boolean;
  /** Render with a new-pending-replication dashed light border. */
  fresh?: boolean;
}

const TYPE_COLORS: Record<EventType, string> = {
  Assert: 'var(--assert)',
  Attest: 'var(--attest)',
  Link: 'var(--link)',
  Contest: 'var(--contest)',
  Amend: 'var(--amend)',
  Tombstone: 'var(--tomb)',
  AuthorizationGrant: 'var(--auth)',
  AuthorizationRevocation: 'var(--revoke)',
  ExportReceipt: 'var(--export)',
};

export interface EventDagProps {
  nodes: DagNode[];
  onNodeClick?: (id: string) => void;
  /** Optional minimum canvas size; the DAG sizes to fit its contents otherwise. */
  minWidth?: number;
  minHeight?: number;
}

const NODE_W = 150;
const NODE_H = 54;
const MARGIN = 18;

export function EventDag({ nodes, onNodeClick, minWidth = 620, minHeight = 290 }: EventDagProps) {
  const byId = Object.fromEntries(nodes.map((n) => [n.id, n] as const));
  const W = Math.max(minWidth, ...nodes.map((e) => e.x + NODE_W)) + MARGIN;
  const H = Math.max(minHeight, ...nodes.map((e) => e.y + NODE_H)) + MARGIN;

  const linkCol = 'var(--link)';
  return (
    <svg className="dag" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMin meet">
      <defs>
        <pattern id="blockedHatch" patternUnits="userSpaceOnUse" width="8" height="8" patternTransform="rotate(45)">
          <rect width="8" height="8" fill="#ffffff" />
          <line x1="0" y1="0" x2="0" y2="8" stroke={linkCol} strokeWidth="2" opacity="0.55" />
        </pattern>
      </defs>
      {nodes.flatMap((e) =>
        e.parents.map((pid) => {
          const a = byId[pid];
          if (!a) return null;
          const x1 = a.x + NODE_W;
          const y1 = a.y + NODE_H / 2;
          const x2 = e.x;
          const y2 = e.y + NODE_H / 2;
          const mx = (x1 + x2) / 2;
          const isInertEdge = e.blocked || e.inert || a.blocked || a.inert;
          const cls = e.type === 'Contest' ? 'edge contest' : isInertEdge ? 'edge blocked' : 'edge';
          return <path key={`${pid}->${e.id}`} className={cls} d={`M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`} />;
        }),
      )}
      {nodes.map((e) => {
        const col = TYPE_COLORS[e.type] ?? 'var(--ink-2)';
        const sub = e.sub ?? e.when ?? e.inst;
        const handle = onNodeClick ? () => onNodeClick(e.id) : undefined;

        if (e.blocked) {
          return (
            <g key={e.id} className="node" onClick={handle} data-testid={`dag-node-${e.id}`}>
              <rect
                x={e.x}
                y={e.y}
                width={NODE_W}
                height={NODE_H}
                rx={11}
                fill="url(#blockedHatch)"
                stroke={linkCol}
                strokeWidth={2}
                strokeDasharray="6 4"
              />
              <text x={e.x + 12} y={e.y + 21} fill={linkCol}>
                {e.type}
              </text>
              <text className="sub" x={e.x + 12} y={e.y + 38} fill={linkCol} opacity={0.85}>
                {truncate(e.inst, 18)}
              </text>
              <text className="sub" x={e.x + NODE_W - 12} y={e.y + 38} textAnchor="end" fill={linkCol} opacity={0.85}>
                {truncate(sub, 16)}
              </text>
              <g transform={`translate(${e.x + NODE_W - 50},${e.y - 9})`}>
                <rect width={56} height={18} rx={9} fill="#b91c1c" />
                <text x={28} y={13} textAnchor="middle" fontSize={10} fontWeight={700} fill="#ffffff">
                  BLOCKED
                </text>
              </g>
            </g>
          );
        }

        if (e.inert) {
          return (
            <g key={e.id} className="node" onClick={handle} opacity={0.55} data-testid={`dag-node-${e.id}`}>
              <rect
                x={e.x}
                y={e.y}
                width={NODE_W}
                height={NODE_H}
                rx={11}
                fill={col}
                stroke={col}
                strokeWidth={1.5}
                strokeDasharray="6 4"
              />
              <text x={e.x + 12} y={e.y + 21}>{e.type}</text>
              <text className="sub" x={e.x + 12} y={e.y + 38}>
                {truncate(e.inst, 18)}
              </text>
              <text className="sub" x={e.x + NODE_W - 12} y={e.y + 38} textAnchor="end">
                {truncate(sub, 16)}
              </text>
              <g transform={`translate(${e.x + NODE_W - 44},${e.y - 9})`}>
                <rect width={50} height={18} rx={9} fill="#b91c1c" />
                <text x={25} y={13} textAnchor="middle" fontSize={10} fontWeight={700} fill="#ffffff">
                  INERT
                </text>
              </g>
            </g>
          );
        }

        return (
          <g key={e.id} className="node" onClick={handle} data-testid={`dag-node-${e.id}`}>
            <rect
              x={e.x}
              y={e.y}
              width={NODE_W}
              height={NODE_H}
              rx={11}
              fill={col}
              stroke={e.fresh ? '#fff' : undefined}
              strokeDasharray={e.fresh ? '4 3' : undefined}
              strokeWidth={e.fresh ? 2 : undefined}
            />
            <text x={e.x + 12} y={e.y + 21}>{e.type}</text>
            <text className="sub" x={e.x + 12} y={e.y + 38}>
              {truncate(e.inst, 18)}
            </text>
            <text className="sub" x={e.x + NODE_W - 12} y={e.y + 38} textAnchor="end">
              {truncate(sub, 16)}
            </text>
          </g>
        );
      })}
      <style>
        {`
          .dag { width: 100%; height: auto; display: block; }
          .node { cursor: pointer; }
          .node rect { transition: filter .1s ease; }
          .node:hover rect { filter: brightness(1.06); }
          .node text { font-size: 11px; fill: #fff; font-weight: 600; pointer-events: none; }
          .node .sub { font-size: 9.5px; fill: rgba(255,255,255,.85); font-weight: 500; }
          .edge { stroke: #b9c6d8; stroke-width: 2; fill: none; }
          .edge.contest { stroke: var(--contest); stroke-dasharray: 5 4; }
          .edge.blocked { stroke: #b91c1c; stroke-width: 1.6; stroke-dasharray: 4 4; opacity: .85; }
        `}
      </style>
    </svg>
  );
}

export function dagLegend(items: Array<[EventType, string]>) {
  return items.map(([n, c]) => ({ name: n, color: c }));
}

export const DEFAULT_LEGEND: Array<[EventType, string]> = [
  ['Assert', 'var(--assert)'],
  ['Link', 'var(--link)'],
  ['Attest', 'var(--attest)'],
  ['Contest', 'var(--contest)'],
  ['Amend', 'var(--amend)'],
];

export function DagLegend({ items = DEFAULT_LEGEND }: { items?: Array<[EventType, string]> }) {
  return (
    <div
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 12,
        padding: '10px 16px 0',
        fontSize: 12,
        color: 'var(--ink-2)',
      }}
    >
      {items.map(([n, c]) => (
        <span key={n} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ width: 11, height: 11, borderRadius: 3, background: c, display: 'inline-block' }} />
          {n}
        </span>
      ))}
    </div>
  );
}

export { TYPE_COLORS as EVENT_TYPE_COLORS };

export const TYPE_DESC: Record<EventType, string> = {
  Assert: 'An institution asserted demographics for this patient (§3.4.1).',
  Attest: 'An institution recorded reliance on this identity chain for a stated purpose (§3.4.4).',
  Link: 'Two subgraphs were asserted to be the same person, with a confidence score (§3.4.2).',
  Contest: 'A party to the subgraph disputed a link (§3.4.3).',
  Amend: 'The originating institution corrected demographics (§3.4.5).',
  Tombstone: 'Content was scrubbed while graph topology is preserved (§3.4.6).',
  AuthorizationGrant: 'The patient granted an institution access for a purpose (§4.3).',
  AuthorizationRevocation: 'An authorization was revoked, signed by the granting party (§4.7).',
  ExportReceipt: 'An authorized export of data was recorded (§4.5, §10.2).',
};
