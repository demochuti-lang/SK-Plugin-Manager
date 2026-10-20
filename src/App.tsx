import { useCallback, useEffect, useRef, useState } from 'react';
import type { ComponentType } from 'react';
import type { AdobeApp, PluginEntry } from './types';
import { detectAdobeApps, historyList, listPlugins } from './lib/tauri';
import { ToastProvider, useToast } from './components/Toast';
import {
  IconBackup,
  IconBell,
  IconDashboard,
  IconDrop,
  IconHistory,
  IconMoon,
  IconSearch,
  IconSettings,
  IconSun,
  IconMonitor,
} from './components/Icons';
import Dashboard from './screens/Dashboard';
import DropZone from './screens/DropZone';
import Backup from './screens/Backup';
import History from './screens/History';
import Settings from './screens/Settings';

export type Screen = 'dashboard' | 'dropzone' | 'backup' | 'history' | 'settings';
export type Theme = 'system' | 'light' | 'dark';

const NAV: { id: Screen; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: 'dashboard', label: 'Dashboard', icon: IconDashboard },
  { id: 'dropzone', label: 'Drop Zone', icon: IconDrop },
  { id: 'backup', label: 'Backup', icon: IconBackup },
  { id: 'history', label: 'History', icon: IconHistory },
  { id: 'settings', label: 'Settings', icon: IconSettings },
];

const TITLES: Record<Screen, string> = {
  dashboard: 'Dashboard',
  dropzone: 'Drop Zone',
  backup: 'Backup',
  history: 'History',
  settings: 'Settings',
};

const THEME_ORDER: Theme[] = ['system', 'light', 'dark'];
const THEME_ICON: Record<Theme, ComponentType<{ className?: string }>> = {
  system: IconMonitor,
  light: IconSun,
  dark: IconMoon,
};
const THEME_LABEL: Record<Theme, string> = {
  system: 'System',
  light: 'Light',
  dark: 'Dark',
};

