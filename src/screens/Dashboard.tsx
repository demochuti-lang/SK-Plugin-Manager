import { useCallback, useEffect, useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import type { AdobeApp, PluginEntry } from '../types';
import { APP_META } from '../lib/apps';
import {
  compareVersion,
  importExisting,
  installDropped,
  listBackups,
  scanExisting,
  uninstall,
} from '../lib/tauri';
import { useToast } from '../components/Toast';
import { Pill, Spinner, card } from '../components/ui';
import {
  IconBackup,
  IconCheck,
  IconCode,
  IconDownload,
  IconMonitor,
  IconPackage,
  IconRefresh,
  IconSearch,
  IconTrash,
  IconX,
} from '../components/Icons';

// ---------------------------------------------------------------------------
// Small pieces
// ---------------------------------------------------------------------------

function AppBadges({ ids }: { ids: PluginEntry['target_apps'] }) {
  if (ids.length === 0) return null;
  return (
    <div className="flex shrink-0 items-center gap-1">
      {ids.map((id) => (
        <span
          key={id}
          title={APP_META[id].name}
          className="inline-flex h-6 min-w-7 items-center justify-center rounded-md px-1.5 text-[10px] font-bold tracking-wide text-white"
          style={{ backgroundColor: APP_META[id].color }}
        >
          {APP_META[id].short}
        </span>
      ))}
    </div>
  );
}

function StatCard({
  icon,
  label,
  value,
  sub,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  sub?: string;
}) {
  return (
    <div className={`${card} sk-rise flex items-center gap-4 p-5`}>
      <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-blue-500/12 text-blue-600 dark:text-blue-400">
        {icon}
      </span>
      <div className="min-w-0">
        <p className="text-[11px] font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">
          {label}
        </p>
        <p className="truncate text-2xl font-semibold text-neutral-900 dark:text-white">
          {value}
        </p>
        {sub && (
          <p className="truncate text-xs text-neutral-400 dark:text-neutral-500">
            {sub}
          </p>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Plugin row: uninstall (inline confirm) + check-update flow
// ---------------------------------------------------------------------------

function PluginRow({
  plugin,
  onChanged,
}: {
  plugin: PluginEntry;
  onChanged: () => void;
}) {
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [update, setUpdate] = useState<{ path: string; newVersion?: string } | null>(null);

  const doUninstall = async () => {
    if (!confirming) {
      setConfirming(true);
      window.setTimeout(() => setConfirming(false), 3200);
      return;
    }
    setBusy(true);
    try {
      const r = await uninstall(plugin.id);
      if (r.success) {
        toast.success('Uninstalled', r.message);
        onChanged();
      } else {
        toast.error('Uninstall failed', r.message);
      }
    } catch (e) {
      toast.error('Uninstall failed', e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  };

  const checkUpdate = async () => {
    let sel: string | string[] | null;
    try {
      sel = await open({
        multiple: false,
        filters: [{ name: 'Extension packages', extensions: ['zxp'] }],
      });
    } catch (e) {
      toast.error('Could not open file picker', e instanceof Error ? e.message : String(e));
      return;
    }
    if (!sel || Array.isArray(sel)) return;
    const droppedPath = sel;
    setChecking(true);
    try {
      const res = await compareVersion(droppedPath);
      if (res.has_update) {
        setUpdate({ path: droppedPath, newVersion: res.new_version });
      } else {
        toast.info(
          'No update available',
          res.plugin_name ? `${res.plugin_name} is already up to date.` : undefined,
        );
      }
    } catch (e) {
      toast.error('Update check failed', e instanceof Error ? e.message : String(e));
    } finally {
      setChecking(false);
    }
  };

  const installUpdate = async () => {
    if (!update) return;
    setBusy(true);
    try {
      const results = await installDropped([update.path]);
      const r = results[0];
      if (r && r.success) {
        toast.success('Updated', r.message);
        onChanged();
      } else {
        toast.error('Update failed', r?.message ?? 'Unknown error');
      }
    } catch (e) {
      toast.error('Update failed', e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setUpdate(null);
    }
  };

  const KindIcon = plugin.kind === 'extension' ? IconPackage : IconCode;

  return (
    <div className="flex items-center gap-4 px-5 py-3.5 transition-colors hover:bg-black/[0.015] dark:hover:bg-white/[0.03]">
      <span
        className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-xl ${
          plugin.kind === 'extension'
            ? 'bg-blue-500/12 text-blue-600 dark:text-blue-400'
            : 'bg-amber-500/15 text-amber-600 dark:text-amber-400'
        }`}
      >
        <KindIcon className="h-5 w-5" />
      </span>

      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <p className="truncate text-sm font-semibold text-neutral-900 dark:text-neutral-100">
            {plugin.name}
          </p>
          <Pill tone={plugin.kind === 'extension' ? 'blue' : 'amber'}>
            {plugin.kind === 'extension' ? 'Extension' : 'Script'}
          </Pill>
          {plugin.source === 'detected' ? (
            <Pill tone="violet">Detected</Pill>
          ) : (
            <Pill tone="green">Installed</Pill>
          )}
        </div>
        <p className="mt-0.5 truncate font-mono text-[11px] text-neutral-400 dark:text-neutral-500">
          {plugin.install_path}
        </p>
      </div>

      <AppBadges ids={plugin.target_apps} />

      <span className="hidden w-20 shrink-0 text-right font-mono text-xs text-neutral-500 dark:text-neutral-400 md:block">
        v{plugin.version}
      </span>

      <div className="flex shrink-0 items-center gap-1.5">
        {update ? (
          <>
            <button
              onClick={installUpdate}
              disabled={busy}
              className="inline-flex items-center gap-1.5 rounded-xl bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-blue-700 disabled:opacity-50"
            >
              {busy ? <Spinner className="h-3.5 w-3.5" /> : <IconDownload className="h-3.5 w-3.5" />}
              Install v{update.newVersion ?? '?'}
            </button>
            <button
              onClick={() => setUpdate(null)}
              className="rounded-xl p-1.5 text-neutral-400 transition-colors hover:bg-black/5 hover:text-neutral-600 dark:hover:bg-white/10"
              aria-label="Cancel update"
            >
              <IconX className="h-4 w-4" />
            </button>
          </>
        ) : (
          <button
            onClick={checkUpdate}
            disabled={checking || busy}
            title="Pick a .zxp package to compare against this plugin"
            className="inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-semibold text-neutral-600 transition-colors hover:bg-black/5 hover:text-neutral-900 disabled:opacity-50 dark:text-neutral-400 dark:hover:bg-white/10 dark:hover:text-white"
          >
            {checking ? <Spinner className="h-3.5 w-3.5" /> : <IconRefresh className="h-3.5 w-3.5" />}
            Check update
          </button>
        )}
        <button
          onClick={doUninstall}
          disabled={busy}
          className={`inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-semibold transition-colors disabled:opacity-50 ${
            confirming
              ? 'bg-red-600 text-white hover:bg-red-700'
              : 'text-neutral-600 hover:bg-red-500/10 hover:text-red-600 dark:text-neutral-400 dark:hover:text-red-400'
          }`}
        >
          {busy && !confirming ? (
            <Spinner className="h-3.5 w-3.5" />
          ) : (
            <IconTrash className="h-3.5 w-3.5" />
          )}
          {confirming ? 'Confirm?' : 'Uninstall'}
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Scan modal: animated steps -> checkbox results -> import
// ---------------------------------------------------------------------------

const SCAN_STEPS = [
  'Locating CEP extension folders',
  'Scanning application script folders',
  'Parsing extension manifests',
];

function ScanModal({
  open: isOpen,
  onClose,
  onImported,
}: {
  open: boolean;
  onClose: () => void;
  onImported: () => void;
}) {
  const toast = useToast();
  const [phase, setPhase] = useState<'scanning' | 'results' | 'importing'>('scanning');
  const [stepIdx, setStepIdx] = useState(0);
  const [results, setResults] = useState<PluginEntry[]>([]);
  const [checked, setChecked] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (!isOpen) return;
    setPhase('scanning');
    setStepIdx(0);
    setResults([]);
    setChecked(new Set());
    let cancelled = false;
    const timers: number[] = [];
    SCAN_STEPS.forEach((_, i) => {
      timers.push(
        window.setTimeout(() => {
          if (!cancelled) setStepIdx(i + 1);
        }, 750 * (i + 1)),
      );
    });
    (async () => {
      try {
        const entries = await scanExisting();
        if (cancelled) return;
        setResults(entries);
        setChecked(new Set(entries.map((e) => e.id)));
      } catch (e) {
        if (cancelled) return;
        toast.error('Scan failed', e instanceof Error ? e.message : String(e));
        onClose();
      }
    })();
    timers.push(
      window.setTimeout(() => {
        if (!cancelled) setPhase('results');
      }, 750 * SCAN_STEPS.length + 500),
    );
    return () => {
      cancelled = true;
      timers.forEach((t) => window.clearTimeout(t));
    };
  }, [isOpen, onClose, toast]);

  const toggle = (id: string) => {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const doImport = async () => {
    const selected = results.filter((r) => checked.has(r.id));
    if (selected.length === 0) {
      toast.info('Nothing selected', 'Tick at least one plugin to import.');
      return;
    }
    setPhase('importing');
    try {
      const { imported } = await importExisting(selected);
      toast.success('Import complete', `Imported ${imported} plugin${imported === 1 ? '' : 's'} into your library.`);
      onImported();
      onClose();
    } catch (e) {
      toast.error('Import failed', e instanceof Error ? e.message : String(e));
      setPhase('results');
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div
        className="absolute inset-0 bg-black/30 backdrop-blur-sm"
        onClick={phase === 'results' ? onClose : undefined}
      />
      <div className="sk-rise relative w-full max-w-lg overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-[#1e1e24]">
        <div className="flex items-center justify-between border-b border-black/[0.06] px-6 py-4 dark:border-white/[0.08]">
          <h2 className="text-base font-semibold text-neutral-900 dark:text-white">
            Scan system for plugins
          </h2>
          {phase === 'results' && (
            <button
              onClick={onClose}
              className="rounded-lg p-1.5 text-neutral-400 transition-colors hover:bg-black/5 dark:hover:bg-white/10"
              aria-label="Close"
            >
              <IconX className="h-4 w-4" />
            </button>
          )}
        </div>

        <div className="px-6 py-5">
          {phase === 'scanning' && (
            <div className="flex flex-col gap-3 py-2">
              {SCAN_STEPS.map((step, i) => {
                const done = i < stepIdx;
                const active = i === stepIdx;
                return (
                  <div key={step} className="flex items-center gap-3">
                    <span
                      className={`flex h-6 w-6 items-center justify-center rounded-full ${
                        done
                          ? 'bg-green-500/15 text-green-600 dark:text-green-400'
                          : active
                            ? 'text-blue-600 dark:text-blue-400'
                            : 'text-neutral-300 dark:text-neutral-600'
                      }`}
                    >
                      {done ? (
                        <IconCheck className="h-4 w-4" />
                      ) : active ? (
                        <Spinner className="h-4 w-4" />
                      ) : (
                        <span className="h-1.5 w-1.5 rounded-full bg-current" />
                      )}
                    </span>
                    <p
                      className={`text-sm ${
                        done || active
                          ? 'text-neutral-800 dark:text-neutral-200'
                          : 'text-neutral-400 dark:text-neutral-500'
                      }`}
                    >
                      {step}
                      {active && !done ? '...' : ''}
                    </p>
                  </div>
                );
              })}
            </div>
          )}

          {phase === 'results' && (
            <>
              {results.length === 0 ? (
                <div className="py-6 text-center">
                  <p className="text-sm font-medium text-neutral-800 dark:text-neutral-200">
                    No existing plugins found
                  </p>
                  <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
                    Your Adobe apps don't have any extensions or scripts installed yet.
                  </p>
                </div>
              ) : (
                <>
                  <p className="mb-3 text-sm text-neutral-600 dark:text-neutral-300">
                    Found <span className="font-semibold">{results.length}</span> plugin
                    {results.length === 1 ? '' : 's'} on your system. Select the ones to
                    import into your library:
                  </p>
                  <div className="max-h-64 overflow-y-auto rounded-xl border border-black/[0.06] dark:border-white/[0.08]">
                    {results.map((r) => (
                      <label
                        key={r.id}
                        className="flex cursor-pointer items-center gap-3 border-b border-black/[0.04] px-4 py-2.5 last:border-0 hover:bg-black/[0.02] dark:border-white/[0.06] dark:hover:bg-white/[0.04]"
                      >
                        <input
                          type="checkbox"
                          checked={checked.has(r.id)}
                          onChange={() => toggle(r.id)}
                          className="h-4 w-4 rounded accent-blue-600"
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm font-medium text-neutral-900 dark:text-neutral-100">
                            {r.name}
                          </span>
                          <span className="block truncate font-mono text-[11px] text-neutral-400">
                            {r.install_path}
                          </span>
                        </span>
                        <AppBadges ids={r.target_apps} />
                      </label>
                    ))}
                  </div>
                </>
              )}
              <div className="mt-5 flex justify-end gap-2">
                <button
                  onClick={onClose}
                  className="rounded-xl px-4 py-2 text-sm font-semibold text-neutral-600 transition-colors hover:bg-black/5 dark:text-neutral-300 dark:hover:bg-white/10"
                >
                  {results.length === 0 ? 'Close' : 'Cancel'}
                </button>
                {results.length > 0 && (
                  <button
                    onClick={doImport}
                    className="rounded-xl bg-blue-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700"
                  >
                    Import selected ({checked.size})
                  </button>
                )}
              </div>
            </>
          )}

          {phase === 'importing' && (
            <div className="flex items-center justify-center gap-3 py-8">
              <Spinner className="h-5 w-5 text-blue-600" />
              <p className="text-sm text-neutral-600 dark:text-neutral-300">
                Importing plugins...
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dashboard screen
// ---------------------------------------------------------------------------

/** Format a backup "created_at" ("YYYY-MM-DD HH:MM:SS") for the stat card. */
function formatBackupStat(s: string | null): { value: string; sub: string } {
  if (!s) return { value: '—', sub: 'No backups yet' };
  const m = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/.exec(s);
  if (!m) return { value: s, sub: 'Latest snapshot' };
  const d = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5]);
  return {
    value: d.toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    }),
    sub: `${d.toLocaleTimeString(undefined, {
      hour: 'numeric',
      minute: '2-digit',
    })} · latest snapshot`,
  };
}

export default function Dashboard({
  plugins,
  apps,
  query,
  loaded,
  onRefresh,
  onOpenDropZone,
}: {
  plugins: PluginEntry[];
  apps: AdobeApp[];
  query: string;
  loaded: boolean;
  onRefresh: () => void;
  onOpenDropZone: () => void;
}) {
  const toast = useToast();
  const [scanOpen, setScanOpen] = useState(false);
  const [lastBackupAt, setLastBackupAt] = useState<string | null>(null);

  const loadLastBackup = useCallback(async () => {
    try {
      const list = await listBackups();
      setLastBackupAt(list[0]?.created_at ?? null);
    } catch {
      // Non-fatal; the stat just shows "no backups yet".
    }
  }, []);

  useEffect(() => {
    void loadLastBackup();
  }, [loadLastBackup]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return plugins;
    return plugins.filter(
      (p) => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q),
    );
  }, [plugins, query]);

  const manualRefresh = async () => {
    try {
      await onRefresh();
      await loadLastBackup();
    } catch (e) {
      toast.error('Refresh failed', e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-5">
      {/* First-run banner */}
      {loaded && plugins.length === 0 && (
        <div className="sk-rise overflow-hidden rounded-2xl bg-gradient-to-br from-blue-600 to-indigo-700 p-6 text-white shadow-[0_8px_24px_rgba(37,99,235,0.35)]">
          <h2 className="text-lg font-semibold">Welcome to SK Plugin Manager</h2>
          <p className="mt-1 max-w-xl text-sm text-blue-100">
            Scan your system to find extensions and scripts already installed in your
            Adobe apps, or drop a .zxp package to install your first plugin.
          </p>
          <div className="mt-4 flex gap-2">
            <button
              onClick={() => setScanOpen(true)}
              className="inline-flex items-center gap-2 rounded-xl bg-white px-4 py-2 text-sm font-semibold text-blue-700 transition-colors hover:bg-blue-50"
            >
              <IconSearch className="h-4 w-4" />
              Scan system
            </button>
            <button
              onClick={onOpenDropZone}
              className="rounded-xl border border-white/30 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-white/10"
            >
              Open Drop Zone
            </button>
          </div>
        </div>
      )}

      {/* Stat cards */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <StatCard
          icon={<IconPackage className="h-5 w-5" />}
          label="Installed"
          value={String(plugins.length)}
          sub={`${plugins.filter((p) => p.kind === 'extension').length} extensions, ${plugins.filter((p) => p.kind === 'script').length} scripts`}
        />
        <StatCard
          icon={<IconMonitor className="h-5 w-5" />}
          label="Adobe apps detected"
          value={String(apps.length)}
          sub={apps.length > 0 ? apps.map((a) => APP_META[a.id]?.short ?? a.id).join(' · ') : 'None found yet'}
        />
        <StatCard
          icon={<IconBackup className="h-5 w-5" />}
          label="Last backup"
          value={formatBackupStat(lastBackupAt).value}
          sub={formatBackupStat(lastBackupAt).sub}
        />
      </div>

      {/* Plugin list */}
      <div>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-neutral-800 dark:text-neutral-200">
            Your plugins
            <span className="ml-2 font-normal text-neutral-400">
              {query ? `${filtered.length} of ${plugins.length}` : plugins.length}
            </span>
          </h2>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setScanOpen(true)}
              className="inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-semibold text-neutral-600 transition-colors hover:bg-black/5 dark:text-neutral-300 dark:hover:bg-white/10"
            >
              <IconSearch className="h-3.5 w-3.5" />
              Scan system
            </button>
            <button
              onClick={manualRefresh}
              className="inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-semibold text-neutral-600 transition-colors hover:bg-black/5 dark:text-neutral-300 dark:hover:bg-white/10"
            >
              <IconRefresh className="h-3.5 w-3.5" />
              Refresh
            </button>
          </div>
        </div>

        {!loaded ? (
          <div className={`${card} flex items-center justify-center gap-3 p-10`}>
            <Spinner className="h-5 w-5 text-blue-600" />
            <p className="text-sm text-neutral-500">Loading your library...</p>
          </div>
        ) : filtered.length === 0 ? (
          <div className={`${card} sk-rise p-10 text-center`}>
            <p className="text-sm font-medium text-neutral-800 dark:text-neutral-200">
              {plugins.length === 0 ? 'No plugins yet' : 'No matches'}
            </p>
            <p className="mx-auto mt-1 max-w-sm text-xs text-neutral-500 dark:text-neutral-400">
              {plugins.length === 0
                ? 'Drop a .zxp, .jsx or .jsxbin file in the Drop Zone, or scan your system for existing plugins.'
                : 'Try a different search term.'}
            </p>
          </div>
        ) : (
          <div className={`${card} sk-rise divide-y divide-black/[0.05] overflow-hidden dark:divide-white/[0.06]`}>
            {filtered.map((p) => (
              <PluginRow key={p.id} plugin={p} onChanged={onRefresh} />
            ))}
          </div>
        )}
      </div>

      <ScanModal
        open={scanOpen}
        onClose={() => setScanOpen(false)}
        onImported={onRefresh}
      />
    </div>
  );
}
