//! Google Drive integration for Phase 2 cloud backups.
//!
//! Manual OAuth2 with `reqwest` (no extra OAuth crate): the app opens the
//! Google consent page in the user's browser, a loopback HTTP server on
//! 127.0.0.1 receives the authorization code, and the code is exchanged for
//! tokens. Scope is limited to `drive.file` (files this app created).
//!
//! Credentials come ONLY from settings (`drive_client_id` /
//! `drive_client_secret`, entered by the user — see DRIVE_SETUP.md).
//! Nothing is hardcoded.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use sqlx::{Pool, Sqlite};
use tauri::{AppHandle, Runtime};

use crate::db;
use crate::settings;
use crate::types::{BackupLocation, DriveStatus, SimpleResult};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const ABOUT_URL: &str = "https://www.googleapis.com/drive/v3/about";
const FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

/// Prefix for "the user hasn't entered their Google Cloud credentials yet".
/// Frontend checks `message.startsWith('NEEDS_SETUP:')` to link DRIVE_SETUP.md.
pub const NEEDS_SETUP_PREFIX: &str = "NEEDS_SETUP: ";

fn needs_setup() -> String {
    format!(
        "{NEEDS_SETUP_PREFIX}Add your Google OAuth Client ID and Client Secret \
         in Settings → Backup → Google Drive first. See DRIVE_SETUP.md for the exact steps."
    )
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AboutResponse {
    user: Option<AboutUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutUser {
    email_address: Option<String>,
}

struct ClientCreds {
    id: String,
    secret: String,
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("could not create HTTP client: {e}"))
}

/// Parse a Google JSON response, turning API/token errors into messages.
async fn check_ok(resp: reqwest::Response, what: &str) -> Result<serde_json::Value, String> {
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("{what}: could not read Google's response: {e}"))?;
    if !status.is_success() {
        let detail = (|| {
            let v: serde_json::Value = serde_json::from_str(&body).ok()?;
            let e = v.get("error")?;
            e.get("error_description")
                .and_then(|d| d.as_str())
                .or_else(|| e.get("message").and_then(|m| m.as_str()))
                .or_else(|| e.as_str())
                .map(|s| s.to_owned())
        })()
        .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(format!("{what}: {detail}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("{what}: bad JSON from Google: {e}"))
}

async fn client_creds(pool: &Pool<Sqlite>) -> Result<ClientCreds, String> {
    let id = db::get_setting(pool, settings::KEY_DRIVE_CLIENT_ID)
        .await
        .map_err(|e| format!("could not read settings: {e}"))?
        .unwrap_or_default();
    let secret = db::get_setting(pool, settings::KEY_DRIVE_CLIENT_SECRET)
        .await
        .map_err(|e| format!("could not read settings: {e}"))?
        .unwrap_or_default();
    if id.trim().is_empty() || secret.trim().is_empty() {
        return Err(needs_setup());
    }
    Ok(ClientCreds {
        id: id.trim().to_string(),
        secret: secret.trim().to_string(),
    })
}

/// Exchange the stored refresh token for a fresh access token.
async fn refresh_access_token(
    pool: &Pool<Sqlite>,
    creds: &ClientCreds,
) -> Result<String, String> {
    let refresh_token = db::get_setting(pool, settings::KEY_DRIVE_REFRESH_TOKEN)
        .await
        .map_err(|e| format!("could not read settings: {e}"))?
        .unwrap_or_default();
    if refresh_token.trim().is_empty() {
        return Err(
            "Google Drive is not connected. Connect it in Settings → Backup → Google Drive."
                .to_string(),
        );
    }
    let client = http_client()?;
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.trim()),
            ("client_id", creds.id.as_str()),
            ("client_secret", creds.secret.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("could not reach Google: {e}"))?;
    let v = check_ok(resp, "Drive token refresh")
        .await
        .map_err(|e| format!("{e} Reconnect Google Drive in Settings."))?;
    v.get("access_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Drive token refresh returned no access token.".to_string())
}

// ---------------------------------------------------------------------------
// `drive_status`
// ---------------------------------------------------------------------------

/// Presence-only check: no network traffic.
pub async fn drive_status<R: Runtime>(app: &AppHandle<R>) -> Result<DriveStatus, String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    let all = db::all_settings(&pool)
        .await
        .map_err(|e| format!("could not read settings: {e}"))?;
    let non_empty = |key: &str| {
        all.get(key)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    };
    Ok(DriveStatus {
        configured: non_empty(settings::KEY_DRIVE_CLIENT_ID)
            && non_empty(settings::KEY_DRIVE_CLIENT_SECRET),
        connected: non_empty(settings::KEY_DRIVE_REFRESH_TOKEN),
        email: all
            .get(settings::KEY_DRIVE_EMAIL)
            .filter(|v| !v.trim().is_empty())
            .cloned(),
    })
}

// ---------------------------------------------------------------------------
// `drive_connect` — loopback OAuth flow
// ---------------------------------------------------------------------------

/// Read the browser's redirect request and pull out the `code` (or `error`)
/// query parameter.
async fn read_auth_code(socket: &mut tokio::net::TcpStream) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if buf.len() > 16 * 1024 {
            return Err("sign-in response too large".to_string());
        }
        let n = tokio::time::timeout(Duration::from_secs(60), socket.read(&mut chunk))
            .await
            .map_err(|_| "timed out waiting for the browser's response".to_string())?
            .map_err(|e| format!("could not read sign-in response: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let request_line = head.lines().next().unwrap_or("");
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}"))
        .map_err(|_| "could not parse the sign-in response".to_string())?;
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if let Some(err) = params.get("error") {
        return Err(format!("Google sign-in was not completed ({err})."));
    }
    params
        .get("code")
        .cloned()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| "Google did not return an authorization code.".to_string())
}

