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
    /// #61 — whether this verified link is currently published to the opt-in find-via-X directory
    /// (`auth.citrate.ai/directory`). Separate, MORE public opt-in than `visibility` (which is
    /// group-scoped). Set true after a successful `directory_publish_approve`; false after revoke.
    #[serde(default)]
    directory_published: bool,
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
    /// #61 — published to the opt-in find-via-X directory (drives the publish toggle state).
    pub directory_published: bool,
}

impl From<&StoredLink> for LinkedIdentity {
    fn from(s: &StoredLink) -> Self {
        LinkedIdentity {
            network: s.network.clone(),
            handle: s.handle.clone(),
            verified: s.binding.is_some(),
            visibility: s.visibility.clone(),
            linked_at: s.linked_at,
            directory_published: s.directory_published,
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
        directory_published: false,
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

// ---------------------------------------------------------------------------
// #61 — self-published bindings directory (find-via-X). A deliberate, opt-in D-7 exception: only a
// VERIFIED link (which required the OAuth ownership proof) can be published, and publishing is a
// signed, human-approved action. The two proofs the authority checks are (1) the app's existing
// verified IdentityBinding (`ownership_proof`) and (2) a fresh, directory-scoped `sig` produced HERE
// via the SignatureCeremony (Rule 3). Squatting caveat (server can't verify handle OWNERSHIP, which is
// device-local by design) is accepted for v1 per owner 2026-09-12 — the app OAuth-gate + human-approved
// invites bound the blast radius to discovery/impersonation, not silent access.
// ---------------------------------------------------------------------------

/// One in-flight directory ceremony (publish or revoke), held against its ceremony id.
#[derive(Clone)]
struct PendingDirectory {
    network: String,
    handle: String,
    address: String,
    /// Present for a PUBLISH (the ownership proof + timestamp to POST on approve); None for a REVOKE.
    publish: Option<PendingPublish>,
}

#[derive(Clone)]
struct PendingPublish {
    bound_at: u64,
    ownership_nonce: String,
    ownership_sig: String,
}

#[derive(Default)]
pub struct DirectoryPendingState {
    pending: Mutex<HashMap<String, PendingDirectory>>,
}
impl DirectoryPendingState {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingDirectory>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Managed Tauri state wrapper for in-flight directory ceremonies.
pub struct DirectoryPendingManaged(pub DirectoryPendingState);

pub fn build_directory_pending_state() -> DirectoryPendingManaged {
    DirectoryPendingManaged(DirectoryPendingState::default())
}

/// The lower-cased, `@`-stripped handle key — the directory's uniqueness/lookup key. MUST match the
/// authority's `handleKeyOf` (citrate-identity directory.ts) byte-for-byte or a signature never matches.
fn handle_key(handle: &str) -> String {
    handle.trim_start_matches('@').to_lowercase()
}

/// The EXACT directory-scoped statement the wallet signs to PUBLISH. Ported byte-for-byte from the
/// authority's `buildDirectoryPublishStatement` — deterministic + case-folded so both sides derive the
/// identical string (lower-cased address, normalized handle key, integer bound_at).
fn directory_publish_statement(platform: &str, handle_key: &str, address: &str, bound_at: u64) -> String {
    format!("citrate-directory-binding:v1:{platform}:{handle_key}:{}:{bound_at}", address.to_lowercase())
}

/// The EXACT statement the wallet signs to REVOKE (no timestamp — re-revoking is idempotent). Ported
/// from the authority's `buildDirectoryRevokeStatement`.
fn directory_revoke_statement(platform: &str, handle_key: &str, address: &str) -> String {
    format!("citrate-directory-revoke:v1:{platform}:{handle_key}:{}", address.to_lowercase())
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

// ---------------------------------------------------------------------------
// #61 — directory commands (opt-in find-via-X publish + lookup/search).
// ---------------------------------------------------------------------------

/// Load the verified binding for `network`, erroring if the link is missing or unverified. Only a
/// verified link can be published (this IS the app-side OAuth gate the directory relies on).
fn verified_binding(app: &tauri::AppHandle, network: &str) -> Result<(String, Binding), String> {
    let links = load_links(app);
    let link = links
        .iter()
        .find(|l| l.network == network)
        .ok_or_else(|| format!("{network} is not linked"))?;
    let binding = link
        .binding
        .clone()
        .ok_or_else(|| format!("{network} must be verified before you can publish it"))?;
    Ok((link.handle.clone(), binding))
}

/// `directory_publish_request` — open a ceremony over the directory-scoped PUBLISH statement the
/// wallet will sign (opt-in find-via-X). Returns the [`CeremonyView`]; signs nothing (I-2). Requires a
/// verified link (the OAuth ownership proof already happened); the ownership proof is reused as-is.
#[tauri::command]
pub fn directory_publish_request(
    app: tauri::AppHandle,
    dir: tauri::State<'_, DirectoryPendingManaged>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    network: String,
) -> Result<CeremonyView, String> {
    let (handle, binding) = verified_binding(&app, &network)?;
    let hk = handle_key(&handle);
    let bound_at = now_unix();
    let statement = directory_publish_statement(&network, &hk, &binding.address, bound_at);
    let view = ceremony.0.request(SignatureIntent {
        origin: format!("directory:publish:{network}"),
        kind: IntentKind::PersonalSign,
        chain_id: CHAIN_ID,
        raw: hex::encode(statement.as_bytes()),
    });
    dir.0.lock().insert(
        view.id.clone(),
        PendingDirectory {
            network,
            handle,
            address: binding.address,
            publish: Some(PendingPublish {
                bound_at,
                ownership_nonce: binding.nonce,
                ownership_sig: binding.signature,
            }),
        },
    );
    Ok(view)
}

/// `directory_publish_approve` — the human approved: sign the directory statement at the ceremony,
/// then POST the binding (ownership proof + fresh sig) to the authority with the member's Bearer.
/// Marks the link published on success. Honest error on a rejected proof / stale write (Rule 1).
#[tauri::command]
pub fn directory_publish_approve(
    app: tauri::AppHandle,
    dir: tauri::State<'_, DirectoryPendingManaged>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    auth: tauri::State<'_, crate::oidc::AuthState>,
    id: String,
    raw_ack: bool,
) -> Result<LinkedIdentity, String> {
    let pending = dir
        .0
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| "no pending directory publish for that request".to_string())?;
    let publish = pending
        .publish
        .clone()
        .ok_or_else(|| "that request is a revoke, not a publish".to_string())?;
    let sig = ceremony
        .0
        .approve(&custody.0, &id, raw_ack)
        .map_err(|e| e.to_string())?;
    // Only drop the pending record once the sign succeeded (a raw-ack refusal can be retried).
    dir.0.lock().remove(&id);
    let sig_hex = format!("0x{}", sig.sig_hex);
    auth.0
        .directory_publish(
            &pending.network,
            &pending.handle,
            &pending.address,
            None,
            publish.bound_at,
            &publish.ownership_nonce,
            &publish.ownership_sig,
            &sig_hex,
        )
        .map_err(|e| e.to_string())?;
    let mut links = load_links(&app);
    let out = {
        let link = links
            .iter_mut()
            .find(|l| l.network == pending.network)
            .ok_or_else(|| "the link was removed before publishing completed".to_string())?;
        link.directory_published = true;
        LinkedIdentity::from(&*link)
    };
    save_links(&app, &links)?;
    Ok(out)
}

/// `directory_unpublish_request` — open a ceremony over the directory REVOKE statement (remove a
/// published binding). Returns the [`CeremonyView`]; signs nothing.
#[tauri::command]
pub fn directory_unpublish_request(
    app: tauri::AppHandle,
    dir: tauri::State<'_, DirectoryPendingManaged>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    network: String,
) -> Result<CeremonyView, String> {
    let (handle, binding) = verified_binding(&app, &network)?;
    let hk = handle_key(&handle);
    let statement = directory_revoke_statement(&network, &hk, &binding.address);
    let view = ceremony.0.request(SignatureIntent {
        origin: format!("directory:revoke:{network}"),
        kind: IntentKind::PersonalSign,
        chain_id: CHAIN_ID,
        raw: hex::encode(statement.as_bytes()),
    });
    dir.0.lock().insert(
        view.id.clone(),
        PendingDirectory { network, handle, address: binding.address, publish: None },
    );
    Ok(view)
}

/// `directory_unpublish_approve` — sign the revoke statement at the ceremony, tombstone the binding at
/// the authority, and mark the link unpublished. Honest error on a failed revoke.
#[tauri::command]
pub fn directory_unpublish_approve(
    app: tauri::AppHandle,
    dir: tauri::State<'_, DirectoryPendingManaged>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    auth: tauri::State<'_, crate::oidc::AuthState>,
    id: String,
    raw_ack: bool,
) -> Result<LinkedIdentity, String> {
    let pending = dir
        .0
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| "no pending directory revoke for that request".to_string())?;
    if pending.publish.is_some() {
        return Err("that request is a publish, not a revoke".to_string());
    }
    let sig = ceremony
        .0
        .approve(&custody.0, &id, raw_ack)
        .map_err(|e| e.to_string())?;
    dir.0.lock().remove(&id);
    let sig_hex = format!("0x{}", sig.sig_hex);
    auth.0
        .directory_revoke(&pending.network, &pending.handle, &pending.address, &sig_hex)
        .map_err(|e| e.to_string())?;
    let mut links = load_links(&app);
    let out = {
        let link = links
            .iter_mut()
            .find(|l| l.network == pending.network)
            .ok_or_else(|| "the link was removed before revoking completed".to_string())?;
        link.directory_published = false;
        LinkedIdentity::from(&*link)
    };
    save_links(&app, &links)?;
    Ok(out)
}

/// `directory_forget` — drop a pending directory ceremony the human rejected.
#[tauri::command]
pub fn directory_forget(dir: tauri::State<'_, DirectoryPendingManaged>, id: String) -> Result<(), String> {
    dir.0.lock().remove(&id);
    Ok(())
}

/// `directory_lookup` — resolve an EXACT social handle to a published address (find-via-X). `None`
/// when nobody opted in for that handle — never a guess (D-7). Bearer-gated in the authority.
#[tauri::command]
pub fn directory_lookup(
    auth: tauri::State<'_, crate::oidc::AuthState>,
    platform: String,
    handle: String,
) -> Result<Option<crate::oidc::DirectoryHit>, String> {
    auth.0
        .directory_lookup(&platform, handle.trim_start_matches('@'))
        .map_err(|e| e.to_string())
}

/// `directory_search` — typeahead over published handles (find-via-X). Honest-empty on no match.
#[tauri::command]
pub fn directory_search(
    auth: tauri::State<'_, crate::oidc::AuthState>,
    platform: String,
    query: String,
) -> Result<Vec<crate::oidc::DirectorySearchHit>, String> {
    auth.0
        .directory_search(&platform, query.trim_start_matches('@'))
        .map_err(|e| e.to_string())
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
            directory_published: false,
        };
        assert!(!LinkedIdentity::from(&s).verified);
        assert!(!LinkedIdentity::from(&s).directory_published);
        s.binding = Some(Binding {
            address: "0xabc".into(),
            nonce: "n".into(),
            signature: "0xsig".into(),
            bound_at: 2,
        });
        assert!(LinkedIdentity::from(&s).verified);
    }

    /// #61 — the handle key is lower-cased + `@`-stripped, matching the authority's `handleKeyOf`.
    #[test]
    fn handle_key_matches_authority() {
        assert_eq!(handle_key("@Dana"), "dana");
        assert_eq!(handle_key("dana"), "dana");
        assert_eq!(handle_key("Dana_X.1"), "dana_x.1");
    }

    /// #61 — the directory-scoped statements MUST be byte-for-byte what citrate-identity's
    /// `buildDirectoryPublishStatement` / `buildDirectoryRevokeStatement` derive, or the signature the
    /// wallet produces never verifies server-side. Address is case-folded; publish carries `bound_at`.
    #[test]
    fn directory_statements_match_authority_format() {
        let addr = "0xABCdef0000000000000000000000000000000001";
        assert_eq!(
            directory_publish_statement("x", "dana", addr, 1_699_999_999),
            "citrate-directory-binding:v1:x:dana:0xabcdef0000000000000000000000000000000001:1699999999"
        );
        assert_eq!(
            directory_revoke_statement("discord", "dana", addr),
            "citrate-directory-revoke:v1:discord:dana:0xabcdef0000000000000000000000000000000001"
        );
    }
}
