//! Social identity — OAuth link + wallet-signed verification (Connections · social discovery).
//! Privacy model: ADR-2026-08-30.
//!
//! LINK (public-client PKCE, no secret): OAuth ownership proof → keyring-sealed token → a
//! DEVICE-LOCAL, private, UNVERIFIED record. VERIFY (D3): the wallet signs an `IdentityBinding`
//! challenge {network, handle, address, nonce} through the Signature Ceremony (Rule 3 — the vault
//! key signs at `ceremony.approve`, nothing here); the resulting EIP-191 signature IS by the wallet
//! address by construction, so recording it flips `verified` and gives group members something they
//! can independently check. The OAuth token NEVER crosses the invoke boundary; the binding signature
//! is device-local (shared server-blind to groups is a follow-up).
use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::Manager;
use zeroize::Zeroize;

use crate::ceremony::{CeremonyView, IntentKind, SignatureIntent};
use crate::connections::{capture_public_pkce, random_state, ConnError, OAUTH_REDIRECT_URI};
use crate::oidc::HttpClient;

const CHAIN_ID: u64 = 40204;

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
        "x" => Some(NetCfg {
            authorize: "https://x.com/i/oauth2/authorize",
            token: "https://api.twitter.com/2/oauth2/token",
            userinfo: "https://api.twitter.com/2/users/me",
            scope: "users.read tweet.read",
            // PUBLIC client id (safe to commit). The X app must be a "Native App / public client"
            // so the PKCE token exchange needs no secret.
            client_id: "MlItYlVLZHFZTkVHMWptZ1QyQV86MTpjaQ",
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Storage: device-local link records (on-disk) + the wire shape (no signature)
// ---------------------------------------------------------------------------

/// The wallet-signed proof that binds a handle to an address (ADR D3). Device-local; shareable to
/// groups later (server-blind). The signature is by the wallet address by construction.
#[derive(Serialize, Deserialize, Clone)]
struct Binding {
    address: String,
    nonce: String,
    /// `0x`-prefixed EIP-191 signature over the binding message.
    signature: String,
    bound_at: u64,
}

/// The on-disk record (carries the binding). Never crosses the bridge as-is.
#[derive(Serialize, Deserialize, Clone)]
struct StoredLink {
    network: String,
    handle: String,
    visibility: String, // "private" | "groups"
    linked_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binding: Option<Binding>,
}

/// The claim-free record crossing the invoke boundary — `verified` is derived; NO signature.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinkedIdentity {
    pub network: String,
    pub handle: String,
    pub verified: bool,
    pub visibility: String,
    pub linked_at: u64,
}

impl From<&StoredLink> for LinkedIdentity {
    fn from(s: &StoredLink) -> Self {
        LinkedIdentity {
            network: s.network.clone(),
            handle: s.handle.clone(),
            verified: s.binding.is_some(),
            visibility: s.visibility.clone(),
            linked_at: s.linked_at,
        }
    }
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

fn load_links(app: &tauri::AppHandle) -> Vec<StoredLink> {
    match store_path(app).ok().and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn save_links(app: &tauri::AppHandle, links: &[StoredLink]) -> Result<(), String> {
    let p = store_path(app)?;
    let bytes = serde_json::to_vec_pretty(links).map_err(|e| e.to_string())?;
    std::fs::write(p, bytes).map_err(|e| e.to_string())
}

/// The OS-keyring slot the sealed OAuth token lives in (never leaves the vault).
fn keyring_slot(network: &str) -> String {
    format!("social.{network}")
}

// --- foreign bindings (ADR D1): verified bindings received from group members, server-blind ---

/// A verified binding shared to us by a peer over the ciphertext-only relay. Held after we recover
/// its signature to the claimed address (so a peer can't assert a handle they don't control).
#[derive(Serialize, Deserialize, Clone)]
struct ForeignBinding {
    network: String,
    handle: String,
    address: String,
    seen_at: u64,
}

/// The shareable payload (rides a group message; the relay only sees ciphertext). It carries exactly
/// what a receiver needs to reconstruct the signed message and recover the signer — no token.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportedBinding {
    pub network: String,
    pub handle: String,
    pub address: String,
    pub nonce: String,
    pub signature: String,
}

fn foreign_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("social");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("foreign.json"))
}

