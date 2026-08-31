//! Group claimable invites (ADR-2026-08-30 D4).
//!
//! Invite-by-@handle without a directory: the owner mints a claimable invite (group + one-time
//! token), stored device-local, and shares its link to the person over the social platform (the
//! rendezvous is the DM — exactly where the @handle relationship lives). Citrate NEVER resolves a
//! handle to an address. The invitee opens the link and, on accept, VOLUNTEERS their address in a
//! claim (consent, D4); the owner verifies the one-time token and adds them via the normal
//! roster path — the invitee's address becomes known only because they joined.
//!
//! (A fully-automated redemption — the invitee self-claiming at the relay — needs a relay
//! claims-inbox in the comms daemon; this is the client-side flow that works today.)
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::connections::random_state;

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
    let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(p, bytes).map_err(|e| e.to_string())
}

/// `group_invite_create` — mint a claimable invite for a group + share link. No address resolution.
#[tauri::command]
pub fn group_invite_create(
    app: tauri::AppHandle,
    group: String,
    for_handle: String,
) -> Result<InviteMinted, String> {
    let token = random_state();
    // CONNECT-S1 — mint an ephemeral keypair; the public half rides the link so the invitee can seal
    // their claim to it (relay stays blind), the private half stays with the owner to open claims.
    let (priv_key, pub_key) = crate::invite_seal::new_invite_keypair();
    let link = format!("citrate://invite?g={group}&t={token}&k={pub_key}");
    let mut invites = load(&app);
    invites.push(PendingInvite {
        group,
        token: token.clone(),
        for_handle,
        created_at: now_unix(),
        priv_key,
        link: link.clone(),
    });
    save(&app, &invites)?;
    Ok(InviteMinted { token, link })
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
pub fn group_invite_submit_claim(app: tauri::AppHandle, link: String) -> Result<(), String> {
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
pub fn group_invite_poll_claims(app: tauri::AppHandle, group: String) -> Result<Vec<ClaimView>, String> {
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
pub fn group_invites(app: tauri::AppHandle, group: String) -> Result<Vec<PendingInvite>, String> {
    Ok(load(&app).into_iter().filter(|i| i.group == group).collect())
}

/// `group_invite_verify_consume` — check a claim's one-time token against an outstanding invite for
/// the group; consume it (single-use) on success. The caller then adds the volunteered address via
/// the normal roster path. Returns whether the token was valid.
#[tauri::command]
pub fn group_invite_verify_consume(
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

/// `group_invite_revoke` — drop an outstanding invite the owner no longer wants claimable.
#[tauri::command]
pub fn group_invite_revoke(app: tauri::AppHandle, group: String, token: String) -> Result<(), String> {
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
}
