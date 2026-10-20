//! SQLite library database, backed by tauri-plugin-sql.
//!
//! The database lives at `<app_data_dir>/sk-plugin-manager/library.db` and
//! the tables are created on startup. Rust commands reach the connection pool
//! through the sql plugin's documented [`tauri_plugin_sql::DbInstances`]
//! state ("managed as Tauri state, so Rust code can reach the pools with
//! `Manager::state` and run its own queries").
//!
//! Schema (per CONTRACT.md):
//! ```sql
//! plugins(id TEXT PK, name, version, kind, target_apps JSON, install_path,
//!         source, installed_at)
//! history(id INTEGER PK AUTOINCREMENT, event, plugin_name, detail,
//!         created_at DEFAULT datetime)
//! ```

use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{Pool, Row, Sqlite};
use tauri::{AppHandle, Manager, Runtime};

use crate::types::{
    AppId, BackupInfo, BackupLocation, HistoryEntry, HistoryEvent, PluginEntry, PluginKind,
    PluginSource,
};

/// Key for our pool inside the sql plugin's `DbInstances` map.
/// Deliberately not a loadable `sqlite:` URL so a frontend `load` call can
/// never collide with (or hijack) the backend pool.
const DB_KEY: &str = "skpm:library";

const CREATE_PLUGINS: &str = "
CREATE TABLE IF NOT EXISTS plugins (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT 'unknown',
    kind TEXT NOT NULL,
    target_apps TEXT NOT NULL DEFAULT '[]',
    install_path TEXT NOT NULL,
    source TEXT NOT NULL,
    installed_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
)";

const CREATE_HISTORY: &str = "
CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event TEXT NOT NULL,
    plugin_name TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
)";

const CREATE_SETTINGS: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
)";

const CREATE_BACKUPS: &str = "
CREATE TABLE IF NOT EXISTS backups (
    id TEXT PRIMARY KEY,
    filename TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    size_bytes INTEGER NOT NULL,
    item_count INTEGER NOT NULL,
    location TEXT NOT NULL DEFAULT 'local'
)";

/// Open (creating) the library database, create tables, and register the pool
/// in the sql plugin's shared `DbInstances` state.
pub async fn init_db<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve app data dir: {e}"))?;
    let dir = data_dir.join("sk-plugin-manager");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create db dir {}: {e}", dir.display()))?;
    let db_path = dir.join("library.db");

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(10));

    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .map_err(|e| format!("could not open library database {}: {e}", db_path.display()))?;

    sqlx::query(CREATE_PLUGINS)
        .execute(&pool)
        .await
        .map_err(|e| format!("could not create plugins table: {e}"))?;
    sqlx::query(CREATE_HISTORY)
        .execute(&pool)
        .await
        .map_err(|e| format!("could not create history table: {e}"))?;
    sqlx::query(CREATE_SETTINGS)
        .execute(&pool)
        .await
        .map_err(|e| format!("could not create settings table: {e}"))?;
    sqlx::query(CREATE_BACKUPS)
        .execute(&pool)
        .await
        .map_err(|e| format!("could not create backups table: {e}"))?;

    let instances = app
        .try_state::<tauri_plugin_sql::DbInstances>()
        .ok_or_else(|| "sql plugin state not registered".to_string())?;
    instances
        .0
        .write()
        .await
        .insert(DB_KEY.to_string(), tauri_plugin_sql::DbPool::Sqlite(pool));

    Ok(())
}

/// Fetch our pool from the sql plugin's state.
pub async fn pool<R: Runtime>(app: &AppHandle<R>) -> Result<Pool<Sqlite>, String> {
    let instances = app
        .try_state::<tauri_plugin_sql::DbInstances>()
        .ok_or_else(|| "library database is not initialized".to_string())?;
    let guard = instances.0.read().await;
    match guard.get(DB_KEY) {
        Some(tauri_plugin_sql::DbPool::Sqlite(p)) => Ok(p.clone()),
        _ => Err("library database is not initialized".to_string()),
    }
}

fn target_apps_to_json(apps: &[AppId]) -> String {
    let strs: Vec<&str> = apps.iter().map(AppId::as_str).collect();
    serde_json::to_string(&strs).unwrap_or_else(|_| "[]".to_string())
}

fn row_to_entry(row: &SqliteRow) -> Result<PluginEntry, sqlx::Error> {
    let target_apps_json: String = row.try_get("target_apps")?;
    let target_apps: Vec<AppId> = serde_json::from_str::<Vec<String>>(&target_apps_json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| match s.as_str() {
            "ae" => Some(AppId::Ae),
            "pp" => Some(AppId::Pp),
            "ps" => Some(AppId::Ps),
            "ai" => Some(AppId::Ai),
            _ => None,
        })
        .collect();
    let kind_str: String = row.try_get("kind")?;
    let source_str: String = row.try_get("source")?;
    Ok(PluginEntry {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        version: row.try_get("version")?,
        kind: PluginKind::from_str(&kind_str).unwrap_or(PluginKind::Extension),
        target_apps,
        install_path: row.try_get("install_path")?,
        source: PluginSource::from_str(&source_str).unwrap_or(PluginSource::Detected),
    })
}

