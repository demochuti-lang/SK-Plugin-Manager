# SK Plugin Manager — Phase 1 IPC Contract

Authoritative contract between the Rust backend (`src-tauri/`) and the React
frontend (`src/`). Both sides MUST implement exactly these command names,
argument names (snake_case), and shapes. Do not rename.

## Shared types

```ts
type AppId = 'ae' | 'pp' | 'ps' | 'ai';          // After Effects, Premiere Pro, Photoshop, Illustrator
type PluginKind = 'extension' | 'script';
type PluginSource = 'installed' | 'detected';    // installed = via this app, detected = found by system scan

interface AdobeApp {
  id: AppId;
  name: string;            // e.g. "Adobe After Effects 2025"
  version: string;         // e.g. "2025" (year parsed from folder name; "unknown" if unparseable)
  install_path: string;    // full path to the .app (mac) or install dir (win)
}

interface PluginEntry {
  id: string;              // stable id: ExtensionBundleId for extensions, "<stem>__<appid>" for scripts
  name: string;            // human name (bundle id prettified, or file stem)
  version: string;         // from manifest, or parsed $VERSION/@version, else "unknown"
  kind: PluginKind;
  target_apps: AppId[];    // from manifest HostList, or sniffed for scripts
  install_path: string;    // where it lives on disk right now
  source: PluginSource;
}

interface InstallResult {
  path: string;            // the dropped path this result is for
  success: boolean;
  message: string;         // human-readable, shown in toast
  plugin?: PluginEntry;    // present on success
  was_update?: boolean;    // true if it overwrote an existing library entry
}

interface UpdateCheck {
  has_update: boolean;
  plugin_name?: string;
  installed_version?: string;
  new_version?: string;
  target_apps?: AppId[];
}

interface HistoryEntry {
  id: number;
  event: 'install' | 'uninstall' | 'update' | 'import' | 'scan';
  plugin_name: string;
  detail: string;
  created_at: string;      // ISO-ish "YYYY-MM-DD HH:MM:SS"
}
```

## Tauri commands (invoke from frontend with `invoke('<name>', { <args> })`)

| Command | Args | Returns |
|---|---|---|
| `detect_adobe_apps` | — | `AdobeApp[]` |
| `list_plugins` | — | `PluginEntry[]` (the library, from SQLite) |
| `scan_existing` | — | `PluginEntry[]` (`source: 'detected'`, NOT persisted) |
| `import_existing` | `{ entries: PluginEntry[] }` | `{ imported: number }` (skips ids already in library; logs `import` history) |
| `install_dropped` | `{ paths: string[] }` | `InstallResult[]` (one per path; sends OS notification per file; logs `install`/`update` history) |
| `uninstall` | `{ entry_id: string }` | `{ success: boolean, message: string }` (deletes files, removes DB row, logs `uninstall` history) |
| `compare_version` | `{ dropped_path: string }` | `UpdateCheck` (parse dropped package manifest version vs library) |
| `history_list` | `{ limit?: number }` | `HistoryEntry[]` (newest first, default 200) |

## Backend rules

- **ZXP = zip.** Find `CSXS/manifest.xml` (case-insensitive) inside. Parse
  `ExtensionBundle @Id`, `@Version`, and `HostList/Host @Name/@Version`.
  Host map: `AEFT→ae, PPRO→pp, PHSP→ps, ILST→ai`. Unknown hosts are dropped;
  if none remain, target_apps = all four ids the app is known to support is
  WRONG — instead return success=false with message "No supported Adobe host
  found in manifest".
- **CEP install dir:** macOS `~/Library/Application Support/Adobe/CEP/extensions`;
  Windows first try `C:\Program Files (x86)\Common Files\Adobe\CEP\extensions`,
  on write failure fall back to `%APPDATA%\Adobe\CEP\extensions`. Extract zip to
  temp, copy tree into `<cep>/<folder_name>` (`folder_name` = zxp file stem,
  sanitized). Remove existing dir first (update path).
