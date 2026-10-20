//! Phase 2 backup engine: stage library items → `metadata.json` → zip.
//!
//! Layout inside the zip:
//! ```text
//! metadata.json
//! items/<entry id>/...          (extension: whole dir tree; script: the single file)
//! ```
//! The pure helpers (`stage_items`, `write_metadata`, `parse_metadata`,
//! `zip_dir`, `unzip_to`, `backup_filename`) need no Tauri state and are
//! unit-tested below. `run_backup` / `restore_backup` / `delete_backup`
//! wire them up to the database, settings, notifications and Drive.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{Pool, Sqlite};
use tauri::{AppHandle, Emitter, Runtime};

use crate::db;
use crate::types::*;

// ---------------------------------------------------------------------------
// Pure, unit-testable pieces
// ---------------------------------------------------------------------------

/// One library item prepared for staging (built from a [`PluginEntry`]).
pub struct StageSource {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    pub target_apps: Vec<AppId>,
    pub install_path: PathBuf,
}

impl StageSource {
    fn from_entry(e: &PluginEntry) -> Self {
        StageSource {
            id: e.id.clone(),
            name: e.name.clone(),
            version: e.version.clone(),
            kind: e.kind,
            target_apps: e.target_apps.clone(),
            install_path: PathBuf::from(&e.install_path),
        }
    }
}

/// One item as recorded in `metadata.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetaItem {
    pub id: String,
    pub name: String,
    pub version: String,
    /// "extension" | "script"
    pub kind: String,
    pub target_apps: Vec<String>,
    pub original_install_path: String,
}

/// The `metadata.json` schema written into every backup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupMetadata {
    pub app: String,
    pub app_version: String,
    pub created_at: String,
    pub items: Vec<MetaItem>,
}

/// `skpm-backup-YYYYMMDD-HHMMSS.zip` (suffixes like `-2` may be appended by
/// `run_backup` when two backups land in the same second).
pub fn backup_filename(stamp: &str) -> String {
    format!("skpm-backup-{stamp}.zip")
}

/// Copy every source into `staging/items/<id>/…`.
/// Returns the metadata items plus the number of skipped sources
/// (missing path, or a copy that failed partway — the partial copy is
/// removed so the staging dir stays clean).
pub fn stage_items(
    sources: &[StageSource],
    staging: &Path,
) -> Result<(Vec<MetaItem>, u64), String> {
    let items_dir = staging.join("items");
    std::fs::create_dir_all(&items_dir)
        .map_err(|e| format!("could not create staging dir: {e}"))?;

    let mut meta_items = Vec::with_capacity(sources.len());
    let mut skipped: u64 = 0;
    for src in sources {
        let dest = items_dir.join(&src.id);
        let staged_ok = match src.kind {
            PluginKind::Extension if src.install_path.is_dir() => {
                copy_dir_all(&src.install_path, &dest).is_ok()
            }
            PluginKind::Script if src.install_path.is_file() => {
                let file_name = src
                    .install_path
                    .file_name()
                    .unwrap_or_default();
                std::fs::create_dir_all(&dest)
                    .and_then(|()| {
                        std::fs::copy(&src.install_path, dest.join(file_name)).map(|_| ())
                    })
                    .is_ok()
            }
            _ => false,
        };
        if staged_ok {
            meta_items.push(MetaItem {
                id: src.id.clone(),
                name: src.name.clone(),
                version: src.version.clone(),
                kind: src.kind.as_str().to_string(),
                target_apps: src
                    .target_apps
                    .iter()
                    .map(|a| a.as_str().to_string())
                    .collect(),
                original_install_path: src.install_path.to_string_lossy().to_string(),
            });
        } else {
            let _ = std::fs::remove_dir_all(&dest);
            skipped += 1;
        }
    }
    Ok((meta_items, skipped))
}

/// Write `staging/metadata.json`.
pub fn write_metadata(staging: &Path, meta: &BackupMetadata) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| format!("could not serialize backup metadata: {e}"))?;
    std::fs::write(staging.join("metadata.json"), json)
        .map_err(|e| format!("could not write metadata.json: {e}"))
}

