//! Install engine for dropped paths (blocking filesystem work).
//!
//! - `.zxp` → unzip to temp, find `CSXS/manifest.xml`, copy tree into the
//!   OS-correct CEP extensions dir as `<cep>/<sanitized zxp stem>`.
//! - folder containing `CSXS/manifest.xml` → copy as extension (same rules).
//! - `.jsx`/`.jsxbin` → sniff target app (8 KiB head), copy into the newest
//!   detected version's script dir.
//!
//! Remove-then-copy on overwrite. All errors are returned as `Err(String)`;
//! the command layer turns them into failed `InstallResult`s (never panics).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use zip::ZipArchive;

use crate::adobe::{
    cep_install_dir, detect_adobe_apps, newest_app, script_install_dir,
};
use crate::manifest::{
    SNIFF_BYTES, parse_manifest_xml, parse_script_version, prettify_name,
    sanitize_folder_name, sniff_script_target,
};
use crate::types::{AdobeApp, AppId, PluginKind};

/// Filesystem outcome of installing one dropped path (DB write happens later,
/// in the async command layer).
pub struct FsInstalled {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    pub target_apps: Vec<AppId>,
    pub install_path: PathBuf,
    /// Human-readable message for the toast / notification / history.
    pub message: String,
}

/// Entry point: classify the path and run the matching flow.
pub fn do_fs_install(path: &Path) -> Result<FsInstalled, String> {
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }
    if path.is_dir() {
        return install_folder(path);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("zxp") => install_zxp(path),
        Some("jsx") | Some("jsxbin") => {
            let apps = detect_adobe_apps();
            install_script(path, &apps)
        }
        Some(other) => Err(format!(
            "unsupported file type: .{other} (expected .zxp, .jsx or .jsxbin)"
        )),
        None => Err("file has no extension; expected .zxp, .jsx or .jsxbin".to_string()),
    }
}

// ---------------------------------------------------------------------------
// ZXP flow
// ---------------------------------------------------------------------------

fn install_zxp(path: &Path) -> Result<FsInstalled, String> {
    let file =
        File::open(path).map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let mut zip =
        ZipArchive::new(file).map_err(|e| format!("not a valid zip archive: {e}"))?;

    // Find CSXS/manifest.xml (case-insensitive).
    let mut manifest_idx = None;
    for i in 0..zip.len() {
        let name = zip
            .by_index(i)
            .map(|f| f.name().to_string())
            .unwrap_or_default();
        if name.to_lowercase().ends_with("csxs/manifest.xml") {
            manifest_idx = Some(i);
            break;
        }
    }
    let manifest_idx =
        manifest_idx.ok_or_else(|| "no CSXS/manifest.xml found in archive".to_string())?;
    let mut manifest_xml = String::new();
    zip.by_index(manifest_idx)
        .map_err(|e| format!("could not read manifest from archive: {e}"))?
        .read_to_string(&mut manifest_xml)
        .map_err(|e| format!("could not read manifest from archive: {e}"))?;
    let manifest = parse_manifest_xml(&manifest_xml)?;

    // Extract to a unique temp dir.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("skpm-extract-{}-{nanos}", std::process::id()));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    std::fs::create_dir_all(&tmp)
        .map_err(|e| format!("could not create temp dir {}: {e}", tmp.display()))?;
    zip.extract(&tmp)
        .map_err(|e| format!("could not extract archive: {e}"))?;

    let src_root = find_manifest_root(&tmp)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("extension");
    let folder_name = sanitize_folder_name(stem);
    let cep = cep_install_dir().map_err(|e| format!("could not resolve CEP dir: {e}"))?;
    let dest = cep.join(&folder_name);

    let copy_result = (|| -> Result<(), String> {
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .map_err(|e| format!("could not remove existing {}: {e}", dest.display()))?;
        }
        copy_dir_all(&src_root, &dest)
            .map_err(|e| format!("could not copy extension files: {e}"))?;
        Ok(())
    })();

    // Best-effort temp cleanup either way.
    let _ = std::fs::remove_dir_all(&tmp);
    copy_result?;

    let name = prettify_name(&manifest.bundle_id);
    Ok(FsInstalled {
        message: format!("Installed extension “{name}”"),
        id: manifest.bundle_id,
        name,
        version: manifest.version,
        kind: PluginKind::Extension,
        target_apps: manifest.target_apps,
        install_path: dest,
    })
}

/// The extracted dir that directly contains the CSXS folder.
fn find_manifest_root(tmp: &Path) -> Result<PathBuf, String> {
    if has_csxs_dir(tmp) {
        return Ok(tmp.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir(tmp) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && has_csxs_dir(&p) {
                return Ok(p);
            }
        }
    }
    Err("extracted archive has no CSXS directory".to_string())
}

