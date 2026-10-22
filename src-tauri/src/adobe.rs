//! Adobe app detection and OS-specific install/scan path rules.
//!
//! Detection per CONTRACT.md:
//! - macOS: scan /Applications, match dir names starting with the product
//!   prefixes (trailing ".app" stripped). Version = trailing 4-digit year.
//! - Windows: scan C:\Program Files\Adobe and C:\Program Files (x86)\Adobe,
//!   plus HKLM\SOFTWARE\Adobe via winreg as a secondary source (deduped).
//! - Other OS: empty vec.

use std::path::{Path, PathBuf};

use crate::types::{AdobeApp, AppId};

/// (prefix, AppId) pairs in match order.
#[cfg(any(target_os = "macos", windows))]
const PREFIXES: &[(&str, AppId)] = &[
    ("Adobe After Effects", AppId::Ae),
    ("Adobe Premiere Pro", AppId::Pp),
    ("Adobe Photoshop", AppId::Ps),
    ("Adobe Illustrator", AppId::Ai),
];

/// Strip a trailing ".app" (case-insensitive) then match a product prefix.
#[cfg(any(target_os = "macos", windows))]
fn match_prefix(dir_name: &str) -> Option<AppId> {
    let mut name = dir_name;
    if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".app") {
        name = &name[..name.len() - 4];
    }
    PREFIXES
        .iter()
        .find(|(prefix, _)| name.starts_with(prefix))
        .map(|(_, id)| *id)
}

