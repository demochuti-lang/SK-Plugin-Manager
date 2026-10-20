//! Phase 2 scheduler: run a backup automatically every X hours/days.
//!
//! The interval math is pure and unit-tested below. The background ticker
//! lives in `run_scheduler`, spawned once from `lib.rs`; every 60 seconds it
//! re-reads settings and calls the same [`crate::backup::run_backup`] the
//! `backup_now` command uses.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Runtime};

use crate::{db, settings};

// ---------------------------------------------------------------------------
// Pure, unit-testable interval math
// ---------------------------------------------------------------------------

/// Convert a (value, unit) pair to seconds.
/// "hours" → ×3600, "days" → ×86400, anything else → 0 (never due).
pub fn interval_to_seconds(value: u64, unit: &str) -> u64 {
    match unit {
        "hours" => value.saturating_mul(3_600),
        "days" => value.saturating_mul(86_400),
        _ => 0,
    }
}

/// Is a backup due right now?
/// - `false` when the interval is 0/unknown (a zero value means "off");
///   the caller checks `auto_backup_enabled` separately.
/// - `true` when no backup has ever run (`last_backup_at == 0`).
/// - otherwise `true` once `now - last_backup_at >= interval`.
pub fn is_backup_due(last_backup_at: u64, now: u64, value: u64, unit: &str) -> bool {
    let interval = interval_to_seconds(value, unit);
    if interval == 0 {
        return false;
    }
    if last_backup_at == 0 {
        return true;
    }
    now.saturating_sub(last_backup_at) >= interval
}

// ---------------------------------------------------------------------------
// Background ticker
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One tick: read settings; run a backup when enabled and due.
/// Scheduled backups never upload to Drive (that stays a manual choice).
async fn tick<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    let all = db::all_settings(&pool)
        .await
        .map_err(|e| format!("could not read settings: {e}"))?;

    if all
        .get(settings::KEY_AUTO_BACKUP_ENABLED)
        .is_none_or(|v| v != "true")
    {
        return Ok(());
    }
    let value: u64 = all
        .get(settings::KEY_INTERVAL_VALUE)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let unit = all
        .get(settings::KEY_INTERVAL_UNIT)
        .map(|s| s.as_str())
        .unwrap_or("hours");
    let last: u64 = all
        .get(settings::KEY_LAST_BACKUP_AT)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if is_backup_due(last, now_secs(), value, unit) {
        // `run_backup` already notifies / logs / emits; a failure here just
        // means the next tick tries again in 60s.
        if let Err(e) = crate::backup::run_backup(app, false).await {
            eprintln!("SK Plugin Manager: scheduled backup failed: {e}");
        }
    }
    Ok(())
}

/// Loop forever: check every 60 seconds whether a backup is due.
/// Spawned once from `lib.rs` setup.
pub async fn run_scheduler<R: Runtime>(app: AppHandle<R>) {
    loop {
        if let Err(e) = tick(&app).await {
            eprintln!("SK Plugin Manager: scheduler tick failed: {e}");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_to_seconds_conversions() {
        assert_eq!(interval_to_seconds(6, "hours"), 21_600);
        assert_eq!(interval_to_seconds(1, "days"), 86_400);
        assert_eq!(interval_to_seconds(2, "days"), 172_800);
        assert_eq!(interval_to_seconds(0, "hours"), 0);
        assert_eq!(interval_to_seconds(5, "weeks"), 0);
        assert_eq!(interval_to_seconds(5, ""), 0);
    }

    #[test]
    fn due_when_never_backed_up() {
        assert!(is_backup_due(0, 1_700_000_000, 24, "hours"));
    }

    #[test]
    fn due_at_and_after_boundary() {
        // 6h interval: last at t, now at t+6h → due; t+6h-1 → not due.
        let last = 1_700_000_000u64;
        assert!(is_backup_due(last, last + 21_600, 6, "hours"));
        assert!(is_backup_due(last, last + 100_000, 6, "hours"));
        assert!(!is_backup_due(last, last + 21_599, 6, "hours"));
    }

    #[test]
    fn not_due_when_disabled_or_unknown() {
        // value == 0 → never due (treated as "off").
        assert!(!is_backup_due(0, 1_700_000_000, 0, "hours"));
        // Unknown unit → 0 seconds → never due.
        assert!(!is_backup_due(0, 1_700_000_000, 6, "fortnights"));
    }

    #[test]
    fn clock_skew_backwards_is_not_due() {
        // now < last (clock moved back): saturating_sub → 0 < interval.
        assert!(!is_backup_due(1_700_000_000, 1_699_000_000, 1, "hours"));
    }
}
