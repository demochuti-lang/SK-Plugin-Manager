use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::types::AppId;

/// Map a manifest `Host @Name` code to an [`AppId`].
/// Unknown hosts are dropped by the caller.
pub fn host_name_to_app_id(name: &str) -> Option<AppId> {
    match name.to_ascii_uppercase().as_str() {
        "AEFT" => Some(AppId::Ae),
        "PPRO" => Some(AppId::Pp),
        "PHSP" => Some(AppId::Ps),
        "ILST" => Some(AppId::Ai),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionManifest {
    pub bundle_id: String,
    pub version: String,
    pub target_apps: Vec<AppId>,
}

fn attr_value<'a>(
    e: &'a quick_xml::events::BytesStart<'a>,
    reader: &Reader<&[u8]>,
    key: &[u8],
) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == key {
            if let Ok(v) = attr.decode_and_unescape_value(reader.decoder()) {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Parse a CSXS `manifest.xml`. Errors when the bundle id is missing or when
/// no supported Adobe host remains after filtering.
///
/// Real-world CEP format (what Adobe and extension authors actually ship):
/// `<ExtensionManifest ExtensionBundleId="com.x" ExtensionBundleVersion="1.0">`
/// with `<ExtensionList><Extension Id="…" Version="…"/></ExtensionList>` and
/// `<HostList><Host Name="PPRO" …/></HostList>` children. A legacy
/// `<ExtensionBundle Id="…" Version="…">` root is also accepted.
pub fn parse_manifest_xml(xml: &str) -> Result<ExtensionManifest, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut bundle_id: Option<String> = None;
    let mut bundle_version: Option<String> = None;
    let mut extension_version: Option<String> = None;
    let mut hosts: Vec<AppId> = Vec::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"ExtensionManifest" => {
                    if bundle_id.is_none() {
                        bundle_id = attr_value(&e, &reader, b"ExtensionBundleId");
                    }
                    if bundle_version.is_none() {
                        bundle_version = attr_value(&e, &reader, b"ExtensionBundleVersion");
                    }
                }
                b"ExtensionBundle" => {
                    if bundle_id.is_none() {
                        bundle_id = attr_value(&e, &reader, b"Id");
                    }
                    if bundle_version.is_none() {
                        bundle_version = attr_value(&e, &reader, b"Version");
                    }
                }
                b"Extension" => {
                    // Fallbacks only: the root element is parsed before its
                    // children, so real bundle attrs always win over these.
                    if extension_version.is_none() {
                        extension_version = attr_value(&e, &reader, b"Version");
                    }
                    if bundle_id.is_none() {
                        bundle_id = attr_value(&e, &reader, b"Id");
                    }
                }
                b"Host" => {
                    if let Some(name) = attr_value(&e, &reader, b"Name") {
                        if let Some(id) = host_name_to_app_id(name.trim()) {
                            if !hosts.contains(&id) {
                                hosts.push(id);
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("manifest XML parse error: {e}")),
            _ => {}
        }
        buf.clear();
    }

    let bundle_id =
        bundle_id.ok_or_else(|| "manifest missing ExtensionBundleId".to_string())?;
    if hosts.is_empty() {
        return Err("No supported Adobe host found in manifest".to_string());
    }
    let version = bundle_version
        .filter(|v| !v.trim().is_empty())
        .or_else(|| extension_version.filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "unknown".to_string());
    Ok(ExtensionManifest {
        bundle_id,
        version,
        target_apps: hosts,
    })
}

/// Turn a bundle id (or file stem) into a human name:
/// last dot-segment, separators become spaces.
pub fn prettify_name(bundle_id_or_stem: &str) -> String {
    let last = bundle_id_or_stem.rsplit('.').next().unwrap_or(bundle_id_or_stem);
    let mut out = String::with_capacity(last.len());
    let mut prev_space = true; // collapse + trim
    for c in last.chars() {
        if c == '_' || c == '-' || c == '.' || c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        "Unnamed".to_string()
    } else {
        trimmed
    }
}

/// Sanitize a file stem for use as an extension folder name.
pub fn sanitize_folder_name(stem: &str) -> String {
    let mut out: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    out = out.trim().to_string();
    if out.is_empty() {
        out = "extension".to_string();
    }
    out
}

// ---------------------------------------------------------------------------
// Script sniffing (first 8 KiB, lossy)
// ---------------------------------------------------------------------------

pub const SNIFF_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct ScriptSniff {
    pub app: AppId,
    pub assumed: bool,
}

/// Detect the target Adobe app for a script from its head text.
///
/// 1. `#target <app>` directives (case-insensitive).
/// 2. API markers: activeSequence→pp, CompItem→ae, activeDocument→ps,
///    app.project→ae (first match in that order wins).
/// 3. Default ae (assumed).
pub fn sniff_script_target(head: &str) -> ScriptSniff {
    // 1. #target directives (but not #targetengine).
    for line in head.lines() {
        let t = line.trim_start();
        if t.len() > 7 && t[..7].eq_ignore_ascii_case("#target") {
            let rest = &t[7..];
            // Skip "#targetengine".
            if rest
                .chars()
                .next()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false)
            {
                continue;
            }
            let word: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric())
                .collect();
            let app = match word.to_ascii_lowercase().as_str() {
                "photoshop" => Some(AppId::Ps),
                "illustrator" => Some(AppId::Ai),
                "aftereffects" => Some(AppId::Ae),
                "premiere" => Some(AppId::Pp),
                _ => None,
            };
            if let Some(app) = app {
                return ScriptSniff {
                    app,
                    assumed: false,
                };
            }
        }
    }

    // 2. API markers, in contract order.
    const MARKERS: &[(&str, AppId)] = &[
        ("activeSequence", AppId::Pp),
        ("CompItem", AppId::Ae),
        ("activeDocument", AppId::Ps),
        ("app.project", AppId::Ae),
    ];
    for (marker, app) in MARKERS {
        if head.contains(marker) {
            return ScriptSniff {
                app: *app,
                assumed: false,
            };
        }
    }

    // 3. Default.
    ScriptSniff {
        app: AppId::Ae,
        assumed: true,
    }
}

/// Parse a `$VERSION` / `@version` token from a script head, else "unknown".
pub fn parse_script_version(head: &str) -> String {
    let lower = head.to_ascii_lowercase();
    for key in ["@version", "$version"] {
        let mut search_from = 0;
        while let Some(pos) = lower[search_from..].find(key) {
            let abs = search_from + pos + key.len();
            let rest = &head[abs..];
            // Skip separators like whitespace, '=', ':', quotes.
            let token: String = rest
                .trim_start_matches(|c: char| {
                    !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
                })
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
                .collect();
            let token = token.trim_matches(|c| c == '\'' || c == '"');
            if token
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
                && token.chars().any(|c| c.is_ascii_digit())
            {
                return token.to_string();
            }
            search_from = abs;
        }
    }
    "unknown".to_string()
}

// ---------------------------------------------------------------------------
// Version comparison: split on '.', compare numeric segments.
// ---------------------------------------------------------------------------

fn version_segments(v: &str) -> Vec<u64> {
    v.split('.')
        .map(|seg| {
            let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// `true` iff `new_v` is strictly newer than `installed_v`.
/// Unknown versions (either side) → false.
pub fn is_newer_version(new_v: &str, installed_v: &str) -> bool {
    if new_v.eq_ignore_ascii_case("unknown") || installed_v.eq_ignore_ascii_case("unknown") {
        return false;
    }
    let a = version_segments(new_v.trim());
    let b = version_segments(installed_v.trim());
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_map_covers_contract_codes() {
        assert_eq!(host_name_to_app_id("AEFT"), Some(AppId::Ae));
        assert_eq!(host_name_to_app_id("PPRO"), Some(AppId::Pp));
        assert_eq!(host_name_to_app_id("PHSP"), Some(AppId::Ps));
        assert_eq!(host_name_to_app_id("ILST"), Some(AppId::Ai));
        assert_eq!(host_name_to_app_id("IDSN"), None);
        assert_eq!(host_name_to_app_id("aeft"), Some(AppId::Ae));
    }

    #[test]
    fn manifest_parses_bundle_and_hosts() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ExtensionBundle Id="com.example.coolfx" Version="1.2.3" xmlns="http://www.adobe.com/CEP">
  <ExtensionList><Extension Id="com.example.coolfx.panel" Version="1.2.3"/></ExtensionList>
  <HostList>
    <Host Name="AEFT" Version="[1.0,99.9]"/>
    <Host Name="PPRO" Version="[1.0,99.9]"/>
    <Host Name="IDSN" Version="[1.0,99.9]"/>
  </HostList>
</ExtensionBundle>"#;
        let m = parse_manifest_xml(xml).unwrap();
        assert_eq!(m.bundle_id, "com.example.coolfx");
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.target_apps, vec![AppId::Ae, AppId::Pp]);
    }

    #[test]
    fn manifest_parses_real_cep_format() {
        // Mirrors what Adobe/extension authors actually ship (e.g. Knights of
        // the Editing Table's Excalibur for Premiere Pro).
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ExtensionManifest Version="7.0" ExtensionBundleId="com.knightsoftheeditingtable.excalibur" ExtensionBundleVersion="1.2.3" ExtensionBundleName="Excalibur">
  <ExtensionList>
    <Extension Id="com.knightsoftheeditingtable.excalibur" Version="1.2.3"/>
  </ExtensionList>
  <ExecutionEnvironment>
    <HostList>
      <Host Name="PPRO" Version="[14.0,99.9]"/>
    </HostList>
    <LocaleList><Locale Code="All"/></LocaleList>
    <RequiredRuntimeList><RequiredRuntime Name="CSXS" Version="7.0"/></RequiredRuntimeList>
  </ExecutionEnvironment>
</ExtensionManifest>"#;
        let m = parse_manifest_xml(xml).unwrap();
        assert_eq!(m.bundle_id, "com.knightsoftheeditingtable.excalibur");
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.target_apps, vec![AppId::Pp]);
    }

    #[test]
    fn manifest_real_format_version_fallback() {
        // No ExtensionBundleVersion: fall back to the Extension entry version.
        let xml = r#"<ExtensionManifest ExtensionBundleId="com.example.y">
  <ExtensionList><Extension Id="com.example.y.panel" Version="4.5.6"/></ExtensionList>
  <ExecutionEnvironment><HostList><Host Name="AEFT" Version="[1.0,99.9]"/></HostList></ExecutionEnvironment>
</ExtensionManifest>"#;
        let m = parse_manifest_xml(xml).unwrap();
        assert_eq!(m.bundle_id, "com.example.y");
        assert_eq!(m.version, "4.5.6");
        assert_eq!(m.target_apps, vec![AppId::Ae]);
    }

    #[test]
    fn manifest_rejects_unsupported_hosts() {
        let xml = r#"<ExtensionBundle Id="com.example.x" Version="1.0">
            <HostList><Host Name="IDSN" Version="1.0"/></HostList>
        </ExtensionBundle>"#;
        let err = parse_manifest_xml(xml).unwrap_err();
        assert_eq!(err, "No supported Adobe host found in manifest");
    }

    #[test]
    fn manifest_rejects_missing_id() {
        let xml = r#"<ExtensionBundle Version="1.0"><HostList>
            <Host Name="AEFT" Version="1.0"/></HostList></ExtensionBundle>"#;
        assert!(parse_manifest_xml(xml).is_err());
    }

    #[test]
    fn version_compare_numeric_segments() {
        assert!(is_newer_version("1.10.0", "1.2.0"));
        assert!(is_newer_version("2.0", "1.9.9"));
        assert!(is_newer_version("1.2.3", "1.2"));
        assert!(!is_newer_version("1.2.0", "1.2.0"));
        assert!(!is_newer_version("1.2", "1.2.3"));
        assert!(!is_newer_version("unknown", "1.0"));
        assert!(!is_newer_version("2.0", "unknown"));
        assert!(!is_newer_version("1.2b", "1.2")); // equal numerically
        assert!(is_newer_version("2025.1", "2024"));
    }

    #[test]
    fn sniff_target_directive_wins() {
        let s = sniff_script_target("#target photoshop\napp.activeDocument;");
        assert_eq!(s.app, AppId::Ps);
        assert!(!s.assumed);
        // #targetengine must not be mistaken for #target.
        let s2 = sniff_script_target("#targetengine \"session\"\napp.project.items;");
        assert_eq!(s2.app, AppId::Ae);
        assert!(!s2.assumed); // marker fallback
    }

    #[test]
    fn sniff_markers_and_default() {
        let s = sniff_script_target("var seq = app.project.activeSequence;");
        assert_eq!(s.app, AppId::Pp);
        let d = sniff_script_target("// nothing recognizable here");
        assert_eq!(d.app, AppId::Ae);
        assert!(d.assumed);
    }

    #[test]
    fn script_version_parsing() {
        assert_eq!(parse_script_version("// @version 1.4.2"), "1.4.2");
        assert_eq!(parse_script_version("$VERSION = \"2.0.1\";"), "2.0.1");
        assert_eq!(parse_script_version("var x = 1;"), "unknown");
    }

    #[test]
    fn year_parsing() {
        use crate::adobe::parse_year;
        assert_eq!(parse_year("Adobe After Effects 2025"), "2025");
        assert_eq!(parse_year("Adobe Photoshop 2024"), "2024");
        assert_eq!(parse_year("Adobe Illustrator CC 2019"), "2019");
        assert_eq!(parse_year("Adobe Premiere Pro"), "unknown");
    }
}
