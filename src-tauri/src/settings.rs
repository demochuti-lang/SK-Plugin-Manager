//! Phase 2 settings: defaults, validation, and the backup folder location.
//!
//! Raw key/value storage lives in `db.rs` (`settings` table); this module
//! owns the *known* keys, their defaults, and per-key validation so the
//! `get_settings` / `set_setting` commands stay thin.

use std::collections::HashMap;
use std::path::PathBuf;

use sqlx::{Pool, Sqlite};
use tauri::{AppHandle, Manager, Runtime};

use crate::db;

// ---------------------------------------------------------------------------
// Known keys
// ---------------------------------------------------------------------------

pub const KEY_BACKUP_FOLDER: &str = "backup_folder";
pub const KEY_AUTO_BACKUP_ENABLED: &str = "auto_backup_enabled";
pub const KEY_INTERVAL_VALUE: &str = "backup_interval_value";
pub const KEY_INTERVAL_UNIT: &str = "backup_interval_unit";
pub const KEY_DRIVE_CLIENT_ID: &str = "drive_client_id";
pub const KEY_DRIVE_CLIENT_SECRET: &str = "drive_client_secret";
pub const KEY_DRIVE_REFRESH_TOKEN: &str = "drive_refresh_token";
pub const KEY_DRIVE_EMAIL: &str = "drive_email";
pub const KEY_LAST_BACKUP_AT: &str = "last_backup_at";
pub const KEY_NOTIFICATIONS_ENABLED: &str = "notifications_enabled";
pub const KEY_LAUNCH_AT_LOGIN: &str = "launch_at_login";

/// Keys the frontend is allowed to read/write through `set_setting`.
/// (`drive_refresh_token` is deliberately excluded: it is managed by the
/// OAuth connect/disconnect flow only.)
const WRITABLE_KEYS: &[&str] = &[
    KEY_BACKUP_FOLDER,
    KEY_AUTO_BACKUP_ENABLED,
    KEY_INTERVAL_VALUE,
    KEY_INTERVAL_UNIT,
    KEY_DRIVE_CLIENT_ID,
    KEY_DRIVE_CLIENT_SECRET,
    KEY_DRIVE_EMAIL,
    KEY_LAST_BACKUP_AT,
    KEY_NOTIFICATIONS_ENABLED,
    KEY_LAUNCH_AT_LOGIN,
];

/// Default `<home>/SKPluginManager/backups`.
pub fn default_backup_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let home = app
        .path()
        .home_dir()
        .map_err(|e| format!("could not resolve home directory: {e}"))?;
    Ok(home.join("SKPluginManager").join("backups"))
}

/// Resolve the effective backup folder, creating it on demand.
/// A stored `backup_folder` wins; otherwise the default above.
pub async fn resolve_backup_dir<R: Runtime>(
    app: &AppHandle<R>,
    pool: &Pool<Sqlite>,
) -> Result<PathBuf, String> {
    let dir = match db::get_setting(pool, KEY_BACKUP_FOLDER)
        .await
        .map_err(|e| format!("could not read settings: {e}"))?
    {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => default_backup_dir(app)?,
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create backup folder {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Every known setting with defaults filled in for keys never stored.
/// (`drive_refresh_token` is never exposed to the frontend.)
pub async fn effective_settings<R: Runtime>(
    app: &AppHandle<R>,
    pool: &Pool<Sqlite>,
) -> Result<HashMap<String, String>, String> {
    let mut out = db::all_settings(pool)
        .await
        .map_err(|e| format!("could not read settings: {e}"))?;
    out.remove(KEY_DRIVE_REFRESH_TOKEN);

    let backup_dir_default = default_backup_dir(app)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let defaults: &[(&str, &str)] = &[
        (KEY_BACKUP_FOLDER, &backup_dir_default),
        (KEY_AUTO_BACKUP_ENABLED, "false"),
        (KEY_INTERVAL_VALUE, "24"),
        (KEY_INTERVAL_UNIT, "hours"),
        (KEY_DRIVE_CLIENT_ID, ""),
        (KEY_DRIVE_CLIENT_SECRET, ""),
        (KEY_DRIVE_EMAIL, ""),
        (KEY_LAST_BACKUP_AT, "0"),
        (KEY_NOTIFICATIONS_ENABLED, "true"),
        (KEY_LAUNCH_AT_LOGIN, "false"),
    ];
    for (key, default) in defaults {
        out.entry(key.to_string()).or_insert_with(|| default.to_string());
    }
    Ok(out)
}

/// Validate and store one setting. Unknown keys and invalid values are
/// rejected with `Err(String)`; the DB is untouched on failure.
pub async fn set_setting_validated(
    pool: &Pool<Sqlite>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    if !WRITABLE_KEYS.contains(&key) {
        return Err(format!("unknown setting: {key}"));
    }
    match key {
        KEY_BACKUP_FOLDER => {
            let path = PathBuf::from(value.trim());
            if path.as_os_str().is_empty() {
                return Err("backup folder must not be empty".to_string());
            }
            std::fs::create_dir_all(&path).map_err(|e| {
                format!("could not use {} as backup folder: {e}", path.display())
            })?;
        }
        KEY_AUTO_BACKUP_ENABLED | KEY_NOTIFICATIONS_ENABLED | KEY_LAUNCH_AT_LOGIN => {
            if value != "true" && value != "false" {
                return Err(format!("{key} must be \"true\" or \"false\""));
            }
        }
        KEY_INTERVAL_VALUE => {
            value
                .parse::<u64>()
                .map_err(|_| format!("{key} must be a positive number"))?;
        }
        KEY_INTERVAL_UNIT => {
            if value != "hours" && value != "days" {
                return Err(format!("{key} must be \"hours\" or \"days\""));
            }
        }
        _ => {}
    }
    // Canonicalize the folder path we just validated.
    let stored = if key == KEY_BACKUP_FOLDER {
        PathBuf::from(value.trim()).to_string_lossy().to_string()
    } else {
        value.to_string()
    };
    db::set_setting_db(pool, key, &stored)
        .await
        .map_err(|e| format!("could not save setting: {e}"))
}
