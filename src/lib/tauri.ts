import { invoke } from '@tauri-apps/api/core';
import type {
  AdobeApp,
  BackupInfo,
  BackupResult,
  DriveStatus,
  HistoryEntry,
  InstallResult,
  PluginEntry,
  RestoreResult,
  SimpleResult,
  UpdateCheck,
} from '../types';

/** Typed wrappers for the Phase 1 IPC contract (CONTRACT.md). */

export async function detectAdobeApps(): Promise<AdobeApp[]> {
  return invoke('detect_adobe_apps');
}

export async function listPlugins(): Promise<PluginEntry[]> {
  return invoke('list_plugins');
}

export async function scanExisting(): Promise<PluginEntry[]> {
  return invoke('scan_existing');
}

export async function importExisting(
  entries: PluginEntry[],
): Promise<{ imported: number }> {
  return invoke('import_existing', { entries });
}

export async function installDropped(paths: string[]): Promise<InstallResult[]> {
  return invoke('install_dropped', { paths });
}

export async function uninstall(
  entry_id: string,
): Promise<{ success: boolean; message: string }> {
  return invoke('uninstall', { entryId: entry_id });
}

export async function compareVersion(dropped_path: string): Promise<UpdateCheck> {
  return invoke('compare_version', { droppedPath: dropped_path });
}

export async function historyList(limit?: number): Promise<HistoryEntry[]> {
  return invoke('history_list', { limit });
}

/** Typed wrappers for the Phase 2 backup IPC contract. */

export async function backupNow(uploadToDrive: boolean): Promise<BackupResult> {
  return invoke('backup_now', { uploadToDrive });
}

export async function listBackups(): Promise<BackupInfo[]> {
  return invoke('list_backups');
}

export async function restoreBackup(backupId: string): Promise<RestoreResult> {
  return invoke('restore_backup', { backupId });
}

export async function deleteBackup(backupId: string): Promise<SimpleResult> {
  return invoke('delete_backup', { backupId });
}

export async function getSettings(): Promise<Record<string, string>> {
  return invoke('get_settings');
}

export async function setSetting(key: string, value: string): Promise<void> {
  return invoke('set_setting', { key, value });
}

export async function driveStatus(): Promise<DriveStatus> {
  return invoke('drive_status');
}

export async function driveConnect(): Promise<SimpleResult> {
  return invoke('drive_connect');
}

export async function driveDisconnect(): Promise<SimpleResult> {
  return invoke('drive_disconnect');
}

export async function uploadBackupToDrive(backupId: string): Promise<SimpleResult> {
  return invoke('upload_backup_to_drive', { backupId });
}

export async function setAutostart(enabled: boolean): Promise<SimpleResult> {
  return invoke('set_autostart', { enabled });
}
