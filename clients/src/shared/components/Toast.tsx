import { createContext, ReactNode, useCallback, useContext, useEffect, useRef, useState } from 'react';

interface ToastCtx {
  show: (msg: string) => void;
}

const Ctx = createContext<ToastCtx>({ show: () => undefined });

export function useToast(): ToastCtx {
  return useContext(Ctx);
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [msg, setMsg] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  const show = useCallback((m: string) => {
    setMsg(m);
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setMsg(null), 2600);
  }, []);

  useEffect(() => () => {
    if (timer.current) window.clearTimeout(timer.current);
  }, []);

  return (
    <Ctx.Provider value={{ show }}>
      {children}
      <div className={['toast', msg && 'on'].filter(Boolean).join(' ')} role="status" aria-live="polite" data-testid="toast">
        {msg && (
          <>
            <span>✓</span> {msg}
          </>
        )}
      </div>
    </Ctx.Provider>
  );
}
