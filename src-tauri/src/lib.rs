//! SK Plugin Manager backend.
//!
//! Phase 1 IPC contract (CONTRACT.md): `detect_adobe_apps`, `list_plugins`,
//! `scan_existing`, `import_existing`, `install_dropped`, `uninstall`,
//! `compare_version`, `history_list`.
//!
//! Phase 2 additions: `backup_now`, `list_backups`, `restore_backup`,
//! `delete_backup`, `get_settings`, `set_setting`, `drive_status`,
//! `drive_connect`, `drive_disconnect`, `upload_backup_to_drive`,
//! `set_autostart` — plus a system tray and a 60s scheduler ticker.

mod adobe;
mod autostart_cfg;
mod backup;
mod commands;
mod db;
mod drive;
mod install;
mod manifest;
mod scheduler;
mod settings;
mod types;

use tauri::{Manager, Runtime};

/// Build the system tray: Open / Back up now / Quit.
///
/// Best effort — a tray failure must not stop the app from starting.
fn build_tray<R: Runtime>(app: &tauri::App<R>) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    let open_item =
        MenuItem::with_id(app, "tray-open", "Open SK Plugin Manager", true, None::<&str>)
            .map_err(|e| e.to_string())?;
    let backup_item =
        MenuItem::with_id(app, "tray-backup", "Back up now", true, None::<&str>)
            .map_err(|e| e.to_string())?;
    let quit_item = MenuItem::with_id(app, "tray-quit", "Quit", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(
        app,
        &[
            &open_item,
            &backup_item,
            &PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?,
            &quit_item,
        ],
    )
    .map_err(|e| e.to_string())?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .map_err(|e| format!("could not load tray icon: {e}"))?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("SK Plugin Manager")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "tray-backup" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::backup::run_backup(&handle, false).await {
                        eprintln!("SK Plugin Manager: tray backup failed: {e}");
                    }
                });
            }
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Library database at <app_data_dir>/sk-plugin-manager/library.db;
            // tables are created on startup. A failure here is non-fatal: the
            // commands report it instead of panicking.
            if let Err(e) =
                tauri::async_runtime::block_on(db::init_db(app.handle()))
            {
                eprintln!("SK Plugin Manager: database init failed: {e}");
            }

            // Request notification permission at startup (best effort; desktop
            // platforms resolve this to Granted without prompting).
            {
                use tauri_plugin_notification::NotificationExt;
                let _ = app.handle().notification().request_permission();
            }

            // System tray (Open / Back up now / Quit).
            if let Err(e) = build_tray(app) {
                eprintln!("SK Plugin Manager: tray init failed: {e}");
            }

            // Scheduled auto-backup ticker (every 60s).
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    crate::scheduler::run_scheduler(handle).await;
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Keep the app alive in the tray: closing the window hides it
            // instead of quitting (Quit is available from the tray menu).
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::detect_adobe_apps,
            commands::list_plugins,
            commands::scan_existing,
            commands::import_existing,
            commands::install_dropped,
            commands::uninstall,
            commands::compare_version,
            commands::history_list,
            commands::backup_now,
            commands::list_backups,
            commands::restore_backup,
            commands::delete_backup,
            commands::get_settings,
            commands::set_setting,
            commands::drive_status,
            commands::drive_connect,
            commands::drive_disconnect,
            commands::upload_backup_to_drive,
            commands::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