/// Tell the user's browser tab the sign-in finished.
async fn respond_browser(socket: &mut tokio::net::TcpStream) {
    use tokio::io::AsyncWriteExt;
    let body = "<!doctype html><html><body style=\"font-family:sans-serif;text-align:center;padding-top:10vh\">\
        <h2>Signed in to Google Drive</h2>\
        <p>You can close this tab and return to SK Plugin Manager.</p>\
        </body></html>";
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = socket.write_all(resp.as_bytes()).await;
    let _ = socket.shutdown().await;
}

async fn fetch_email(client: &reqwest::Client, access_token: &str) -> Option<String> {
    let url = reqwest::Url::parse_with_params(ABOUT_URL, &[("fields", "user")]).ok()?;
    let resp = client.get(url).bearer_auth(access_token).send().await.ok()?;
    let v = check_ok(resp, "drive about").await.ok()?;
    let about: AboutResponse = serde_json::from_value(v).ok()?;
    about
        .user?
        .email_address
        .filter(|e| !e.trim().is_empty())
}

pub async fn drive_connect<R: Runtime>(app: &AppHandle<R>) -> Result<SimpleResult, String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    let creds = match client_creds(&pool).await {
        Ok(c) => c,
        Err(e) => return Ok(SimpleResult { success: false, message: e }),
    };

    // Loopback server for the OAuth redirect (Google accepts any 127.0.0.1
    // port for Desktop-app clients).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("could not start local sign-in server: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("could not read local port: {e}"))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/");

    let auth_url = reqwest::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", creds.id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
    .map_err(|e| format!("could not build sign-in URL: {e}"))?;

    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(auth_url.as_str(), None::<&str>)
            .map_err(|e| format!("could not open the browser for Google sign-in: {e}"))?;
    }

    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(300), listener.accept())
        .await
        .map_err(|_| "Timed out waiting for Google sign-in (5 minutes) — try again.".to_string())?
        .map_err(|e| format!("sign-in server error: {e}"))?;

    let code = match read_auth_code(&mut socket).await {
        Ok(c) => c,
        Err(e) => {
            respond_browser(&mut socket).await;
            return Ok(SimpleResult { success: false, message: e });
        }
    };
    respond_browser(&mut socket).await;

    let client = http_client()?;
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("code", code.as_str()),
            ("client_id", creds.id.as_str()),
            ("client_secret", creds.secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| format!("token exchange failed: {e}"))?;
    let token_value = check_ok(resp, "token exchange").await.map_err(|e| {
        format!("{e} If this keeps failing, check the Client ID/Secret in Settings.")
    })?;
    let tokens: TokenResponse = serde_json::from_value(token_value)
        .map_err(|e| format!("token exchange returned an unexpected response: {e}"))?;
    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        "Google did not return a refresh token. Revoke SK Plugin Manager at \
         myaccount.google.com → Security → Third-party access, then connect again."
            .to_string()
    })?;

    // Best effort: the account email is only for display.
    let email = fetch_email(&client, &tokens.access_token).await;

    db::set_setting_db(&pool, settings::KEY_DRIVE_REFRESH_TOKEN, &refresh_token)
        .await
        .map_err(|e| format!("could not save Drive credentials: {e}"))?;
    db::set_setting_db(&pool, settings::KEY_DRIVE_EMAIL, email.as_deref().unwrap_or(""))
        .await
        .map_err(|e| format!("could not save Drive account: {e}"))?;

    Ok(SimpleResult {
        success: true,
        message: match email {
            Some(e) => format!("Connected to Google Drive as {e}"),
            None => "Connected to Google Drive".to_string(),
        },
    })
}

// ---------------------------------------------------------------------------
// `drive_disconnect`
// ---------------------------------------------------------------------------

