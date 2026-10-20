import { useCallback, useEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import { basename } from '../lib/apps';
import { installDropped } from '../lib/tauri';
import { useToast } from '../components/Toast';
import { Spinner, card } from '../components/ui';
import { IconCheck, IconFolder, IconUpload, IconX } from '../components/Icons';

interface Job {
  path: string;
  status: 'working' | 'ok' | 'fail';
  message?: string;
}

const FORMAT_CHIPS = ['.zxp', '.jsx', '.jsxbin', 'folder'];

export default function DropZone({ onChanged }: { onChanged: () => void }) {
  const toast = useToast();
  const [hovering, setHovering] = useState(false);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);

  const handlePaths = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0 || busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      setHovering(false);
      setJobs(paths.map((p) => ({ path: p, status: 'working' as const })));
      try {
        const results = await installDropped(paths);
        const next: Job[] = results.map((r) => ({
          path: r.path,
          status: r.success ? ('ok' as const) : ('fail' as const),
          message: r.message,
        }));
        setJobs(next);
        for (const r of results) {
          if (r.success) {
            toast.success(r.was_update ? 'Updated' : 'Installed', r.message);
          } else {
            toast.error('Install failed', r.message);
          }
        }
        onChanged();
      } catch (e) {
        toast.error('Install failed', e instanceof Error ? e.message : String(e));
        setJobs([]);
      } finally {
        busyRef.current = false;
        setBusy(false);
      }
    },
    [onChanged, toast],
  );

  // Real Tauri drag-drop. Guarded: drag-drop events only fire inside the
  // Tauri webview; in plain `vite dev` browser this silently no-ops.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const off = await getCurrentWebview().onDragDropEvent((event) => {
          const t = event.payload.type;
          if (t === 'enter' || t === 'over') {
            setHovering(true);
          } else if (t === 'leave') {
            setHovering(false);
          } else if (t === 'drop') {
            setHovering(false);
            void handlePaths(event.payload.paths);
          }
        });
        if (cancelled) off();
        else unlisten = off;
      } catch {
        // Not running inside Tauri (e.g. vite dev in a browser) — ignore.
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handlePaths]);

  const browse = async () => {
    let sel: string | string[] | null;
    try {
      sel = await open({
        multiple: true,
        filters: [
          { name: 'Plugin packages', extensions: ['zxp', 'jsx', 'jsxbin'] },
          { name: 'All files', extensions: ['*'] },
        ],
      });
    } catch (e) {
      toast.error('Could not open file picker', e instanceof Error ? e.message : String(e));
      return;
    }
    if (!sel) return;
    await handlePaths(Array.isArray(sel) ? sel : [sel]);
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5">
      <div>
        <h2 className="text-lg font-semibold text-neutral-900 dark:text-white">
          Install plugins
        </h2>
        <p className="mt-0.5 text-sm text-neutral-500 dark:text-neutral-400">
          Drop extension packages or scripts and they will be installed into the
          right Adobe app automatically.
        </p>
      </div>

      {/* Drop area */}
      <div
        className={`sk-rise flex min-h-[280px] flex-col items-center justify-center gap-3 rounded-3xl border-2 border-dashed p-10 text-center transition-all duration-200 ${
          hovering
            ? 'scale-[1.01] border-blue-500 bg-blue-500/[0.07] shadow-[0_0_0_6px_rgba(59,130,246,0.12)]'
            : 'border-neutral-300 bg-white/50 dark:border-neutral-700 dark:bg-white/[0.02]'
        } ${busy ? 'pointer-events-none opacity-70' : ''}`}
      >
        <span
          className={`flex h-16 w-16 items-center justify-center rounded-3xl transition-colors ${
            hovering
              ? 'bg-blue-500/20 text-blue-600 dark:text-blue-400'
              : 'bg-blue-500/12 text-blue-600 dark:text-blue-400'
          }`}
        >
          {busy ? <Spinner className="h-7 w-7" /> : <IconUpload className="h-7 w-7" />}
        </span>
        <div>
          <p className="text-base font-semibold text-neutral-900 dark:text-white">
            {busy ? 'Installing...' : 'Drop .zxp files or folders here'}
          </p>
          <p className="mt-1 text-sm text-neutral-500 dark:text-neutral-400">
            {hovering ? 'Release to install' : 'CEP extensions and JSX scripts are supported'}
          </p>
        </div>
        <button
          onClick={browse}
          disabled={busy}
          className="inline-flex items-center gap-2 rounded-xl bg-blue-600 px-5 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-blue-700 disabled:opacity-50"
        >
          <IconFolder className="h-4 w-4" />
          or browse files
        </button>
        <div className="mt-1 flex flex-wrap items-center justify-center gap-1.5">
          {FORMAT_CHIPS.map((f) => (
            <span
              key={f}
              className="rounded-full bg-black/[0.05] px-2.5 py-1 font-mono text-[11px] text-neutral-500 dark:bg-white/[0.07] dark:text-neutral-400"
            >
              {f}
            </span>
          ))}
        </div>
      </div>

      {/* Per-file progress */}
      {jobs.length > 0 && (
        <div className={`${card} sk-rise overflow-hidden`}>
          <div className="border-b border-black/[0.06] px-5 py-3 dark:border-white/[0.08]">
            <p className="text-sm font-semibold text-neutral-800 dark:text-neutral-200">
              {busy ? 'Installing' : 'Results'}
              <span className="ml-2 font-normal text-neutral-400">
                {jobs.filter((j) => j.status === 'ok').length} of {jobs.length} succeeded
              </span>
            </p>
          </div>
          <div className="divide-y divide-black/[0.05] dark:divide-white/[0.06]">
            {jobs.map((job) => (
              <div key={job.path} className="flex items-center gap-3 px-5 py-3">
                <span
                  className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full ${
                    job.status === 'ok'
                      ? 'bg-green-500/15 text-green-600 dark:text-green-400'
                      : job.status === 'fail'
                        ? 'bg-red-500/15 text-red-600 dark:text-red-400'
                        : 'text-blue-600 dark:text-blue-400'
                  }`}
                >
                  {job.status === 'ok' ? (
                    <IconCheck className="h-4 w-4" />
                  ) : job.status === 'fail' ? (
                    <IconX className="h-4 w-4" />
                  ) : (
                    <Spinner className="h-4 w-4" />
                  )}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate font-mono text-xs text-neutral-800 dark:text-neutral-200">
                    {basename(job.path)}
                  </p>
                  {job.message && (
                    <p className="truncate text-[11px] text-neutral-400 dark:text-neutral-500">
                      {job.message}
                    </p>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