/// Read `metadata.json` from an extracted backup, rejecting archives that
/// were not made by SK Plugin Manager.
pub fn parse_metadata(extract_dir: &Path) -> Result<BackupMetadata, String> {
    let raw = std::fs::read_to_string(extract_dir.join("metadata.json"))
        .map_err(|_| "not an SK Plugin Manager backup (metadata.json missing)".to_string())?;
    let meta: BackupMetadata = serde_json::from_str(&raw)
        .map_err(|e| format!("could not parse backup metadata: {e}"))?;
    if meta.app != "sk-plugin-manager" {
        return Err("not an SK Plugin Manager backup".to_string());
    }
    Ok(meta)
}

/// Zip a directory (including empty subdirs); returns the zip's size in bytes.
/// Entry names always use forward slashes.
pub fn zip_dir(src: &Path, dst: &Path) -> Result<u64, String> {
    let file = std::fs::File::create(dst)
        .map_err(|e| format!("could not create {}: {e}", dst.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|e| format!("could not read {}: {e}", dir.display()))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(src)
                .map_err(|e| format!("could not relativize path: {e}"))?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                zip.add_directory(format!("{rel}/"), options)
                    .map_err(|e| format!("could not add dir to zip: {e}"))?;
                stack.push(path);
            } else if path.is_file() {
                zip.start_file(&rel, options)
                    .map_err(|e| format!("could not add file to zip: {e}"))?;
                let mut f = std::fs::File::open(&path)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?;
                std::io::copy(&mut f, &mut zip)
                    .map_err(|e| format!("could not write {} to zip: {e}", path.display()))?;
            }
            // Anything else (symlinks, sockets): skipped deliberately.
        }
    }
    zip.finish()
        .map_err(|e| format!("could not finalize zip: {e}"))?;
    let size = std::fs::metadata(dst)
        .map_err(|e| format!("could not stat {}: {e}", dst.display()))?
        .len();
    Ok(size)
}

