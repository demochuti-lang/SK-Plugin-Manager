import type { ReactNode } from 'react';

/** Shared Apple-style card surface. */
export const card =
  'rounded-2xl bg-white dark:bg-[#1b1b20] border border-black/[0.06] dark:border-white/[0.08] shadow-[0_1px_3px_rgba(0,0,0,0.05)]';

type PillTone = 'blue' | 'green' | 'violet' | 'amber' | 'gray' | 'red';

const PILL_TONES: Record<PillTone, string> = {
  blue: 'bg-blue-500/12 text-blue-700 dark:text-blue-300',
  green: 'bg-green-500/12 text-green-700 dark:text-green-300',
  violet: 'bg-violet-500/12 text-violet-700 dark:text-violet-300',
  amber: 'bg-amber-500/15 text-amber-700 dark:text-amber-300',
  gray: 'bg-neutral-500/12 text-neutral-600 dark:text-neutral-400',
  red: 'bg-red-500/12 text-red-700 dark:text-red-300',
};

export function Pill({
  tone,
  children,
  title,
}: {
  tone: PillTone;
  children: ReactNode;
  title?: string;
}) {
  return (
    <span
      title={title}
      className={`inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-[11px] font-semibold leading-4 ${PILL_TONES[tone]}`}
    >
      {children}
    </span>
  );
}

/** Apple-style toggle switch. */
export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors duration-200 ${
        checked ? 'bg-green-500' : 'bg-neutral-300 dark:bg-neutral-600'
      }`}
    >
      <span
        className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all duration-200 ${
          checked ? 'left-[22px]' : 'left-0.5'
        }`}
      />
    </button>
  );
}

/** Small spinning loader. */
export function Spinner({ className = 'h-4 w-4' }: { className?: string }) {
  return (
    <span
      className={`inline-block animate-spin rounded-full border-2 border-current border-t-transparent ${className}`}
      aria-hidden="true"
    />
  );
}
