//! Group invites — INVITE-S2 self-admit (#72) + the CONNECT-S1 claim-back fallback (ADR-2026-08-30 D4).
//!
//! Invite-by-@handle without a directory: the owner mints a one-time, group-bound token, stored
//! device-local, and shares its link over the social platform (the rendezvous is the DM — exactly
//! where the @handle relationship lives). Citrate NEVER resolves a handle to an address.
//!
//! ## INVITE-S2 — self-admit (the primary path, owner 2026-09-12)
//! On mint, the owner PUBLISHES `BLAKE3(token)` + an expiry to the relay (`comms::publish_invite`);
//! the RAW token stays in the share link and never reaches the relay. The invitee opens the link and
//! **self-admits in one click** (`group_invite_redeem` → `RedeemInvite`): a token-holder joins by MLS
//! external commit with NO owner action, even if the owner is offline. The token is single-use / TTL /
//! revocable, so a leaked link admits exactly one join. The relay records the referral attribution
//! (inviter→joiner) automatically on a successful redeem; this module ALSO keeps a device-local audit
//! copy (#73) the member can export — the relay tally is authoritative for airdrop scoring.
//!
//! ## CONNECT-S1 — claim-back (fallback)
//! Retained: the invitee VOLUNTEERS their address in a sealed claim (consent, D4) and the owner
//! approves it via the normal roster path. Used when self-admit isn't wanted (e.g. the owner prefers
//! to vet each joiner). Both paths go through the same relay.
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::connections::random_state;

/// INVITE-S2 default time-to-live for a published invite: 14 days (Unix ms are added at publish).
pub(crate) const INVITE_TTL_MS: u64 = 14 * 24 * 60 * 60 * 1000;

/// A one-time claimable invite the owner minted for a group. `for_handle` is a label only.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingInvite {
    pub group: String,
    pub token: String,
    pub for_handle: String,
    pub created_at: u64,
    /// CONNECT-S1 — the invite's ephemeral PRIVATE key (hex), owner-only. Claims from the relay's
    /// server-blind inbox are sealed to the matching public key (in the link) and open only with this.
    /// `#[serde(default)]` so pre-S1 invites (no key) still load — they fall back to the manual path.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub priv_key: String,
    /// CONNECT-S1 — the full share link (carries the ephemeral PUBLIC key `k=`), so the owner's
    /// "Copy link" hands out a one-click link. Public; safe to serialize. `default` for pre-S1 records.
    #[serde(default)]
    pub link: String,
    /// GROW-S2 — the relay rendezvous `lookup` (hex) the daemon confirmed for this invite, empty when
    /// no rendezvous was opened (an older daemon, a relay without rendezvous, or a pre-S2 invite).
    /// Public (the relay's own index). Revoke closes it with `rendezvousClose`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lookup: String,
}

/// PBA-L4-008: the webview view of an outstanding invite. It deliberately has NO private-key
/// field: the invite's ECIES `priv_key` is owner-only, lives only in the 0600 `pending.json`, and
/// never crosses `invoke` (I-2). `group_invites` returns this, never [`PendingInvite`].
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingInviteView {
    pub group: String,
    pub token: String,
    pub for_handle: String,
    pub created_at: u64,
    pub link: String,
    /// GROW-S2 — the rendezvous lookup (matches `InviteView.lookup`), "" when none was opened.
    pub lookup: String,
}

impl From<&PendingInvite> for PendingInviteView {
    fn from(i: &PendingInvite) -> Self {
        PendingInviteView {
            group: i.group.clone(),
            token: i.token.clone(),
            for_handle: i.for_handle.clone(),
            created_at: i.created_at,
            link: i.link.clone(),
            lookup: i.lookup.clone(),
        }
    }
}

/// The owner's outstanding invites for `group`, as key-free views (PBA-L4-008).
pub fn invite_views_for_group(all: &[PendingInvite], group: &str) -> Vec<PendingInviteView> {
    all.iter()
        .filter(|i| i.group == group)
        .map(PendingInviteView::from)
        .collect()
}

/// The link + token returned to the owner to share (DM to the @handle on the platform).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InviteMinted {
    pub token: String,
    pub link: String,
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// INVITE-S2 — the token hash the owner publishes to the relay. MUST be `BLAKE3(raw_token)` (hex) so
/// it matches what `comms-relay::redeem_invite` computes from the invitee's raw token — otherwise the
/// self-admit never matches. Distinct from `invite_seal::token_hash` (SHA-256), which keys the
/// CONNECT-S1 claims-inbox (both sides client-computed there, so that one need not match the relay).
pub(crate) fn blake3_token_hash(token: &str) -> String {
    hex::encode(blake3::hash(token.as_bytes()).as_bytes())
}

/// GROW-S2 (§15 M2) — the result of the two publishes at mint that handed out a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MintDecision {
    /// The rendezvous lookup to record for revoke ("" only when the daemon or relay doesn't speak
    /// rendezvous, so nothing can have been opened).
    pub lookup: String,
}

/// No link: both publishes failed. `lookup_to_close` is an open the daemon may still hold (an
/// ambiguous answer), which the caller closes so the daemon drops its secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MintRefused {
    pub message: String,
    pub lookup_to_close: String,
}

/// The lookup to record for an open, whatever the answer (I6: ambiguity is never terminal). The
/// daemon persists the open before it talks to the relay (§9), so a timeout, a dropped socket or an
/// error can still leave it open at the daemon, resending. Only "this daemon / relay has no
/// rendezvous" means nothing was opened. A daemon answer with another lookup is recorded as given,
/// so the close reaches what the daemon holds.
fn lookup_to_record(
    open: &Result<crate::comms::RendezvousOpened, crate::rendezvous::RvError>,
    expected_lookup: &str,
) -> String {
    use crate::rendezvous::RvError;
    match open {
        Ok(o) => o.lookup.to_ascii_lowercase(),
        Err(RvError::DaemonUnsupported) => String::new(),
        Err(e) if crate::rendezvous::is_relay_unsupported(e) => String::new(),
        Err(_) => expected_lookup.to_ascii_lowercase(),
    }
}

/// GROW-S2 (§15 M2) — decide the mint from the two publishes: the INVITE-S2 `publishInvite` and the
/// rendezvous `rendezvousOpen` (same `expires_at`). Fail-closed: no link when both fail; one
/// confirmed success is enough (a relay without rendezvous still serves the legacy self-admit, and
/// a rendezvous still works if the legacy publish failed). A confirmed open needs the daemon's
/// lookup to equal the §3 derivation computed here. A failed publish is not recorded as "nothing
/// published": its answer may have been lost after the relay stored the invite, so revoke always
/// needs the legacy revoke confirmed (or the relay's "no such invite").
pub(crate) fn decide_mint(
    publish: Result<(), String>,
    open: Result<crate::comms::RendezvousOpened, crate::rendezvous::RvError>,
    expected_lookup: &str,
) -> Result<MintDecision, MintRefused> {
    let lookup = lookup_to_record(&open, expected_lookup);
    let opened = match &open {
        Ok(o) if o.lookup.eq_ignore_ascii_case(expected_lookup) => Ok(()),
        Ok(_) => {
            Err("the comms daemon registered a different rendezvous for this invite".to_string())
        }
        Err(e) => Err(e.to_string()),
    };
    match (publish, opened) {
        (Ok(()), _) | (Err(_), Ok(())) => Ok(MintDecision { lookup }),
        (Err(p), Err(o)) => Err(MintRefused {
            message: format!("couldn't publish the invite to the relay ({p}; rendezvous: {o})"),
            lookup_to_close: lookup,
        }),
    }
}

