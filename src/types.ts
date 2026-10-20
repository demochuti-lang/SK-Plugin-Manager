// SK Plugin Manager — Phase 1 IPC contract types.
// Mirrors CONTRACT.md "Shared types" verbatim.

export type AppId = 'ae' | 'pp' | 'ps' | 'ai'; // After Effects, Premiere Pro, Photoshop, Illustrator
export type PluginKind = 'extension' | 'script';
export type PluginSource = 'installed' | 'detected'; // installed = via this app, detected = found by system scan

export interface AdobeApp {
  id: AppId;
  name: string; // e.g. "Adobe After Effects 2025"
  version: string; // e.g. "2025" (year parsed from folder name; "unknown" if unparseable)
  install_path: string; // full path to the .app (mac) or install dir (win)
}

export interface PluginEntry {
  id: string; // stable id: ExtensionBundleId for extensions, "<stem>__<appid>" for scripts
  name: string; // human name (bundle id prettified, or file stem)
  version: string; // from manifest, or parsed $VERSION/@version, else "unknown"
  kind: PluginKind;
  target_apps: AppId[]; // from manifest HostList, or sniffed for scripts
  install_path: string; // where it lives on disk right now
  source: PluginSource;
}

export interface InstallResult {
  path: string; // the dropped path this result is for
  success: boolean;
  message: string; // human-readable, shown in toast
  plugin?: PluginEntry; // present on success
  was_update?: boolean; // true if it overwrote an existing library entry
}

export interface UpdateCheck {
  has_update: boolean;
  plugin_name?: string;
  installed_version?: string;
  new_version?: string;
  target_apps?: AppId[];
}

export type HistoryEvent =
  | 'install'
  | 'uninstall'
  | 'update'
  | 'import'
  | 'scan'
  | 'backup'
  | 'restore';

export interface HistoryEntry {
  id: number;
  event: HistoryEvent;
  plugin_name: string;
  detail: string;
  created_at: string; // ISO-ish "YYYY-MM-DD HH:MM:SS"
}

// ---------------------------------------------------------------------------
// Phase 2 — backup system types. Mirrors CONTRACT.md "Backup" section.
// ---------------------------------------------------------------------------

export type BackupLocation = 'local' | 'drive' | 'both';

export interface BackupInfo {
  id: string;
  filename: string;
  created_at: string; // ISO-ish "YYYY-MM-DD HH:MM:SS"
  size_bytes: number;
  item_count: number;
  location: BackupLocation;
}

export interface BackupResult {
  success: boolean;
  message: string; // human-readable, shown in toast
  backup_id?: string;
  item_count?: number;
  size_bytes?: number;
}

export interface RestoreResult {
  success: boolean;
  message: string;
  restored: number;
  failed: number;
}

export interface SimpleResult {
  success: boolean;
  message: string;
}

export interface DriveStatus {
  configured: boolean;
  connected: boolean;
  email?: string;
}
