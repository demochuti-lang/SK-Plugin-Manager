import {
  createContext,
  useCallback,
  useContext,
  useRef,
  useState,
} from 'react';
import type { ReactNode } from 'react';
import { IconAlert, IconCheck, IconInfo, IconX } from './Icons';

export type ToastKind = 'success' | 'error' | 'info';

interface ToastItem {
  id: number;
  kind: ToastKind;
  title: string;
  message?: string;
}

interface ToastApi {
  notify: (kind: ToastKind, title: string, message?: string) => void;
  success: (title: string, message?: string) => void;
  error: (title: string, message?: string) => void;
  info: (title: string, message?: string) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used inside <ToastProvider>');
  return ctx;
}

const KIND_STYLE: Record<ToastKind, { icon: (c?: string) => ReactNode; ring: string }> = {
  success: {
    icon: (c) => <IconCheck className={c} />,
    ring: 'bg-green-500/15 text-green-600 dark:text-green-400',
  },
  error: {
    icon: (c) => <IconAlert className={c} />,
    ring: 'bg-red-500/15 text-red-600 dark:text-red-400',
  },
  info: {
    icon: (c) => <IconInfo className={c} />,
    ring: 'bg-blue-500/15 text-blue-600 dark:text-blue-400',
  },
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const idRef = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const notify = useCallback(
    (kind: ToastKind, title: string, message?: string) => {
      const id = idRef.current++;
      setToasts((prev) => [...prev.slice(-4), { id, kind, title, message }]);
      window.setTimeout(() => dismiss(id), 4500);
    },
    [dismiss],
  );

  const api: ToastApi = {
    notify,
    success: (title, message) => notify('success', title, message),
    error: (title, message) => notify('error', title, message),
    info: (title, message) => notify('info', title, message),
  };

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div
        className="pointer-events-none fixed right-4 top-4 z-[100] flex w-[340px] flex-col gap-2"
        aria-live="polite"
      >
        {toasts.map((t) => {
          const style = KIND_STYLE[t.kind];
          return (
            <div
              key={t.id}
              className="sk-toast-in pointer-events-auto flex items-start gap-3 rounded-2xl border border-black/[0.06] bg-white/85 p-3.5 shadow-[0_8px_30px_rgba(0,0,0,0.12)] backdrop-blur-xl dark:border-white/10 dark:bg-[#1e1e24]/90"
            >
              <span
                className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-full ${style.ring}`}
              >
                {style.icon('h-4 w-4')}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-[13px] font-semibold leading-tight text-neutral-900 dark:text-neutral-100">
                  {t.title}
                </p>
                {t.message && (
                  <p className="mt-0.5 break-words text-xs leading-snug text-neutral-500 dark:text-neutral-400">
                    {t.message}
                  </p>
                )}
              </div>
              <button
                onClick={() => dismiss(t.id)}
                className="rounded-lg p-1 text-neutral-400 transition-colors hover:bg-black/5 hover:text-neutral-600 dark:hover:bg-white/10 dark:hover:text-neutral-300"
                aria-label="Dismiss"
              >
                <IconX className="h-3.5 w-3.5" />
              </button>
            </div>
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}