/// The `pending.json` record for a minted invite.
pub(crate) fn minted_invite(
    group: &str,
    token: &str,
    for_handle: &str,
    created_at: u64,
    priv_key: String,
    link: String,
    decision: MintDecision,
) -> PendingInvite {
    PendingInvite {
        group: group.to_string(),
        token: token.to_string(),
        for_handle: for_handle.to_string(),
        created_at,
        priv_key,
        link,
        lookup: decision.lookup,
    }
}

/// GROW-S2 (§9 "call both") — the outcome of a revoke from its two answers: `close` is the
/// `rendezvousClose` answer (`None` when no rendezvous was recorded), `legacy` the INVITE-S2
/// revoke's. Fail-closed: both must be confirmed. A recorded lookup means a capable daemon opened a
/// rendezvous, so a daemon that now lacks the op (a downgrade) has not closed it: that fails too.
/// The relay's "no such invite" means there is no legacy record left to redeem.
pub(crate) fn revoke_outcome(
    close: Option<Result<(), crate::rendezvous::RvError>>,
    legacy: Result<(), String>,
) -> Result<(), String> {
    match close {
        None | Some(Ok(())) => {}
        Some(Err(e)) => return Err(format!("couldn't revoke the invite: {e}")),
    }
    match legacy {
        Ok(()) => Ok(()),
        Err(e) if e.contains("no such invite") => Ok(()),
        Err(e) => Err(format!("couldn't revoke the invite: {e}")),
    }
}

/// Look up a group's human name from the owner's own group list (best-effort). Used to label the
/// invitee's joined group; `None` if the daemon is unreachable or the group isn't listed. `async`
/// (the daemon IPC is async) — a hiccup here is non-fatal, the invitee just gets a generic label.
async fn group_name_of(app: &tauri::AppHandle, group: &str) -> Option<String> {
    let names = crate::comms::groups_list(app.clone()).await.ok()?;
    names
        .into_iter()
        .find(|(id, _)| id == group)
        .map(|(_, name)| name)
}

fn store_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("invites");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("pending.json"))
}

/// The outstanding invites for display and the tick's local-work check: empty when the file can't
/// be read. Anything that acts on an invite (revoke, consume) uses [`view_pending_at`] /
/// [`update_pending_at`], which fail instead.
pub(crate) fn load(app: &tauri::AppHandle) -> Vec<PendingInvite> {
    store_path(app)
        .and_then(|p| view_pending_at(&p))
        .unwrap_or_default()
}

/// Parse `pending.json`. A missing file is no invites; a file that can't be read or parsed is an
/// error, never "no invites" (a writer truncates before it writes, and the file holds private keys).
/// Call it only under [`PENDING_LOCK`].
fn read_pending(path: &std::path::Path) -> Result<Vec<PendingInvite>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("couldn't read the pending invites: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("couldn't read the pending invites: {e}")),
    }
}

/// Read `pending.json` under [`PENDING_LOCK`], so a read never sees a write in progress.
fn view_pending_at(path: &std::path::Path) -> Result<Vec<PendingInvite>, String> {
    let _guard = PENDING_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    read_pending(path)
}

/// Serializes every `pending.json` read-change-write in this process (Tauri commands and the MCP
/// tools both mint and revoke).
static PENDING_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Read, change and write `pending.json` in one step, under [`PENDING_LOCK`]. Every writer goes
/// through here, and none holds a copy across a network call, so a create that lands while a revoke
/// waits on the relay isn't overwritten, and a removed record isn't written back.
fn update_pending_at<T>(
    path: &std::path::Path,
    f: impl FnOnce(&mut Vec<PendingInvite>) -> T,
) -> Result<T, String> {
    let _guard = PENDING_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut v = read_pending(path)?;
    let out = f(&mut v);
    write_pending(path, &v)?;
    Ok(out)
}

/// Serialize the pending invites and write them owner-only (`0600`).
///
/// CORE-B-006: `pending.json` holds the plaintext ECIES PRIVATE keys of every
/// outstanding invite (`PendingInvite::priv_key`). The old `fs::write` created it
/// at the process umask (`0644`), so another local user could read those keys and
/// open the sealed relay claims they protect. Route through the shared
/// [`citrate_core_kit::fsutil`] writer, which creates the file `0600` in the
/// `open(2)` call itself.
fn write_pending(path: &std::path::Path, v: &[PendingInvite]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    citrate_core_kit::fsutil::write_secret_file(path, &bytes).map_err(|e| e.to_string())
}

/// `group_invite_create` — mint a single-use, group-bound invite + share link. No address resolution.
///
/// INVITE-S2: publishes `BLAKE3(token)` + a 14-day expiry to the relay so a token-holder can
/// SELF-ADMIT (`group_invite_redeem`) with no owner action — the raw token rides the link and never
/// reaches the relay. The link ALSO carries the CONNECT-S1 ephemeral public key (`k=`) so the
/// claim-back fallback still works. Fails CLOSED if the publish can't reach the relay (Rule 1 — a
/// link nobody can redeem is not handed out as if it worked). Records a device-local audit event (#73).
#[tauri::command]
pub async fn group_invite_create(
    app: tauri::AppHandle,
    group: String,
    for_handle: String,
) -> Result<InviteMinted, String> {
    let token = random_state();
    // CONNECT-S1 — mint an ephemeral keypair; the public half rides the link so the invitee can seal
    // their claim to it (relay stays blind), the private half stays with the owner to open claims.
    let (priv_key, pub_key) = crate::invite_seal::new_invite_keypair();
    // INVITE-S2 — publish the token HASH (never the raw token) + expiry so the invitee self-admits.
    let token_hash = blake3_token_hash(&token);
    let expires_at = now_unix_ms().saturating_add(INVITE_TTL_MS);
    let publish = crate::comms::publish_invite(&app, group.clone(), token_hash.clone(), expires_at);
    // GROW-S2 — open the relay rendezvous right after, with the same expiry (§9). The token crosses
    // to the local daemon only, as hex of its bytes; the daemon sends the relay `lookup` and
    // `token_hash`, never the token.
    let open = crate::comms::rendezvous_open(
        &app,
        group.clone(),
        crate::rendezvous::token_ipc_hex(&token),
        expires_at,
    );
    let decision = match decide_mint(publish, open, &crate::rendezvous::lookup_hex(&token)) {
        Ok(d) => d,
        Err(refused) => {
            // No link. Close an open the daemon may still hold, so it drops its secrets.
            if !refused.lookup_to_close.is_empty() {
                let _ = crate::comms::rendezvous_close(&app, refused.lookup_to_close);
            }
            return Err(refused.message);
        }
    };
    // Label the invitee's joined group with the real name when we can read it (best-effort).
    let name_hex = match group_name_of(&app, &group).await {
        Some(n) if !n.is_empty() => hex::encode(n.as_bytes()),
        _ => String::new(),
    };
    let link = if name_hex.is_empty() {
        format!("citrate://invite?g={group}&t={token}&k={pub_key}")
    } else {
        format!("citrate://invite?g={group}&t={token}&k={pub_key}&n={name_hex}")
    };
    let record = minted_invite(
        &group,
        &token,
        &for_handle,
        now_unix(),
        priv_key,
        link.clone(),
        decision,
    );
    update_pending_at(&store_path(&app)?, |v| v.push(record))?;
    // #73 — the inviter's audit copy (provable intent; the relay tally is authoritative for scoring).
    append_referral(
        &app,
        ReferralEvent {
            role: "inviter".into(),
            event: "invited".into(),
            group,
            group_name: group_name_hex_decode(&name_hex),
            token_hash,
            for_handle,
            ts: now_unix(),
        },
    );
    Ok(InviteMinted { token, link })
}