function Shell() {
  const toast = useToast();
  const [screen, setScreen] = useState<Screen>('dashboard');
  const [theme, setTheme] = useState<Theme>('system');
  const [query, setQuery] = useState('');
  const [apps, setApps] = useState<AdobeApp[]>([]);
  const [plugins, setPlugins] = useState<PluginEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [unread, setUnread] = useState(0);
  const seenRef = useRef(0);
  const loadErrorShown = useRef(false);

  // Theme: default = OS preference; toggles `dark` class on <html>.
  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => {
      const dark = theme === 'dark' || (theme === 'system' && mq.matches);
      document.documentElement.classList.toggle('dark', dark);
    };
    apply();
    mq.addEventListener('change', apply);
    return () => mq.removeEventListener('change', apply);
  }, [theme]);

  const refreshAll = useCallback(async () => {
    try {
      const [a, p] = await Promise.all([detectAdobeApps(), listPlugins()]);
      setApps(a);
      setPlugins(p);
      const h = await historyList(200);
      const maxId = h.reduce((m, e) => Math.max(m, e.id), 0);
      if (seenRef.current === 0) {
        // First load: nothing is "unseen" yet.
        seenRef.current = maxId;
      } else {
        setUnread(h.filter((e) => e.id > seenRef.current).length);
      }
    } catch (e) {
      if (!loadErrorShown.current) {
        loadErrorShown.current = true;
        toast.error(
          'Could not reach the backend',
          'Plugin data will appear once the app backend is running.',
        );
      }
      console.error('refreshAll failed:', e);
    } finally {
      setLoaded(true);
    }
  }, [toast]);

  useEffect(() => {
    void refreshAll();
  }, [refreshAll]);

  const cycleTheme = () => {
    const next = THEME_ORDER[(THEME_ORDER.indexOf(theme) + 1) % THEME_ORDER.length];
    setTheme(next);
  };

  const goHistory = useCallback(async () => {
    setScreen('history');
    setQuery('');
    try {
      const h = await historyList(1);
      if (h[0]) seenRef.current = h[0].id;
    } catch {
      // Non-fatal; badge just stays.
    }
    setUnread(0);
  }, []);

  const navigate = (s: Screen) => {
    if (s === 'history') {
      void goHistory();
      return;
    }
    setScreen(s);
    if (s !== 'dashboard') setQuery('');
  };

  const ThemeIcon = THEME_ICON[theme];

  return (
    <div className="flex h-full overflow-hidden">
      {/* Sidebar */}
      <aside className="flex w-[230px] shrink-0 flex-col border-r border-black/[0.06] bg-white/70 backdrop-blur-2xl dark:border-white/[0.08] dark:bg-black/40">
        <div className="flex items-center gap-3 px-5 pb-6 pt-6">
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-blue-500 to-blue-700 text-sm font-bold text-white shadow-[0_4px_14px_rgba(37,99,235,0.4)]">
            SK
          </span>
          <div className="leading-tight">
            <p className="text-[15px] font-semibold text-neutral-900 dark:text-white">
              Plugin Manager
            </p>
            <p className="text-[11px] text-neutral-400 dark:text-neutral-500">
              Adobe plugin toolkit
            </p>
          </div>
        </div>

        <nav className="flex flex-1 flex-col gap-1 px-3">
          {NAV.map((item) => {
            const Icon = item.icon;
            const active = screen === item.id;
            return (
              <button
                key={item.id}
                onClick={() => navigate(item.id)}
                className={`flex items-center gap-3 rounded-xl px-3 py-2 text-sm transition-colors ${
                  active
                    ? 'bg-blue-600/10 font-semibold text-blue-700 dark:bg-blue-500/15 dark:text-blue-300'
                    : 'text-neutral-600 hover:bg-black/[0.04] dark:text-neutral-400 dark:hover:bg-white/[0.06]'
                }`}
              >
                <Icon className="h-[18px] w-[18px]" />
                {item.label}
              </button>
            );
          })}
        </nav>

        <p className="px-5 py-4 text-[11px] leading-relaxed text-neutral-400 dark:text-neutral-500">
          Free · v1.0 · by Md. Shakib Hossain
        </p>
      </aside>

      {/* Main column */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Top bar */}
        <header className="flex h-16 shrink-0 items-center gap-4 border-b border-black/[0.06] bg-white/60 px-6 backdrop-blur-2xl dark:border-white/[0.08] dark:bg-black/30">
          <h1 className="text-[17px] font-semibold text-neutral-900 dark:text-white">
            {TITLES[screen]}
          </h1>

          <div className="flex-1" />

          {screen === 'dashboard' && (
            <div className="relative">
              <IconSearch className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-neutral-400" />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search plugins..."
                className="w-56 rounded-full border border-black/[0.06] bg-black/[0.04] py-1.5 pl-9 pr-3 text-sm text-neutral-900 outline-none transition-all placeholder:text-neutral-400 focus:w-64 focus:border-blue-500/50 focus:bg-white dark:border-white/[0.08] dark:bg-white/[0.06] dark:text-white dark:focus:bg-white/[0.08]"
              />
            </div>
          )}

          <button
            onClick={cycleTheme}
            title={`Theme: ${THEME_LABEL[theme]} — click to change`}
            aria-label={`Theme: ${THEME_LABEL[theme]} — click to change`}
            className="flex h-9 w-9 items-center justify-center rounded-xl text-neutral-500 transition-colors hover:bg-black/[0.05] hover:text-neutral-800 dark:text-neutral-400 dark:hover:bg-white/[0.08] dark:hover:text-neutral-100"
          >
            <ThemeIcon className="h-[18px] w-[18px]" />
          </button>

          <button
            onClick={() => void goHistory()}
            title="Notifications"
            aria-label="Notifications"
            className="relative flex h-9 w-9 items-center justify-center rounded-xl text-neutral-500 transition-colors hover:bg-black/[0.05] hover:text-neutral-800 dark:text-neutral-400 dark:hover:bg-white/[0.08] dark:hover:text-neutral-100"
          >
            <IconBell className="h-[18px] w-[18px]" />
            {unread > 0 && (
              <span className="absolute -right-0.5 -top-0.5 flex h-5 min-w-5 items-center justify-center rounded-full bg-red-500 px-1 text-[10px] font-bold text-white">
                {unread > 99 ? '99+' : unread}
              </span>
            )}
          </button>
        </header>

        {/* Screen content */}
        <main className="flex-1 overflow-y-auto p-6">
          {screen === 'dashboard' && (
            <Dashboard
              plugins={plugins}
              apps={apps}
              query={query}
              loaded={loaded}
              onRefresh={refreshAll}
              onOpenDropZone={() => setScreen('dropzone')}
            />
          )}
          {screen === 'dropzone' && <DropZone onChanged={refreshAll} />}
          {screen === 'backup' && <Backup onLibraryChanged={refreshAll} />}
          {screen === 'history' && <History />}
          {screen === 'settings' && (
            <Settings theme={theme} onTheme={setTheme} apps={apps} />
          )}
        </main>
      </div>
    </div>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <Shell />
    </ToastProvider>
  );
}
