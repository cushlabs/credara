import { confColor } from '../lib/format';

export interface ConfidenceMeterProps {
  percent: number;
  width?: number;
  /** Show the numeric percent on the left. */
  showLabel?: boolean;
  align?: 'left' | 'right';
}

export function ConfidenceMeter({ percent, width = 90, showLabel = true, align = 'left' }: ConfidenceMeterProps) {
  const color = confColor(percent);
  return (
    <div className="conf" style={{ justifyContent: align === 'right' ? 'flex-end' : 'flex-start' }}>
      {showLabel && (
        <span className="pct" style={{ color }}>
          {percent}%
        </span>
      )}
      <div className="meter" style={{ width }}>
        <span style={{ width: `${percent}%`, background: color }} />
      </div>
    </div>
  );
}
