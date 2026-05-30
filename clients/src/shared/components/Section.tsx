import { CSSProperties, ReactNode } from 'react';

export interface SectionProps {
  title: ReactNode;
  /** Element rendered on the right side of the section header (badge, link, etc.). */
  aside?: ReactNode;
  bodyStyle?: CSSProperties;
  className?: string;
  children: ReactNode;
}

export function Section({ title, aside, bodyStyle, className, children }: SectionProps) {
  return (
    <div className={['section', className].filter(Boolean).join(' ')}>
      <div className="hd">
        <h2>{title}</h2>
        {aside && <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 8 }}>{aside}</div>}
      </div>
      <div className="bd" style={bodyStyle}>
        {children}
      </div>
    </div>
  );
}
