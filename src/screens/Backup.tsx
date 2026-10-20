import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type { BackupInfo, BackupLocation, DriveStatus } from '../types';
import {
  backupNow,
  deleteBackup,
  driveConnect,
  driveDisconnect,
  driveStatus,
  getSettings,
  listBackups,
  restoreBackup,
  setSetting,
  uploadBackupToDrive,
} from '../lib/tauri';
import { useToast } from '../components/Toast';
import { Pill, Spinner, Switch, card } from '../components/ui';
import {
  IconCloud,
  IconDownload,
  IconFolder,
  IconRefresh,
  IconTrash,
  IconUpload,
  IconX,
} from '../components/Icons';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function humanSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}

function formatUnix(sec: number): string {
  const d = new Date(sec * 1000);
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}

const LOCATION_META: Record<BackupLocation, { label: string; tone: 'gray' | 'blue' | 'violet' }> = {
  local: { label: 'Local', tone: 'gray' },
  drive: { label: 'Drive', tone: 'blue' },
  both: { label: 'Local + Drive', tone: 'violet' },
};

type IntervalUnit = 'hours' | 'days';

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

// ---------------------------------------------------------------------------
// Drive setup modal
// ---------------------------------------------------------------------------

const DRIVE_STEPS = [
  'Create a project in the Google Cloud Console.',
  'Enable the Google Drive API for that project.',
  'Create an OAuth client ID of type "Desktop app".',
  'Paste the Client ID and Client secret below.',
];