fn has_csxs_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                e.file_name().to_string_lossy().eq_ignore_ascii_case("csxs")
                    && e.path().is_dir()
            })
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Dropped-folder flow
// ---------------------------------------------------------------------------

/// Locate `CSXS/manifest.xml` inside a folder (case-insensitive), if present.
pub fn find_manifest_in_dir(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case("csxs")
            && entry.path().is_dir()
        {
            let csxs = entry.path();
            if let Ok(inner) = std::fs::read_dir(&csxs) {
                for f in inner.flatten() {
                    if f.file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case("manifest.xml")
                        && f.path().is_file()
                    {
                        return Some(f.path());
                    }
                }
            }
        }
    }
    None
}

fn install_folder(path: &Path) -> Result<FsInstalled, String> {
    let manifest_path = find_manifest_in_dir(path)
        .ok_or_else(|| "folder does not contain CSXS/manifest.xml".to_string())?;
    let xml = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("could not read manifest: {e}"))?;
    let manifest = parse_manifest_xml(&xml)?;

    let folder_name = sanitize_folder_name(
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("extension"),
    );
    let cep = cep_install_dir().map_err(|e| format!("could not resolve CEP dir: {e}"))?;
    let dest = cep.join(&folder_name);

    // Guard against copying a folder into itself (e.g. re-dropping a folder
    // that already lives in the CEP extensions dir).
    let src_canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let dest_canon = dest.canonicalize().unwrap_or_else(|_| dest.clone());
    if dest_canon == src_canon {
        // Already in place: just (re)register it.
        let name = prettify_name(&manifest.bundle_id);
        return Ok(FsInstalled {
            message: format!("Registered extension “{name}” (already in place)"),
            id: manifest.bundle_id,
            name,
            version: manifest.version,
            kind: PluginKind::Extension,
            target_apps: manifest.target_apps,
            install_path: dest,
        });
    }
    if dest_canon.starts_with(&src_canon) {
        return Err("cannot install: destination is inside the source folder".to_string());
    }

    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| format!("could not remove existing {}: {e}", dest.display()))?;
    }
    copy_dir_all(path, &dest).map_err(|e| format!("could not copy extension files: {e}"))?;

    let name = prettify_name(&manifest.bundle_id);
    Ok(FsInstalled {
        message: format!("Installed extension “{name}”"),
        id: manifest.bundle_id,
        name,
        version: manifest.version,
        kind: PluginKind::Extension,
        target_apps: manifest.target_apps,
        install_path: dest,
    })
}

// ---------------------------------------------------------------------------
// Script flow
// ---------------------------------------------------------------------------

fn read_head(path: &Path, max_bytes: usize) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let mut buf = vec![0u8; max_bytes];
    let n = file
        .read(&mut buf)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn install_script(path: &Path, apps: &[AdobeApp]) -> Result<FsInstalled, String> {
    let head = read_head(path, SNIFF_BYTES)?;
    let sniff = sniff_script_target(&head);
    let version = parse_script_version(&head);

    let app = newest_app(apps, sniff.app).ok_or_else(|| {
        format!(
            "no {} installation detected; install the app first, or copy the script manually",
            sniff.app.display_name()
        )
    })?;
    let dir = script_install_dir(app)
        .map_err(|e| format!("could not resolve script dir for {}: {e}", app.name))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "invalid file name".to_string())?;
    let dest = dir.join(file_name);
    if dest.exists() {
        std::fs::remove_file(&dest)
            .map_err(|e| format!("could not remove existing {}: {e}", dest.display()))?;
    }
    std::fs::copy(path, &dest).map_err(|e| format!("could not copy script: {e}"))?;

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("script")
        .to_string();
    let id = format!("{stem}__{}", sniff.app.as_str());
    let mut message = format!(
        "Installed script “{stem}” for {}",
        sniff.app.display_name()
    );
    if sniff.assumed {
        message.push_str(" (target app assumed)");
    }

    Ok(FsInstalled {
        id,
        name: stem,
        version,
        kind: PluginKind::Script,
        target_apps: vec![sniff.app],
        install_path: dest,
        message,
    })
}

// ---------------------------------------------------------------------------
// Recursive copy
// ---------------------------------------------------------------------------

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else if ft.is_symlink() {
            copy_symlink(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

fn copy_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    let target = std::fs::read_link(src)?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, dst)
    }
    #[cfg(windows)]
    {
        // Best effort: try file symlink, then dir symlink.
        std::os::windows::fs::symlink_file(&target, dst)
            .or_else(|_| std::os::windows::fs::symlink_dir(&target, dst))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, dst);
        Ok(())
    }
}
