import { CSSProperties, ReactNode } from 'react';
import { classNames } from '../lib/format';

export type BadgeVariant =
  | 'good'
  | 'warn'
  | 'info'
  | 'neutral'
  | 'violation'
  | 'grant'
  | 'revoke'
  | 'export'
  | 'pass'
  | 'test'
  | 'policy'
  | 'blocked'
  | 'dup'
  | 'conflict'
  | 'contest'
  | 'synthetic'
  | 'stale';

export interface BadgeProps {
  variant?: BadgeVariant;
  /** Optional coloured dot prefix; uses the variant colour by default when set to true. */
  dot?: boolean | string;
  style?: CSSProperties;
  children: ReactNode;
}

/**
 * Pill badge — used everywhere for state chips (Active / Disputed / TEST DATA / etc.).
 * The variant maps to the `.b-*` classes in globals.css.
 */
export function Badge({ variant = 'neutral', dot, style, children }: BadgeProps) {
  return (
    <span className={classNames('badge', `b-${variant}`)} style={style}>
      {dot && <span className="d" style={typeof dot === 'string' ? { background: dot } : undefined} />}
      {children}
    </span>
  );
}
