import { ReactNode, useEffect } from 'react';

export interface SlideOverProps {
  open: boolean;
  onClose: () => void;
  header: ReactNode;
  children: ReactNode;
}

/** Right-side slide-over panel for event detail. Mirrors the .sheet mockup pattern. */
export function SlideOver({ open, onClose, header, children }: SlideOverProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  return (
    <>
      <div className={['scrim', open && 'on'].filter(Boolean).join(' ')} onClick={onClose} />
      <aside className={['sheet', open && 'on'].filter(Boolean).join(' ')} aria-hidden={!open} data-testid="slideover">
        <div className="sh-hd">
          {header}
          <button className="x" onClick={onClose} aria-label="Close">
            ×
          </button>
        </div>
        <div className="sh-bd">{children}</div>
      </aside>
    </>
  );
}