- **Dropped folder** containing `CSXS/manifest.xml` → copy as extension (same rules).
- **Scripts** (`.jsx`/`.jsxbin`): read first 8 KiB as lossy text, detect target:
  1. `#target photoshop`→ps, `#target illustrator`→ai,
     `#target aftereffects`→ae, `#target premiere`→pp (case-insensitive)
  2. `activeSequence`→pp, `CompItem`→ae, `activeDocument`→ps,
     `app.project`→ae
  3. default ae, message notes the assumption.
  Script dirs (use newest detected version of the target app; create dir if missing):
  - ae mac: `<install>/Contents/Scripts/ScriptUI Panels`, else `<install>/Scripts/ScriptUI Panels`
  - ae win: `<install>\Support Files\Scripts\ScriptUI Panels`
  - ps/ai mac: `<install>/../Presets/Scripts`, else `<install>/Presets/Scripts`
    (Photoshop/Illustrator ship a top-level `Presets` next to the `.app`)
  - ps/ai win: `<install>\Presets\Scripts`
  - Illustrator locale variant: also try `Presets/<locale>/Scripts` if plain `Presets/Scripts` missing — accept any existing `Scripts` dir under `Presets`.
- **DB** at `<app_data_dir>/sk-plugin-manager/library.db`, tables:
  `plugins(id TEXT PK, name, version, kind, target_apps JSON, install_path, source, installed_at)`,
  `history(id INTEGER PK AUTOINCREMENT, event, plugin_name, detail, created_at DEFAULT datetime)`.
- **Version compare:** split on `.`, compare numeric segments; `has_update` =
  new > installed. Unknown/"unknown" versions → has_update=false.
- **Notifications:** `tauri-plugin-notification`, title "SK Plugin Manager",
  body = per-file success/fail message. Request permission at startup if needed.
- **detect_adobe_apps:** macOS scan `/Applications`; Windows scan
  `C:\Program Files\Adobe` and `C:\Program Files (x86)\Adobe`
  (+ `winreg` HKLM\SOFTWARE\Adobe as secondary, deduped). Match dir names starting
  with `Adobe After Effects`→ae, `Adobe Premiere Pro`→pp, `Adobe Photoshop`→ps,
  `Adobe Illustrator`→ai (also handle trailing `.app`). Version = trailing
  4-digit year in name else "unknown". Linux/other OS → `[]`.
- **scan_existing:** walk existing CEP dirs + each detected app's script dirs
  (same candidates as install); parse manifests; never touch the DB.
- All commands must be `#[tauri::command]` + `async` where doing IO, registered
  in the builder with `.invoke_handler(...)`. Errors inside a command must be
  caught and returned as failed `InstallResult`/message — never panic to the
  frontend. Only truly unexpected failures may `Err(String)`.

## Frontend rules

- `src/types.ts` mirrors the TS types above verbatim.
- `src/lib/tauri.ts` exports typed wrappers: `detectAdobeApps()`,
  `listPlugins()`, `scanExisting()`, `importExisting(e)`, `installDropped(p)`,
  `uninstall(id)`, `compareVersion(p)`, `historyList()`.
- App state in `src/App.tsx`; screens in `src/screens/`
  (`Dashboard.tsx`, `DropZone.tsx`, `Backup.tsx`, `History.tsx`, `Settings.tsx`).
- Tailwind `darkMode: 'class'`; theme cycle System→Light→Dark in top bar AND
  Settings segmented control, synced, default = system (`matchMedia`).
- Drop Zone: real drag-drop via `getCurrentWebview().onDragDropEvent`
  (`@tauri-apps/api/webview`) → on `drop` event call `installDropped(paths)`;
  "browse" button via `open({ multiple: true })` from `@tauri-apps/plugin-dialog`.
  Show per-file progress → toast per result; refresh library + history.
- Dashboard: stats (Installed count, apps detected, last backup = "Phase 2"),
  search filter, per-row Uninstall (confirm inline), per-row "Check update"
  → file picker → `compare_version` → if `has_update`, confirm → `installDropped`.
  Imported/detected rows show a "Detected" pill; installed-via-app rows show
  "Installed" pill.
- First-run banner + "Scan system" button → modal with animated steps →
  results with checkboxes → `import_existing` → toast + list refresh.
- History screen: timeline from `history_list()`.
- Settings: theme control, notification toggle (in-memory), detected apps with
  mono paths, About card (SK Plugin Manager, Free, by Md. Shakib Hossain, v1.0.0).
