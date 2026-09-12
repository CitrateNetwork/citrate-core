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
const INVITE_TTL_MS: u64 = 14 * 24 * 60 * 60 * 1000;

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
}

/// The link + token returned to the owner to share (DM to the @handle on the platform).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InviteMinted {
    pub token: String,
    pub link: String,
}

fn now_unix() -> u64 {
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
fn blake3_token_hash(token: &str) -> String {
    hex::encode(blake3::hash(token.as_bytes()).as_bytes())
}

/// Look up a group's human name from the owner's own group list (best-effort). Used to label the
/// invitee's joined group; `None` if the daemon is unreachable or the group isn't listed. `async`
/// (the daemon IPC is async) — a hiccup here is non-fatal, the invitee just gets a generic label.
async fn group_name_of(app: &tauri::AppHandle, group: &str) -> Option<String> {
    let names = crate::comms::groups_list(app.clone()).await.ok()?;
    names.into_iter().find(|(id, _)| id == group).map(|(_, name)| name)
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

fn load(app: &tauri::AppHandle) -> Vec<PendingInvite> {
    match store_path(app).ok().and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn save(app: &tauri::AppHandle, v: &[PendingInvite]) -> Result<(), String> {
    let p = store_path(app)?;
    write_pending(&p, v)
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
    crate::comms::publish_invite(&app, group.clone(), token_hash.clone(), expires_at)?;
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
    let mut invites = load(&app);
    invites.push(PendingInvite {
        group: group.clone(),
        token: token.clone(),
        for_handle: for_handle.clone(),
        created_at: now_unix(),
        priv_key,
        link: link.clone(),
    });
    save(&app, &invites)?;
    // #73 — the inviter's audit copy (provable intent; the relay tally is authoritative for scoring).
    append_referral(&app, ReferralEvent {
        role: "inviter".into(),
        event: "invited".into(),
        group,
        group_name: group_name_hex_decode(&name_hex),
        token_hash,
        for_handle,
        ts: now_unix(),
    });
    Ok(InviteMinted { token, link })
}

/// `group_invite_redeem` — INVITEE self-admit (INVITE-S2). Parse the link (`g`, `t`, optional `n`),
/// then self-admit into the group by external commit with the RAW token — no owner action, even if
/// the owner is offline. Honest error if the daemon/relay is unreachable, the token is spent/expired,
/// or the invite was revoked (Rule 1 — never a fabricated "joined"). Records the joiner's audit copy (#73).
#[tauri::command]
pub async fn group_invite_redeem(app: tauri::AppHandle, link: String) -> Result<(), String> {
    let group = link_param(&link, "g").ok_or("invite link missing group")?;
    let token = link_param(&link, "t").ok_or("invite link missing token")?;
    let name = link_param(&link, "n")
        .and_then(|h| hex::decode(h).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Invited group".to_string());
    crate::comms::redeem_invite(&app, group.clone(), token.clone(), name.clone())?;
    append_referral(&app, ReferralEvent {
        role: "joiner".into(),
        event: "joined".into(),
        group,
        group_name: name,
        token_hash: blake3_token_hash(&token),
        for_handle: String::new(),
        ts: now_unix(),
    });
    Ok(())
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
    match referral_log_path(app).ok().and_then(|p| std::fs::read(p).ok()) {
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
    let claim = serde_json::json!({ "group": group, "token": token, "address": address }).to_string();
    let sealed = crate::invite_seal::seal_to(&key, claim.as_bytes())?;
    let th = hex::encode(crate::invite_seal::token_hash(&token));
    crate::comms::submit_claim(&app, th, hex::encode(sealed))
}

/// `group_invite_poll_claims` — OWNER side (CONNECT-S1). For each outstanding invite in a group, poll
/// the relay's claims-inbox by token hash and OPEN each sealed claim with that invite's private key.
/// Returns the volunteered claims (deduped by address) for the Requests inbox. Sealed blobs that don't
/// open (wrong invite / tampered) are skipped silently — never surfaced as a claim (Rule 1).
#[tauri::command]
pub async fn group_invite_poll_claims(app: tauri::AppHandle, group: String) -> Result<Vec<ClaimView>, String> {
    let invites: Vec<PendingInvite> = load(&app).into_iter().filter(|i| i.group == group).collect();
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
            let Ok(sealed) = hex::decode(&ct_hex) else { continue };
            let Ok(plain) = crate::invite_seal::open_with(&inv.priv_key, &sealed) else { continue };
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&plain) else { continue };
            // Only accept a claim that matches THIS invite (group + token), carrying an address.
            let g = v.get("group").and_then(|x| x.as_str()).unwrap_or("");
            let t = v.get("token").and_then(|x| x.as_str()).unwrap_or("");
            let addr = v.get("address").and_then(|x| x.as_str()).unwrap_or("");
            if g == inv.group && t == inv.token && !addr.is_empty() && seen.insert(addr.to_lowercase()) {
                out.push(ClaimView { group: g.to_string(), token: t.to_string(), address: addr.to_string() });
            }
        }
    }
    Ok(out)
}

/// Extract a query param from a `citrate://invite?...` link.
fn link_param(link: &str, key: &str) -> Option<String> {
    let q = link.split('?').nth(1)?;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some(key) {
            return it.next().map(|v| v.to_string());
        }
    }
    None
}

/// `group_invites` — the owner's outstanding claimable invites for a group.
#[tauri::command]
pub async fn group_invites(app: tauri::AppHandle, group: String) -> Result<Vec<PendingInvite>, String> {
    Ok(load(&app).into_iter().filter(|i| i.group == group).collect())
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
    let mut invites = load(&app);
    let before = invites.len();
    invites.retain(|i| !(i.group == group && i.token == token));
    let consumed = invites.len() < before;
    if consumed {
        save(&app, &invites)?;
    }
    Ok(consumed)
}

/// `group_invite_revoke` — drop an outstanding invite the owner no longer wants redeemable. Revokes
/// it on the RELAY too (INVITE-S2 `RevokeInvite`) so a leaked link can no longer self-admit — then
/// drops the local record. If the relay revoke fails (daemon down), the call errors honestly and the
/// local record is KEPT (Rule 1 — we don't report "revoked" while the link still redeems).
#[tauri::command]
pub async fn group_invite_revoke(app: tauri::AppHandle, group: String, token: String) -> Result<(), String> {
    crate::comms::revoke_invite(&app, blake3_token_hash(&token))?;
    let mut invites = load(&app);
    invites.retain(|i| !(i.group == group && i.token == token));
    save(&app, &invites)
}

#[cfg(test)]
mod tests {
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
        assert_eq!(super::blake3_token_hash("tok-abc"), super::blake3_token_hash("tok-abc"));
        assert_ne!(super::blake3_token_hash("tok-abc"), super::blake3_token_hash("tok-abd"));
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
