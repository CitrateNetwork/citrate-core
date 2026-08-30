//! Social identity OAuth link (Connections · social discovery) — ADR-2026-08-30.
//!
//! Device-local link store + keyring-sealed token; PUBLIC-client PKCE (no client secret, and NONE
//! embedded — Discord's PUBLIC_OAUTH2_CLIENT flag lets the token exchange omit the secret when a
//! `code_verifier` is present). Reuses the loopback-PKCE + keyring machinery from `connections.rs`.
//!
//! Privacy model (ADR): the binding is DEVICE-LOCAL (a JSON file in the app data dir); the OAuth
//! token seals in the OS keyring and NEVER crosses the invoke boundary (I-2). Links start PRIVATE
//! and UNVERIFIED — this module does the LINK (OAuth ownership proof) only. The wallet-signed
//! IdentityBinding that flips `verified` (D3) lands in a follow-up. Rule 3 holds: nothing here signs.
use serde::{Deserialize, Serialize};
use tauri::Manager;
use zeroize::Zeroize;

use crate::connections::{capture_public_pkce, ConnError, OAUTH_REDIRECT_URI};
use crate::oidc::HttpClient;

/// A network's OAuth endpoints + PUBLIC client id (safe to embed — no secret).
struct NetCfg {
    authorize: &'static str,
    token: &'static str,
    userinfo: &'static str,
    scope: &'static str,
    client_id: &'static str,
}

fn net_cfg(network: &str) -> Option<NetCfg> {
    match network {
        "discord" => Some(NetCfg {
            authorize: "https://discord.com/oauth2/authorize",
            token: "https://discord.com/api/oauth2/token",
            userinfo: "https://discord.com/api/users/@me",
            scope: "identify",
            // PUBLIC client id (safe to commit). Requires the Discord app's PUBLIC_OAUTH2_CLIENT
            // flag so the token exchange omits the secret when a PKCE code_verifier is supplied.
            client_id: "1543454988540444742",
        }),
        // "x" / "linkedin" land when their client ids are registered.
        _ => None,
    }
}

/// The device-local link record (mirrors the TS `LinkedIdentity`). Carries NO token.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinkedIdentity {
    pub network: String,
    pub handle: String,
    pub verified: bool,
    pub visibility: String, // "private" | "groups"
    pub linked_at: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn store_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("social");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("links.json"))
}

fn load_links(app: &tauri::AppHandle) -> Vec<LinkedIdentity> {
    match store_path(app).ok().and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn save_links(app: &tauri::AppHandle, links: &[LinkedIdentity]) -> Result<(), String> {
    let p = store_path(app)?;
    let bytes = serde_json::to_vec_pretty(links).map_err(|e| e.to_string())?;
    std::fs::write(p, bytes).map_err(|e| e.to_string())
}

/// The OS-keyring slot the sealed OAuth token lives in (never leaves the vault).
fn keyring_slot(network: &str) -> String {
    format!("social.{network}")
}

/// Build the provider authorize URL (query values percent-encoded via the `url` crate).
fn authorize_url(cfg: &NetCfg, state: &str, challenge: &str) -> String {
    let enc = |s: &str| -> String { url::form_urlencoded::byte_serialize(s.as_bytes()).collect() };
    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        cfg.authorize,
        enc(cfg.client_id),
        enc(OAUTH_REDIRECT_URI),
        enc(cfg.scope),
        enc(state),
        enc(challenge),
    )
}

/// The provider token response (public-client shape). Unknown fields dropped.
#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// The token record sealed in the keyring — JSON, zeroized after the put.
#[derive(Serialize)]
struct SealedToken<'a> {
    access_token: &'a str,
    refresh_token: Option<&'a str>,
    scope: Option<&'a str>,
    expires_at: Option<u64>,
    linked_at: u64,
}

/// Discord `/users/@me`: prefer the display `global_name`, else the `username`.
#[derive(Deserialize)]
struct DiscordUser {
    #[serde(default)]
    username: String,
    #[serde(default)]
    global_name: Option<String>,
}

fn fetch_handle(http: &impl HttpClient, cfg: &NetCfg, token: &str) -> Result<String, String> {
    let body = http
        .get(cfg.userinfo, Some(token))
        .map_err(|_| "could not read your profile from the provider".to_string())?;
    // Discord shape today; other networks parse their own userinfo when added.
    let u: DiscordUser =
        serde_json::from_str(&body).map_err(|_| "profile response was unparsable".to_string())?;
    Ok(u
        .global_name
        .filter(|s| !s.is_empty())
        .unwrap_or(u.username))
}