fn load_foreign(app: &tauri::AppHandle) -> Vec<ForeignBinding> {
    match foreign_path(app).ok().and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn save_foreign(app: &tauri::AppHandle, v: &[ForeignBinding]) -> Result<(), String> {
    let p = foreign_path(app)?;
    let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(p, bytes).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// LINK flow (public-client OAuth PKCE)
// ---------------------------------------------------------------------------

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

#[derive(Serialize)]
struct SealedToken<'a> {
    access_token: &'a str,
    refresh_token: Option<&'a str>,
    scope: Option<&'a str>,
    expires_at: Option<u64>,
    linked_at: u64,
}

#[derive(Deserialize)]
struct DiscordUser {
    #[serde(default)]
    username: String,
    #[serde(default)]
    global_name: Option<String>,
}

/// X `/2/users/me` → `{ "data": { "username": "...", ... } }`.
#[derive(Deserialize)]
struct XUser {
    data: XUserData,
}
#[derive(Deserialize)]
struct XUserData {
    #[serde(default)]
    username: String,
}

fn fetch_handle(http: &impl HttpClient, network: &str, cfg: &NetCfg, token: &str) -> Result<String, String> {
    let body = http
        .get(cfg.userinfo, Some(token))
        .map_err(|_| "could not read your profile from the provider".to_string())?;
    let handle = match network {
        "x" => {
            let u: XUser = serde_json::from_str(&body)
                .map_err(|_| "profile response was unparsable".to_string())?;
            u.data.username
        }
        // discord (and default): flat shape, prefer the display global_name.
        _ => {
            let u: DiscordUser = serde_json::from_str(&body)
                .map_err(|_| "profile response was unparsable".to_string())?;
            u.global_name.filter(|s| !s.is_empty()).unwrap_or(u.username)
        }
    };
    if handle.is_empty() {
        return Err("the provider returned no handle".to_string());
    }
    Ok(handle)
}

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

    let (code, verifier) =
        capture_public_pkce(|state, challenge| authorize_url(&cfg, state, challenge), open)
            .map_err(|e| e.to_string())?;

    // Public client: send the verifier, NO client_secret.
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

    let handle = fetch_handle(&http, network, &cfg, &tok.access_token)?;
    let mut links = load_links(app);
    links.retain(|l| l.network != network);
    let stored = StoredLink {
        network: network.to_string(),
        handle,
        visibility: "private".to_string(),
        linked_at,
        binding: None,
    };
    let li = LinkedIdentity::from(&stored);
    links.push(stored);
    save_links(app, &links)?;
    Ok(li)
}

// ---------------------------------------------------------------------------
// VERIFY flow (wallet-signed IdentityBinding via the ceremony) — mirrors wallet_link
// ---------------------------------------------------------------------------

/// One in-flight verification, held against its ceremony id.
#[derive(Clone)]
struct PendingBind {
    network: String,
    address: String,
    nonce: String,
}

/// Process-wide pending-verification table (bounded by open ceremonies).
#[derive(Default)]
pub struct SocialBindState {
    pending: Mutex<HashMap<String, PendingBind>>,
}