- **Backup screen: full UI exists, but EVERY action button shows toast
  "Backup engine coming in Phase 2".** Do not implement backup logic.
- Toast system in `src/components/Toast.tsx` (success/error/info).
- No emojis in UI chrome; use inline SVG icons. English UI text.

---

# Phase 2 additions (backup system, scheduler, Google Drive)

Everything in Phase 1 above is unchanged. This section documents the new
commands, types, settings keys, history events, and the tray event added in
Phase 2. The Backup screen's placeholder toasts are replaced by real calls.

## New shared types

```ts
type BackupLocation = 'local' | 'drive' | 'both';

interface BackupInfo {
  id: string;            // e.g. "skpm-backup-20261019-183000" (filename without .zip)
  filename: string;      // e.g. "skpm-backup-20261019-183000.zip"
  created_at: string;    // ISO-ish "YYYY-MM-DD HH:MM:SS" (local time)
  size_bytes: number;
  item_count: number;
  location: BackupLocation;
}

interface BackupResult {
  success: boolean;
  message: string;       // human-readable, shown in toast
  backup_id?: string;    // present on success
  item_count?: number;   // present on success (staged items; skipped items are only in message)
  size_bytes?: number;   // present on success
}

interface RestoreResult {
  success: boolean;      // false if ANY item failed
  message: string;       // e.g. "Restored 3 item(s)" or "Restored 2 item(s), 1 failed …: <reasons>"
  restored: number;
  failed: number;
}

interface SimpleResult {
  success: boolean;
  message: string;       // drive commands: may start with "NEEDS_SETUP: " (see below)
}

interface DriveStatus {
  configured: boolean;   // client id + secret are stored
  connected: boolean;    // a refresh token is stored
  email?: string;        // Google account email, when known
}

// HistoryEntry.event gains: 'backup' | 'restore'
```

## New Tauri commands