function DriveSetupModal({
  open: isOpen,
  onClose,
  onSaved,
}: {
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const toast = useToast();
  const [clientId, setClientId] = useState('');
  const [clientSecret, setClientSecret] = useState('');
  const [saving, setSaving] = useState(false);

  if (!isOpen) return null;

  const save = async () => {
    if (!clientId.trim() || !clientSecret.trim()) {
      toast.info('Missing credentials', 'Paste both the Client ID and Client secret.');
      return;
    }
    setSaving(true);
    try {
      await setSetting('drive_client_id', clientId.trim());
      await setSetting('drive_client_secret', clientSecret.trim());
      toast.success('Drive credentials saved', 'You can now connect Google Drive.');
      onSaved();
      onClose();
    } catch (e) {
      toast.error('Could not save credentials', errMsg(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div
        className="absolute inset-0 bg-black/30 backdrop-blur-sm"
        onClick={onClose}
      />
      <div className="sk-rise relative w-full max-w-lg overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-[#1e1e24]">
        <div className="flex items-center justify-between border-b border-black/[0.06] px-6 py-4 dark:border-white/[0.08]">
          <h2 className="text-base font-semibold text-neutral-900 dark:text-white">
            Set up Google Drive
          </h2>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-neutral-400 transition-colors hover:bg-black/5 dark:hover:bg-white/10"
            aria-label="Close"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>

        <div className="px-6 py-5">
          <p className="text-sm text-neutral-600 dark:text-neutral-300">
            Connect your own Google Cloud project so backups can be uploaded to
            your Drive:
          </p>
          <ol className="mt-3 flex flex-col gap-2">
            {DRIVE_STEPS.map((step, i) => (
              <li key={step} className="flex items-start gap-3">
                <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-blue-500/12 text-[11px] font-bold text-blue-600 dark:text-blue-400">
                  {i + 1}
                </span>
                <p className="text-sm text-neutral-700 dark:text-neutral-300">{step}</p>
              </li>
            ))}
          </ol>
          <p className="mt-3 rounded-xl bg-black/[0.03] px-3 py-2 text-[11px] text-neutral-500 dark:bg-white/[0.04] dark:text-neutral-400">
            The full walkthrough lives in DRIVE_SETUP.md at the project root.
          </p>

          <div className="mt-4 flex flex-col gap-3">
            <label className="block">
              <span className="mb-1 block text-xs font-semibold text-neutral-600 dark:text-neutral-300">
                Client ID
              </span>
              <input
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="xxxx.apps.googleusercontent.com"
                spellCheck={false}
                className="w-full rounded-xl border border-black/10 bg-transparent px-3 py-2 font-mono text-xs text-neutral-900 outline-none focus:border-blue-500/50 dark:border-white/10 dark:text-white"
              />
            </label>
            <label className="block">
              <span className="mb-1 block text-xs font-semibold text-neutral-600 dark:text-neutral-300">
                Client secret
              </span>
              <input
                value={clientSecret}
                onChange={(e) => setClientSecret(e.target.value)}
                placeholder="Paste your client secret"
                spellCheck={false}
                type="password"
                className="w-full rounded-xl border border-black/10 bg-transparent px-3 py-2 font-mono text-xs text-neutral-900 outline-none focus:border-blue-500/50 dark:border-white/10 dark:text-white"
              />
            </label>
          </div>

          <div className="mt-5 flex justify-end gap-2">
            <button
              onClick={onClose}
              className="rounded-xl px-4 py-2 text-sm font-semibold text-neutral-600 transition-colors hover:bg-black/5 dark:text-neutral-300 dark:hover:bg-white/10"
            >
              Cancel
            </button>
            <button
              onClick={save}
              disabled={saving}
              className="inline-flex items-center gap-2 rounded-xl bg-blue-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700 disabled:opacity-50"
            >
              {saving && <Spinner className="h-3.5 w-3.5" />}
              Save
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Backup history row (restore + inline-confirm delete + upload to Drive)
// ---------------------------------------------------------------------------

function BackupRow({
  backup,
  driveConnected,
  onRestore,
  onDelete,
  onUpload,
  busyAction,
}: {
  backup: BackupInfo;
  driveConnected: boolean;
  onRestore: (id: string) => void;
  onDelete: (id: string) => void;
  onUpload: (id: string) => void;
  busyAction: string | null; // backup id currently busy, or null
}) {
  const [confirming, setConfirming] = useState(false);
  const loc = LOCATION_META[backup.location];
  const busy = busyAction === backup.id;

  const doDelete = () => {
    if (!confirming) {
      setConfirming(true);
      window.setTimeout(() => setConfirming(false), 3200);
      return;
    }
    setConfirming(false);
    onDelete(backup.id);
  };

  return (
    <div className="flex items-center gap-4 px-5 py-3.5 transition-colors hover:bg-black/[0.015] dark:hover:bg-white/[0.03]">
      <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-blue-500/12 text-blue-600 dark:text-blue-400">
        <IconDownload className="h-5 w-5" />
      </span>

      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <p className="truncate font-mono text-sm font-semibold text-neutral-900 dark:text-neutral-100">
            {backup.filename}
          </p>
          <Pill tone={loc.tone}>{loc.label}</Pill>
        </div>
        <p className="mt-0.5 truncate text-xs text-neutral-500 dark:text-neutral-400">
          {backup.created_at} · {humanSize(backup.size_bytes)} · {backup.item_count}{' '}
          item{backup.item_count === 1 ? '' : 's'}
        </p>
      </div>

      <div className="flex shrink-0 items-center gap-1.5">
        <button
          onClick={() => onRestore(backup.id)}
          disabled={busy}
          title="Restore this backup"
          className="inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-semibold text-neutral-600 transition-colors hover:bg-black/5 hover:text-neutral-900 disabled:opacity-50 dark:text-neutral-400 dark:hover:bg-white/10 dark:hover:text-white"
        >
          {busy ? <Spinner className="h-3.5 w-3.5" /> : <IconRefresh className="h-3.5 w-3.5" />}
          Restore
        </button>
        {driveConnected && backup.location === 'local' && (
          <button
            onClick={() => onUpload(backup.id)}
            disabled={busy}
            title="Upload this backup to Google Drive"
            className="inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-semibold text-neutral-600 transition-colors hover:bg-black/5 hover:text-neutral-900 disabled:opacity-50 dark:text-neutral-400 dark:hover:bg-white/10 dark:hover:text-white"
          >
            {busy ? <Spinner className="h-3.5 w-3.5" /> : <IconUpload className="h-3.5 w-3.5" />}
            Upload to Drive
          </button>
        )}
        <button
          onClick={doDelete}
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
          {confirming ? 'Confirm?' : 'Delete'}
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Backup screen
// ---------------------------------------------------------------------------

export default function Backup({
  onLibraryChanged,
}: {
  onLibraryChanged?: () => void;
}) {
  const toast = useToast();

  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [listLoading, setListLoading] = useState(true);
  const [backingUp, setBackingUp] = useState(false);
  const [uploadToDrive, setUploadToDrive] = useState(false);

  const [drive, setDrive] = useState<DriveStatus | null>(null);
  const [driveBusy, setDriveBusy] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);

  const [scheduleOn, setScheduleOn] = useState(false);
  const [every, setEvery] = useState(24);
  const [unit, setUnit] = useState<IntervalUnit>('hours');
  const [scheduleSaving, setScheduleSaving] = useState(false);

  const [backupFolder, setBackupFolder] = useState('');
  const [lastBackupAt, setLastBackupAt] = useState(''); // unix seconds, string

  const [busyAction, setBusyAction] = useState<string | null>(null);

  // Load backups + drive status + settings.
  const refresh = useCallback(async () => {
    try {
      const [list, status, settings] = await Promise.all([
        listBackups(),
        driveStatus(),
        getSettings(),
      ]);
      setBackups(list);
      setDrive(status);
      setScheduleOn((settings.auto_backup_enabled ?? 'false') === 'true');
      const n = Number(settings.backup_interval_value ?? '24');
      setEvery(Number.isFinite(n) && n >= 1 ? Math.floor(n) : 24);
      const u = settings.backup_interval_unit ?? 'hours';
      setUnit(u === 'days' ? 'days' : 'hours');
      setBackupFolder(settings.backup_folder ?? '');
      setLastBackupAt(settings.last_backup_at ?? '');
    } catch (e) {
      toast.error('Could not load backup data', errMsg(e));
    } finally {
      setListLoading(false);
    }
  }, [toast]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Refresh when any backup completes (manual, tray or scheduled).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        unlisten = await listen<BackupInfo>('skpm://backup-completed', () => {
          void refresh();
        });
      } catch (e) {
        console.error('backup-completed listener failed:', e);
      }
    })();
    return () => {
      unlisten?.();
    };
  }, [refresh]);

  // --- Actions -----------------------------------------------------------

  const doBackupNow = async () => {
    if (backingUp) return;
    setBackingUp(true);
    try {
      const r = await backupNow(uploadToDrive);
      if (r.success) {
        toast.success('Backup complete', r.message);
        await refresh();
      } else {
        toast.error('Backup failed', r.message);
      }
    } catch (e) {
      toast.error('Backup failed', errMsg(e));
    } finally {
      setBackingUp(false);
    }
  };

  const saveSchedule = async () => {
    setScheduleSaving(true);
    try {
      await setSetting('auto_backup_enabled', scheduleOn ? 'true' : 'false');
      await setSetting('backup_interval_value', String(every));
      await setSetting('backup_interval_unit', unit);
      toast.success('Schedule saved');
    } catch (e) {
      toast.error('Could not save schedule', errMsg(e));
    } finally {
      setScheduleSaving(false);
    }
  };

  const changeFolder = async () => {
    let sel: string | string[] | null;
    try {
      sel = await open({ directory: true, multiple: false });
    } catch (e) {
      toast.error('Could not open folder picker', errMsg(e));
      return;
    }
    if (!sel || Array.isArray(sel)) return;
    try {
      await setSetting('backup_folder', sel);
      setBackupFolder(sel);
      toast.success('Backup folder changed', sel);
    } catch (e) {
      toast.error('Could not save backup folder', errMsg(e));
    }
  };

  const connectDrive = async () => {
    setDriveBusy(true);
    try {
      const r = await driveConnect();
      if (r.success) {
        toast.success('Google Drive connected');
        setDrive(await driveStatus());
      } else if (r.message.startsWith('NEEDS_SETUP:')) {
        setSetupOpen(true);
      } else {
        toast.error('Could not connect Drive', r.message);
      }
    } catch (e) {
      toast.error('Could not connect Drive', errMsg(e));
    } finally {
      setDriveBusy(false);
    }
  };

  const disconnectDrive = async () => {
    setDriveBusy(true);
    try {
      const r = await driveDisconnect();
      if (r.success) {
        toast.success('Google Drive disconnected');
        setDrive(await driveStatus());
      } else {
        toast.error('Could not disconnect Drive', r.message);
      }
    } catch (e) {
      toast.error('Could not disconnect Drive', errMsg(e));
    } finally {
      setDriveBusy(false);
    }
  };

  const doRestore = async (id: string) => {
    setBusyAction(id);
    try {
      const r = await restoreBackup(id);
      if (r.success) {
        toast.success(
          'Restore complete',
          `${r.restored} restored, ${r.failed} failed.`,
        );
        onLibraryChanged?.();
        await refresh();
      } else {
        toast.error('Restore failed', r.message);
      }
    } catch (e) {
      toast.error('Restore failed', errMsg(e));
    } finally {
      setBusyAction(null);
    }
  };

  const doDelete = async (id: string) => {
    setBusyAction(id);
    try {
      const r = await deleteBackup(id);
      if (r.success) {
        toast.success('Backup deleted', r.message);
        await refresh();
      } else {
        toast.error('Could not delete backup', r.message);
      }
    } catch (e) {
      toast.error('Could not delete backup', errMsg(e));
    } finally {
      setBusyAction(null);
    }
  };

  const doUpload = async (id: string) => {
    setBusyAction(id);
    try {
      const r = await uploadBackupToDrive(id);
      if (r.success) {
        toast.success('Uploaded to Drive', r.message);
        await refresh();
      } else {
        toast.error('Upload failed', r.message);
      }
    } catch (e) {
      toast.error('Upload failed', errMsg(e));
    } finally {
      setBusyAction(null);
    }
  };

  // --- Derived -----------------------------------------------------------

  const lastBackupSec = Number(lastBackupAt);
  const lastBackupLabel =
    lastBackupAt && Number.isFinite(lastBackupSec) && lastBackupSec > 0
      ? formatUnix(lastBackupSec)
      : 'Never';
  const nextBackupLabel = !scheduleOn
    ? '—'
    : formatUnix(
        (Number.isFinite(lastBackupSec) && lastBackupSec > 0
          ? lastBackupSec
          : Math.floor(Date.now() / 1000)) + every * (unit === 'hours' ? 3600 : 86400),
      );

  const driveConfigured = drive?.configured ?? false;
  const driveConnected = drive?.connected ?? false;

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5">
      <div>
        <h2 className="text-lg font-semibold text-neutral-900 dark:text-white">
          Backup
        </h2>
        <p className="mt-0.5 text-sm text-neutral-500 dark:text-neutral-400">
          Protect your plugin library with automatic backups.
        </p>
      </div>

      {/* Back up now */}
      <div className={`${card} sk-rise flex flex-col gap-4 p-5`}>
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <h3 className="text-sm font-semibold text-neutral-900 dark:text-white">
              Back up now
            </h3>
            <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
              Snapshot every installed plugin, script and setting in one click.
            </p>
          </div>
          <button
            onClick={doBackupNow}
            disabled={backingUp}
            className="inline-flex shrink-0 items-center gap-2 rounded-xl bg-blue-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-blue-700 disabled:opacity-60"
          >
            {backingUp ? (
              <>
                <Spinner className="h-4 w-4" />
                Backing up…
              </>
            ) : (
              <>
                <IconDownload className="h-4 w-4" />
                Back up now
              </>
            )}
          </button>
        </div>
        <div className="flex items-center gap-3 border-t border-black/[0.05] pt-4 dark:border-white/[0.06]">
          <div
            className={
              driveConnected ? '' : 'pointer-events-none opacity-40'
            }
          >
            <Switch
              checked={uploadToDrive && driveConnected}
              onChange={setUploadToDrive}
              label="Also upload to Google Drive"
            />
          </div>
          <div>
            <p className="text-xs font-medium text-neutral-700 dark:text-neutral-300">
              Also upload to Google Drive
            </p>
            {!driveConnected && (
              <p className="text-[11px] text-neutral-400 dark:text-neutral-500">
                Connect Drive first
              </p>
            )}
          </div>
        </div>
      </div>

      {/* Schedule */}
      <div className={`${card} sk-rise p-5`}>
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="text-sm font-semibold text-neutral-900 dark:text-white">
              Scheduled backups
            </h3>
            <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
              Automatically back up your library on a timer.
            </p>
          </div>
          <Switch checked={scheduleOn} onChange={setScheduleOn} label="Scheduled backups" />
        </div>
        <div className="mt-4 flex flex-wrap items-center gap-2">
          <span className="text-sm text-neutral-600 dark:text-neutral-300">Every</span>
          <input
            type="number"
            min={1}
            value={every}
            onChange={(e) => setEvery(Math.max(1, Number(e.target.value) || 1))}
            disabled={!scheduleOn}
            className="w-20 rounded-xl border border-black/10 bg-transparent px-3 py-1.5 text-sm text-neutral-900 outline-none transition-opacity disabled:opacity-40 dark:border-white/10 dark:text-white"
          />
          <div className="flex rounded-xl bg-black/[0.05] p-1 dark:bg-white/[0.07]">
            {(['hours', 'days'] as const).map((u) => (
              <button
                key={u}
                onClick={() => setUnit(u)}
                disabled={!scheduleOn}
                className={`rounded-lg px-3 py-1 text-xs font-semibold capitalize transition-all disabled:opacity-40 ${
                  unit === u
                    ? 'bg-white text-neutral-900 shadow dark:bg-white/15 dark:text-white'
                    : 'text-neutral-500 dark:text-neutral-400'
                }`}
              >
                {u}
              </button>
            ))}
          </div>
          <button
            onClick={saveSchedule}
            disabled={scheduleSaving}
            className="ml-auto inline-flex items-center gap-2 rounded-xl border border-black/10 px-4 py-1.5 text-sm font-semibold text-neutral-700 transition-colors hover:bg-black/5 disabled:opacity-50 dark:border-white/10 dark:text-neutral-200 dark:hover:bg-white/10"
          >
            {scheduleSaving && <Spinner className="h-3.5 w-3.5" />}
            Save schedule
          </button>
        </div>
        <div className="mt-4 flex flex-wrap gap-x-6 gap-y-1 border-t border-black/[0.05] pt-3 text-xs text-neutral-500 dark:border-white/[0.06] dark:text-neutral-400">
          <p>
            Last backup:{' '}
            <span className="font-medium text-neutral-700 dark:text-neutral-200">
              {lastBackupLabel}
            </span>
          </p>
          <p>
            Next backup:{' '}
            <span className="font-medium text-neutral-700 dark:text-neutral-200">
              {nextBackupLabel}
            </span>
          </p>
        </div>
      </div>

      {/* Destinations */}
      <div className={`${card} sk-rise p-5`}>
        <h3 className="text-sm font-semibold text-neutral-900 dark:text-white">
          Backup destinations
        </h3>
        <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
          Choose where your backups are stored.
        </p>

        <div className="mt-4 flex items-center gap-4 rounded-xl border border-black/[0.06] p-4 dark:border-white/[0.08]">
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-amber-500/15 text-amber-600 dark:text-amber-400">
            <IconFolder className="h-5 w-5" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-neutral-900 dark:text-white">
              Local folder
            </p>
            <p
              className="truncate font-mono text-[11px] text-neutral-400 dark:text-neutral-500"
              title={backupFolder || undefined}
            >
              {backupFolder || 'Not set'}
            </p>
          </div>
          <button
            onClick={changeFolder}
            className="shrink-0 rounded-xl border border-black/10 px-3 py-1.5 text-xs font-semibold text-neutral-700 transition-colors hover:bg-black/5 dark:border-white/10 dark:text-neutral-200 dark:hover:bg-white/10"
          >
            Change
          </button>
        </div>

        <div className="mt-3 flex items-center gap-4 rounded-xl border border-black/[0.06] p-4 dark:border-white/[0.08]">
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-blue-500/12 text-blue-600 dark:text-blue-400">
            <IconCloud className="h-5 w-5" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-neutral-900 dark:text-white">
              Google Drive
            </p>
            {driveConnected ? (
              <p className="truncate text-[11px] text-neutral-500 dark:text-neutral-400">
                {drive?.email ? (
                  <>
                    Connected as{' '}
                    <span className="font-medium text-neutral-700 dark:text-neutral-300">
                      {drive.email}
                    </span>
                  </>
                ) : (
                  'Connected'
                )}
              </p>
            ) : driveConfigured ? (
              <p className="text-[11px] text-neutral-400 dark:text-neutral-500">
                Not connected
              </p>
            ) : (
              <p className="text-[11px] text-neutral-400 dark:text-neutral-500">
                Setup required
              </p>
            )}
          </div>
          {driveConnected ? (
            <button
              onClick={disconnectDrive}
              disabled={driveBusy}
              className="inline-flex shrink-0 items-center gap-1.5 rounded-xl border border-black/10 px-3 py-1.5 text-xs font-semibold text-neutral-700 transition-colors hover:bg-black/5 disabled:opacity-50 dark:border-white/10 dark:text-neutral-200 dark:hover:bg-white/10"
            >
              {driveBusy && <Spinner className="h-3.5 w-3.5" />}
              Disconnect
            </button>
          ) : driveConfigured ? (
            <button
              onClick={connectDrive}
              disabled={driveBusy}
              className="inline-flex shrink-0 items-center gap-1.5 rounded-xl bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-blue-700 disabled:opacity-60"
            >
              {driveBusy && <Spinner className="h-3.5 w-3.5" />}
              Connect
            </button>
          ) : (
            <button
              onClick={() => setSetupOpen(true)}
              className="shrink-0 rounded-xl bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-blue-700"
            >
              Set up
            </button>
          )}
        </div>
      </div>

      {/* Backup history */}
      <div className={`${card} sk-rise overflow-hidden`}>
        <div className="p-5 pb-3">
          <h3 className="text-sm font-semibold text-neutral-900 dark:text-white">
            Backup history
          </h3>
          <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
            Restore or delete any previous snapshot.
          </p>
        </div>
        {listLoading ? (
          <div className="flex items-center justify-center gap-3 p-10">
            <Spinner className="h-5 w-5 text-blue-600" />
            <p className="text-sm text-neutral-500">Loading backups...</p>
          </div>
        ) : backups.length === 0 ? (
          <div className="px-5 pb-6 pt-2 text-center">
            <p className="text-sm font-medium text-neutral-800 dark:text-neutral-200">
              No backups yet
            </p>
            <p className="mx-auto mt-1 max-w-sm text-xs text-neutral-500 dark:text-neutral-400">
              Create your first snapshot with the button above.
            </p>
          </div>
        ) : (
          <div className="divide-y divide-black/[0.05] border-t border-black/[0.05] dark:divide-white/[0.06] dark:border-white/[0.06]">
            {backups.map((b) => (
              <BackupRow
                key={b.id}
                backup={b}
                driveConnected={driveConnected}
                onRestore={doRestore}
                onDelete={doDelete}
                onUpload={doUpload}
                busyAction={busyAction}
              />
            ))}
          </div>
        )}
      </div>

      <DriveSetupModal
        open={setupOpen}
        onClose={() => setSetupOpen(false)}
        onSaved={() => void refresh()}
      />
    </div>
  );
}
