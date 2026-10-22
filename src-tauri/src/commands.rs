//! The 8 Tauri commands of the Phase 1 IPC contract.
//!
//! Every command catches its errors and returns them to the frontend as data
//! (failed `InstallResult` / message); only truly unexpected failures use
//! `Err(String)`. Nothing here panics.

use std::path::PathBuf;

use tauri::{AppHandle, Runtime};

use crate::adobe::{cep_scan_dirs, script_scan_dirs};
use crate::db;
use crate::install::{do_fs_install, find_manifest_in_dir};
use crate::manifest::{
    is_newer_version, parse_manifest_xml, parse_script_version, prettify_name,
    sniff_script_target, SNIFF_BYTES,
};
use crate::types::*;

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn notify<R: Runtime>(app: &AppHandle<R>, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    // Best effort: a missing notification server (e.g. headless Linux) must
    // not fail the install.
    let _ = app
        .notification()
        .builder()
        .title("SK Plugin Manager")
        .body(body)
        .show();
}

fn failed_install(path: &str, message: String) -> InstallResult {
    InstallResult {
        path: path.to_string(),
        success: false,
        message,
        plugin: None,
        was_update: None,
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// `detect_adobe_apps` → `AdobeApp[]`
#[tauri::command]
pub async fn detect_adobe_apps() -> Vec<AdobeApp> {
    // read_dir is blocking; keep it off the async runtime.
    tokio::task::spawn_blocking(crate::adobe::detect_adobe_apps)
        .await
        .unwrap_or_default()
}

/// `list_plugins` → `PluginEntry[]` (the library, from SQLite)
#[tauri::command]
pub async fn list_plugins<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<PluginEntry>, String> {
    let pool = db::pool(&app).await?;
    db::list_all_plugins(&pool)
        .await
        .map_err(|e| format!("could not list library: {e}"))
}

/// `scan_existing` → `PluginEntry[]` (`source: 'detected'`, NOT persisted)
#[tauri::command]
pub async fn scan_existing() -> Result<Vec<PluginEntry>, String> {
    tokio::task::spawn_blocking(scan_existing_blocking)
        .await
        .map_err(|e| format!("scan task failed: {e}"))
}

fn scan_existing_blocking() -> Vec<PluginEntry> {
    let mut out: Vec<PluginEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut push = |entry: PluginEntry| {
        if seen.insert(entry.id.clone()) {
            out.push(entry);
        }
    };

    // 1. CEP extension dirs.
    for cep_dir in cep_scan_dirs() {
        let entries = match std::fs::read_dir(&cep_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let manifest_path = match find_manifest_in_dir(&dir) {
                Some(p) => p,
                None => continue,
            };
            let xml = match std::fs::read_to_string(&manifest_path) {
                Ok(x) => x,
                Err(_) => continue,
            };
            let manifest = match parse_manifest_xml(&xml) {
                Ok(m) => m,
                Err(_) => continue, // unparseable / unsupported hosts: skip silently
            };
            push(PluginEntry {
                id: manifest.bundle_id.clone(),
                name: prettify_name(&manifest.bundle_id),
                version: manifest.version,
                kind: PluginKind::Extension,
                target_apps: manifest.target_apps,
                install_path: dir.to_string_lossy().to_string(),
                source: PluginSource::Detected,
            });
        }
    }

    // 2. Script dirs of every detected app.
    for app in crate::adobe::detect_adobe_apps() {
        for dir in script_scan_dirs(&app) {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_file() {
                    continue;
                }
                let is_script = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("jsx") || e.eq_ignore_ascii_case("jsxbin"))
                    .unwrap_or(false);
                if !is_script {
                    continue;
                }
                let stem = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("script")
                    .to_string();
                push(PluginEntry {
                    id: format!("{stem}__{}", app.id.as_str()),
                    name: stem,
                    version: "unknown".to_string(),
                    kind: PluginKind::Script,
                    target_apps: vec![app.id],
                    install_path: p.to_string_lossy().to_string(),
                    source: PluginSource::Detected,
                });
            }
        }
    }

    out.sort_by(|a, b| {
        (a.kind.as_str(), a.name.to_lowercase())
            .cmp(&(b.kind.as_str(), b.name.to_lowercase()))
    });
    out
}