| Command | Args | Returns |
|---|---|---|
| `backup_now` | `{ upload_to_drive: boolean }` | `BackupResult` (stages every library item → zip in the backup folder; `success:false` + `"Nothing to back up yet…"` when the library is empty; emits `skpm://backup-completed`) |
| `list_backups` | — | `BackupInfo[]` (newest first) |
| `restore_backup` | `{ backup_id: string }` | `RestoreResult` (copies files back to each item's original install path; extension dirs are removed first like the install update path; library rows are upserted with their existing `source`) |
| `delete_backup` | `{ backup_id: string }` | `SimpleResult` (deletes the zip + DB row; when `location` is `drive`/`both`, best-effort Drive delete too) |
| `get_settings` | — | `{ [key: string]: string }` (all known keys, defaults filled; never includes the Drive refresh token) |
| `set_setting` | `{ key: string, value: string }` | `()` (`Err` on unknown key or invalid value; `backup_folder` is created on demand) |
| `drive_status` | — | `DriveStatus` (presence-only, no network) |
| `drive_connect` | — | `SimpleResult` (opens the Google consent page in the browser; loopback OAuth flow; stores refresh token + email. Message starts with `NEEDS_SETUP: ` when no Client ID/Secret is stored) |
| `drive_disconnect` | — | `SimpleResult` (clears refresh token + email) |
| `upload_backup_to_drive` | `{ backup_id: string }` | `SimpleResult` (multipart upload of an existing local backup; flips its `location` to `both`; `NEEDS_SETUP: `-prefixed message when credentials are missing) |
| `set_autostart` | `{ enabled: boolean }` | `SimpleResult` (currently always `{ success:false, message:"Launch at login is coming soon" }`; the preference IS persisted under `launch_at_login` for when a stable autostart plugin ships) |

## Settings keys (SQLite `settings` table)

| Key | Values | Default |
|---|---|---|
| `backup_folder` | absolute path (created on demand) | `<home>/SKPluginManager/backups` |
| `auto_backup_enabled` | `"true"`/`"false"` | `"false"` |
| `backup_interval_value` | e.g. `"6"` | `"24"` |
| `backup_interval_unit` | `"hours"`/`"days"` | `"hours"` |
| `drive_client_id` | Google OAuth client id | `""` |
| `drive_client_secret` | Google OAuth client secret | `""` |
| `drive_refresh_token` | stored by `drive_connect`; never exposed via `get_settings` | `""` |
| `drive_email` | Google account email from `drive_connect` | `""` |
| `last_backup_at` | unix seconds, `"0"` = never | `"0"` |
| `notifications_enabled` | `"true"`/`"false"` | `"true"` |
| `launch_at_login` | `"true"`/`"false"` (persisted; OS toggle not implemented yet) | `"false"` |

## Backend rules (Phase 2)

- **DB:** new tables `settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)` and
  `backups(id TEXT PK, filename, created_at DEFAULT datetime, size_bytes,
  item_count, location DEFAULT 'local')`, created in `init_db` alongside the
  Phase 1 tables.
- **Backup zip layout:** `metadata.json` at the root plus
  `items/<entry id>/…` (extension → whole dir tree; script → the single file,
  original filename preserved). `metadata.json` =
  `{ app:"sk-plugin-manager", app_version:"1.0.0", created_at, items:[
  { id, name, version, kind:"extension"|"script", target_apps:["ae",…],
  original_install_path } ] }`. Sources missing from disk are skipped and
  counted in the result message.
- **Scheduler:** background task ticks every 60s; when
  `auto_backup_enabled=="true"` and `now - last_backup_at >= interval`
  (or never backed up), it runs the same core as `backup_now` with
  `upload_to_drive=false`. Scheduled runs respect `notifications_enabled`.
- **System tray** (Tauri 2 core `tray-icon` feature — NOT a separate plugin;
  no `tray-icon:default` capability exists): menu = "Open SK Plugin Manager",
  "Back up now", separator, "Quit". Closing the main window hides it to the
  tray instead of quitting; Quit comes from the tray menu.
- **Google Drive:** manual OAuth2 loopback flow (`reqwest` + rustls, scope
  `https://www.googleapis.com/auth/drive.file`). Credentials ONLY from
  settings — never hardcoded. Any drive command without Client ID/Secret
  returns `success:false` with a message starting `NEEDS_SETUP: ` pointing
  at `DRIVE_SETUP.md`. Frontend: when a message starts with `NEEDS_SETUP: `,
  show the Drive setup guidance instead of a plain error toast.
- **Events:** `skpm://backup-completed` is emitted with the `BackupInfo`
  after every backup (manual, tray, or scheduled) so the UI refreshes.
- **`set_autostart`:** the only published `tauri-plugin-autostart` v2 is an
  alpha written against the tauri 2.0.0-alpha.9 plugin API
  (`AppHandle::env()` no longer exists in tauri 2.12), so the dependency was
  deliberately dropped to protect the build. Returns the "coming soon"
  `SimpleResult`; the `launch_at_login` preference is still persisted.

## Frontend rules (Phase 2)

- `src/types.ts`: add the Phase 2 types above verbatim; extend
  `HistoryEntry['event']` with `'backup' | 'restore'`.
- `src/lib/tauri.ts`: add typed wrappers `backupNow(uploadToDrive)`,
  `listBackups()`, `restoreBackup(id)`, `deleteBackup(id)`, `getSettings()`,
  `setSetting(k, v)`, `driveStatus()`, `driveConnect()`,
  `driveDisconnect()`, `uploadBackupToDrive(id)`, `setAutostart(enabled)`.
- Backup screen: wire the real commands (replace the "coming in Phase 2"
  toasts); list backups newest-first with Restore/Delete per row; "Back up
  now" passes the Drive toggle state as `upload_to_drive`; listen for
  `skpm://backup-completed` (via `listen` from `@tauri-apps/api/event`) to
  refresh the list and the Dashboard "last backup" stat.
- Settings screen: backup folder picker (via `set_setting`), auto-backup
  enable + interval value/unit controls, notification toggle (persisted via
  `set_setting("notifications_enabled", …)` — the backend now respects it),
  Google Drive section (Client ID/Secret fields → `set_setting`; Connect /
  Disconnect buttons; `NEEDS_SETUP: ` messages link to `DRIVE_SETUP.md`;
  show `DriveStatus.email` when connected), launch-at-login toggle (expects
  the "coming soon" message for now).
- Dashboard: "last backup" stat reads `last_backup_at` from `get_settings()`
  (unix seconds; `"0"` = never) instead of the Phase 1 placeholder.