/// Version = trailing 4-digit year in the name, else "unknown".
/// Used on macOS/Windows app detection (dead on other targets by cfg).
#[allow(dead_code)]
pub fn parse_year(name: &str) -> String {
    let t = name.trim();
    if t.len() >= 4 {
        let tail = &t[t.len() - 4..];
        let prev_is_digit = t[..t.len() - 4]
            .chars()
            .last()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        if !prev_is_digit && tail.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(year) = tail.parse::<u32>() {
                if (1990..=2100).contains(&year) {
                    return tail.to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

#[cfg(any(target_os = "macos", windows))]
fn app_from_dir(dir_name: &str, full_path: &Path) -> Option<AdobeApp> {
    let id = match_prefix(dir_name)?;
    // Display name keeps the ".app" stripped form.
    let mut display = dir_name.to_string();
    if display.len() > 4 && display[display.len() - 4..].eq_ignore_ascii_case(".app") {
        display.truncate(display.len() - 4);
    }
    Some(AdobeApp {
        id,
        name: display.clone(),
        version: parse_year(&display),
        install_path: full_path.to_string_lossy().to_string(),
    })
}

#[cfg(any(target_os = "macos", windows))]
fn scan_dir(root: &Path, out: &mut Vec<AdobeApp>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(app) = app_from_dir(&name, &entry.path()) {
            out.push(app);
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_platform() -> Vec<AdobeApp> {
    let mut apps = Vec::new();
    scan_dir(Path::new("/Applications"), &mut apps);
    dedupe(apps)
}

#[cfg(windows)]
fn detect_platform() -> Vec<AdobeApp> {
    let mut apps = Vec::new();
    for root in [
        PathBuf::from(r"C:\Program Files\Adobe"),
        PathBuf::from(r"C:\Program Files (x86)\Adobe"),
    ] {
        scan_dir(&root, &mut apps);
    }
    for reg_app in scan_registry() {
        apps.push(reg_app);
    }
    dedupe(apps)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn detect_platform() -> Vec<AdobeApp> {
    Vec::new()
}

/// Remove duplicates by canonical-ish install path (case-insensitive).
#[cfg(any(target_os = "macos", windows))]
fn dedupe(mut apps: Vec<AdobeApp>) -> Vec<AdobeApp> {
    let mut seen = std::collections::HashSet::new();
    apps.retain(|a| seen.insert(a.install_path.to_lowercase()));
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

/// Secondary Windows source: HKLM\SOFTWARE\Adobe (best effort).
///
/// Looks one level deep for subkeys whose names mention a known product and
/// which carry an install-path-like value. Entries already found by the
/// directory scan are removed by [`dedupe`].
#[cfg(windows)]
fn scan_registry() -> Vec<AdobeApp> {
    use winreg::enums::*;
    use winreg::RegKey;

    const PRODUCT_HINTS: &[(&str, AppId)] = &[
        ("after effects", AppId::Ae),
        ("premiere pro", AppId::Pp),
        ("photoshop", AppId::Ps),
        ("illustrator", AppId::Ai),
    ];
    const PATH_VALUES: &[&str] = &[
        "InstallPath",
        "InstallDir",
        "ApplicationPath",
        "Path",
        "InstallLocation",
    ];

    let mut out = Vec::new();
    // Default 64-bit view first, then the 32-bit view as a fallback.
    for flags in [KEY_READ, KEY_READ | KEY_WOW64_32KEY] {
        let adobe = match RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(r"SOFTWARE\Adobe", flags)
        {
            Ok(k) => k,
            Err(_) => continue,
        };
        for subkey_name in adobe.enum_keys().flatten() {
            let lowered = subkey_name.to_lowercase();
            let app_id = match PRODUCT_HINTS
                .iter()
                .find(|(hint, _)| lowered.contains(hint))
            {
                Some((_, id)) => *id,
                None => continue,
            };
            let subkey = match adobe.open_subkey_with_flags(&subkey_name, flags) {
                Ok(k) => k,
                Err(_) => continue,
            };
            // Product keys sometimes nest one version level deeper.
            let mut candidates: Vec<RegKey> = vec![subkey];
            let nested: Option<RegKey> = {
                let first_level = &candidates[0];
                first_level
                    .enum_keys()
                    .flatten()
                    .next()
                    .and_then(|v| first_level.open_subkey_with_flags(v, flags).ok())
            };
            if let Some(key) = nested {
                candidates.push(key);
            }
            for key in &candidates {
                for value_name in PATH_VALUES {
                    if let Ok(path_str) = key.get_value::<String, _>(value_name) {
                        let path = PathBuf::from(path_str.trim());
                        if path.is_dir() {
                            let dir_name = path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| subkey_name.clone());
                            let id = match_prefix(&dir_name).unwrap_or(app_id);
                            out.push(AdobeApp {
                                id,
                                name: dir_name.clone(),
                                version: parse_year(&dir_name),
                                install_path: path.to_string_lossy().to_string(),
                            });
                        }
                        break;
                    }
                }
            }
        }
    }
    out
}

/// Detect installed Adobe apps on this machine.
pub fn detect_adobe_apps() -> Vec<AdobeApp> {
    detect_platform()
}

/// Newest detected app for the given id (year parsed as number, "unknown" = 0).
pub fn newest_app<'a>(apps: &'a [AdobeApp], id: AppId) -> Option<&'a AdobeApp> {
    apps.iter()
        .filter(|a| a.id == id)
        .max_by_key(|a| a.version.parse::<u32>().unwrap_or(0))
}

// ---------------------------------------------------------------------------
// CEP extension directories
// ---------------------------------------------------------------------------

/// All CEP extension dirs to walk during `scan_existing` (existing ones only
/// are walked; callers filter by existence).
pub fn cep_scan_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = home_dir() {
            return vec![home.join("Library/Application Support/Adobe/CEP/extensions")];
        }
        return vec![];
    }
    #[cfg(windows)]
    {
        let mut dirs = vec![PathBuf::from(
            r"C:\Program Files (x86)\Common Files\Adobe\CEP\extensions",
        )];
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(appdata).join(r"Adobe\CEP\extensions"));
        }
        return dirs;
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        // No real Adobe apps exist on Linux; best-effort location for dev.
        if let Some(home) = home_dir() {
            return vec![home.join(".config/Adobe/CEP/extensions")];
        }
        return vec![];
    }
}

/// Pick the CEP dir to install into. On Windows this probes writability of the
/// preferred dir first and falls back to %APPDATA% on failure.
pub fn cep_install_dir() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = home_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
        })?;
        let dir = home.join("Library/Application Support/Adobe/CEP/extensions");
        std::fs::create_dir_all(&dir)?;
        return Ok(dir);
    }
    #[cfg(windows)]
    {
        let primary =
            PathBuf::from(r"C:\Program Files (x86)\Common Files\Adobe\CEP\extensions");
        if is_writable_dir(&primary) {
            return Ok(primary);
        }
        let appdata = std::env::var("APPDATA").map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "APPDATA not set; cannot determine fallback CEP dir",
            )
        })?;
        let fallback = PathBuf::from(appdata).join(r"Adobe\CEP\extensions");
        std::fs::create_dir_all(&fallback)?;
        return Ok(fallback);
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let home = home_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
        })?;
        let dir = home.join(".config/Adobe/CEP/extensions");
        std::fs::create_dir_all(&dir)?;
        return Ok(dir);
    }
}