/// `import_existing` → `{ imported: number }`
/// (skips ids already in the library; logs an `import` history event)
#[tauri::command]
pub async fn import_existing<R: Runtime>(
    app: AppHandle<R>,
    entries: Vec<PluginEntry>,
) -> Result<ImportResult, String> {
    let pool = db::pool(&app).await?;
    let mut imported: i64 = 0;
    for entry in &entries {
        match db::insert_plugin_ignore(&pool, entry).await {
            Ok(true) => imported += 1,
            Ok(false) => {}
            Err(e) => return Err(format!("could not import {}: {e}", entry.id)),
        }
    }
    if imported > 0 {
        let _ = db::log_history(
            &pool,
            HistoryEvent::Import,
            "System scan",
            &format!("Imported {imported} detected plugin(s) into the library"),
        )
        .await;
    }
    Ok(ImportResult { imported })
}

/// `install_dropped` → `InstallResult[]`
/// (one per path; OS notification per file; logs `install`/`update` history)
#[tauri::command]
pub async fn install_dropped<R: Runtime>(
    app: AppHandle<R>,
    paths: Vec<String>,
) -> Result<Vec<InstallResult>, String> {
    let mut results = Vec::with_capacity(paths.len());
    for raw in &paths {
        results.push(install_one(&app, raw).await);
    }
    Ok(results)
}

async fn install_one<R: Runtime>(app: &AppHandle<R>, raw: &str) -> InstallResult {
    // Fail fast when the library DB is unavailable: installing files we
    // cannot record would leave orphaned, untracked installs.
    let pool = match db::pool(app).await {
        Ok(p) => p,
        Err(e) => return failed_install(raw, format!("library database unavailable: {e}")),
    };

    // Heavy filesystem work off the async runtime.
    let path = PathBuf::from(raw);
    let fs = match tokio::task::spawn_blocking(move || do_fs_install(&path)).await {
        Ok(Ok(fs)) => fs,
        Ok(Err(e)) => return failed_install(raw, e),
        Err(e) => return failed_install(raw, format!("install worker failed: {e}")),
    };

    // Persist: upsert the library row.
    let existed = db::plugin_exists(&pool, &fs.id).await.unwrap_or(false);
    let install_path_str = fs.install_path.to_string_lossy().to_string();
    if let Err(e) = db::upsert_plugin(
        &pool,
        &fs.id,
        &fs.name,
        &fs.version,
        fs.kind,
        &fs.target_apps,
        &install_path_str,
        PluginSource::Installed,
    )
    .await
    {
        return failed_install(raw, format!("files installed but library update failed: {e}"));
    }
    let event = if existed {
        HistoryEvent::Update
    } else {
        HistoryEvent::Install
    };
    let _ = db::log_history(&pool, event, &fs.name, &fs.message).await;

    notify(app, &fs.message);

    let mut message = fs.message.clone();
    if existed {
        message.push_str(" (updated)");
    }
    InstallResult {
        path: raw.to_string(),
        success: true,
        message,
        plugin: Some(PluginEntry {
            id: fs.id,
            name: fs.name,
            version: fs.version,
            kind: fs.kind,
            target_apps: fs.target_apps,
            install_path: install_path_str,
            source: PluginSource::Installed,
        }),
        was_update: Some(existed),
    }
}

/// `uninstall` → `{ success: boolean, message: string }`
/// (deletes files, removes the DB row, logs an `uninstall` history event)
#[tauri::command]
pub async fn uninstall<R: Runtime>(
    app: AppHandle<R>,
    entry_id: String,
) -> Result<UninstallResult, String> {
    let pool = db::pool(&app).await?;
    let entry = db::get_plugin(&pool, &entry_id)
        .await
        .map_err(|e| format!("could not look up library entry: {e}"))?;
    let entry = match entry {
        Some(e) => e,
        None => {
            return Ok(UninstallResult {
                success: false,
                message: "Entry is not in the library".to_string(),
            })
        }
    };

    // Delete files (blocking).
    let install_path = PathBuf::from(&entry.install_path);
    let kind = entry.kind;
    let name = entry.name.clone();
    let fs_ok = tokio::task::spawn_blocking(move || -> Result<(), String> {
        if !install_path.exists() {
            return Ok(()); // already gone: still drop the DB row
        }
        let res = match kind {
            PluginKind::Extension => std::fs::remove_dir_all(&install_path),
            PluginKind::Script => std::fs::remove_file(&install_path),
        };
        match res {
            Ok(()) => Ok(()),
            Err(_) if !install_path.exists() => Ok(()), // raced away: fine
            Err(e) => {
                // Windows: extensions in the system CEP folder
                // (C:\Program Files (x86)\Common Files\Adobe\CEP\extensions)
                // cannot be deleted by a normal process — retry via UAC.
                #[cfg(windows)]
                {
                    if crate::adobe::remove_path_elevated(&install_path).is_ok()
                        && !install_path.exists()
                    {
                        return Ok(());
                    }
                }
                Err(format!(
                    "could not delete {}: {e}. If the plugin lives in a protected folder, approve the admin prompt when asked; also close the Adobe app if it is open, since open apps can lock the files.",
                    install_path.display()
                ))
            }
        }
    })
    .await
    .map_err(|e| format!("uninstall worker failed: {e}"))?;

    if let Err(e) = fs_ok {
        return Ok(UninstallResult {
            success: false,
            message: e,
        });
    }

    db::delete_plugin(&pool, &entry_id)
        .await
        .map_err(|e| format!("files removed but library update failed: {e}"))?;
    let _ = db::log_history(
        &pool,
        HistoryEvent::Uninstall,
        &name,
        &format!("Removed {}", entry.install_path),
    )
    .await;

    Ok(UninstallResult {
        success: true,
        message: format!("Uninstalled “{name}”"),
    })
}

