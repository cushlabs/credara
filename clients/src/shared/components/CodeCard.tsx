import { ReactNode } from 'react';

export interface CodeLine {
  key: string;
  value: string;
}

/**
 * Dark-coded "signed-event preview" card. Used in every persona's confirm-modal and slide-over
 * to show what's about to be (or was just) appended to the patient subgraph.
 */
export function CodeCard({ lines, header }: { lines: CodeLine[]; header?: ReactNode }) {
  // Width-align keys so the colons line up — same effect as the manual &nbsp; padding in the
  // mockups, but compiled per-render so it works for any key set.
  const maxKey = lines.reduce((m, l) => Math.max(m, l.key.length), 0);
  return (
    <div className="codecard">
      {header && (
        <>
          {header}
          {'\n'}
        </>
      )}
      {lines.map((l, i) => {
        const pad = ' '.repeat(maxKey - l.key.length);
        return (
          <span key={i}>
            <span className="kk">{l.key}</span>
            {`${pad}: `}
            <span className="vv">{l.value}</span>
            {i < lines.length - 1 ? '\n' : ''}
          </span>
        );
      })}
    </div>
  );
}