/// create_dir_all + prove we can write by creating/removing a probe file.
#[cfg(windows)]
fn is_writable_dir(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".skpm-writetest");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Base64-encode (standard alphabet) — just enough for PowerShell
/// `-EncodedCommand`, which expects UTF-16LE text.
#[cfg(windows)]
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0] as u32,
            *chunk.get(1).unwrap_or(&0) as u32,
            *chunk.get(2).unwrap_or(&0) as u32,
        ];
        let n = (b[0] << 16) | (b[1] << 8) | b[2];
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Delete a file or directory with elevation (shows the Windows UAC prompt).
///
/// Needed for paths under `C:\Program Files (x86)\...` — where most existing
/// CEP extensions live — which a non-elevated process cannot modify. The
/// elevated PowerShell child does the removal; success is judged by the path
/// being gone afterwards (also covers the user cancelling the UAC prompt).
#[cfg(windows)]
pub fn remove_path_elevated(path: &Path) -> Result<(), String> {
    // Single-quote the path for PowerShell ('' escapes a quote).
    let ps_path = path.to_string_lossy().replace('\'', "''");
    let inner = format!(
        "Remove-Item -LiteralPath '{ps_path}' -Recurse -Force -ErrorAction Stop"
    );
    let inner_utf16: Vec<u8> = inner
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let encoded = base64_encode(&inner_utf16);
    // The outer (non-elevated) shell only launches the elevated one and waits.
    // The argument list contains base64 only, so quoting stays safe.
    let outer = format!(
        "Start-Process powershell -Verb RunAs -Wait -ArgumentList @('-NoProfile','-EncodedCommand','{encoded}')"
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &outer])
        .status()
        .map_err(|e| format!("could not launch admin prompt: {e}"))?;
    if path.exists() {
        Err(format!(
            "{} is still there — the admin prompt may have been cancelled, or the files are in use by an open Adobe app",
            path.display()
        ))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Script directories
// ---------------------------------------------------------------------------

/// Candidate script dirs for an app, in preference order (may not exist).
fn script_candidates(app: &AdobeApp) -> Vec<PathBuf> {
    let install = Path::new(&app.install_path);
    match app.id {
        // Premiere Pro follows the After Effects layout (the contract only
        // spells out ae/ps/ai; pp targets are still sniffed, so they need a
        // home — ScriptUI Panels is the closest analogue).
        AppId::Ae | AppId::Pp => {
            #[cfg(target_os = "macos")]
            {
                vec![
                    install.join("Contents/Scripts/ScriptUI Panels"),
                    install.join("Scripts/ScriptUI Panels"),
                ]
            }
            #[cfg(windows)]
            {
                vec![install.join(r"Support Files\Scripts\ScriptUI Panels")]
            }
            #[cfg(not(any(target_os = "macos", windows)))]
            {
                vec![install.join("Scripts/ScriptUI Panels")]
            }
        }
        AppId::Ps | AppId::Ai => {
            let mut cands = Vec::new();
            #[cfg(target_os = "macos")]
            {
                // Photoshop/Illustrator ship a top-level Presets next to the .app.
                if let Some(parent) = install.parent() {
                    cands.push(parent.join("Presets/Scripts"));
                }
                cands.push(install.join("Presets/Scripts"));
            }
            #[cfg(not(target_os = "macos"))]
            {
                cands.push(install.join("Presets/Scripts"));
                #[cfg(windows)]
                {
                    // (same path; kept explicit for clarity)
                }
            }
            // Illustrator locale variant: any existing Scripts dir under Presets.
            if app.id == AppId::Ai {
                cands.extend(illustrator_locale_script_dirs(install));
            }
            cands
        }
    }
}

/// Illustrator locale variant: accept any existing `Scripts` dir under a
/// `Presets` tree (e.g. Presets/en_US/Scripts). Searched shallowly (depth ≤ 2).
fn illustrator_locale_script_dirs(install: &Path) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Some(parent) = install.parent() {
            bases.push(parent.join("Presets"));
        }
        bases.push(install.join("Presets"));
    }
    #[cfg(not(target_os = "macos"))]
    {
        bases.push(install.join("Presets"));
    }
    let mut found = Vec::new();
    for base in bases {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                let direct = p.join("Scripts");
                if direct.is_dir() {
                    found.push(direct);
                    continue;
                }
                // One more level (e.g. Presets/<locale>/Scripts handled above;
                // this covers Presets/<locale>/<sub>/Scripts just in case).
                if let Ok(inner) = std::fs::read_dir(&p) {
                    for e in inner.flatten() {
                        let q = e.path().join("Scripts");
                        if q.is_dir() {
                            found.push(q);
                        }
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Existing script dirs for scanning (never creates anything).
pub fn script_scan_dirs(app: &AdobeApp) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = script_candidates(app)
        .into_iter()
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Script dir to install into: first existing candidate, else create the
/// first candidate.
pub fn script_install_dir(app: &AdobeApp) -> std::io::Result<PathBuf> {
    let cands = script_candidates(app);
    for c in &cands {
        if c.is_dir() {
            return Ok(c.clone());
        }
    }
    let first = cands.into_iter().next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no script directory candidates for {}", app.name),
        )
    })?;
    std::fs::create_dir_all(&first)?;
    Ok(first)
}