/// Dropped-package info parsed without installing anything.
struct DroppedInfo {
    id: String,
    name: String,
    version: String,
    target_apps: Vec<AppId>,
}

fn parse_dropped_package(path: &PathBuf) -> Result<DroppedInfo, String> {
    if path.is_dir() {
        let manifest_path = find_manifest_in_dir(path)
            .ok_or_else(|| "folder does not contain CSXS/manifest.xml".to_string())?;
        let xml = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("could not read manifest: {e}"))?;
        let m = parse_manifest_xml(&xml)?;
        return Ok(DroppedInfo {
            name: prettify_name(&m.bundle_id),
            id: m.bundle_id,
            version: m.version,
            target_apps: m.target_apps,
        });
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("zxp") => {
            let file = std::fs::File::open(path)
                .map_err(|e| format!("could not open {}: {e}", path.display()))?;
            let mut zip = zip::ZipArchive::new(file)
                .map_err(|e| format!("not a valid zip archive: {e}"))?;
            let mut manifest_xml: Option<String> = None;
            for i in 0..zip.len() {
                let mut f = zip
                    .by_index(i)
                    .map_err(|e| format!("could not read archive: {e}"))?;
                if f.name().to_lowercase().ends_with("csxs/manifest.xml") {
                    let mut s = String::new();
                    use std::io::Read;
                    f.read_to_string(&mut s)
                        .map_err(|e| format!("could not read manifest: {e}"))?;
                    manifest_xml = Some(s);
                    break;
                }
            }
            let xml = manifest_xml
                .ok_or_else(|| "no CSXS/manifest.xml found in archive".to_string())?;
            let m = parse_manifest_xml(&xml)?;
            Ok(DroppedInfo {
                name: prettify_name(&m.bundle_id),
                id: m.bundle_id,
                version: m.version,
                target_apps: m.target_apps,
            })
        }
        Some("jsx") | Some("jsxbin") => {
            let mut file = std::fs::File::open(path)
                .map_err(|e| format!("could not open {}: {e}", path.display()))?;
            let mut buf = vec![0u8; SNIFF_BYTES];
            use std::io::Read;
            let n = file
                .read(&mut buf)
                .map_err(|e| format!("could not read {}: {e}", path.display()))?;
            buf.truncate(n);
            let head = String::from_utf8_lossy(&buf).to_string();
            let sniff = sniff_script_target(&head);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("script")
                .to_string();
            Ok(DroppedInfo {
                id: format!("{stem}__{}", sniff.app.as_str()),
                name: stem,
                version: parse_script_version(&head),
                target_apps: vec![sniff.app],
            })
        }
        Some(other) => Err(format!("unsupported file type: .{other}")),
        None => Err("file has no extension; expected .zxp, .jsx or .jsxbin".to_string()),
    }
}

/// `compare_version` → `UpdateCheck`
/// (parse the dropped package's manifest version vs the library entry)
#[tauri::command]
pub async fn compare_version<R: Runtime>(
    app: AppHandle<R>,
    dropped_path: String,
) -> Result<UpdateCheck, String> {
    let path = PathBuf::from(&dropped_path);
    if !path.exists() {
        return Err(format!("path does not exist: {dropped_path}"));
    }
    let info = tokio::task::spawn_blocking(move || parse_dropped_package(&path))
        .await
        .map_err(|e| format!("compare worker failed: {e}"))??;

    let pool = db::pool(&app).await?;
    let existing = db::get_plugin(&pool, &info.id)
        .await
        .map_err(|e| format!("could not look up library entry: {e}"))?;

    match existing {
        None => Ok(UpdateCheck {
            has_update: false,
            plugin_name: Some(info.name),
            installed_version: None,
            new_version: Some(info.version),
            target_apps: Some(info.target_apps),
        }),
        Some(row) => Ok(UpdateCheck {
            has_update: is_newer_version(&info.version, &row.version),
            plugin_name: Some(row.name),
            installed_version: Some(row.version),
            new_version: Some(info.version),
            target_apps: Some(info.target_apps),
        }),
    }
}