fn row_to_history(row: &SqliteRow) -> Result<HistoryEntry, sqlx::Error> {
    let event_str: String = row.try_get("event")?;
    let event = match event_str.as_str() {
        "install" => HistoryEvent::Install,
        "uninstall" => HistoryEvent::Uninstall,
        "update" => HistoryEvent::Update,
        "import" => HistoryEvent::Import,
        "backup" => HistoryEvent::Backup,
        "restore" => HistoryEvent::Restore,
        _ => HistoryEvent::Scan,
    };
    Ok(HistoryEntry {
        id: row.try_get("id")?,
        event,
        plugin_name: row.try_get("plugin_name")?,
        detail: row.try_get("detail")?,
        created_at: row.try_get("created_at")?,
    })
}

pub async fn plugin_exists(pool: &Pool<Sqlite>, id: &str) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 AS one FROM plugins WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Insert or replace a library row (install/update path).
#[allow(clippy::too_many_arguments)]
pub async fn upsert_plugin(
    pool: &Pool<Sqlite>,
    id: &str,
    name: &str,
    version: &str,
    kind: PluginKind,
    target_apps: &[AppId],
    install_path: &str,
    source: PluginSource,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO plugins (id, name, version, kind, target_apps, install_path, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           version = excluded.version,
           kind = excluded.kind,
           target_apps = excluded.target_apps,
           install_path = excluded.install_path,
           source = excluded.source,
           installed_at = datetime('now','localtime')",
    )
    .bind(id)
    .bind(name)
    .bind(version)
    .bind(kind.as_str())
    .bind(target_apps_to_json(target_apps))
    .bind(install_path)
    .bind(source.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert a row, skipping it when the id already exists.
/// Returns `true` when the row was actually inserted.
pub async fn insert_plugin_ignore(
    pool: &Pool<Sqlite>,
    entry: &PluginEntry,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "INSERT OR IGNORE INTO plugins
         (id, name, version, kind, target_apps, install_path, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(&entry.id)
    .bind(&entry.name)
    .bind(&entry.version)
    .bind(entry.kind.as_str())
    .bind(target_apps_to_json(&entry.target_apps))
    .bind(&entry.install_path)
    .bind(entry.source.as_str())
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

pub async fn get_plugin(
    pool: &Pool<Sqlite>,
    id: &str,
) -> Result<Option<PluginEntry>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, version, kind, target_apps, install_path, source
         FROM plugins WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| row_to_entry(&r)).transpose()
}

pub async fn delete_plugin(pool: &Pool<Sqlite>, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM plugins WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_all_plugins(pool: &Pool<Sqlite>) -> Result<Vec<PluginEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, version, kind, target_apps, install_path, source
         FROM plugins ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_to_entry).collect()
}

pub async fn log_history(
    pool: &Pool<Sqlite>,
    event: HistoryEvent,
    plugin_name: &str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO history (event, plugin_name, detail) VALUES (?1, ?2, ?3)")
        .bind(event.as_str())
        .bind(plugin_name)
        .bind(detail)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_history(
    pool: &Pool<Sqlite>,
    limit: i64,
) -> Result<Vec<HistoryEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, event, plugin_name, detail, created_at
         FROM history ORDER BY id DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_to_history).collect()
}

// ---------------------------------------------------------------------------
// Phase 2: settings key/value store
// ---------------------------------------------------------------------------

/// Read one setting; `Ok(None)` when the key was never stored.
pub async fn get_setting(
    pool: &Pool<Sqlite>,
    key: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    row.map(|r| r.try_get("value")).transpose()
}

/// Insert or replace one setting.
pub async fn set_setting_db(
    pool: &Pool<Sqlite>,
    key: &str,
    value: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// All stored settings (keys the user never touched are simply absent;
/// callers fill in defaults).
pub async fn all_settings(
    pool: &Pool<Sqlite>,
) -> Result<std::collections::HashMap<String, String>, sqlx::Error> {
    let rows = sqlx::query("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await?;
    let mut map = std::collections::HashMap::with_capacity(rows.len());
    for row in &rows {
        let key: String = row.try_get("key")?;
        let value: String = row.try_get("value")?;
        map.insert(key, value);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Phase 2: backups table
// ---------------------------------------------------------------------------

fn row_to_backup(row: &SqliteRow) -> Result<BackupInfo, sqlx::Error> {
    let location_str: String = row.try_get("location")?;
    Ok(BackupInfo {
        id: row.try_get("id")?,
        filename: row.try_get("filename")?,
        created_at: row.try_get("created_at")?,
        size_bytes: row.try_get("size_bytes")?,
        item_count: row.try_get("item_count")?,
        location: BackupLocation::from_str(&location_str).unwrap_or(BackupLocation::Local),
    })
}

pub async fn insert_backup(
    pool: &Pool<Sqlite>,
    id: &str,
    filename: &str,
    size_bytes: i64,
    item_count: i64,
    location: BackupLocation,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO backups (id, filename, size_bytes, item_count, location)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id)
    .bind(filename)
    .bind(size_bytes)
    .bind(item_count)
    .bind(location.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

/// All backups, newest first.
pub async fn list_backups(pool: &Pool<Sqlite>) -> Result<Vec<BackupInfo>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, filename, created_at, size_bytes, item_count, location
         FROM backups ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_to_backup).collect()
}

pub async fn get_backup(
    pool: &Pool<Sqlite>,
    id: &str,
) -> Result<Option<BackupInfo>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, filename, created_at, size_bytes, item_count, location
         FROM backups WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| row_to_backup(&r)).transpose()
}

pub async fn set_backup_location(
    pool: &Pool<Sqlite>,
    id: &str,
    location: BackupLocation,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE backups SET location = ?1 WHERE id = ?2")
        .bind(location.as_str())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_backup_row(pool: &Pool<Sqlite>, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM backups WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