impl SocialBindState {
    pub fn new() -> Self {
        Self::default()
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingBind>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Managed Tauri state wrapper.
pub struct SocialBindManaged(pub SocialBindState);

pub fn build_social_bind_state() -> SocialBindManaged {
    SocialBindManaged(SocialBindState::new())
}

/// The EIP-191 message the human sees + signs — plain, verbatim at the ceremony.
fn binding_message(network: &str, handle: &str, address: &str, nonce: &str) -> String {
    format!(
        "Citrate identity binding\nNetwork: {network}\nHandle: @{handle}\nAddress: {address}\nNonce: {nonce}\n\nSigning proves this wallet controls this social account. Shared only with your groups, never published."
    )
}

// ---------------------------------------------------------------------------
// Tauri command surface (I-2: none returns a token or a raw signature buffer)
// ---------------------------------------------------------------------------

/// `social_status` — the device-local linked identities (no token/signature; verified derived).
#[tauri::command]
pub fn social_status(app: tauri::AppHandle) -> Result<Vec<LinkedIdentity>, String> {
    Ok(load_links(&app).iter().map(LinkedIdentity::from).collect())
}

/// `social_start` — run the public-client OAuth link in the SYSTEM browser (async: the loopback
/// blocks off the main thread). Returns the unverified, private link; the token seals in the keyring.
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
        let link = links
            .iter_mut()
            .find(|l| l.network == network)
            .ok_or_else(|| format!("{network} is not linked"))?;
        link.visibility = visibility;
        LinkedIdentity::from(&*link)
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

/// `social_verify_request` — open a ceremony over the exact binding message the wallet will sign.
/// Returns the [`CeremonyView`] the approval UI renders — NEVER a signature (I-2). The address is
/// read first so a locked vault fails before a one-time nonce is minted.
#[tauri::command]
pub fn social_verify_request(
    app: tauri::AppHandle,
    bind: tauri::State<'_, SocialBindManaged>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    network: String,
) -> Result<CeremonyView, String> {
    let handle = {
        let links = load_links(&app);
        let link = links
            .iter()
            .find(|l| l.network == network)
            .ok_or_else(|| format!("{network} is not linked"))?;
        if link.binding.is_some() {
            return Err(format!("{network} is already verified"));
        }
        link.handle.clone()
    };
    let info = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;
    let nonce = random_state();
    let message = binding_message(&network, &handle, &info.address, &nonce);
    let view = ceremony.0.request(SignatureIntent {
        origin: format!("social:{network}"),
        kind: IntentKind::PersonalSign,
        chain_id: CHAIN_ID,
        raw: hex::encode(message.as_bytes()),
    });
    bind.0.lock().insert(
        view.id.clone(),
        PendingBind {
            network,
            address: info.address,
            nonce,
        },
    );
    Ok(view)
}

/// `social_verify_approve` — the human approved: sign the binding at the ceremony (the vault key
/// signs; Rule 3), record the `IdentityBinding`, and flip `verified`. Returns the updated link. The
/// signature is stored device-local, never returned as a raw buffer.
#[tauri::command]
pub fn social_verify_approve(
    app: tauri::AppHandle,
    bind: tauri::State<'_, SocialBindManaged>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    id: String,
    raw_ack: bool,
) -> Result<LinkedIdentity, String> {
    // Read (do not remove) so a raw-ack refusal can be retried with the ack.
    let pending = bind
        .0
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| "no pending verification for that request".to_string())?;
    let sig = ceremony
        .0
        .approve(&custody.0, &id, raw_ack)
        .map_err(|e| e.to_string())?;
    bind.0.lock().remove(&id);

    let mut links = load_links(&app);
    let out = {
        let link = links
            .iter_mut()
            .find(|l| l.network == pending.network)
            .ok_or_else(|| "the link was removed before verification completed".to_string())?;
        link.binding = Some(Binding {
            address: pending.address,
            nonce: pending.nonce,
            signature: format!("0x{}", sig.sig_hex),
            bound_at: now_unix(),
        });
        LinkedIdentity::from(&*link)
    };
    save_links(&app, &links)?;
    Ok(out)
}

/// `social_verify_forget` — drop a pending verification whose ceremony the human rejected.
#[tauri::command]
pub fn social_verify_forget(bind: tauri::State<'_, SocialBindManaged>, id: String) -> Result<(), String> {
    bind.0.lock().remove(&id);
    Ok(())
}

/// The face a viewer may see for an address — a verified, group-visible handle.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedIdentity {
    pub address: String,
    pub network: String,
    pub handle: String,
}

/// `social_resolve` — given member addresses, return the verified, group-visible handles this
/// device can resolve. TODAY that's the user's OWN binding(s); cross-member handles arrive when
/// bindings are shared server-blind to groups (ADR D1 — the follow-up). PRIVATE links never resolve
/// to anyone (D2), and only a verified binding (a wallet signature) ever produces a face (D3).
#[tauri::command]
pub fn social_resolve(app: tauri::AppHandle, addresses: Vec<String>) -> Result<Vec<ResolvedIdentity>, String> {
    let want: std::collections::HashSet<String> =
        addresses.iter().map(|a| a.to_lowercase()).collect();
    // Own verified + group-visible bindings (self-resolution).
    let mut out: Vec<ResolvedIdentity> = load_links(&app)
        .iter()
        .filter_map(|l| {
            let b = l.binding.as_ref()?; // verified only (D3)
            if l.visibility != "groups" {
                return None; // private never resolves to others (D2)
            }
            if !want.contains(&b.address.to_lowercase()) {
                return None;
            }
            Some(ResolvedIdentity {
                address: b.address.clone(),
                network: l.network.clone(),
                handle: l.handle.clone(),
            })
        })
        .collect();
    // Foreign bindings shared to us by group members (already recover-verified on ingest, D1).
    for f in load_foreign(&app) {
        if want.contains(&f.address.to_lowercase()) {
            out.push(ResolvedIdentity {
                address: f.address,
                network: f.network,
                handle: f.handle,
            });
        }
    }
    Ok(out)
}

