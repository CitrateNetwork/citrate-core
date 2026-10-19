//! v2 invite links (server-blind envelope): the Tauri command surface over
//! [`citrate_core_kit::join`] (GROW-S1 task 10).
//!
//! The inviter mints a short code at the citrate.ai join service with their auth.citrate.ai
//! session (the kit [`AuthState`] bearer, never handed to the webview), seals the relay invite in
//! Rust, and gets back one share URL `https://citrate.ai/join/<code>#<K>`. The invitee's app
//! resolves the code, verifies the signature against the pinned kids, opens the envelope, and holds
//! the relay invite in Rust until the member clicks Join (`join_redeem`), which runs the existing
//! self-admit path ([`crate::invites::redeem_link`]).
//!
//! What crosses `invoke`: [`MintView`] (code, kind, status, expiry, and the final share URL, the only
//! value that contains `K`), [`ResolvedView`] (signed display data and two flags), and
//! [`JoinCodeEntry`] rows. Never `K` on its own, the envelope, the relay token or the bearer.
//!
//! Data sources (Rule 7): `https://www.citrate.ai/api/join` (citrate-landing join service: POST
//! reserve, PUT attach, GET resolve, DELETE revoke, GET mine) and `/api/join/jwks` (pinned kids
//! only); the relay invite link comes from `group_invite_create`; redeem goes to the comms relay
//! through the existing `group_invite_redeem` body.
//!
//! UI wiring is tasks 11 (inviter) and 12 (invitee); this module only adds the commands.

use citrate_core_kit::join::{
    InvitePayload, JoinClient, JoinCodeEntry, JoinConfig, JoinError, JoinHttp, MintRequest,
    MintView, ResolvedView,
};
use citrate_core_kit::oidc::AuthState;
use tauri::Manager;

/// Managed state: the process-wide join client (in-memory pending attaches + opened invites).
pub struct JoinState(pub JoinClient);

/// Production join client: the fixed citrate.ai endpoints + the kit's redirect-free join HTTP
/// client ([`JoinHttp`]: a 3xx is refused, so the bearer is never replayed elsewhere).
pub fn build_join_state() -> JoinState {
    JoinState(JoinClient::new(
        JoinConfig::production(),
        Box::new(JoinHttp),
    ))
}

fn err(e: JoinError) -> String {
    e.to_string()
}

fn with_states<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&JoinClient, &AuthState) -> Result<T, JoinError>,
) -> Result<T, String> {
    let join = app
        .try_state::<JoinState>()
        .ok_or_else(|| "internal: managed state unavailable".to_string())?;
    let auth = app
        .try_state::<AuthState>()
        .ok_or_else(|| "internal: managed state unavailable".to_string())?;
    f(&join.0, &auth).map_err(err)
}

/// `join_mint` — reserve a code and, for `direct`, seal `invite_link` (the
/// `citrate://invite?…` link from `group_invite_create`) and attach it. Returns the share URL once
/// attached; a reserved-but-unattached direct code comes back `status: "pending"` with
/// `attachError`, to retry with `join_attach`.
#[tauri::command]
pub async fn join_mint(
    app: tauri::AppHandle,
    request: MintRequest,
    invite_link: Option<String>,
) -> Result<MintView, String> {
    crate::blocking::off_main(move || {
        let invite = invite_link
            .as_deref()
            .map(InvitePayload::from_invite_link)
            .transpose()
            .map_err(err)?;
        with_states(&app, |join, auth| {
            join.mint(&auth.0, &request, invite.as_ref())
        })
    })
    .await
}

/// `join_attach` — retry attaching the sealed envelope of a pending direct code this app minted
/// (the identical body, so a repeat is idempotent at the service).
#[tauri::command]
pub async fn join_attach(app: tauri::AppHandle, code: String) -> Result<MintView, String> {
    crate::blocking::off_main(move || with_states(&app, |join, auth| join.attach(&auth.0, &code)))
        .await
}

/// `join_resolve` — resolve and verify an invite link (`https://citrate.ai/join/<code>#K`,
/// `citrate://join/<code>#K` or a bare code). Signed display data only; a direct invite that opened
/// is held in Rust for `join_redeem`. Resolve is not redeem: the service allows a code up to its
/// `maxResolves` (direct 1..=50, default 50) and the relay redeem is the single-use step, but each
/// call counts, so resolve on the member's action. The JWKS is checked before a resolve is spent.
#[tauri::command]
pub async fn join_resolve(app: tauri::AppHandle, link: String) -> Result<ResolvedView, String> {
    crate::blocking::off_main(move || with_states(&app, |join, _auth| join.resolve(&link))).await
}

/// `join_redeem` — the member's explicit Join for a resolved direct invite: hands the opened relay
/// invite (held in Rust since `join_resolve`) to the existing self-admit path. The invite is
/// forgotten only on success; if the self-admit fails (relay or daemon unreachable) it stays held
/// until it expires, so the member can press Join again.
#[tauri::command]
pub async fn join_redeem(app: tauri::AppHandle, code: String) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let join = app
            .try_state::<JoinState>()
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        join.0.redeem_opened(&code, |payload| {
            crate::invites::redeem_link(&app, &payload.to_invite_link())
        })
    })
    .await
}

/// `join_revoke` — revoke one of the member's codes at the service (the relay invite inside a
/// direct envelope is revoked separately with `group_invite_revoke`).
#[tauri::command]
pub async fn join_revoke(app: tauri::AppHandle, code: String) -> Result<(), String> {
    crate::blocking::off_main(move || with_states(&app, |join, auth| join.revoke(&auth.0, &code)))
        .await
}

/// `join_list` — the member's codes at the service, newest first.
#[tauri::command]
pub async fn join_list(app: tauri::AppHandle) -> Result<Vec<JoinCodeEntry>, String> {
    crate::blocking::off_main(move || with_states(&app, |join, auth| join.list(&auth.0))).await
}

#[cfg(test)]
mod tests {
    include!("join_tests.rs");
}
