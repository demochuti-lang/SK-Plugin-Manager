//! Shared types mirroring the Phase 1 IPC contract (CONTRACT.md).
//! These serialize exactly as the TypeScript types on the frontend.

use serde::{Deserialize, Serialize};

/// 'ae' | 'pp' | 'ps' | 'ai'
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppId {
    Ae,
    Pp,
    Ps,
    Ai,
}

impl AppId {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppId::Ae => "ae",
            AppId::Pp => "pp",
            AppId::Ps => "ps",
            AppId::Ai => "ai",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AppId::Ae => "After Effects",
            AppId::Pp => "Premiere Pro",
            AppId::Ps => "Photoshop",
            AppId::Ai => "Illustrator",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Extension,
    Script,
}

impl PluginKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginKind::Extension => "extension",
            PluginKind::Script => "script",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "extension" => Some(PluginKind::Extension),
            "script" => Some(PluginKind::Script),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginSource {
    Installed,
    Detected,
}

impl PluginSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginSource::Installed => "installed",
            PluginSource::Detected => "detected",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "installed" => Some(PluginSource::Installed),
            "detected" => Some(PluginSource::Detected),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdobeApp {
    pub id: AppId,
    pub name: String,
    pub version: String,
    pub install_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    pub target_apps: Vec<AppId>,
    pub install_path: String,
    pub source: PluginSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub path: String,
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_update: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub has_update: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_apps: Option<Vec<AppId>>,
}

/// 'install' | 'uninstall' | 'update' | 'import' | 'scan' | 'backup' | 'restore'
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryEvent {
    Install,
    Uninstall,
    Update,
    Import,
    Scan,
    Backup,
    Restore,
}

impl HistoryEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            HistoryEvent::Install => "install",
            HistoryEvent::Uninstall => "uninstall",
            HistoryEvent::Update => "update",
            HistoryEvent::Import => "import",
            HistoryEvent::Scan => "scan",
            HistoryEvent::Backup => "backup",
            HistoryEvent::Restore => "restore",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub event: HistoryEvent,
    pub plugin_name: String,
    pub detail: String,
    pub created_at: String,
}

/// Return of `import_existing`: `{ imported: number }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: i64,
}

/// Return of `uninstall`: `{ success: boolean, message: string }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallResult {
    pub success: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Phase 2: backup system
// ---------------------------------------------------------------------------

/// 'local' | 'drive' | 'both' — where a backup's zip currently lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupLocation {
    Local,
    Drive,
    Both,
}

impl BackupLocation {
    pub fn as_str(&self) -> &'static str {
        match self {
            BackupLocation::Local => "local",
            BackupLocation::Drive => "drive",
            BackupLocation::Both => "both",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "local" => Some(BackupLocation::Local),
            "drive" => Some(BackupLocation::Drive),
            "both" => Some(BackupLocation::Both),
            _ => None,
        }
    }
}

/// One row of the `backups` table, newest first from `list_backups`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub filename: String,
    pub created_at: String,
    pub size_bytes: i64,
    pub item_count: i64,
    pub location: BackupLocation,
}

/// Return of `backup_now`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
}

/// Return of `restore_backup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub success: bool,
    pub message: String,
    pub restored: i64,
    pub failed: i64,
}

/// Generic `{ success: boolean, message: string }` return
/// (used by `delete_backup`, `drive_connect`, `drive_disconnect`,
/// `upload_backup_to_drive`, `set_autostart`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleResult {
    pub success: bool,
    pub message: String,
}

/// Return of `drive_status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveStatus {
    pub configured: bool,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}