/// `history_list` → `HistoryEntry[]` (newest first, default 200)
#[tauri::command]
pub async fn history_list<R: Runtime>(
    app: AppHandle<R>,
    limit: Option<i64>,
) -> Result<Vec<HistoryEntry>, String> {
    let limit = match limit {
        Some(n) if n > 0 => n.min(5000),
        _ => 200,
    };
    let pool = db::pool(&app).await?;
    db::list_history(&pool, limit)
        .await
        .map_err(|e| format!("could not read history: {e}"))
}

// ---------------------------------------------------------------------------
// Phase 2: backup system, settings, Google Drive, autostart
// ---------------------------------------------------------------------------

/// `backup_now` → `BackupResult`
/// (full backup now; optionally also uploads the zip to Google Drive)
#[tauri::command]
pub async fn backup_now<R: Runtime>(
    app: AppHandle<R>,
    upload_to_drive: bool,
) -> Result<BackupResult, String> {
    crate::backup::run_backup(&app, upload_to_drive).await
}

/// `list_backups` → `BackupInfo[]` (newest first)
#[tauri::command]
pub async fn list_backups<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<BackupInfo>, String> {
    let pool = db::pool(&app).await?;
    db::list_backups(&pool)
        .await
        .map_err(|e| format!("could not list backups: {e}"))
}

/// `restore_backup` → `RestoreResult`
/// (copies a backup's files back to their original install paths)
#[tauri::command]
pub async fn restore_backup<R: Runtime>(
    app: AppHandle<R>,
    backup_id: String,
) -> Result<RestoreResult, String> {
    crate::backup::restore_backup(&app, &backup_id).await
}

/// `delete_backup` → `{ success, message }`
/// (deletes the zip + DB row; best-effort remote delete when on Drive)
#[tauri::command]
pub async fn delete_backup<R: Runtime>(
    app: AppHandle<R>,
    backup_id: String,
) -> Result<SimpleResult, String> {
    crate::backup::delete_backup(&app, &backup_id).await
}

/// `get_settings` → `{ [key]: string }` (known keys, defaults filled in)
#[tauri::command]
pub async fn get_settings<R: Runtime>(
    app: AppHandle<R>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let pool = db::pool(&app).await?;
    crate::settings::effective_settings(&app, &pool).await
}

/// `set_setting` → `()` (validates per key; rejects unknown keys/values)
#[tauri::command]
pub async fn set_setting<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    value: String,
) -> Result<(), String> {
    let pool = db::pool(&app).await?;
    crate::settings::set_setting_validated(&pool, &key, &value).await
}

/// `drive_status` → `DriveStatus` (presence-only; no network)
#[tauri::command]
pub async fn drive_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<DriveStatus, String> {
    crate::drive::drive_status(&app).await
}

/// `drive_connect` → `{ success, message }` (loopback OAuth flow)
#[tauri::command]
pub async fn drive_connect<R: Runtime>(
    app: AppHandle<R>,
) -> Result<SimpleResult, String> {
    crate::drive::drive_connect(&app).await
}

/// `drive_disconnect` → `{ success, message }` (clears token + email)
#[tauri::command]
pub async fn drive_disconnect<R: Runtime>(
    app: AppHandle<R>,
) -> Result<SimpleResult, String> {
    crate::drive::drive_disconnect(&app).await
}

/// `upload_backup_to_drive` → `{ success, message }`
/// (uploads one existing local backup's zip to Google Drive)
#[tauri::command]
pub async fn upload_backup_to_drive<R: Runtime>(
    app: AppHandle<R>,
    backup_id: String,
) -> Result<SimpleResult, String> {
    crate::drive::upload_existing_backup(&app, &backup_id).await
}

/// `set_autostart` → `{ success, message }`
/// (launch at login; see `crate::autostart_cfg`)
#[tauri::command]
pub async fn set_autostart<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
) -> Result<SimpleResult, String> {
    crate::autostart_cfg::apply(&app, enabled).await
}