/// Extract a zip archive into `dst` (created if needed).
pub fn unzip_to(zip_path: &Path, dst: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("could not open {}: {e}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("not a valid zip archive: {e}"))?;
    zip.extract(dst)
        .map_err(|e| format!("could not extract {}: {e}", zip_path.display()))
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Small runtime helpers
// ---------------------------------------------------------------------------

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn notifications_on(pool: &Pool<Sqlite>) -> bool {
    // Default is on; only an explicit "false" disables.
    db::get_setting(pool, crate::settings::KEY_NOTIFICATIONS_ENABLED)
        .await
        .ok()
        .flatten()
        .is_none_or(|v| v != "false")
}

fn notify<R: Runtime>(app: &AppHandle<R>, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    // Best effort: a missing notification server must not fail the backup.
    let _ = app
        .notification()
        .builder()
        .title("SK Plugin Manager")
        .body(body)
        .show();
}

/// Stage → metadata → zip, all on a blocking thread. Returns
/// (metadata items, skipped count, zip size in bytes).
fn build_backup(
    sources: &[StageSource],
    staging: &Path,
    dest: &Path,
    created_at: &str,
) -> Result<(Vec<MetaItem>, u64, u64), String> {
    if staging.exists() {
        std::fs::remove_dir_all(staging)
            .map_err(|e| format!("could not clear staging dir: {e}"))?;
    }
    std::fs::create_dir_all(staging)
        .map_err(|e| format!("could not create staging dir: {e}"))?;
    let (items, skipped) = stage_items(sources, staging)?;
    write_metadata(
        staging,
        &BackupMetadata {
            app: "sk-plugin-manager".to_string(),
            app_version: "1.0.0".to_string(),
            created_at: created_at.to_string(),
            items: items.clone(),
        },
    )?;
    let size = zip_dir(staging, dest)?;
    let _ = std::fs::remove_dir_all(staging); // best-effort cleanup
    Ok((items, skipped, size))
}

// ---------------------------------------------------------------------------
// `backup_now` core — also called by the scheduler ticker
// ---------------------------------------------------------------------------

/// Run a full backup. Shared by the `backup_now` command and the scheduler.
pub async fn run_backup<R: Runtime>(
    app: &AppHandle<R>,
    upload_to_drive: bool,
) -> Result<BackupResult, String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    let plugins = db::list_all_plugins(&pool)
        .await
        .map_err(|e| format!("could not read library: {e}"))?;
    if plugins.is_empty() {
        return Ok(BackupResult {
            success: false,
            message: "Nothing to back up yet — install or import a plugin first.".to_string(),
            backup_id: None,
            item_count: None,
            size_bytes: None,
        });
    }

    let backup_dir = crate::settings::resolve_backup_dir(app, &pool).await?;
    let stamp: String = sqlx::query_scalar("SELECT strftime('%Y%m%d-%H%M%S','now','localtime')")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("could not read clock: {e}"))?;
    let created_at: String = sqlx::query_scalar("SELECT datetime('now','localtime')")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("could not read clock: {e}"))?;

    // Unique id even if two backups land in the same second.
    let mut filename = backup_filename(&stamp);
    let mut n = 2u32;
    while db::get_backup(&pool, filename.trim_end_matches(".zip"))
        .await
        .map_err(|e| format!("could not check existing backups: {e}"))?
        .is_some()
    {
        filename = backup_filename(&format!("{stamp}-{n}"));
        n += 1;
    }
    let backup_id = filename.trim_end_matches(".zip").to_string();
    let dest = backup_dir.join(&filename);

    let sources: Vec<StageSource> = plugins.iter().map(StageSource::from_entry).collect();
    let staging = std::env::temp_dir().join(format!("skpm-staging-{backup_id}"));
    let dest_for_worker = dest.clone();
    let (items, skipped, size) = tokio::task::spawn_blocking(move || {
        build_backup(&sources, &staging, &dest_for_worker, &created_at)
    })
    .await
    .map_err(|e| format!("backup worker failed: {e}"))??;

    let item_count = items.len() as i64;
    if items.is_empty() {
        let _ = std::fs::remove_file(&dest);
        return Ok(BackupResult {
            success: false,
            message: format!(
                "Nothing was backed up — all {} library item(s) are missing from disk.",
                plugins.len()
            ),
            backup_id: None,
            item_count: Some(0),
            size_bytes: None,
        });
    }

    db::insert_backup(
        &pool,
        &backup_id,
        &filename,
        size as i64,
        item_count,
        BackupLocation::Local,
    )
    .await
    .map_err(|e| format!("could not record backup: {e}"))?;
    let _ = db::set_setting_db(
        &pool,
        crate::settings::KEY_LAST_BACKUP_AT,
        &unix_secs().to_string(),
    )
    .await;

    let skipped_note = if skipped > 0 {
        format!(" ({skipped} item(s) skipped — missing from disk)")
    } else {
        String::new()
    };
    let _ = db::log_history(
        &pool,
        HistoryEvent::Backup,
        "Backup",
        &format!("Backed up {item_count} item(s){skipped_note} → {filename}"),
    )
    .await;

    let mut drive_note = String::new();
    if upload_to_drive {
        match crate::drive::upload_backup_file(&pool, &backup_id, &filename, &dest).await {
            Ok(()) => {}
            Err(e) => drive_note = format!(" (Drive upload failed: {e})"),
        }
    }

    if notifications_on(&pool).await {
        notify(app, &format!("Backup completed — {item_count} item(s)"));
    }

    // Re-read the row so the emitted info carries the DB-generated
    // `created_at` and the final location ("both" after a Drive upload).
    let info = db::get_backup(&pool, &backup_id)
        .await
        .map_err(|e| format!("could not read backup record: {e}"))?
        .ok_or_else(|| "backup record vanished".to_string())?;
    let _ = app.emit("skpm://backup-completed", &info);

    Ok(BackupResult {
        success: true,
        message: format!(
            "Backup completed — {item_count} item(s) backed up{skipped_note}{drive_note}"
        ),
        backup_id: Some(backup_id),
        item_count: Some(item_count),
        size_bytes: Some(size as i64),
    })
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/// Locate the staged file for a script item: `items/<id>/<filename>`,
/// falling back to the first file found in the item dir.
fn staged_script_file(staged_dir: &Path, dest: &Path) -> Result<PathBuf, String> {
    if let Some(name) = dest.file_name() {
        let direct = staged_dir.join(name);
        if direct.is_file() {
            return Ok(direct);
        }
    }
    std::fs::read_dir(staged_dir)
        .map_err(|_| "backup is missing this script's file".to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_file())
        .ok_or_else(|| "backup is missing this script's file".to_string())
}

