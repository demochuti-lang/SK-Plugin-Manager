import { useEffect, useState } from 'react';
import type { ComponentType } from 'react';
import type { HistoryEntry } from '../types';
import { historyList } from '../lib/tauri';
import { useToast } from '../components/Toast';
import { Pill, Spinner, card } from '../components/ui';
import {
  IconArrowUp,
  IconDownload,
  IconHistory,
  IconInbox,
  IconRefresh,
  IconSearch,
  IconTrash,
} from '../components/Icons';

const EVENT_META: Record<
  HistoryEntry['event'],
  {
    label: string;
    icon: ComponentType<{ className?: string }>;
    dot: string;
    pill: 'green' | 'red' | 'blue' | 'violet' | 'gray' | 'amber';
  }
> = {
  install: {
    label: 'Installed',
    icon: IconDownload,
    dot: 'bg-green-500/15 text-green-600 dark:text-green-400',
    pill: 'green',
  },
  uninstall: {
    label: 'Uninstalled',
    icon: IconTrash,
    dot: 'bg-red-500/15 text-red-600 dark:text-red-400',
    pill: 'red',
  },
  update: {
    label: 'Updated',
    icon: IconArrowUp,
    dot: 'bg-blue-500/15 text-blue-600 dark:text-blue-400',
    pill: 'blue',
  },
  import: {
    label: 'Imported',
    icon: IconInbox,
    dot: 'bg-violet-500/15 text-violet-600 dark:text-violet-400',
    pill: 'violet',
  },
  scan: {
    label: 'Scanned',
    icon: IconSearch,
    dot: 'bg-neutral-500/15 text-neutral-500 dark:text-neutral-400',
    pill: 'gray',
  },
  backup: {
    label: 'Backed up',
    icon: IconDownload,
    dot: 'bg-blue-500/15 text-blue-600 dark:text-blue-400',
    pill: 'blue',
  },
  restore: {
    label: 'Restored',
    icon: IconRefresh,
    dot: 'bg-amber-500/15 text-amber-600 dark:text-amber-400',
    pill: 'amber',
  },
};

export default function History() {
  const toast = useToast();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await historyList(200);
        if (!cancelled) setEntries(list);
      } catch (e) {
        if (!cancelled)
          toast.error('Could not load history', e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [toast]);

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5">
      <div>
        <h2 className="text-lg font-semibold text-neutral-900 dark:text-white">
          History
        </h2>
        <p className="mt-0.5 text-sm text-neutral-500 dark:text-neutral-400">
          Every install, update, import, backup, restore and scan, newest first.
        </p>
      </div>

      {loading ? (
        <div className={`${card} flex items-center justify-center gap-3 p-10`}>
          <Spinner className="h-5 w-5 text-blue-600" />
          <p className="text-sm text-neutral-500">Loading history...</p>
        </div>
      ) : entries.length === 0 ? (
        <div className={`${card} sk-rise p-10 text-center`}>
          <span className="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl bg-neutral-500/10 text-neutral-400">
            <IconHistory className="h-6 w-6" />
          </span>
          <p className="mt-3 text-sm font-medium text-neutral-800 dark:text-neutral-200">
            No activity yet
          </p>
          <p className="mx-auto mt-1 max-w-sm text-xs text-neutral-500 dark:text-neutral-400">
            Install your first plugin and it will show up here.
          </p>
        </div>
      ) : (
        <div className="relative">
          {/* Timeline rail */}
          <div className="absolute bottom-4 left-[19px] top-4 w-px bg-black/[0.08] dark:bg-white/[0.1]" />
          <div className="flex flex-col gap-3">
            {entries.map((e) => {
              const meta = EVENT_META[e.event];
              const Icon = meta.icon;
              return (
                <div key={e.id} className="sk-rise relative flex items-start gap-4">
                  <span
                    className={`z-10 flex h-10 w-10 shrink-0 items-center justify-center rounded-full ring-4 ring-[#f5f5f7] dark:ring-[#0e0e12] ${meta.dot}`}
                  >
                    <Icon className="h-4 w-4" />
                  </span>
                  <div className={`${card} min-w-0 flex-1 px-4 py-3`}>
                    <div className="flex flex-wrap items-center gap-2">
                      <p className="truncate text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                        {e.plugin_name}
                      </p>
                      <Pill tone={meta.pill}>{meta.label}</Pill>
                    </div>
                    {e.detail && (
                      <p className="mt-1 break-words text-xs text-neutral-500 dark:text-neutral-400">
                        {e.detail}
                      </p>
                    )}
                    <p className="mt-1.5 font-mono text-[11px] text-neutral-400 dark:text-neutral-500">
                      {e.created_at}
                    </p>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