/// The blocking link flow: OAuth (public PKCE) → seal token in the keyring → fetch handle → record
/// the device-local, private, unverified link.
fn do_link(
    app: &tauri::AppHandle,
    custody: &crate::custody::CustodyVault,
    network: &str,
) -> Result<LinkedIdentity, String> {
    let cfg = net_cfg(network).ok_or_else(|| format!("{network} isn't configured for linking yet"))?;
    let http = crate::oidc::UreqClient;

    let app_open = app.clone();
    let open = move |u: &str| -> std::result::Result<(), ConnError> {
        use tauri_plugin_opener::OpenerExt;
        app_open
            .opener()
            .open_url(u.to_string(), None::<&str>)
            .map_err(|_| ConnError::Network)
    };

    // 1. loopback + PKCE capture (public client — returns the code + the verifier, never a secret).
    let (code, verifier) =
        capture_public_pkce(|state, challenge| authorize_url(&cfg, state, challenge), open)
            .map_err(|e| e.to_string())?;

    // 2. token exchange — PUBLIC client: send the verifier, NO client_secret.
    let form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("client_id", cfg.client_id),
        ("code", &code),
        ("redirect_uri", OAUTH_REDIRECT_URI),
        ("code_verifier", verifier.as_str()),
    ];
    let body = http
        .post_form(cfg.token, &form)
        .map_err(|_| "the provider rejected the sign-in (token exchange failed)".to_string())?;
    let tok: TokenResp =
        serde_json::from_str(&body).map_err(|_| "the token response was unparsable".to_string())?;

    // 3. seal the token in the OS keyring (never crosses the invoke boundary).
    let linked_at = now_unix();
    let expires_at = tok.expires_in.map(|e| linked_at.saturating_add(e));
    let sealed = SealedToken {
        access_token: &tok.access_token,
        refresh_token: tok.refresh_token.as_deref(),
        scope: tok.scope.as_deref(),
        expires_at,
        linked_at,
    };
    let mut bytes = serde_json::to_vec(&sealed).map_err(|e| e.to_string())?;
    let put = custody.put(&keyring_slot(network), &mut bytes);
    bytes.zeroize();
    put.map_err(|_| "could not seal the token in the keyring".to_string())?;

    // 4. read the handle (proves ownership), then 5. record the link — private + unverified (ADR).
    let handle = fetch_handle(&http, &cfg, &tok.access_token)?;
    let mut links = load_links(app);
    links.retain(|l| l.network != network);
    let li = LinkedIdentity {
        network: network.to_string(),
        handle,
        verified: false,
        visibility: "private".to_string(),
        linked_at,
    };
    links.push(li.clone());
    save_links(app, &links)?;
    Ok(li)
}

// ---------------------------------------------------------------------------
// Tauri command surface (I-2: none returns a token)
// ---------------------------------------------------------------------------

/// `social_status` — the device-local linked identities (no token, honest-empty when none).
#[tauri::command]
pub fn social_status(app: tauri::AppHandle) -> Result<Vec<LinkedIdentity>, String> {
    Ok(load_links(&app))
}

/// `social_start` — run the public-client OAuth link for a network in the SYSTEM browser (third-party
/// OAuth forbids embedded webviews). Async so Tauri runs it off the main thread while the loopback
/// blocks. Returns the unverified, private link; the token is sealed in the keyring.
#[tauri::command]
pub async fn social_start(
    app: tauri::AppHandle,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    network: String,
) -> Result<LinkedIdentity, String> {
    do_link(&app, &custody.0, &network)
}

/// `social_set_visibility` — set a link's visibility (private | groups). Narrowing takes effect now.
#[tauri::command]
pub fn social_set_visibility(
    app: tauri::AppHandle,
    network: String,
    visibility: String,
) -> Result<LinkedIdentity, String> {
    if visibility != "private" && visibility != "groups" {
        return Err("visibility must be 'private' or 'groups'".to_string());
    }
    let mut links = load_links(&app);
    let out = {
        let li = links
            .iter_mut()
            .find(|l| l.network == network)
            .ok_or_else(|| format!("{network} is not linked"))?;
        li.visibility = visibility;
        li.clone()
    };
    save_links(&app, &links)?;
    Ok(out)
}

/// `social_disconnect` — forget a link: wipe the keyring token (best-effort) + drop the local record.
#[tauri::command]
pub fn social_disconnect(
    app: tauri::AppHandle,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    network: String,
) -> Result<(), String> {
    let _ = custody.0.clear_slot(&keyring_slot(&network));
    let mut links = load_links(&app);
    links.retain(|l| l.network != network);
    save_links(&app, &links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_is_configured_public_no_secret() {
        let cfg = net_cfg("discord").expect("discord configured");
        assert_eq!(cfg.client_id, "1543454988540444742");
        assert_eq!(cfg.scope, "identify");
        assert!(cfg.token.starts_with("https://"));
    }

    #[test]
    fn unknown_network_is_none() {
        assert!(net_cfg("myspace").is_none());
    }

    #[test]
    fn authorize_url_carries_pkce_and_loopback_no_secret() {
        let cfg = net_cfg("discord").unwrap();
        let u = authorize_url(&cfg, "st8", "chal");
        assert!(u.contains("response_type=code"));
        assert!(u.contains("code_challenge=chal"));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("client_id=1543454988540444742"));
        // percent-encoded loopback redirect
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8975%2Foauth%2Fcallback"));
        // a public-client authorize URL never carries a secret
        assert!(!u.to_lowercase().contains("client_secret"));
    }
}