fn restore_one(staged: &Path, dest: &Path, kind: &str) -> Result<(), String> {
    match kind {
        "extension" => {
            if !staged.is_dir() {
                return Err("backup is missing this extension's files".to_string());
            }
            if dest.exists() {
                // Same update path as install: remove first, then copy.
                std::fs::remove_dir_all(dest)
                    .map_err(|e| format!("could not remove existing {}: {e}", dest.display()))?;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            copy_dir_all(staged, dest)
                .map_err(|e| format!("could not restore {}: {e}", dest.display()))
        }
        "script" => {
            let staged_file = staged_script_file(staged, dest)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            std::fs::copy(&staged_file, dest)
                .map_err(|e| format!("could not restore {}: {e}", dest.display()))?;
            Ok(())
        }
        other => Err(format!("unknown item kind in backup: {other}")),
    }
}

fn app_id_from_str(s: &str) -> Option<AppId> {
    match s {
        "ae" => Some(AppId::Ae),
        "pp" => Some(AppId::Pp),
        "ps" => Some(AppId::Ps),
        "ai" => Some(AppId::Ai),
        _ => None,
    }
}

pub async fn restore_backup<R: Runtime>(
    app: &AppHandle<R>,
    backup_id: &str,
) -> Result<RestoreResult, String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    let info = db::get_backup(&pool, backup_id)
        .await
        .map_err(|e| format!("could not look up backup: {e}"))?
        .ok_or_else(|| "Backup not found".to_string())?;

    let zip_path = crate::settings::resolve_backup_dir(app, &pool)
        .await?
        .join(&info.filename);
    if !zip_path.is_file() {
        return Ok(RestoreResult {
            success: false,
            message: format!(
                "Backup file “{}” is missing from the backup folder.",
                info.filename
            ),
            restored: 0,
            failed: 0,
        });
    }

    let extract_dir = std::env::temp_dir().join(format!("skpm-restore-{backup_id}"));
    let meta = tokio::task::spawn_blocking({
        let zip_path = zip_path.clone();
        let extract_dir = extract_dir.clone();
        move || -> Result<BackupMetadata, String> {
            if extract_dir.exists() {
                std::fs::remove_dir_all(&extract_dir)
                    .map_err(|e| format!("could not clear temp dir: {e}"))?;
            }
            std::fs::create_dir_all(&extract_dir)
                .map_err(|e| format!("could not create temp dir: {e}"))?;
            unzip_to(&zip_path, &extract_dir)?;
            parse_metadata(&extract_dir)
        }
    })
    .await
    .map_err(|e| format!("restore worker failed: {e}"))??;

    let items_dir = extract_dir.join("items");
    let mut restored: i64 = 0;
    let mut failed: i64 = 0;
    let mut failures: Vec<String> = Vec::new();
    for item in &meta.items {
        let staged = items_dir.join(&item.id);
        let dest = PathBuf::from(&item.original_install_path);
        let kind = item.kind.clone();
        let outcome = tokio::task::spawn_blocking(move || restore_one(&staged, &dest, &kind))
            .await
            .map_err(|e| format!("restore worker failed: {e}"))?;
        match outcome {
            Ok(()) => {
                restored += 1;
                // Keep the library in sync; the rows are normally already there.
                let source = db::get_plugin(&pool, &item.id)
                    .await
                    .ok()
                    .flatten()
                    .map(|e| e.source)
                    .unwrap_or(PluginSource::Installed);
                let kind = PluginKind::from_str(&item.kind).unwrap_or(PluginKind::Extension);
                let apps: Vec<AppId> =
                    item.target_apps.iter().filter_map(|s| app_id_from_str(s)).collect();
                let _ = db::upsert_plugin(
                    &pool,
                    &item.id,
                    &item.name,
                    &item.version,
                    kind,
                    &apps,
                    &item.original_install_path,
                    source,
                )
                .await;
            }
            Err(e) => {
                failed += 1;
                failures.push(format!("{}: {e}", item.name));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&extract_dir);

    let detail = if failures.is_empty() {
        format!("Restored {restored} item(s) from “{}”", info.filename)
    } else {
        format!(
            "Restored {restored} item(s), {failed} failed from “{}”: {}",
            info.filename,
            failures.join("; ")
        )
    };
    let _ = db::log_history(&pool, HistoryEvent::Restore, "Restore", &detail).await;
    if notifications_on(&pool).await {
        notify(app, &format!("Restore completed — {restored} item(s) restored"));
    }
    Ok(RestoreResult {
        success: failed == 0,
        message: detail,
        restored,
        failed,
    })
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

pub async fn delete_backup<R: Runtime>(
    app: &AppHandle<R>,
    backup_id: &str,
) -> Result<SimpleResult, String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    let info = db::get_backup(&pool, backup_id)
        .await
        .map_err(|e| format!("could not look up backup: {e}"))?
        .ok_or_else(|| "Backup not found".to_string())?;

    let zip_path = crate::settings::resolve_backup_dir(app, &pool)
        .await?
        .join(&info.filename);
    let _ = std::fs::remove_file(&zip_path); // a missing file is fine
    db::delete_backup_row(&pool, backup_id)
        .await
        .map_err(|e| format!("could not delete backup record: {e}"))?;

    let mut message = format!("Deleted backup “{}”", info.filename);
    if matches!(info.location, BackupLocation::Both | BackupLocation::Drive) {
        // Best effort: the local record is already gone either way.
        if let Err(e) = crate::drive::delete_remote_by_name(&pool, &info.filename).await {
            message.push_str(&format!(" (its Google Drive copy may remain: {e})"));
        }
    }
    Ok(SimpleResult {
        success: true,
        message,
    })
}

// ---------------------------------------------------------------------------
// Unit tests (pure pieces only — no AppHandle, no database)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmp(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "skpm-test-{prefix}-{}-{n}",
            std::process::id()
        ))
    }

    fn source(
        id: &str,
        kind: PluginKind,
        install_path: PathBuf,
        target_apps: Vec<AppId>,
    ) -> StageSource {
        StageSource {
            id: id.to_string(),
            name: format!("name-of-{id}"),
            version: "1.2.3".to_string(),
            kind,
            target_apps,
            install_path,
        }
    }

    #[test]
    fn backup_filename_format() {
        assert_eq!(
            backup_filename("20261019-183000"),
            "skpm-backup-20261019-183000.zip"
        );
    }

    #[test]
    fn stage_skips_missing_sources() {
        let base = unique_tmp("missing");
        let sources = vec![source(
            "gone",
            PluginKind::Extension,
            base.join("does-not-exist"),
            vec![AppId::Ae],
        )];
        let (items, skipped) = stage_items(&sources, &base.join("staging")).unwrap();
        assert!(items.is_empty());
        assert_eq!(skipped, 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parse_metadata_rejects_foreign_backups() {
        let base = unique_tmp("foreign");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("metadata.json"),
            r#"{"app":"other-app","app_version":"9.9","created_at":"x","items":[]}"#,
        )
        .unwrap();
        let err = parse_metadata(&base).unwrap_err();
        assert!(
            err.contains("not an SK Plugin Manager backup"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parse_metadata_rejects_missing_file() {
        let base = unique_tmp("nometa");
        std::fs::create_dir_all(&base).unwrap();
        let err = parse_metadata(&base).unwrap_err();
        assert!(
            err.contains("not an SK Plugin Manager backup"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn stage_zip_unzip_roundtrip() {
        let base = unique_tmp("roundtrip");

        // Fake extension dir with nested content.
        let ext_dir = base.join("FakeExt");
        std::fs::create_dir_all(ext_dir.join("CSXS")).unwrap();
        std::fs::create_dir_all(ext_dir.join("assets")).unwrap();
        std::fs::write(ext_dir.join("CSXS").join("manifest.xml"), "<xml>fake</xml>").unwrap();
        std::fs::write(ext_dir.join("assets").join("icon.png"), b"fake-png-bytes").unwrap();
        // Fake script file.
        let script = base.join("my script.jsx");
        std::fs::write(&script, "// fake script contents").unwrap();

        let sources = vec![
            source(
                "com.example.fakeext",
                PluginKind::Extension,
                ext_dir.clone(),
                vec![AppId::Ae, AppId::Pp],
            ),
            source(
                "myscript__ae",
                PluginKind::Script,
                script.clone(),
                vec![AppId::Ae],
            ),
        ];

        // Stage.
        let staging = base.join("staging");
        let (items, skipped) = stage_items(&sources, &staging).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "extension");
        assert_eq!(items[0].target_apps, vec!["ae", "pp"]);
        assert_eq!(items[1].kind, "script");

        // Metadata.
        let meta = BackupMetadata {
            app: "sk-plugin-manager".to_string(),
            app_version: "1.0.0".to_string(),
            created_at: "2026-10-19 18:30:00".to_string(),
            items,
        };
        write_metadata(&staging, &meta).unwrap();

        // Zip.
        let zip_path = base.join("backup.zip");
        let size = zip_dir(&staging, &zip_path).unwrap();
        assert!(size > 0);

        // Unzip + parse.
        let extract = base.join("extract");
        std::fs::create_dir_all(&extract).unwrap();
        unzip_to(&zip_path, &extract).unwrap();
        let parsed = parse_metadata(&extract).unwrap();
        assert_eq!(parsed.app, "sk-plugin-manager");
        assert_eq!(parsed.app_version, "1.0.0");
        assert_eq!(parsed.items.len(), 2);

        // File bytes survived the round trip.
        let orig_manifest = std::fs::read(ext_dir.join("CSXS").join("manifest.xml")).unwrap();
        let back_manifest = std::fs::read(
            extract
                .join("items")
                .join("com.example.fakeext")
                .join("CSXS")
                .join("manifest.xml"),
        )
        .unwrap();
        assert_eq!(orig_manifest, back_manifest);
        let orig_script = std::fs::read(&script).unwrap();
        let back_script = std::fs::read(
            extract
                .join("items")
                .join("myscript__ae")
                .join("my script.jsx"),
        )
        .unwrap();
        assert_eq!(orig_script, back_script);

        // Metadata schema: every required field present.
        let v = serde_json::to_value(&parsed).unwrap();
        for key in ["app", "app_version", "created_at", "items"] {
            assert!(v.get(key).is_some(), "missing metadata field {key}");
        }
        for key in [
            "id",
            "name",
            "version",
            "kind",
            "target_apps",
            "original_install_path",
        ] {
            assert!(
                v["items"][0].get(key).is_some(),
                "missing item field {key}"
            );
            assert!(
                v["items"][1].get(key).is_some(),
                "missing item field {key}"
            );
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_one_roundtrip() {
        let base = unique_tmp("restoreone");

        // Staged extension.
        let staged_ext = base.join("staged").join("com.example.x");
        std::fs::create_dir_all(&staged_ext).unwrap();
        std::fs::write(staged_ext.join("file.txt"), b"ext-data").unwrap();
        let dest_ext = base.join("dest").join("FakeExt");
        restore_one(&staged_ext, &dest_ext, "extension").unwrap();
        assert_eq!(
            std::fs::read(dest_ext.join("file.txt")).unwrap(),
            b"ext-data"
        );

        // Restoring over an existing dir replaces it (update path).
        std::fs::write(staged_ext.join("file.txt"), b"ext-data-v2").unwrap();
        std::fs::write(dest_ext.join("stale.txt"), b"stale").unwrap();
        restore_one(&staged_ext, &dest_ext, "extension").unwrap();
        assert_eq!(
            std::fs::read(dest_ext.join("file.txt")).unwrap(),
            b"ext-data-v2"
        );
        assert!(!dest_ext.join("stale.txt").exists());

        // Staged script.
        let staged_script = base.join("staged").join("tool__ae");
        std::fs::create_dir_all(&staged_script).unwrap();
        std::fs::write(staged_script.join("tool.jsx"), b"// v1").unwrap();
        let dest_script = base.join("dest2").join("tool.jsx");
        restore_one(&staged_script, &dest_script, "script").unwrap();
        assert_eq!(std::fs::read(&dest_script).unwrap(), b"// v1");

        // Unknown kind fails.
        assert!(restore_one(&staged_script, &dest_script, "theme").is_err());

        let _ = std::fs::remove_dir_all(&base);
    }
}
