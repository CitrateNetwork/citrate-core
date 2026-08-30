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
    let link = format!("citrate://invite?g={group}&t={token}");
    let mut invites = load(&app);
    invites.push(PendingInvite {
        group,
        token: token.clone(),
        for_handle,
        created_at: now_unix(),
    });
    save(&app, &invites)?;
    Ok(InviteMinted { token, link })
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
