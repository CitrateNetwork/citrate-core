//! v2 invite links (server-blind envelope): the Tauri command surface over
//! [`citrate_core_kit::join`] (GROW-S1 task 10).
//!
//! The inviter mints a short code at the citrate.ai join service with their auth.citrate.ai
//! session (the kit [`AuthState`] bearer, never handed to the webview), seals the relay invite in
//! Rust, and gets back one share URL `https://citrate.ai/join/<code>#<K>`. The invitee's app
//! resolves the held invite (`pending_join_resolve`, the only resolve path), verifies the signature
//! against the pinned keys, opens the envelope, and holds
//! the relay invite in Rust until the member clicks Join (`join_redeem`), which runs the existing
//! self-admit path ([`crate::invites::redeem_link`]).
//!
//! What crosses `invoke`: [`MintView`] (code, kind, status, expiry, and the final share URL, the only
//! value that contains `K`), [`citrate_core_kit::join::ResolvedView`] (signed display data and two
//! flags, from `pending_join_resolve`), and
//! [`JoinCodeEntry`] rows. Never `K` on its own, the envelope, the relay token or the bearer.
//!
//! Data sources (Rule 7): `https://www.citrate.ai/api/join` (citrate-landing join service: POST
//! reserve, PUT attach, GET resolve, DELETE revoke, GET mine) and `/api/join/jwks` (pinned keys
//! only); the relay invite link comes from `group_invite_create`; redeem goes to the comms relay
//! through the existing `group_invite_redeem` body.
//!
//! UI wiring is tasks 11 (inviter) and 12 (invitee); the pending invite store is `pending_join`.

use citrate_core_kit::join::{
    InvitePayload, JoinClient, JoinCodeEntry, JoinConfig, JoinError, JoinHttp, MintRequest,
    MintView,
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
        let view = with_states(&app, |join, auth| {
            join.mint(&auth.0, &request, invite.as_ref())
        })?;
        // Remember which relay invite this code seals, so revoking the code closes it too.
        if let Some(invite) = &invite {
            let _ = crate::invites::update_pending(&app, |v| {
                crate::invites::tag_join_code(v, &invite.t, &view.code)
            });
        }
        Ok(view)
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

/// `join_redeem` — the member's explicit Join for a resolved direct invite: hands the opened relay
/// invite (held in Rust since `pending_join_resolve`, the only resolve path) to the existing self-admit path. The invite is
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
        })?;
        // Joined: the pending invite (task 12) for this code is spent, so wipe it.
        crate::pending_join::forget_code(&app, &code);
        Ok(())
    })
    .await
}

/// What a revoke closed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRevoked {
    /// The relay invite sealed inside a direct code was closed at the relay (now or by an earlier
    /// attempt). `false`: this device has no record of one (a public link, or a code minted
    /// elsewhere), so nothing at the relay was changed.
    pub relay_invite_closed: bool,
}

/// `join_revoke` — revoke one of the member's codes. For a direct code this device minted, the
/// relay invite sealed inside it is closed FIRST (someone who already opened the link holds that
/// invite and could still redeem it), then the code is revoked at the service. A relay failure
/// stops before the code is touched; a service failure after the relay closed is retried without
/// the relay step.
#[tauri::command]
pub async fn join_revoke(app: tauri::AppHandle, code: String) -> Result<JoinRevoked, String> {
    crate::blocking::off_main(move || {
        let closed = crate::invites::close_relay_for_join_code(
            &app,
            &code,
            |lookup| crate::comms::rendezvous_close(&app, lookup),
            |hash| crate::comms::revoke_invite(&app, hash, false),
        )?;
        with_states(&app, |join, auth| join.revoke(&auth.0, &code))?;
        crate::invites::update_pending(&app, |v| crate::invites::forget_join_code(v, &code))?;
        Ok(JoinRevoked {
            relay_invite_closed: closed,
        })
    })
    .await
}

/// `join_list` — the member's codes at the service, newest first.
#[tauri::command]
pub async fn join_list(app: tauri::AppHandle) -> Result<Vec<JoinCodeEntry>, String> {
    crate::blocking::off_main(move || with_states(&app, |join, auth| join.list(&auth.0))).await
}

/// `join_share_qr` — the QR modules for a share link the member is looking at (the webview draws
/// them as SVG). Only a well-formed join link is encoded; nothing is stored or logged.
#[tauri::command]
pub async fn join_share_qr(link: String) -> Result<crate::fleet_pairing::QrMatrix, String> {
    crate::blocking::off_main(move || {
        let link = zeroize::Zeroizing::new(link);
        citrate_core_kit::join::parse_link(&link).map_err(err)?;
        crate::fleet_pairing::qr_matrix(link.trim())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("join_tests.rs");
}
