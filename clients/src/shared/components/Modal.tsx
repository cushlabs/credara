import { ReactNode, useEffect } from 'react';

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  header: ReactNode;
  body: ReactNode;
  /** Primary action button (label + click handler + optional className). */
  confirm?: { label: string; onClick: () => void; className?: string };
  /** Cancel button label. Default "Cancel". Pass null to suppress. */
  cancelLabel?: string | null;
}

export function Modal({ open, onClose, header, body, confirm, cancelLabel = 'Cancel' }: ModalProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  return (
    <div className={['modal', open && 'on'].filter(Boolean).join(' ')} role="dialog" aria-modal="true">
      <div className="modal-scrim" onClick={onClose} />
      <div className="card" data-testid="modal-card">
        <div className="m-hd">{header}</div>
        <div className="m-bd">{body}</div>
        <div className="m-ft">
          {cancelLabel !== null && (
            <button className="btn ghost" onClick={onClose}>
              {cancelLabel}
            </button>
          )}
          {confirm && (
            <button
              className={confirm.className ?? 'btn primary'}
              onClick={confirm.onClick}
              data-testid="modal-confirm"
            >
              {confirm.label}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