pub async fn drive_disconnect<R: Runtime>(app: &AppHandle<R>) -> Result<SimpleResult, String> {
    let pool = db::pool(app)
        .await
        .map_err(|e| format!("library database unavailable: {e}"))?;
    db::set_setting_db(&pool, settings::KEY_DRIVE_REFRESH_TOKEN, "")
        .await
        .map_err(|e| format!("could not clear Drive credentials: {e}"))?;
    db::set_setting_db(&pool, settings::KEY_DRIVE_EMAIL, "")
        .await
        .map_err(|e| format!("could not clear Drive account: {e}"))?;
    Ok(SimpleResult {
        success: true,
        message: "Disconnected from Google Drive".to_string(),
    })
}

// ---------------------------------------------------------------------------
// Upload + remote delete
// ---------------------------------------------------------------------------

/// Multipart-upload one backup zip to Drive and mark its row "both".
pub async fn upload_backup_file(
    pool: &Pool<Sqlite>,
    backup_id: &str,
    filename: &str,
    zip_path: &Path,
) -> Result<(), String> {
    let creds = client_creds(pool).await?;
    let access_token = refresh_access_token(pool, &creds).await?;
    let bytes = tokio::task::spawn_blocking({
        let zip_path = zip_path.to_path_buf();
        move || {
            std::fs::read(&zip_path)
                .map_err(|e| format!("could not read {}: {e}", zip_path.display()))
        }
    })
    .await
    .map_err(|e| format!("upload worker failed: {e}"))??;

    let metadata =
        serde_json::json!({ "name": filename, "mimeType": "application/zip" }).to_string();
    let form = reqwest::multipart::Form::new()
        .part(
            "metadata",
            reqwest::multipart::Part::text(metadata)
                .mime_str("application/json")
                .map_err(|e| format!("could not build upload: {e}"))?,
        )
        .part(
            "file",
            reqwest::multipart::Part::bytes(bytes)
                .file_name(filename.to_string())
                .mime_str("application/zip")
                .map_err(|e| format!("could not build upload: {e}"))?,
        );

    let client = http_client()?;
    let url = reqwest::Url::parse_with_params(UPLOAD_URL, &[("uploadType", "multipart")])
        .map_err(|e| format!("could not build upload URL: {e}"))?;
    let resp = client
        .post(url)
        .bearer_auth(&access_token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Drive upload failed: {e}"))?;
    check_ok(resp, "Drive upload").await.map(|_| ())?;

    db::set_backup_location(pool, backup_id, BackupLocation::Both)
        .await
        .map_err(|e| format!("uploaded, but could not update the backup record: {e}"))?;
    Ok(())
}

/// Upload the zip of an existing backup (the `upload_backup_to_drive` command).
pub async fn upload_existing_backup<R: Runtime>(
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
    if matches!(info.location, BackupLocation::Both | BackupLocation::Drive) {
        return Ok(SimpleResult {
            success: true,
            message: "This backup is already on Google Drive".to_string(),
        });
    }
    let zip_path = settings::resolve_backup_dir(app, &pool)
        .await?
        .join(&info.filename);
    if !zip_path.is_file() {
        return Ok(SimpleResult {
            success: false,
            message: format!(
                "Backup file “{}” is missing from the backup folder.",
                info.filename
            ),
        });
    }
    match upload_backup_file(&pool, backup_id, &info.filename, &zip_path).await {
        Ok(()) => Ok(SimpleResult {
            success: true,
            message: format!("Uploaded “{}” to Google Drive", info.filename),
        }),
        Err(e) => Ok(SimpleResult { success: false, message: e }),
    }
}

/// Best-effort delete of Drive file(s) with this exact name.
/// Used when deleting a backup whose location includes Drive.
pub async fn delete_remote_by_name(
    pool: &Pool<Sqlite>,
    filename: &str,
) -> Result<(), String> {
    let creds = client_creds(pool).await?;
    let access_token = refresh_access_token(pool, &creds).await?;
    let client = http_client()?;

    // Escape single quotes for the Drive query language.
    let q = format!(
        "name = '{}' and trashed = false",
        filename.replace('\'', "\\'")
    );
    let url = reqwest::Url::parse_with_params(FILES_URL, &[("q", q.as_str()), ("fields", "files(id)")])
        .map_err(|e| format!("could not build Drive query: {e}"))?;
    let resp = client
        .get(url)
        .bearer_auth(&access_token)
        .send()
        .await
        .map_err(|e| format!("could not reach Google Drive: {e}"))?;
    let v = check_ok(resp, "Drive file lookup").await?;
    let ids: Vec<String> = v
        .get("files")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    f.get("id")
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    for id in ids {
        let resp = client
            .delete(format!("{FILES_URL}/{id}"))
            .bearer_auth(&access_token)
            .send()
            .await
            .map_err(|e| format!("could not reach Google Drive: {e}"))?;
        check_ok(resp, "Drive delete").await.map(|_| ())?;
    }
    Ok(())
}