/// `group_invite_redeem` — INVITEE: join from a `citrate://invite?g&t&k[&n]` link (the paste box and
/// the pending-invite banner). GROW-S2: asks the member daemon to join through the relay rendezvous
/// (`rendezvousJoin`), and falls back to the INVITE-S2 self-admit ([`redeem_link`]) only when the
/// relay doesn't speak rendezvous or has no record for the invite (§12). Returns which path ran and
/// the daemon's join view; never a fabricated "joined" (Rule 1).
#[tauri::command]
pub async fn group_invite_redeem(
    app: tauri::AppHandle,
    link: String,
) -> Result<crate::rendezvous::JoinRedeemView, String> {
    // GROW-S2 (§3, §12): every invite link joins through the relay rendezvous first, like the v2
    // links. Once a rendezvous exists for a token the relay refuses the legacy RedeemInvite in
    // every state (§4 rev 6), so the legacy self-admit runs only on the §12 fallback, inside
    // `join_opened`.
    crate::blocking::off_main(move || {
        let group = link_param(&link, "g").ok_or("invite link missing group")?;
        let token = link_param(&link, "t").ok_or("invite link missing token")?;
        let name_hex = link_param(&link, "n").unwrap_or_default();
        let out = crate::rendezvous::join_opened(&app, &group, &token, &name_hex, &link)
            .map(|(view, _forget)| view);
        crate::rendezvous::tick_soon(&app);
        out
    })
    .await
}

/// The local display name for a group joined from an invite: the link's hex `n` decoded, or
/// "Invited group" when it is absent or not UTF-8. Shared by the legacy redeem and the GROW-S2
/// rendezvous join.
pub(crate) fn invite_display_name(n_hex: &str) -> String {
    hex::decode(n_hex)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Invited group".to_string())
}

/// The INVITE-S2 self-admit body, the §12 legacy fallback of [`group_invite_redeem`] and the v2
/// invite-link redeem (`join::join_redeem`, which rebuilds this link from an opened envelope in
/// Rust); both reach it through `rendezvous::join_opened`.
pub(crate) fn redeem_link(app: &tauri::AppHandle, link: &str) -> Result<(), String> {
    let group = link_param(link, "g").ok_or("invite link missing group")?;
    let token = link_param(link, "t").ok_or("invite link missing token")?;
    let name = invite_display_name(link_param(link, "n").as_deref().unwrap_or(""));
    let wire = redeem_token_wire(&token)?;
    crate::comms::redeem_invite(app, group.clone(), wire, name.clone())?;
    append_referral(
        app,
        ReferralEvent {
            role: "joiner".into(),
            event: "joined".into(),
            group,
            group_name: name,
            token_hash: blake3_token_hash(&token),
            for_handle: String::new(),
            ts: now_unix(),
        },
    );
    Ok(())
}

/// Length of an invite token as minted ([`random_state`]: 32 bytes, unpadded base64url).
const INVITE_TOKEN_LEN: usize = 43;

/// The token in the form the member daemon's `RedeemInvite` expects: hex of the bytes the relay
/// hashes. The relay looks the invite up by `BLAKE3(bytes)` and the owner published
/// [`blake3_token_hash`]`(token)` = `BLAKE3(token's UTF-8 bytes)`, so the bytes are the token string
/// itself, hex-encoded (not the base64url-decoded value, which would hash to a different record).
/// Validates the minted shape first (43 base64url chars that decode to 32 bytes). Errors never
/// contain the token.
pub(crate) fn redeem_token_wire(token: &str) -> Result<String, String> {
    use base64::Engine as _;
    let shaped = token.len() == INVITE_TOKEN_LEN
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token)
            .map(|raw| raw.len() == 32)
            .unwrap_or(false);
    if !shaped {
        return Err("invite link token is malformed; ask for a new link".to_string());
    }
    Ok(hex::encode(token.as_bytes()))
}

/// Decode a hex-encoded group name back to a display string (empty on any failure).
fn group_name_hex_decode(name_hex: &str) -> String {
    if name_hex.is_empty() {
        return String::new();
    }
    hex::decode(name_hex)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// #73 — the device-local referral audit log.
//
// The comms RELAY records the authoritative referral attribution (inviter→joiner) on every
// successful redeem — that is the source of truth for first-airdrop scoring (server-blind, addresses
// only). This is the MEMBER's own audit copy: what invites *I* minted, and which groups *I* joined via
// an invite. It is a personal ledger (provable intent + the same `token_hash` the relay keys on), not
// a competing tally — so it never fabricates a "someone joined" the owner's device can't actually
// observe (Rule 1). Exportable so the member can hand over their own record during distribution.
// ---------------------------------------------------------------------------

/// One entry in the local referral ledger.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReferralEvent {
    /// This device's part: `inviter` (I minted an invite) | `joiner` (I self-admitted via one).
    pub role: String,
    /// `invited` | `joined` | `revoked`.
    pub event: String,
    /// The group id the invite is for.
    pub group: String,
    /// The group's human name when known (may be empty).
    pub group_name: String,
    /// `BLAKE3(token)` hex — ties this row to the relay's authoritative attribution record.
    pub token_hash: String,
    /// The handle the invite was labelled for (inviter rows only; a label, never a resolved address).
    pub for_handle: String,
    /// Unix seconds.
    pub ts: u64,
}

fn referral_log_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("invites");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("referral-log.json"))
}