/// `social_export_binding` — the shareable payload for a verified, group-visible link (or null). The
/// caller rides it over the ciphertext-only group relay (server-blind); it carries no token.
#[tauri::command]
pub fn social_export_binding(app: tauri::AppHandle, network: String) -> Result<Option<ExportedBinding>, String> {
    let links = load_links(&app);
    Ok(links.iter().find_map(|l| {
        if l.network != network || l.visibility != "groups" {
            return None;
        }
        let b = l.binding.as_ref()?;
        Some(ExportedBinding {
            network: l.network.clone(),
            handle: l.handle.clone(),
            address: b.address.clone(),
            nonce: b.nonce.clone(),
            signature: b.signature.clone(),
        })
    }))
}

/// `social_ingest_binding` — accept a peer's binding received over the relay. VERIFIES before
/// trusting: the message `sender` must equal the claimed address AND the signature must recover to
/// it (recover_personal over the exact binding message). Only then is the face stored. Returns
/// whether it was accepted. This is the gate that stops anyone asserting a handle for an address
/// they don't control.
#[tauri::command]
pub fn social_ingest_binding(
    app: tauri::AppHandle,
    sender: String,
    binding: ExportedBinding,
) -> Result<bool, String> {
    // 1. the sharer must be the address they claim (the relay attributes the message to `sender`).
    if sender.to_lowercase() != binding.address.to_lowercase() {
        return Ok(false);
    }
    // 2. the signature must recover to that address over the EXACT binding message (D3).
    let message = binding_message(&binding.network, &binding.handle, &binding.address, &binding.nonce);
    let recovered = crate::wallet::recover_personal_hex(message.as_bytes(), &binding.signature)
        .map_err(|e| e.to_string())?;
    if recovered.to_lowercase() != binding.address.to_lowercase() {
        return Ok(false);
    }
    // 3. accepted — upsert the foreign face (keyed by address+network).
    let mut foreign = load_foreign(&app);
    foreign.retain(|f| !(f.address.eq_ignore_ascii_case(&binding.address) && f.network == binding.network));
    foreign.push(ForeignBinding {
        network: binding.network,
        handle: binding.handle,
        address: binding.address,
        seen_at: now_unix(),
    });
    save_foreign(&app, &foreign)?;
    Ok(true)
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
    fn x_is_configured_public_no_secret() {
        let cfg = net_cfg("x").expect("x configured");
        assert_eq!(cfg.client_id, "MlItYlVLZHFZTkVHMWptZ1QyQV86MTpjaQ");
        assert!(cfg.scope.contains("users.read"));
        assert_eq!(cfg.userinfo, "https://api.twitter.com/2/users/me");
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
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8975%2Foauth%2Fcallback"));
        assert!(!u.to_lowercase().contains("client_secret"));
    }

    #[test]
    fn binding_message_names_all_fields() {
        let m = binding_message("discord", "dana", "0xabc", "n0nce");
        assert!(m.contains("Network: discord"));
        assert!(m.contains("Handle: @dana"));
        assert!(m.contains("Address: 0xabc"));
        assert!(m.contains("Nonce: n0nce"));
        assert!(m.contains("never published"));
    }

    #[test]
    fn verified_is_derived_from_binding_presence() {
        let mut s = StoredLink {
            network: "discord".into(),
            handle: "dana".into(),
            visibility: "private".into(),
            linked_at: 1,
            binding: None,
        };
        assert!(!LinkedIdentity::from(&s).verified);
        s.binding = Some(Binding {
            address: "0xabc".into(),
            nonce: "n".into(),
            signature: "0xsig".into(),
            bound_at: 2,
        });
        assert!(LinkedIdentity::from(&s).verified);
    }
}
