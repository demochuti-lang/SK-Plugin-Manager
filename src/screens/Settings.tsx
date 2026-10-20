import { useEffect, useState } from 'react';
import type { AdobeApp } from '../types';
import { APP_META } from '../lib/apps';
import { getSettings, setAutostart, setSetting } from '../lib/tauri';
import { Switch, card } from '../components/ui';
import { IconMonitor, IconMoon, IconSun } from '../components/Icons';
import { useToast } from '../components/Toast';
import type { Theme } from '../App';

const THEME_OPTIONS: { id: Theme; label: string }[] = [
  { id: 'system', label: 'System' },
  { id: 'light', label: 'Light' },
  { id: 'dark', label: 'Dark' },
];

function Row({
  title,
  desc,
  control,
}: {
  title: string;
  desc?: string;
  control: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-4">
      <div className="min-w-0">
        <p className="text-sm font-medium text-neutral-900 dark:text-white">{title}</p>
        {desc && (
          <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">{desc}</p>
        )}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}

export default function Settings({
  theme,
  onTheme,
  apps,
}: {
  theme: Theme;
  onTheme: (t: Theme) => void;
  apps: AdobeApp[];
}) {
  // Persisted preferences (stored via the backend settings store).
  const toast = useToast();
  const [notifications, setNotifications] = useState(true);
  const [launchAtLogin, setLaunchAtLogin] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const s = await getSettings();
        if (cancelled) return;
        setNotifications((s.notifications_enabled ?? 'true') === 'true');
        setLaunchAtLogin((s.launch_at_login ?? 'false') === 'true');
      } catch (e) {
        if (!cancelled) console.error('Could not load settings:', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const toggleNotifications = async (v: boolean) => {
    const prev = notifications;
    setNotifications(v);
    try {
      await setSetting('notifications_enabled', v ? 'true' : 'false');
    } catch (e) {
      setNotifications(prev);
      toast.error(
        'Could not save setting',
        e instanceof Error ? e.message : String(e),
      );
    }
  };

  const toggleLaunchAtLogin = async (v: boolean) => {
    const prev = launchAtLogin;
    setLaunchAtLogin(v);
    try {
      await setSetting('launch_at_login', v ? 'true' : 'false');
      const r = await setAutostart(v);
      if (!r.success) {
        // Backend declined (e.g. platform support coming later): revert both.
        toast.info('Launch at login unavailable', r.message);
        setLaunchAtLogin(prev);
        await setSetting('launch_at_login', prev ? 'true' : 'false');
      }
    } catch (e) {
      setLaunchAtLogin(prev);
      toast.error(
        'Could not save setting',
        e instanceof Error ? e.message : String(e),
      );
    }
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5">
      <div>
        <h2 className="text-lg font-semibold text-neutral-900 dark:text-white">
          Settings
        </h2>
        <p className="mt-0.5 text-sm text-neutral-500 dark:text-neutral-400">
          Appearance, notifications and detected applications.
        </p>
      </div>

      {/* Appearance */}
      <div className={`${card} sk-rise px-5`}>
        <Row
          title="Appearance"
          desc="Syncs with the theme toggle in the top bar. System follows your OS."
          control={
            <div className="flex rounded-xl bg-black/[0.05] p-1 dark:bg-white/[0.07]">
              {THEME_OPTIONS.map((o) => (
                <button
                  key={o.id}
                  onClick={() => onTheme(o.id)}
                  className={`inline-flex items-center gap-1.5 rounded-lg px-4 py-1.5 text-xs font-semibold transition-all ${
                    theme === o.id
                      ? 'bg-white text-neutral-900 shadow dark:bg-white/15 dark:text-white'
                      : 'text-neutral-500 hover:text-neutral-700 dark:text-neutral-400 dark:hover:text-neutral-200'
                  }`}
                >
                  {o.id === 'system' ? (
                    <IconMonitor className="h-3.5 w-3.5" />
                  ) : o.id === 'light' ? (
                    <IconSun className="h-3.5 w-3.5" />
                  ) : (
                    <IconMoon className="h-3.5 w-3.5" />
                  )}
                  {o.label}
                </button>
              ))}
            </div>
          }
        />
      </div>

      {/* Preferences */}
      <div className={`${card} sk-rise divide-y divide-black/[0.05] px-5 dark:divide-white/[0.06]`}>
        <Row
          title="Notifications"
          desc="Show a notification when a plugin install finishes."
          control={
            <Switch
              checked={notifications}
              onChange={(v) => void toggleNotifications(v)}
              label="Notifications"
            />
          }
        />
        <Row
          title="Launch at login"
          desc="Open SK Plugin Manager when you sign in."
          control={
            <Switch
              checked={launchAtLogin}
              onChange={(v) => void toggleLaunchAtLogin(v)}
              label="Launch at login"
            />
          }
        />
      </div>

      {/* Detected Adobe apps */}
      <div className={`${card} sk-rise p-5`}>
        <h3 className="text-sm font-semibold text-neutral-900 dark:text-white">
          Detected Adobe apps
        </h3>
        <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
          Found by scanning your system for Adobe installations.
        </p>
        {apps.length === 0 ? (
          <p className="mt-3 rounded-xl bg-black/[0.03] px-4 py-3 text-xs text-neutral-500 dark:bg-white/[0.04] dark:text-neutral-400">
            No Adobe apps detected yet.
          </p>
        ) : (
          <div className="mt-3 divide-y divide-black/[0.05] rounded-xl border border-black/[0.06] dark:divide-white/[0.06] dark:border-white/[0.08]">
            {apps.map((a) => (
              <div key={a.id + a.install_path} className="flex items-center gap-3 px-4 py-2.5">
                <span
                  className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-[10px] font-bold text-white"
                  style={{ backgroundColor: APP_META[a.id]?.color ?? '#6b7280' }}
                  title={APP_META[a.id]?.name ?? a.id}
                >
                  {APP_META[a.id]?.short ?? a.id.toUpperCase()}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-neutral-900 dark:text-neutral-100">
                    {a.name}
                    <span className="ml-2 font-mono text-[11px] font-normal text-neutral-400">
                      {a.version}
                    </span>
                  </p>
                  <p className="truncate font-mono text-[11px] text-neutral-400 dark:text-neutral-500">
                    {a.install_path}
                  </p>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* About */}
      <div className={`${card} sk-rise p-5`}>
        <h3 className="text-sm font-semibold text-neutral-900 dark:text-white">About</h3>
        <div className="mt-3 flex items-center gap-4">
          <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-blue-500 to-blue-700 text-base font-bold text-white shadow-[0_4px_14px_rgba(37,99,235,0.4)]">
            SK
          </span>
          <div>
            <p className="text-sm font-semibold text-neutral-900 dark:text-white">
              SK Plugin Manager
            </p>
            <p className="text-xs text-neutral-500 dark:text-neutral-400">Free</p>
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              by Md. Shakib Hossain
            </p>
            <p className="mt-1 font-mono text-[11px] text-neutral-400 dark:text-neutral-500">
              v1.0.0
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