fn load_referrals(app: &tauri::AppHandle) -> Vec<ReferralEvent> {
    match referral_log_path(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
    {
        Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Append one audit event (best-effort — a logging hiccup must never fail the invite/redeem itself).
/// Owner-only (`0600`): the log carries the labelled @handle and group ids.
fn append_referral(app: &tauri::AppHandle, ev: ReferralEvent) {
    let mut log = load_referrals(app);
    log.push(ev);
    if let Ok(path) = referral_log_path(app) {
        if let Ok(bytes) = serde_json::to_vec_pretty(&log) {
            let _ = citrate_core_kit::fsutil::write_secret_file(&path, &bytes);
        }
    }
}

/// `group_referral_log` — the device-local referral ledger (newest last). The member's own audit copy;
/// the relay's `referral_tally()` is authoritative for airdrop scoring.
#[tauri::command]
pub async fn group_referral_log(app: tauri::AppHandle) -> Result<Vec<ReferralEvent>, String> {
    Ok(load_referrals(&app))
}

/// `group_referral_export` — the referral ledger as pretty JSON, for the member to save (audit copy).
/// Honest empty array `[]` when nothing has been recorded (never a fabricated row, Rule 1).
#[tauri::command]
pub async fn group_referral_export(app: tauri::AppHandle) -> Result<String, String> {
    serde_json::to_string_pretty(&load_referrals(&app)).map_err(|e| e.to_string())
}

/// A claim recovered from the server-blind inbox: the invitee volunteered this address under the
/// one-time token. Shown to the owner in the Requests inbox; approving runs the normal add path.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaimView {
    pub group: String,
    pub token: String,
    pub address: String,
}

/// `group_invite_submit_claim` — INVITEE side (CONNECT-S1). Parse an invite link (`g`, `t`, `k`), seal
/// `{group, token, address}` (address = this device's comms identity) to the link's ephemeral key, and
/// submit it to the relay's server-blind claims-inbox keyed by the token hash. Replaces the manual
/// "copy the claim and DM it back" round-trip. Honest error if the daemon/relay is unreachable.
#[tauri::command]
pub async fn group_invite_submit_claim(app: tauri::AppHandle, link: String) -> Result<(), String> {
    let group = link_param(&link, "g").ok_or("invite link missing group")?;
    let token = link_param(&link, "t").ok_or("invite link missing token")?;
    let key = link_param(&link, "k")
        .ok_or("this invite link predates one-click connect — ask for a fresh invite, or use the manual claim")?;
    let address = crate::comms::device_identity(&app)?.address;
    let claim =
        serde_json::json!({ "group": group, "token": token, "address": address }).to_string();
    let sealed = crate::invite_seal::seal_to(&key, claim.as_bytes())?;
    let th = hex::encode(crate::invite_seal::token_hash(&token));
    crate::comms::submit_claim(&app, th, hex::encode(sealed))
}

/// `group_invite_poll_claims` — OWNER side (CONNECT-S1). For each outstanding invite in a group, poll
/// the relay's claims-inbox by token hash and OPEN each sealed claim with that invite's private key.
/// Returns the volunteered claims (deduped by address) for the Requests inbox. Sealed blobs that don't
/// open (wrong invite / tampered) are skipped silently — never surfaced as a claim (Rule 1).
#[tauri::command]
pub async fn group_invite_poll_claims(
    app: tauri::AppHandle,
    group: String,
) -> Result<Vec<ClaimView>, String> {
    let invites: Vec<PendingInvite> = load(&app)
        .into_iter()
        .filter(|i| i.group == group)
        .collect();
    let mut out: Vec<ClaimView> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for inv in invites {
        if inv.priv_key.is_empty() {
            continue; // pre-S1 invite → no key to open with (manual path)
        }
        let th = hex::encode(crate::invite_seal::token_hash(&inv.token));
        let ciphertexts = match crate::comms::poll_claims(&app, th) {
            Ok(c) => c,
            Err(_) => continue, // relay/daemon unavailable for this token — honest skip, never fabricate
        };
        for ct_hex in ciphertexts {
            let Ok(sealed) = hex::decode(&ct_hex) else {
                continue;
            };
            let Ok(plain) = crate::invite_seal::open_with(&inv.priv_key, &sealed) else {
                continue;
            };
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&plain) else {
                continue;
            };
            // Only accept a claim that matches THIS invite (group + token), carrying an address.
            let g = v.get("group").and_then(|x| x.as_str()).unwrap_or("");
            let t = v.get("token").and_then(|x| x.as_str()).unwrap_or("");
            let addr = v.get("address").and_then(|x| x.as_str()).unwrap_or("");
            if g == inv.group
                && t == inv.token
                && !addr.is_empty()
                && seen.insert(addr.to_lowercase())
            {
                out.push(ClaimView {
                    group: g.to_string(),
                    token: t.to_string(),
                    address: addr.to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// Extract a query param from a `citrate://invite?...` link.
pub(crate) fn link_param(link: &str, key: &str) -> Option<String> {
    let q = link.split('?').nth(1)?;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some(key) {
            return it.next().map(|v| v.to_string());
        }
    }
    None
}

/// `group_invites` — the owner's outstanding claimable invites for a group. PBA-L4-008: returns
/// key-free [`PendingInviteView`]s; the invite private key never crosses `invoke`.
#[tauri::command]
pub async fn group_invites(
    app: tauri::AppHandle,
    group: String,
) -> Result<Vec<PendingInviteView>, String> {
    Ok(invite_views_for_group(&load(&app), &group))
}

/// `group_invite_verify_consume` — check a claim's one-time token against an outstanding invite for
/// the group; consume it (single-use) on success. The caller then adds the volunteered address via
/// the normal roster path. Returns whether the token was valid.
#[tauri::command]
pub async fn group_invite_verify_consume(
    app: tauri::AppHandle,
    group: String,
    token: String,
) -> Result<bool, String> {
    let path = store_path(&app)?;
    consume_at(
        &path,
        &group,
        &token,
        |lookup| crate::comms::rendezvous_close(&app, lookup),
        |token_hash| crate::comms::revoke_invite(&app, token_hash),
    )
}

/// The consume itself, over the `pending.json` at `path` and the two relay calls (as [`revoke_at`]).
pub(crate) fn consume_at(
    path: &std::path::Path,
    group: &str,
    token: &str,
    close: impl FnOnce(String) -> Result<(), crate::rendezvous::RvError>,
    legacy: impl FnOnce(String) -> Result<(), String>,
) -> Result<bool, String> {
    // Consuming spends the invite: close its rendezvous and revoke its legacy record first (the same
    // calls as a revoke), so no other holder of the link can still be admitted. A failure keeps the
    // record, so the approval can be retried. Only the call that removes the record returns true
    // (single-use, even for two approvals at once).
    let exists = view_pending_at(path)?
        .iter()
        .any(|i| i.group == group && i.token == token);
    if !exists {
        return Ok(false);
    }
    revoke_and_remove_at(path, group, token, close, legacy)
}

/// `group_invite_revoke` — drop an outstanding invite the owner no longer wants redeemable. Revokes
/// it on the RELAY too (INVITE-S2 `RevokeInvite`) so a leaked link can no longer self-admit — then
/// drops the local record. If the relay revoke fails (daemon down), the call errors honestly and the
/// local record is KEPT (Rule 1 — we don't report "revoked" while the link still redeems).
#[tauri::command]
pub async fn group_invite_revoke(
    app: tauri::AppHandle,
    group: String,
    token: String,
) -> Result<(), String> {
    let path = store_path(&app)?;
    revoke_at(
        &path,
        &group,
        &token,
        |lookup| crate::comms::rendezvous_close(&app, lookup),
        |token_hash| crate::comms::revoke_invite(&app, token_hash),
    )
}

/// The revoke itself, over the `pending.json` at `path` and the two relay calls (`close` sends
/// `rendezvousClose` for a lookup, `legacy` the INVITE-S2 revoke for a token hash).
pub(crate) fn revoke_at(
    path: &std::path::Path,
    group: &str,
    token: &str,
    close: impl FnOnce(String) -> Result<(), crate::rendezvous::RvError>,
    legacy: impl FnOnce(String) -> Result<(), String>,
) -> Result<(), String> {
    revoke_and_remove_at(path, group, token, close, legacy).map(|_| ())
}

/// [`revoke_at`], returning whether this call removed the record.
fn revoke_and_remove_at(
    path: &std::path::Path,
    group: &str,
    token: &str,
    close: impl FnOnce(String) -> Result<(), crate::rendezvous::RvError>,
    legacy: impl FnOnce(String) -> Result<(), String>,
) -> Result<bool, String> {
    // Read only the lookup here (under the lock, released before the network calls); the removal
    // below re-reads the file under the lock. An unreadable file fails before any relay call.
    let lookup = view_pending_at(path)?
        .into_iter()
        .find(|i| i.group == group && i.token == token)
        .map(|i| i.lookup)
        .filter(|l| !l.is_empty());
    // Close first (the daemon persists the close and resends it until the relay confirms; it also
    // drops its local secrets for it), then always the INVITE-S2 revoke, whatever the close said.
    let close = lookup.map(close);
    let legacy = legacy(blake3_token_hash(token));
    revoke_outcome(close, legacy)?;
    update_pending_at(path, |v| {
        let before = v.len();
        v.retain(|i| !(i.group == group && i.token == token));
        v.len() < before
    })
}

#[cfg(test)]
mod tests {
    // ---- GROW-S2 (Join 9, M2) ----------------------------------------------------------------
    use crate::comms::RendezvousOpened;
    use crate::rendezvous::RvError;

    const T: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
    const LOOKUP: &str = "57e96326581e57d94d5673f8a3c42b21d54aa07ba0cdf62934d73cd947cfdd13";

    fn opened(lookup: &str) -> Result<RendezvousOpened, RvError> {
        Ok(RendezvousOpened {
            lookup: lookup.into(),
            expires_at: 1,
        })
    }

    fn ok(lookup: &str) -> Result<super::MintDecision, super::MintRefused> {
        Ok(super::MintDecision {
            lookup: lookup.to_string(),
        })
    }

    #[test]
    fn mint_hands_out_the_link_when_both_publishes_succeed() {
        assert_eq!(crate::rendezvous::lookup_hex(T), LOOKUP);
        assert_eq!(
            super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP),
            ok(LOOKUP)
        );
    }

    #[test]
    fn mint_records_no_rendezvous_only_when_the_daemon_or_relay_lacks_it() {
        for open in [
            Err(RvError::Daemon("relay_unsupported".into())),
            Err(RvError::DaemonUnsupported),
        ] {
            assert_eq!(super::decide_mint(Ok(()), open, LOOKUP), ok(""));
        }
    }

    /// I6 / §9 "call both": a timeout or error on rendezvousOpen is ambiguous. The daemon may
    /// have persisted the open and keep resending it, so the derived lookup is recorded and
    /// revoke can close it.
    #[test]
    fn an_ambiguous_open_still_records_the_derived_lookup() {
        for open in [
            Err(RvError::Ipc(
                "comms daemon did not respond within 15000 ms".into(),
            )),
            Err(RvError::Ipc("connect: refused".into())),
            Err(RvError::Daemon("relay unreachable".into())),
        ] {
            assert_eq!(super::decide_mint(Ok(()), open, LOOKUP), ok(LOOKUP));
        }
    }

    #[test]
    fn open_times_out_then_revoke_still_closes_the_rendezvous() {
        let d = super::decide_mint(Ok(()), Err(RvError::Ipc("timed out".into())), LOOKUP)
            .expect("the link is handed out (the legacy publish worked)");
        let path = scratch("open-timed-out");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let closed = std::cell::RefCell::new(None);
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |l| {
                *closed.borrow_mut() = Some(l);
                Ok(())
            },
            |_| Ok(()),
        );
        assert_eq!(r, Ok(()));
        assert_eq!(closed.into_inner().as_deref(), Some(LOOKUP));
    }

    #[test]
    fn an_invite_without_a_rendezvous_is_revoked_by_the_legacy_call_alone() {
        let d = super::decide_mint(Ok(()), Err(RvError::DaemonUnsupported), LOOKUP).expect("link");
        let path = scratch("no-rendezvous");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| panic!("no rendezvous was recorded, so nothing is closed"),
            |_| Ok(()),
        );
        assert_eq!(r, Ok(()));
        assert!(tokens(&path).is_empty());
    }

    #[test]
    fn mint_hands_out_the_link_when_only_the_rendezvous_opened() {
        assert_eq!(
            super::decide_mint(Err("relay down".into()), opened(LOOKUP), LOOKUP),
            ok(LOOKUP)
        );
    }

    #[test]
    fn mint_fails_closed_when_both_publishes_fail() {
        let e = super::decide_mint(
            Err("relay down".into()),
            Err(RvError::DaemonUnsupported),
            LOOKUP,
        )
        .expect_err("no link when nothing was published");
        assert!(!e.message.contains(T), "the error never carries the token");
        assert_eq!(e.lookup_to_close, "", "nothing was opened");
        assert!(super::decide_mint(
            Err("x".into()),
            Err(RvError::Daemon("relay_unsupported".into())),
            LOOKUP
        )
        .is_err());
    }

    #[test]
    fn a_refused_mint_closes_an_open_the_daemon_may_have_kept() {
        // Both failed, but the open timed out: the daemon may hold it. No link is handed out, and
        // the caller closes the rendezvous so the daemon drops its secrets.
        let e = super::decide_mint(
            Err("x".into()),
            Err(RvError::Ipc("timed out".into())),
            LOOKUP,
        )
        .expect_err("no link");
        assert_eq!(e.lookup_to_close, LOOKUP);
    }

    #[test]
    fn a_daemon_lookup_that_doesnt_match_the_derivation_is_not_trusted_but_is_closable() {
        let other = "00".repeat(32);
        // Not counted as an open (no link on its own), but recorded so revoke closes what the
        // daemon actually holds.
        assert_eq!(
            super::decide_mint(Ok(()), opened(&other), LOOKUP),
            ok(&other)
        );
        let e = super::decide_mint(Err("x".into()), opened(&other), LOOKUP).expect_err("no link");
        assert_eq!(e.lookup_to_close, other);
        assert_eq!(
            super::decide_mint(Ok(()), opened(&LOOKUP.to_uppercase()), LOOKUP),
            ok(LOOKUP)
        );
    }

    #[test]
    fn pre_s2_pending_records_load_without_a_rendezvous() {
        let old = r#"[{"group":"g","token":"t","forHandle":"h","createdAt":1,"privKey":"aa","link":"l"}]"#;
        let v: Vec<super::PendingInvite> = serde_json::from_str(old).expect("old file loads");
        assert!(v[0].lookup.is_empty());
        // A record written by an earlier build of this branch (with `published`) still loads.
        let s2 = r#"[{"group":"g","token":"t","forHandle":"h","createdAt":1,"privKey":"aa","link":"l","lookup":"ab","published":false}]"#;
        let v2: Vec<super::PendingInvite> = serde_json::from_str(s2).expect("loads");
        assert_eq!(v2[0].lookup, "ab");
        let view = super::PendingInviteView::from(&v[0]);
        assert_eq!(view.lookup, "");
    }

    #[test]
    fn mint_opens_the_rendezvous_with_the_same_expiry_and_hex_token() {
        let src = include_str!("invites.rs");
        let body = &src[src.find("pub async fn group_invite_create").expect("fn")..];
        let body = &body[..body.find("\n}\n").expect("end")];
        let publish = body
            .find("publish_invite(&app, group.clone(), token_hash.clone(), expires_at)")
            .expect("publish");
        let open = body.find("rendezvous_open(").expect("open");
        assert!(publish < open, "rendezvousOpen right after publishInvite");
        assert!(body.contains("crate::rendezvous::token_ipc_hex(&token),\n        expires_at,"));
        assert!(body.contains("decide_mint(publish, open,"));
    }

    /// A real invite token, exactly as `group_invite_create` mints it (`random_state`: 32 random
    /// bytes, unpadded base64url, 43 chars), taken from a link in the real share format.
    fn real_token_from_link() -> String {
        let token = crate::connections::random_state();
        let link = format!(
            "citrate://invite?g={}&t={token}&k={}",
            "ab".repeat(16),
            "02".repeat(33)
        );
        super::link_param(&link, "t").expect("t= in the link")
    }

    /// The member daemon's `RedeemInvite` hex-decodes `token` and the relay recomputes
    /// `BLAKE3(bytes)` to find the record the owner published as `blake3_token_hash(token)`. So the
    /// wire form must hex-decode to exactly the bytes that were hashed at publish: the token's own
    /// UTF-8 bytes. A real 43-char base64url token must round-trip to that.
    #[test]
    fn redeem_token_wire_round_trips_a_real_token_to_the_published_hash() {
        for _ in 0..32 {
            let token = real_token_from_link();
            assert_eq!(token.len(), 43);
            let wire = super::redeem_token_wire(&token).expect("a real token is accepted");
            assert!(
                wire.chars().all(|c| c.is_ascii_hexdigit()),
                "the daemon hex-decodes it"
            );
            let bytes = hex::decode(&wire).expect("hex");
            assert_eq!(bytes, token.as_bytes());
            assert_eq!(
                hex::encode(blake3::hash(&bytes).as_bytes()),
                super::blake3_token_hash(&token),
                "the relay finds the record the owner published"
            );
        }
    }

    /// The relay contract's §13 vector (citrate-federation JOIN_GROW_S2_RELAY.md, G1 / task M0): the
    /// IPC `token` is the hex of the token's UTF-8 bytes, and BLAKE3 of those bytes is the published
    /// `token_hash`.
    #[test]
    fn redeem_token_wire_matches_the_relay_contract_vector() {
        const T: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
        const TOKEN_UTF8_HEX: &str = "41414543417751464267634943516f4c4441304f4478415245684d554652595847426b6147787764486838";
        const TOKEN_HASH: &str = "0807456e11ea80a0cabf51014bdab6bb6ae59402cacb3b323853203b178e28d3";
        assert_eq!(super::redeem_token_wire(T).as_deref(), Ok(TOKEN_UTF8_HEX));
        assert_eq!(super::blake3_token_hash(T), TOKEN_HASH);
        let bytes = hex::decode(TOKEN_UTF8_HEX).expect("hex");
        assert_eq!(hex::encode(blake3::hash(&bytes).as_bytes()), TOKEN_HASH);
    }

    /// The raw string a real token used to be sent as is NOT valid hex, which is the failure this
    /// fixes ("token must be hex" from the daemon).
    #[test]
    fn a_real_token_is_not_hex_on_its_own() {
        let token = real_token_from_link();
        assert!(hex::decode(&token).is_err() || token.len() % 2 == 1);
        assert_ne!(super::redeem_token_wire(&token).ok(), Some(token));
    }

    #[test]
    fn redeem_token_wire_refuses_malformed_tokens_without_echoing_them() {
        let good = real_token_from_link();
        let short = &good[..42];
        let long = format!("{good}A");
        let bad_char = format!("{}+", &good[..42]);
        let padded = format!("{}=", &good[..42]);
        for bad in [
            "",
            short,
            long.as_str(),
            bad_char.as_str(),
            padded.as_str(),
            "tok123",
        ] {
            let err = super::redeem_token_wire(bad).expect_err(bad);
            if !bad.is_empty() {
                assert!(!err.contains(bad), "the error never repeats the token");
            }
        }
    }

    #[test]
    fn redeem_sends_the_wire_form_not_the_raw_token() {
        let src = include_str!("invites.rs");
        let body = &src[src.find("pub(crate) fn redeem_link").expect("fn")..];
        let body = &body[..body.find("\n}\n").expect("end")];
        assert!(body.contains("redeem_token_wire(&token)"));
        assert!(!body.contains("redeem_invite(app, group.clone(), token.clone()"));
    }

    // ---- GROW-S2 review round 2: revoke outcome and pending.json writers -------------------------

    /// A private scratch `pending.json` for one test (unique per test and process).
    fn scratch(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "citrate-invites-r2-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir.join("pending.json")
    }

    fn minted(group: &str, token: &str, d: super::MintDecision) -> super::PendingInvite {
        super::minted_invite(group, token, "@h", 1, "aa".into(), "l".into(), d)
    }

    fn tokens(path: &std::path::Path) -> Vec<String> {
        super::view_pending_at(path)
            .expect("pending.json reads")
            .into_iter()
            .map(|i| i.token)
            .collect()
    }

    /// Codex P1: a publishInvite whose answer was lost may have reached the relay. A failed legacy
    /// revoke must then fail the revoke and keep the record, even though the close worked.
    #[test]
    fn a_lost_publish_answer_then_a_failed_legacy_revoke_keeps_the_invite() {
        let path = scratch("lost-publish");
        let d = super::decide_mint(
            Err("comms daemon did not respond within 15000 ms".into()),
            opened(LOOKUP),
            LOOKUP,
        )
        .expect("the rendezvous opened, so the link is handed out");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| Ok(()),
            |_| Err("relay error: connection reset".into()),
        );
        assert!(r.is_err(), "a failed legacy revoke is not a revoke: {r:?}");
        assert_eq!(tokens(&path), vec![T.to_string()], "the record is kept");
    }

    /// The relay's own "no such invite" is confirmed absence: there is nothing left to redeem through
    /// the legacy path, so the revoke completes.
    #[test]
    fn a_relay_that_has_no_such_invite_completes_the_revoke() {
        let path = scratch("absent");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| Ok(()),
            |_| Err("relay error: server error: no such invite".into()),
        );
        assert_eq!(r, Ok(()));
        assert!(tokens(&path).is_empty());
    }

    /// pysol-dev #1: the legacy revoke is sent even when the close fails, and the revoke fails closed.
    #[test]
    fn a_failed_close_still_sends_the_legacy_revoke_and_keeps_the_invite() {
        let path = scratch("close-fails");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let sent = std::cell::Cell::new(false);
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| Err(RvError::Daemon("unknown rendezvous".into())),
            |h| {
                assert_eq!(h, super::blake3_token_hash(T));
                sent.set(true);
                Ok(())
            },
        );
        assert!(sent.get(), "the INVITE-S2 revoke is always sent");
        assert!(r.is_err(), "a failed close fails the revoke: {r:?}");
        assert_eq!(tokens(&path), vec![T.to_string()]);
    }

    /// pysol-dev #2: every combination of the two answers. Success needs both confirmed (or the
    /// close unsupported, or the legacy record confirmed absent); anything else keeps the record.
    #[test]
    fn revoke_fails_closed_if_either_call_fails() {
        type Close = fn(String) -> Result<(), RvError>;
        type Legacy = fn(String) -> Result<(), String>;
        let closes: [(Close, bool); 4] = [
            (|_| Ok(()), true),
            (|_| Err(RvError::DaemonUnsupported), false),
            (|_| Err(RvError::Ipc("timed out".into())), false),
            (|_| Err(RvError::Daemon("relay unreachable".into())), false),
        ];
        let legacies: [(Legacy, bool); 4] = [
            (|_| Ok(()), true),
            (|_| Err("relay error: no such invite".into()), true),
            (
                |_| Err("comms daemon did not respond within 15000 ms".into()),
                false,
            ),
            (|_| Err("relay error: not authorized".into()), false),
        ];
        for published in [Ok(()), Err("timed out".to_string())] {
            let d = super::decide_mint(published.clone(), opened(LOOKUP), LOOKUP).expect("link");
            for (ci, (close, close_ok)) in closes.iter().enumerate() {
                for (li, (legacy, legacy_ok)) in legacies.iter().enumerate() {
                    let path = scratch(&format!("combo-{ci}-{li}"));
                    super::write_pending(&path, &[minted("g", T, d.clone())]).expect("write");
                    let r = super::revoke_at(&path, "g", T, close, legacy);
                    let want = *close_ok && *legacy_ok;
                    assert_eq!(
                        r.is_ok(),
                        want,
                        "publish {published:?}, close #{ci}, legacy #{li}: {r:?}"
                    );
                    assert_eq!(
                        tokens(&path).is_empty(),
                        want,
                        "record kept iff not revoked"
                    );
                }
            }
        }
    }

    /// Codex P2: an invite created while a revoke waits on the relay survives the revoke.
    #[test]
    fn an_invite_created_during_a_revoke_survives_it() {
        let path = scratch("create-during-revoke");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d.clone())]).expect("write");
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| {
                // A create through Tauri or MCP lands while the close is in flight.
                super::update_pending_at(&path, |v| v.push(minted("g", "new", d.clone())))
                    .expect("concurrent create");
                Ok(())
            },
            |_| Ok(()),
        );
        assert_eq!(r, Ok(()));
        assert_eq!(tokens(&path), vec!["new".to_string()]);
    }

    /// Codex P2: two overlapping revokes don't resurrect each other's record.
    #[test]
    fn overlapping_revokes_dont_resurrect_a_removed_invite() {
        let path = scratch("revoke-revoke");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d.clone()), minted("g", "b", d)])
            .expect("write");
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| super::revoke_at(&path, "g", "b", |_| Ok(()), |_| Ok(())).map_err(RvError::Daemon),
            |_| Ok(()),
        );
        assert_eq!(r, Ok(()));
        assert!(
            tokens(&path).is_empty(),
            "both revoked: {:?}",
            tokens(&path)
        );
    }

    /// Every writer goes through the shared lock: parallel creates all land.
    #[test]
    fn parallel_pending_writers_dont_lose_updates() {
        let path = scratch("parallel");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|n| {
                let (path, d, barrier) = (path.clone(), d.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    for k in 0..8 {
                        super::update_pending_at(&path, |v| {
                            v.push(minted("g", &format!("t{n}-{k}"), d.clone()))
                        })
                        .expect("write");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("writer thread");
        }
        assert_eq!(tokens(&path).len(), 16 * 8);
    }

    /// Codex round 3 (P1): a writer truncates `pending.json` before it writes. A revoke whose lookup
    /// read lands in that window must wait for the write, not read "no invites" and skip the close.
    #[test]
    fn a_revoke_whose_lookup_read_overlaps_a_write_still_closes_the_rendezvous() {
        let path = scratch("overlap");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let full = serde_json::to_vec_pretty(&[minted("g", T, d)]).expect("json");
        // A create holds the lock and has written half the file.
        let guard = super::PENDING_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::fs::write(&path, &full[..full.len() / 2]).expect("partial");
        let closed = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let (p2, c2) = (path.clone(), closed.clone());
        let revoke = std::thread::spawn(move || {
            super::revoke_at(
                &p2,
                "g",
                T,
                |l| {
                    *c2.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(l);
                    Ok(())
                },
                |_| Ok(()),
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        std::fs::write(&path, &full).expect("the write completes");
        drop(guard);
        let r = revoke.join().expect("revoke thread");
        assert_eq!(r, Ok(()));
        assert_eq!(
            closed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_deref(),
            Some(LOOKUP),
            "the rendezvous is closed"
        );
        assert!(tokens(&path).is_empty());
    }

    /// Codex round 3 (P1): an unreadable `pending.json` is not "no invites". Revoke fails before
    /// either relay call, and the file is left as it is.
    #[test]
    fn a_revoke_over_an_unreadable_pending_file_fails_without_calling_the_relay() {
        let path = scratch("corrupt-revoke");
        std::fs::write(&path, b"[{\"group\":\"g\",\"tok").expect("corrupt");
        let calls = std::cell::Cell::new(0);
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
        );
        assert!(r.is_err(), "an unreadable file fails the revoke: {r:?}");
        assert_eq!(calls.get(), 0, "no relay call on an unreadable file");
        assert_eq!(
            std::fs::read(&path).expect("still there"),
            b"[{\"group\":\"g\",\"tok"
        );
    }

    /// An unreadable `pending.json` is never overwritten by a writer (it holds invite private keys).
    #[test]
    fn a_writer_never_overwrites_an_unreadable_pending_file() {
        let path = scratch("corrupt-write");
        std::fs::write(&path, b"not json").expect("corrupt");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let r = super::update_pending_at(&path, |v| v.push(minted("g", T, d)));
        assert!(r.is_err(), "a writer refuses an unreadable file");
        assert_eq!(std::fs::read(&path).expect("still there"), b"not json");
        assert!(super::view_pending_at(&path).is_err());
        // A missing file is an empty list, not an error.
        let fresh = scratch("missing");
        assert_eq!(super::view_pending_at(&fresh).map(|v| v.len()), Ok(0));
    }

    /// Tripwire: `pending.json` is only read under `PENDING_LOCK` (inside `update_pending_at` and
    /// `view_pending_at`).
    #[test]
    fn pending_json_is_only_read_under_the_lock() {
        let src = include_str!("invites.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests {").expect("tests")];
        assert_eq!(
            prod.matches("read_pending(").count(),
            3,
            "read_pending is defined once and called only in update_pending_at and view_pending_at"
        );
        for f in ["fn update_pending_at", "fn view_pending_at"] {
            let body = &prod[prod.find(f).expect(f)..];
            let body = &body[..body.find("\n}\n").expect("end")];
            assert!(body.contains("PENDING_LOCK"), "{f} takes the lock");
            assert!(body.contains("read_pending("), "{f} reads");
        }
    }

    /// Codex round 4 (P1): approving a claim consumes the invite. Its rendezvous is closed and its
    /// legacy record revoked first, so another holder of the same link can't still be admitted.
    #[test]
    fn consuming_a_claim_closes_the_rendezvous_and_revokes_the_legacy_record() {
        let path = scratch("consume");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let closed = std::cell::RefCell::new(None);
        let revoked = std::cell::RefCell::new(None);
        let r = super::consume_at(
            &path,
            "g",
            T,
            |l| {
                *closed.borrow_mut() = Some(l);
                Ok(())
            },
            |h| {
                *revoked.borrow_mut() = Some(h);
                Ok(())
            },
        );
        assert_eq!(r, Ok(true));
        assert_eq!(closed.into_inner().as_deref(), Some(LOOKUP));
        assert_eq!(revoked.into_inner(), Some(super::blake3_token_hash(T)));
        assert!(tokens(&path).is_empty());
    }

    /// A consume whose close fails keeps the record, so the approval can be retried (and revoked).
    #[test]
    fn a_consume_whose_close_fails_keeps_a_retryable_record() {
        let path = scratch("consume-fails");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::consume_at(
            &path,
            "g",
            T,
            |_| Err(RvError::Ipc("timed out".into())),
            |_| Ok(()),
        );
        assert!(
            r.is_err(),
            "the claim isn't accepted while the rendezvous may be open"
        );
        assert_eq!(tokens(&path), vec![T.to_string()]);
    }

    /// Two approvals of the same claim at once: only one is accepted (the invite is single-use).
    #[test]
    fn two_overlapping_consumes_accept_the_token_once() {
        let path = scratch("consume-twice");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let inner = std::cell::Cell::new(None);
        let outer = super::consume_at(
            &path,
            "g",
            T,
            |_| {
                inner.set(Some(super::consume_at(
                    &path,
                    "g",
                    T,
                    |_| Ok(()),
                    |_| Ok(()),
                )));
                Ok(())
            },
            |_| Ok(()),
        );
        let inner = inner.into_inner().expect("ran");
        assert_eq!((inner, outer), (Ok(true), Ok(false)));
    }

    /// A token with no outstanding invite is not valid, and nothing is sent.
    #[test]
    fn consuming_an_unknown_token_sends_nothing() {
        let path = scratch("consume-unknown");
        let calls = std::cell::Cell::new(0);
        let r = super::consume_at(
            &path,
            "g",
            T,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
        );
        assert_eq!(r, Ok(false));
        assert_eq!(calls.get(), 0);
    }

    /// Codex round 4 (P1): a recorded lookup means a capable daemon opened a rendezvous. A daemon that
    /// now says it doesn't know the op (a downgrade) can't have closed it, so the revoke fails.
    #[test]
    fn a_recorded_rendezvous_with_an_unsupported_close_keeps_the_invite() {
        let path = scratch("downgrade");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::revoke_at(
            &path,
            "g",
            T,
            |_| Err(RvError::DaemonUnsupported),
            |_| Ok(()),
        );
        assert!(r.is_err(), "{r:?}");
        assert_eq!(tokens(&path), vec![T.to_string()]);
    }

    /// Tripwire: an invite record is only ever removed by `revoke_and_remove_at` (close + legacy
    /// revoke first), behind both revoke and consume.
    #[test]
    fn invite_records_are_only_removed_by_revoke_at() {
        let src = include_str!("invites.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests {").expect("tests")];
        assert_eq!(prod.matches(".retain(").count(), 1, "one removal site");
        let body = &prod[prod
            .find("fn revoke_and_remove_at")
            .expect("revoke_and_remove_at")..];
        let body = &body[..body.find("\n}\n").expect("end")];
        assert!(body.contains(".retain("));
    }

    /// Tripwire: `pending.json` is only written through `update_pending_at` (one lock, read and write
    /// together), never from a copy loaded before a network call.
    #[test]
    fn pending_json_is_only_written_through_update_pending_at() {
        let src = include_str!("invites.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests {").expect("tests")];
        let writes = prod.matches("write_pending(").count();
        assert_eq!(
            writes, 2,
            "write_pending is defined once and called once, inside update_pending_at"
        );
        let upd = &prod[prod
            .find("fn update_pending_at")
            .expect("update_pending_at")..];
        let upd = &upd[..upd.find("\n}\n").expect("end")];
        assert!(upd.contains("write_pending("));
        assert!(!prod.contains("fn save("), "no load-then-save helper");
    }

    // Pure helper: the invite link format the invitee's app parses.
    #[test]
    fn invite_link_shape() {
        let link = format!("citrate://invite?g={}&t={}", "grp_abc", "tok123");
        assert!(link.starts_with("citrate://invite?"));
        assert!(link.contains("g=grp_abc"));
        assert!(link.contains("t=tok123"));
    }

    /// INVITE-S2 parity: the token hash the owner publishes MUST be `BLAKE3(token)` (hex, 32 bytes),
    /// byte-for-byte what `comms-relay::redeem_invite` recomputes from the invitee's raw token —
    /// otherwise self-admit never matches. This pins the algorithm + shape.
    #[test]
    fn blake3_token_hash_matches_relay() {
        let h = super::blake3_token_hash("tok-abc");
        assert_eq!(h.len(), 64, "32 bytes as hex");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h, hex::encode(blake3::hash(b"tok-abc").as_bytes()));
        assert_eq!(
            super::blake3_token_hash("tok-abc"),
            super::blake3_token_hash("tok-abc")
        );
        assert_ne!(
            super::blake3_token_hash("tok-abc"),
            super::blake3_token_hash("tok-abd")
        );
    }

    /// The publish token hash (BLAKE3) is deliberately DISTINCT from the CONNECT-S1 claims-inbox key
    /// (`invite_seal::token_hash`, SHA-256) — mixing them would make self-admit silently fail to match.
    #[test]
    fn publish_hash_differs_from_claims_inbox_hash() {
        assert_ne!(
            super::blake3_token_hash("same-token"),
            hex::encode(crate::invite_seal::token_hash("same-token"))
        );
    }

    /// The group-name hex label round-trips (owner encodes it into the link; the invitee decodes it
    /// as the local group label), and any garbage decodes to empty rather than panicking.
    #[test]
    fn group_name_hex_round_trips() {
        let name = "Aperture Science 🧪";
        let encoded = hex::encode(name.as_bytes());
        assert_eq!(super::group_name_hex_decode(&encoded), name);
        assert_eq!(super::group_name_hex_decode(""), "");
        assert_eq!(super::group_name_hex_decode("zznothex"), "");
    }

    /// CORE-B-006 tripwire: the pending-invite store — which holds the plaintext
    /// PBA-L4-008: what `group_invites` hands the webview carries no invite private key.
    #[test]
    fn pba_l4_008_group_invites_view_never_carries_the_private_key() {
        let invites = vec![super::PendingInvite {
            group: "grp_abc".into(),
            token: "tok123".into(),
            for_handle: "@alice".into(),
            created_at: 1000,
            priv_key: "5ec2e7ba5eba11deadbeefc0ffee0123456789abcdef0123456789abcdef0123".into(),
            link: "citrate://invite?g=grp_abc&t=tok123&k=pub".into(),
            lookup: String::new(),
        }];
        let views = super::invite_views_for_group(&invites, "grp_abc");
        assert_eq!(views.len(), 1);
        let json = serde_json::to_string(&views).unwrap();
        assert!(
            !json.contains("5ec2e7ba5eba11"),
            "private key leaked: {json}"
        );
        assert!(
            !json.to_lowercase().contains("priv"),
            "no private-key field: {json}"
        );
        assert!(json.contains("tok123") && json.contains("@alice"));
        assert!(super::invite_views_for_group(&invites, "other").is_empty());
    }

    /// ECIES private keys of outstanding invites — must be written owner-only
    /// (no group/other read bits). Fails on the old `fs::write` (umask `0644`).
    #[cfg(unix)]
    #[test]
    fn pending_invites_are_written_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("citrate-invites-b006-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pending.json");
        let invites = vec![super::PendingInvite {
            group: "grp_abc".into(),
            token: "tok123".into(),
            for_handle: "@alice".into(),
            created_at: 1000,
            priv_key: "deadbeef".into(),
            link: "citrate://invite?g=grp_abc&t=tok123&k=pub".into(),
            lookup: String::new(),
        }];
        super::write_pending(&path, &invites).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "pending.json holds invite private keys — must not be group/other-readable (mode {mode:o})"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
