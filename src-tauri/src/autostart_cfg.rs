//! Launch-at-login placeholder.
//!
//! The `tauri-plugin-autostart` crate only exists as a `2.0.0-alpha.0`
//! written against the tauri 2.0.0-alpha.9 plugin API (it calls
//! `AppHandle::env()`, which no longer exists in tauri 2.12), so wiring it
//! would break the build. Per the Phase 2 plan the dependency is dropped and
//! `set_autostart` reports "coming soon" instead. The user's preference is
//! still persisted under the `launch_at_login` setting so it applies
//! automatically once a stable autostart plugin ships.

use tauri::{AppHandle, Runtime};

use crate::{db, settings};
use crate::types::SimpleResult;

/// Persist the preference; the OS-level toggle is not implemented yet.
pub async fn apply<R: Runtime>(
    app: &AppHandle<R>,
    enabled: bool,
) -> Result<SimpleResult, String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    let _ = db::set_setting_db(
        &pool,
        settings::KEY_LAUNCH_AT_LOGIN,
        if enabled { "true" } else { "false" },
    )
    .await;
    Ok(SimpleResult {
        success: false,
        message: "Launch at login is coming soon".to_string(),
    })
}
