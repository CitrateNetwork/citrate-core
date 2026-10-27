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
use crate::invite_seal::InvitePrivKey;

/// INVITE-S2 default time-to-live for a published invite: 14 days (Unix ms are added at publish).
pub(crate) const INVITE_TTL_MS: u64 = 14 * 24 * 60 * 60 * 1000;

/// A one-time claimable invite the owner minted for a group. `for_handle` is a label only.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingInvite {
    pub group: String,
    pub token: String,
    pub for_handle: String,
    pub created_at: u64,
    /// CONNECT-S1 — the invite's ephemeral PRIVATE key (hex), owner-only. Claims from the relay's
    /// server-blind inbox are sealed to the matching public key (in the link) and open only with this.
    /// `#[serde(default)]` so pre-S1 invites (no key) still load — they fall back to the manual path.
    #[serde(default, skip_serializing_if = "InvitePrivKey::is_empty")]
    pub priv_key: InvitePrivKey,
    /// CONNECT-S1 — the full share link (carries the ephemeral PUBLIC key `k=`), so the owner's
    /// "Copy link" hands out a one-click link. Public; safe to serialize. `default` for pre-S1 records.
    #[serde(default)]
    pub link: String,
    /// GROW-S2 — the relay rendezvous `lookup` (hex) the daemon confirmed for this invite, empty when
    /// no rendezvous was opened (an older daemon, a relay without rendezvous, or a pre-S2 invite).
    /// Public (the relay's own index). Revoke closes it with `rendezvousClose`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lookup: String,
    /// GROW-S2 — revoked or consumed here, with its rendezvous close persisted at the daemon but not
    /// yet confirmed by the relay. Not claimable; kept until a tick reports the rendezvous revoked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub revoking: bool,
    /// GROW-S2 — the invite was used (a claim approved), not revoked; set together with `revoking`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub consumed: bool,
    /// GROW-S2 — the claimant approved on this invite, added once the relay confirms the close.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub approved_member: String,
    /// The short invite code (v2 link) this relay invite is sealed inside, when this device minted
    /// one for it. Revoking that code also closes this relay invite. Not a secret.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub join_code: String,
    /// The relay invite is already closed (revoked) but its short code isn't revoked yet; a retry of
    /// the code's revoke skips the relay step.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub relay_closed: bool,
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
    /// GROW-S2 — revoke sent, waiting for the relay to confirm the rendezvous is closed.
    pub revoking: bool,
    /// GROW-S2 — it was used (a claim approved) rather than revoked.
    pub consumed: bool,
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
            revoking: i.revoking,
            consumed: i.consumed,
        }
    }
}

/// The invites for `group` that are still outstanding: claimable, and listed as open to agents.
/// A revoking invite (revoked or used here, waiting for the relay) is neither: its claims aren't
/// polled, so a member just let in doesn't show up again under Requests.
pub(crate) fn outstanding_invites<'a>(
    all: &'a [PendingInvite],
    group: &str,
) -> Vec<&'a PendingInvite> {
    all.iter()
        .filter(|i| i.group == group && !i.revoking)
        .collect()
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
    priv_key: InvitePrivKey,
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
        revoking: false,
        consumed: false,
        approved_member: String::new(),
        join_code: String::new(),
        relay_closed: false,
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
        Some(Err(crate::rendezvous::RvError::DaemonUnsupported)) => {
            return Err(
                "couldn't revoke the invite: this device's comms daemon can't close its relay \
                 rendezvous (it lacks the rendezvous ops, for example after a downgrade). Update \
                 Citrate Core, then try again."
                    .to_string(),
            )
        }
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
        // The file holds invite private keys: the read buffer is wiped once parsed.
        Ok(bytes) => serde_json::from_slice(&zeroize::Zeroizing::new(bytes))
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

/// Load the pending invites, change them with `f`, and write them back (owner-only), under
/// [`PENDING_LOCK`] (see [`update_pending_at`]).
pub(crate) fn update_pending<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&mut Vec<PendingInvite>) -> T,
) -> Result<T, String> {
    update_pending_at(&store_path(app)?, f)
}

/// Read the store at `path` under [`PENDING_LOCK`] (no write). An unreadable file is an error.
fn read_pending_at<T>(
    path: &std::path::Path,
    f: impl FnOnce(&[PendingInvite]) -> T,
) -> Result<T, String> {
    view_pending_at(path).map(|v| f(&v))
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
    let staged = stage_pending(path, v)?;
    commit_pending(&staged, path)
}

/// #431 — write the new contents to a 0600 sibling and fsync it; [`commit_pending`] renames it over
/// `pending.json`. A crash before the rename leaves the previous file whole.
///
/// The sibling's name is unique to this write (`pending.json.tmp-<pid>-<n>`, as
/// `atomic_file::replace` names its temp files, #420/#508), so no two writers ever share one, even
/// outside [`PENDING_LOCK`] (another process, or a test writing the same path). A failed stage
/// removes its own file.
fn stage_pending(
    path: &std::path::Path,
    v: &[PendingInvite],
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.tmp-{}-{n}", std::process::id()));
    let staged = (|| {
        let mut f = citrate_core_kit::fsutil::create_secret_file(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()
    })();
    match staged {
        Ok(()) => Ok(tmp),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.to_string())
        }
    }
}

/// Rename the staged file over `path` and fsync the directory, so the rename itself is durable.
/// A failed rename removes the staged file; `path` keeps its previous contents.
fn commit_pending(staged: &std::path::Path, path: &std::path::Path) -> Result<(), String> {
    if let Err(e) = std::fs::rename(staged, path) {
        let _ = std::fs::remove_file(staged);
        return Err(e.to_string());
    }
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
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

/// `group_invite_redeem` — INVITEE self-admit (INVITE-S2). Parse the link (`g`, `t`, optional `n`),
/// then self-admit into the group by external commit with the invite token — no owner action, even if
/// the owner is offline. Honest error if the daemon/relay is unreachable, the token is spent/expired,
/// or the invite was revoked (Rule 1 — never a fabricated "joined"). Records the joiner's audit copy (#73).
#[tauri::command]
pub async fn group_invite_redeem(app: tauri::AppHandle, link: String) -> Result<(), String> {
    redeem_link(&app, &link)
}

/// The self-admit body shared by [`group_invite_redeem`] and the v2 invite-link redeem
/// (`join::join_redeem`, which rebuilds this link from an opened envelope in Rust).
pub(crate) fn redeem_link(app: &tauri::AppHandle, link: &str) -> Result<(), String> {
    let req = redeem_request(link)?;
    crate::comms::redeem_invite(app, req.group.clone(), req.token, req.name.clone())?;
    let (group, name, token_hash) = (req.group, req.name, req.token_hash);
    append_referral(
        app,
        ReferralEvent {
            role: "joiner".into(),
            event: "joined".into(),
            group,
            group_name: name,
            token_hash,
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
pub(crate) fn redeem_token_wire(token: &str) -> Result<WireToken, String> {
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
    Ok(WireToken(hex::encode(token.as_bytes())))
}

/// An invite token in the member daemon's wire encoding (see [`redeem_token_wire`]). The field is
/// private, so [`redeem_token_wire`] is the only way to make one, and `comms::redeem_invite` takes
/// this type: a raw link token can't reach the daemon (it doesn't compile). No `Debug`.
pub(crate) struct WireToken(String);

impl WireToken {
    /// The hex the daemon IPC carries (tests read it; production only moves it into the request).
    #[cfg(test)]
    pub(crate) fn as_hex(&self) -> &str {
        &self.0
    }
    /// Hand the hex to the IPC request.
    pub(crate) fn into_hex(self) -> String {
        self.0
    }
}

/// Everything the invitee's self-admit needs, read from a `citrate://invite?g=…&t=…[&n=…]` link.
pub(crate) struct RedeemRequest {
    pub(crate) group: String,
    /// The token in the daemon's encoding (never the raw link value).
    pub(crate) token: WireToken,
    /// The display name for the joined group (from `n=`, else a generic label).
    pub(crate) name: String,
    /// `BLAKE3(token)`, the key the relay and the referral log share.
    pub(crate) token_hash: String,
}

/// Build the redeem request from an invite link (pure; no IPC). Fails closed on a missing group or
/// token, and on a token that isn't the minted shape. Errors never contain the token.
pub(crate) fn redeem_request(link: &str) -> Result<RedeemRequest, String> {
    let group = link_param(link, "g")
        .filter(|g| !g.is_empty())
        .ok_or("invite link missing group")?;
    let token = link_param(link, "t").ok_or("invite link missing token")?;
    let name = link_param(link, "n")
        .and_then(|h| hex::decode(h).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Invited group".to_string());
    let token_hash = blake3_token_hash(&token);
    let token = redeem_token_wire(&token)?;
    Ok(RedeemRequest {
        group,
        token,
        name,
        token_hash,
    })
}

/// Remember which short code (v2 link) seals the relay invite `token`. Returns whether an invite
/// was found. Called after a direct `join_mint` on this device.
pub(crate) fn tag_join_code(invites: &mut [PendingInvite], token: &str, code: &str) -> bool {
    match invites.iter_mut().find(|i| i.token == token) {
        Some(i) => {
            i.join_code = code.to_string();
            true
        }
        None => false,
    }
}

/// [`close_relay_for_code_at`] on this device's pending-invite store (`join_revoke`'s relay step).
pub(crate) fn close_relay_for_join_code(
    app: &tauri::AppHandle,
    code: &str,
    close: impl FnOnce(String) -> Result<(), crate::rendezvous::RvError>,
    revoke_relay: impl FnOnce(String) -> Result<(), String>,
) -> Result<bool, String> {
    close_relay_for_code_at(&store_path(app)?, code, close, revoke_relay)
}

/// Close the relay invite sealed inside the short code `code`, if this device minted that code:
/// its rendezvous (`close`, when one was recorded) and its INVITE-S2 record (`revoke_relay`), the
/// same two calls as [`revoke_at`]. `Ok(true)` = closed now or earlier; `Ok(false)` = this device
/// has no record of it; `Err` = a close or the revoke failed, and nothing changed.
///
/// core#355 B-1: the relay calls run with no copy of the store held. The record is read under the
/// lock, the relay is called, then the store is re-read under the lock and only that record is
/// marked closed, so invites created, consumed or tagged while the relay answers are kept as they
/// are (a record consumed meanwhile stays gone). An invite with a rendezvous is also marked
/// revoking (not claimable, key dropped) and is settled by the tick once the relay confirms.
pub(crate) fn close_relay_for_code_at(
    path: &std::path::Path,
    code: &str,
    close: impl FnOnce(String) -> Result<(), crate::rendezvous::RvError>,
    revoke_relay: impl FnOnce(String) -> Result<(), String>,
) -> Result<bool, String> {
    if code.is_empty() {
        return Ok(false);
    }
    let found = read_pending_at(path, |v| {
        v.iter().find(|i| i.join_code == code).map(|i| {
            (
                i.token.clone(),
                i.lookup.clone(),
                i.relay_closed || i.revoking,
            )
        })
    })?;
    let (token, lookup) = match found {
        None => return Ok(false),
        Some((_, _, true)) => return Ok(true),
        Some((token, lookup, false)) => (token, lookup),
    };
    let closed = (!lookup.is_empty()).then(|| close(lookup.clone()));
    let revoked = revoke_relay(blake3_token_hash(&token));
    revoke_outcome(closed, revoked)?;
    update_pending_at(path, |v| {
        if let Some(i) = v
            .iter_mut()
            .find(|i| i.join_code == code && i.token == token)
        {
            i.relay_closed = true;
            if !i.lookup.is_empty() {
                i.revoking = true;
                i.priv_key = InvitePrivKey::default();
            }
        }
    })?;
    Ok(true)
}

/// Drop the records of relay invites closed for `code` (after the code itself is revoked). Only a
/// record without a rendezvous: one with a rendezvous stays revoking until the tick sees the relay
/// confirm the close (`settle_revocations`).
pub(crate) fn forget_join_code(invites: &mut Vec<PendingInvite>, code: &str) {
    invites.retain(|i| !(i.relay_closed && i.join_code == code && i.lookup.is_empty()));
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
    let all = load(&app);
    let invites = outstanding_invites(&all, &group);
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
    address: String,
) -> Result<ConsumeOutcome, String> {
    let path = store_path(&app)?;
    consume_at(
        &path,
        &group,
        &token,
        &address,
        |lookup| {
            crate::comms::rendezvous_tick(&app)
                .ok()?
                .invites
                .into_iter()
                .find(|v| v.lookup.eq_ignore_ascii_case(lookup))
                .map(|v| v.state)
        },
        |lookup| crate::comms::rendezvous_close(&app, lookup),
        |token_hash| crate::comms::revoke_invite(&app, token_hash, true),
    )
}

/// The relay's answer (passed through by the comms daemon) when a revoke that asked about it names
/// an invite that was already redeemed. Mirrors `comms_wire::frames::INVITE_ALREADY_USED`.
pub(crate) const INVITE_ALREADY_USED: &str = "invite already used";

/// Why a claim on an invite someone else already used can't be approved.
const ALREADY_USED_REFUSAL: &str =
    "This invite was already used through its link, so this request can't be approved.";

/// What approving a claim did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumeOutcome {
    /// The caller adds the claimant now.
    Approved,
    /// The claimant is added once the relay confirms the invite closed (by the tick).
    Pending,
    /// No outstanding invite for this token.
    Invalid,
}

/// The consume itself, over the `pending.json` at `path`, the daemon's last view of the invite
/// (`view_of(lookup)`) and the two relay calls (as [`revoke_at`]). `legacy` asks the relay to
/// report an invite that was already redeemed ([`INVITE_ALREADY_USED`]); that answer refuses the
/// approval and keeps the record as it is.
pub(crate) fn consume_at(
    path: &std::path::Path,
    group: &str,
    token: &str,
    member: &str,
    view_of: impl FnOnce(&str) -> Option<crate::rendezvous::InviteState>,
    close: impl FnOnce(String) -> Result<(), crate::rendezvous::RvError>,
    legacy: impl FnOnce(String) -> Result<(), String>,
) -> Result<ConsumeOutcome, String> {
    // Consuming spends the invite: close its rendezvous and revoke its legacy record first (the same
    // calls as a revoke), so no other holder of the link can still be admitted. A failure keeps the
    // record, so the approval can be retried. Only the call that removes the record returns true
    // (single-use, even for two approvals at once).
    use crate::rendezvous::InviteState;
    let Some(record) = view_pending_at(path)?
        .into_iter()
        .find(|i| i.group == group && i.token == token && !i.revoking)
    else {
        return Ok(ConsumeOutcome::Invalid);
    };
    // Another holder of the link is being (or was) admitted through the rendezvous: the invite is
    // used, so this claim can't be approved. Nothing is sent and the invite is left as it is.
    if !record.lookup.is_empty()
        && matches!(
            view_of(&record.lookup),
            Some(InviteState::Adding | InviteState::Added | InviteState::PickedUp)
        )
    {
        return Err(ALREADY_USED_REFUSAL.to_string());
    }
    // The legacy record was already redeemed (by another holder of the link, through the older
    // self-admit path): the invite is used, so this claim can't be approved.
    let used = std::cell::Cell::new(false);
    let legacy = |token_hash: String| {
        legacy(token_hash).inspect_err(|e| used.set(e.contains(INVITE_ALREADY_USED)))
    };
    match revoke_and_remove_at(path, group, token, Some(member), close, legacy) {
        Ok(true) => {}
        Ok(false) => return Ok(ConsumeOutcome::Invalid),
        Err(_) if used.get() => return Err(ALREADY_USED_REFUSAL.to_string()),
        Err(e) => return Err(e),
    }
    // With a rendezvous, the close is only persisted at the daemon: a request already at the relay
    // could still be admitted until the relay confirms. The claimant is added by the tick once it
    // does (`approvals_to_complete`); without a rendezvous there is nothing to wait for.
    Ok(if record.lookup.is_empty() {
        ConsumeOutcome::Approved
    } else {
        ConsumeOutcome::Pending
    })
}

fn view_state<'a>(
    invites: &'a [crate::rendezvous::InviteView],
    lookup: &str,
) -> Option<&'a crate::rendezvous::InviteState> {
    invites
        .iter()
        .find(|v| v.lookup.eq_ignore_ascii_case(lookup))
        .map(|v| &v.state)
}

/// The `(group, member, token)` approvals the relay has now confirmed: the close is `revoked` (or the
/// rendezvous `expired`), so no one else can be admitted through the link.
pub(crate) fn approvals_to_complete(
    records: &[PendingInvite],
    invites: &[crate::rendezvous::InviteView],
) -> Vec<(String, String, String)> {
    use crate::rendezvous::InviteState;
    records
        .iter()
        .filter(|r| r.revoking && !r.approved_member.is_empty())
        .filter(|r| {
            matches!(
                view_state(invites, &r.lookup),
                Some(InviteState::Revoked | InviteState::Expired)
            )
        })
        .map(|r| (r.group.clone(), r.approved_member.clone(), r.token.clone()))
        .collect()
}

/// Drop pending approvals whose invite another holder used through the rendezvous after all.
pub(crate) fn drop_superseded_approvals(
    records: &mut [PendingInvite],
    invites: &[crate::rendezvous::InviteView],
) {
    use crate::rendezvous::InviteState;
    for r in records.iter_mut().filter(|r| !r.approved_member.is_empty()) {
        if matches!(
            view_state(invites, &r.lookup),
            Some(InviteState::Adding | InviteState::Added | InviteState::PickedUp)
        ) {
            r.approved_member.clear();
        }
    }
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
        |token_hash| crate::comms::revoke_invite(&app, token_hash, false),
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
    revoke_and_remove_at(path, group, token, None, close, legacy).map(|_| ())
}

/// [`revoke_at`], returning whether this call removed the record.
fn revoke_and_remove_at(
    path: &std::path::Path,
    group: &str,
    token: &str,
    approved: Option<&str>,
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
    // The close is persisted at the daemon, which resends it until the relay confirms. So an invite
    // with a rendezvous is kept, marked revoking (not claimable, shown as pending, still local work
    // for the tick after a restart), until a tick reports it revoked (`settle_revocations`). One
    // without a rendezvous is done once the legacy revoke is confirmed.
    update_pending_at(path, |v| {
        let Some(pos) = v.iter().position(|i| i.group == group && i.token == token) else {
            return false;
        };
        if v[pos].revoking {
            return false;
        }
        if v[pos].lookup.is_empty() {
            v.retain(|i| !(i.group == group && i.token == token));
        } else {
            // Nothing is opened with the key again: drop it with the claimability.
            v[pos].revoking = true;
            v[pos].consumed = approved.is_some();
            v[pos].approved_member = approved.unwrap_or_default().to_string();
            v[pos].priv_key = InvitePrivKey::default();
        }
        true
    })
}

/// GROW-S2 — drop the revoking records the relay has confirmed (pure; `invites` from the daemon's
/// tick). `now_unix` in seconds.
pub(crate) fn settle_revocations(
    mut records: Vec<PendingInvite>,
    invites: &[crate::rendezvous::InviteView],
    now_unix: u64,
) -> Vec<PendingInvite> {
    let ttl_secs = INVITE_TTL_MS / 1000;
    records.retain(|r| {
        let confirmed = invites.iter().any(|v| {
            v.lookup.eq_ignore_ascii_case(&r.lookup)
                && matches!(
                    v.state,
                    crate::rendezvous::InviteState::Revoked
                        | crate::rendezvous::InviteState::Expired
                )
        });
        let expired = r.created_at.saturating_add(ttl_secs) <= now_unix;
        // A record with a pending approval stays until the tick has added the claimant.
        !(r.revoking && ((confirmed && r.approved_member.is_empty()) || expired))
    });
    records
}

/// After a tick: drop the revoking records the relay has confirmed. Writes only when there is one.
pub(crate) fn settle_revocations_after_tick(
    app: &tauri::AppHandle,
    views: &crate::rendezvous::TickViews,
) {
    let Ok(path) = store_path(app) else {
        return;
    };
    if !view_pending_at(&path).is_ok_and(|v| v.iter().any(|i| i.revoking)) {
        return;
    }
    // Pending approvals: drop the ones the link was used for after all, then add the claimants whose
    // invite the relay has confirmed closed (a failed add is retried on the next tick).
    let todo = update_pending_at(&path, |v| {
        drop_superseded_approvals(v, &views.invites);
        approvals_to_complete(v, &views.invites)
    })
    .unwrap_or_default();
    for (group, member, token) in todo {
        if crate::comms::add_member(app, group.clone(), member.clone()).is_ok() {
            let _ = update_pending_at(&path, |v| {
                for r in v
                    .iter_mut()
                    .filter(|r| r.group == group && r.token == token && r.approved_member == member)
                {
                    r.approved_member.clear();
                }
            });
        }
    }
    let now = now_unix();
    let _ = update_pending_at(&path, |v| {
        *v = settle_revocations(std::mem::take(v), &views.invites, now);
    });
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
        assert!(live_tokens(&path).is_empty());
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
        super::minted_invite(
            group,
            token,
            "@h",
            1,
            crate::invite_seal::InvitePrivKey::from_hex("aa".into()),
            "l".into(),
            d,
        )
    }

    /// Tokens of the invites that are still outstanding (not revoked or consumed).
    fn live_tokens(path: &std::path::Path) -> Vec<String> {
        super::view_pending_at(path)
            .expect("pending.json reads")
            .into_iter()
            .filter(|i| !i.revoking)
            .map(|i| i.token)
            .collect()
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
        assert_eq!(
            live_tokens(&path),
            vec![T.to_string()],
            "the record is kept"
        );
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
        assert!(live_tokens(&path).is_empty());
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
        assert_eq!(live_tokens(&path), vec![T.to_string()]);
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
                        live_tokens(&path).is_empty(),
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
        assert_eq!(live_tokens(&path), vec!["new".to_string()]);
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
            live_tokens(&path).is_empty(),
            "both revoked: {:?}",
            live_tokens(&path)
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
        assert!(live_tokens(&path).is_empty());
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
            "0xm",
            |_| None,
            |l| {
                *closed.borrow_mut() = Some(l);
                Ok(())
            },
            |h| {
                *revoked.borrow_mut() = Some(h);
                Ok(())
            },
        );
        assert_eq!(r, Ok(super::ConsumeOutcome::Pending));
        assert_eq!(closed.into_inner().as_deref(), Some(LOOKUP));
        assert_eq!(revoked.into_inner(), Some(super::blake3_token_hash(T)));
        assert!(live_tokens(&path).is_empty());
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
            "0xm",
            |_| None,
            |_| Err(RvError::Ipc("timed out".into())),
            |_| Ok(()),
        );
        assert!(
            r.is_err(),
            "the claim isn't accepted while the rendezvous may be open"
        );
        assert_eq!(live_tokens(&path), vec![T.to_string()]);
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
            "0xm",
            |_| None,
            |_| {
                inner.set(Some(super::consume_at(
                    &path,
                    "g",
                    T,
                    "0xm",
                    |_| None,
                    |_| Ok(()),
                    |_| Ok(()),
                )));
                Ok(())
            },
            |_| Ok(()),
        );
        let inner = inner.into_inner().expect("ran");
        assert_eq!(
            (inner, outer),
            (
                Ok(super::ConsumeOutcome::Pending),
                Ok(super::ConsumeOutcome::Invalid)
            )
        );
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
            "0xm",
            |_| None,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
        );
        assert_eq!(r, Ok(super::ConsumeOutcome::Invalid));
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
        assert_eq!(live_tokens(&path), vec![T.to_string()]);
    }

    /// Tripwire: an invite record is only ever removed by `revoke_and_remove_at` (close + legacy
    /// revoke first, behind both revoke and consume), by `settle_revocations` (relay confirmed), or
    /// by `forget_join_code` (#355: a code's relay invite, closed by `close_relay_for_code_at` with
    /// the same two calls, and only when it has no rendezvous left to confirm).
    #[test]
    fn invite_records_are_only_removed_by_revoke_at() {
        let src = include_str!("invites.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests {").expect("tests")];
        assert_eq!(prod.matches(".retain(").count(), 3, "three removal sites");
        let forget = &prod[prod.find("fn forget_join_code").expect("forget")..];
        let forget = &forget[..forget.find("\n}\n").expect("end")];
        assert!(
            forget.contains("i.relay_closed") && forget.contains("i.lookup.is_empty()"),
            "forget_join_code drops only a closed record with no rendezvous"
        );
        let close = &prod[prod.find("fn close_relay_for_code_at").expect("close")..];
        let close = &close[..close.find("\n}\n").expect("end")];
        assert!(
            close.contains("close(lookup") && close.contains("revoke_outcome("),
            "a code revoke closes the rendezvous and the legacy record"
        );
        for f in [
            "fn revoke_and_remove_at",
            "fn settle_revocations(",
            "fn forget_join_code",
        ] {
            let body = &prod[prod.find(f).expect(f)..];
            let body = &body[..body.find("\n}\n").expect("end")];
            assert!(body.contains(".retain("), "{f}");
        }
    }

    // ---- Codex round 5 (P1): a persisted close isn't a relay-confirmed revoke ---------------------

    fn invite_view(
        lookup: &str,
        state: crate::rendezvous::InviteState,
    ) -> crate::rendezvous::InviteView {
        crate::rendezvous::InviteView {
            lookup: lookup.into(),
            group: "g".into(),
            state,
            joiner: None,
            expires_at: 1,
            updated_at: Some(1),
            reason: None,
            dropped: 0,
        }
    }

    /// `rendezvousClose` answering Ok means the daemon persisted the close and keeps resending it;
    /// the relay hasn't confirmed. The record stays, marked revoking (shown as pending, never
    /// claimable), and keeps the device's local work so a restart resumes the tick.
    #[test]
    fn a_persisted_close_keeps_the_invite_as_revoking_across_a_restart() {
        let path = scratch("revoking");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        assert_eq!(
            super::revoke_at(&path, "g", T, |_| Ok(()), |_| Ok(())),
            Ok(())
        );
        // "Restart": read the file again.
        let after = super::view_pending_at(&path).expect("reads");
        assert_eq!(after.len(), 1, "kept until the relay confirms");
        assert!(after[0].revoking);
        assert!(super::PendingInviteView::from(&after[0]).revoking);
        assert!(live_tokens(&path).is_empty(), "no longer outstanding");
        assert!(
            crate::rendezvous::local_work(&[], &after, after[0].created_at + 60),
            "the tick still starts after a restart"
        );
    }

    /// A revoking invite isn't claimable: a claim approval for it is refused without relay calls.
    #[test]
    fn a_revoking_invite_cant_be_consumed() {
        let path = scratch("revoking-consume");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        super::revoke_at(&path, "g", T, |_| Ok(()), |_| Ok(())).expect("revoke");
        let calls = std::cell::Cell::new(0);
        let r = super::consume_at(
            &path,
            "g",
            T,
            "0xm",
            |_| None,
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
        );
        assert_eq!(r, Ok(super::ConsumeOutcome::Invalid));
        assert_eq!(calls.get(), 0);
    }

    /// The tick settles a revoking record once the daemon reports the rendezvous `revoked` (or the
    /// invite's TTL has passed, so the relay holds nothing). Anything else keeps it.
    #[test]
    fn a_revoking_record_settles_only_on_a_confirmed_revoke_or_expiry() {
        use crate::rendezvous::InviteState;
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let mut rec = minted("g", T, d.clone());
        rec.revoking = true;
        let live = minted("g", "other", d);
        let now = rec.created_at + 60;
        for state in [
            InviteState::Open,
            InviteState::RelayUnreachable,
            InviteState::Adding,
        ] {
            let kept = super::settle_revocations(
                vec![rec.clone(), live.clone()],
                &[invite_view(LOOKUP, state)],
                now,
            );
            assert_eq!(kept.len(), 2, "{state:?} isn't a confirmed revoke");
        }
        let kept = super::settle_revocations(vec![rec.clone()], &[], now);
        assert_eq!(
            kept.len(),
            1,
            "a lookup missing from the views isn't a confirmation"
        );
        let kept = super::settle_revocations(
            vec![rec.clone(), live.clone()],
            &[invite_view(LOOKUP, InviteState::Revoked)],
            now,
        );
        assert_eq!(
            kept.iter().map(|i| i.token.as_str()).collect::<Vec<_>>(),
            vec!["other"]
        );
        let ttl = super::INVITE_TTL_MS / 1000;
        let kept = super::settle_revocations(vec![rec, live], &[], 1 + ttl);
        assert_eq!(
            kept.len(),
            1,
            "an expired revoking record settles; a live invite is untouched"
        );
    }

    /// The tick applies the settlement.
    #[test]
    fn the_tick_settles_revocations() {
        let src = include_str!("rendezvous.rs");
        let body = &src[src.find("pub(crate) fn tick_and_emit").expect("fn")..];
        let body = &body[..body.find("\n}\n").expect("end")];
        assert!(body.contains("crate::invites::settle_revocations_after_tick(app, &views)"));
    }

    // ---- independent review (#373 @6d00011): a revoking invite is no longer outstanding ----------

    /// An approved or revoked invite (kept as revoking until the relay confirms) is not claimable and
    /// not listed as outstanding: its claims aren't polled, so the person just let in doesn't show up
    /// again under Requests, and agents don't see it as open.
    #[test]
    fn a_revoking_invite_is_not_outstanding() {
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let mut used = minted("g", T, d.clone());
        used.revoking = true;
        let open = minted("g", "open", d.clone());
        let other = minted("h", "other", d);
        let all = vec![used, open, other];
        let out: Vec<&str> = super::outstanding_invites(&all, "g")
            .into_iter()
            .map(|i| i.token.as_str())
            .collect();
        assert_eq!(out, vec!["open"]);
    }

    /// The claims poll and the agent-facing list both go through `outstanding_invites`.
    #[test]
    fn claims_and_the_agent_list_read_only_outstanding_invites() {
        let src = include_str!("invites.rs");
        let body = &src[src
            .find("pub async fn group_invite_poll_claims")
            .expect("fn")..];
        let body = &body[..body.find("\n}\n").expect("end")];
        assert!(body.contains("outstanding_invites(&all, &group)"));
        let mcp = include_str!("node_mcp_live.rs");
        let body = &mcp[mcp.find("fn invites(&self, group: &str)").expect("fn")..];
        let body = &body[..body.find("\n    }\n").expect("end")];
        assert!(body.contains("crate::invites::outstanding_invites(&all, group)"));
    }

    /// Once retired, an invite's private key is dropped: nothing will be opened with it again.
    #[test]
    fn a_retired_invite_drops_its_private_key() {
        let path = scratch("drop-key");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        super::revoke_at(&path, "g", T, |_| Ok(()), |_| Ok(())).expect("revoke");
        let after = super::view_pending_at(&path).expect("reads");
        assert!(after[0].revoking);
        assert!(after[0].priv_key.is_empty(), "the key is gone");
    }

    /// #433: a used invite says it was used, a revoked one doesn't.
    #[test]
    fn a_consumed_invite_is_marked_consumed() {
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let path = scratch("consumed");
        super::write_pending(&path, &[minted("g", T, d.clone()), minted("g", "b", d)])
            .expect("write");
        assert_eq!(
            super::consume_at(&path, "g", T, "0xm", |_| None, |_| Ok(()), |_| Ok(())),
            Ok(super::ConsumeOutcome::Pending)
        );
        super::revoke_at(&path, "g", "b", |_| Ok(()), |_| Ok(())).expect("revoke");
        let after = super::view_pending_at(&path).expect("reads");
        let by = |t: &str| after.iter().find(|i| i.token == t).cloned().expect("kept");
        assert!(by(T).consumed && super::PendingInviteView::from(&by(T)).consumed);
        assert!(!by("b").consumed);
    }

    /// #433: the daemon's `expired` for the lookup also settles a revoking record.
    #[test]
    fn a_revoking_record_settles_on_expired_too() {
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let mut rec = minted("g", T, d);
        rec.revoking = true;
        let now = rec.created_at + 60;
        let kept = super::settle_revocations(
            vec![rec],
            &[invite_view(LOOKUP, crate::rendezvous::InviteState::Expired)],
            now,
        );
        assert!(kept.is_empty());
    }

    /// #433: a downgraded daemon's refusal says what to do.
    #[test]
    fn an_unsupported_close_says_to_update() {
        let e = super::revoke_outcome(Some(Err(RvError::DaemonUnsupported)), Ok(()))
            .expect_err("fails closed");
        assert!(e.contains("Update Citrate Core"), "{e}");
    }

    /// #431: a write that stops after staging (a crash before the rename) leaves the previous file
    /// readable and unchanged.
    #[test]
    fn an_interrupted_write_leaves_the_previous_file() {
        let path = scratch("atomic");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d.clone())]).expect("v1");
        let _staged = super::stage_pending(&path, &[minted("g", "v2", d)]).expect("staged");
        // No commit: the process died here.
        assert_eq!(tokens(&path), vec![T.to_string()]);
    }

    /// #508 + #431: every staged write has its own temp name (never the shared fixed
    /// `pending.json.tmp`), it is beside the file and owner-only, and a committed write leaves no
    /// temp file behind.
    #[test]
    fn each_staged_write_has_its_own_temp_file_and_a_commit_leaves_none() {
        let path = scratch("unique-temp");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let a = super::stage_pending(&path, &[minted("g", T, d.clone())]).expect("a");
        let b = super::stage_pending(&path, &[minted("g", "v2", d.clone())]).expect("b");
        assert_ne!(a, b, "two writes share a temp file");
        assert_ne!(
            a,
            path.with_extension("json.tmp"),
            "the fixed temp name is back"
        );
        assert_eq!(a.parent(), path.parent());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&a).expect("staged").permissions().mode();
            assert_eq!(
                mode & 0o077,
                0,
                "the staged file holds private keys: {mode:o}"
            );
        }
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let dir = path.parent().expect("dir");
        let leftovers: Vec<_> = std::fs::read_dir(dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
    }

    // ---- re-review: an approval waits for the relay to confirm the close -------------------------

    fn approval_record(member: &str) -> super::PendingInvite {
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let mut r = minted("g", T, d);
        r.revoking = true;
        r.consumed = true;
        r.approved_member = member.into();
        r
    }

    /// The daemon's last view says another holder of the link is being (or was) admitted: the
    /// approval is refused before any relay call, and the invite is left as it is.
    #[test]
    fn an_approval_is_refused_while_the_link_is_being_or_was_used() {
        use crate::rendezvous::InviteState;
        for state in [
            InviteState::Adding,
            InviteState::Added,
            InviteState::PickedUp,
        ] {
            let path = scratch("used-by-link");
            let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
            super::write_pending(&path, &[minted("g", T, d)]).expect("write");
            let calls = std::cell::Cell::new(0);
            let r = super::consume_at(
                &path,
                "g",
                T,
                "0xclaimant",
                |l| {
                    assert_eq!(l, LOOKUP);
                    Some(state)
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            );
            assert!(r.is_err(), "{state:?}: {r:?}");
            assert_eq!(calls.get(), 0, "{state:?}: no relay call");
            assert_eq!(live_tokens(&path), vec![T.to_string()]);
        }
    }

    /// Race: the close is persisted but the relay hasn't confirmed, so a request already in flight
    /// at the relay could still be admitted. The approval is pending, not approved, and the claimant
    /// is recorded for the tick to add after the relay confirms.
    #[test]
    fn an_approval_with_an_unconfirmed_close_is_pending() {
        let path = scratch("pending-approval");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::consume_at(
            &path,
            "g",
            T,
            "0xclaimant",
            |_| Some(crate::rendezvous::InviteState::Open),
            |_| Ok(()),
            |_| Ok(()),
        );
        assert_eq!(r, Ok(super::ConsumeOutcome::Pending));
        let after = super::view_pending_at(&path).expect("reads");
        assert_eq!(after[0].approved_member, "0xclaimant");
        assert!(after[0].revoking && after[0].consumed);
    }

    /// The relay's answer when the legacy record was already redeemed, as the daemon passes it on.
    fn already_used() -> Result<(), String> {
        Err(format!(
            "relay: server error: {}",
            super::INVITE_ALREADY_USED
        ))
    }

    /// An invite without a rendezvous whose legacy record another holder of the link already
    /// redeemed: the approval is refused and the record is left as it is.
    #[test]
    fn an_approval_on_an_already_redeemed_legacy_invite_is_refused() {
        let path = scratch("approve-legacy-used");
        let d = super::decide_mint(Ok(()), Err(RvError::DaemonUnsupported), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::consume_at(
            &path,
            "g",
            T,
            "0xc",
            |_| None,
            |_| Ok(()),
            |_| already_used(),
        );
        assert_eq!(r, Err(super::ALREADY_USED_REFUSAL.to_string()));
        assert_eq!(tokens(&path), vec![T.to_string()], "the record is kept");
        let after = super::view_pending_at(&path).expect("reads");
        assert!(after[0].approved_member.is_empty() && !after[0].consumed);
    }

    /// The same with a rendezvous: the legacy answer alone refuses it, and nothing is queued for
    /// the tick to add.
    #[test]
    fn an_approval_on_an_already_redeemed_invite_with_a_rendezvous_is_refused() {
        let path = scratch("approve-rv-used");
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::consume_at(
            &path,
            "g",
            T,
            "0xc",
            |_| Some(crate::rendezvous::InviteState::Open),
            |_| Ok(()),
            |_| already_used(),
        );
        assert_eq!(r, Err(super::ALREADY_USED_REFUSAL.to_string()));
        let after = super::view_pending_at(&path).expect("reads");
        assert_eq!(after.len(), 1);
        assert!(after[0].approved_member.is_empty(), "no claimant is queued");
        assert!(!after[0].revoking);
    }

    /// Any other legacy failure still reads as a retryable revoke failure, not as "used".
    #[test]
    fn an_approval_whose_legacy_revoke_fails_otherwise_is_retryable() {
        let path = scratch("approve-legacy-err");
        let d = super::decide_mint(Ok(()), Err(RvError::DaemonUnsupported), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::consume_at(
            &path,
            "g",
            T,
            "0xc",
            |_| None,
            |_| Ok(()),
            |_| Err("relay: timed out".to_string()),
        );
        let msg = r.expect_err("refused");
        assert!(msg.starts_with("couldn't revoke the invite"), "{msg}");
        assert_eq!(tokens(&path), vec![T.to_string()]);
    }

    /// Tripwire: only claim approval asks the relay to report a used invite; the owner's revoke
    /// and the join-code revoke keep the original request (and its OK).
    #[test]
    fn only_claim_approval_asks_whether_the_invite_was_used() {
        let prod = prod_source();
        let consume = fn_body(prod, "pub async fn group_invite_verify_consume");
        assert!(
            consume.contains("comms::revoke_invite(&app, token_hash, true)"),
            "claim approval must ask the relay to report an already-redeemed invite"
        );
        let revoke = fn_body(prod, "pub async fn group_invite_revoke");
        assert!(revoke.contains("comms::revoke_invite(&app, token_hash, false)"));
        let join = include_str!("join.rs");
        assert!(join.contains("comms::revoke_invite(&app, hash, false)"));
        assert_eq!(
            prod.matches("comms::revoke_invite(").count()
                + join.matches("comms::revoke_invite(").count(),
            3,
            "a new revoke caller must decide whether it needs the used answer"
        );
    }

    /// An invite without a rendezvous has nothing to wait for: approved now.
    #[test]
    fn an_approval_without_a_rendezvous_is_approved_now() {
        let path = scratch("approve-legacy");
        let d = super::decide_mint(Ok(()), Err(RvError::DaemonUnsupported), LOOKUP).expect("link");
        super::write_pending(&path, &[minted("g", T, d)]).expect("write");
        let r = super::consume_at(&path, "g", T, "0xc", |_| None, |_| Ok(()), |_| Ok(()));
        assert_eq!(r, Ok(super::ConsumeOutcome::Approved));
        assert!(tokens(&path).is_empty());
    }

    /// The tick completes a pending approval only once the relay confirms the close (`revoked`);
    /// the record is kept until then (and until the add is done).
    #[test]
    fn a_pending_approval_completes_on_a_confirmed_close() {
        use crate::rendezvous::InviteState;
        let rec = approval_record("0xclaimant");
        let now = rec.created_at + 60;
        assert!(super::approvals_to_complete(
            std::slice::from_ref(&rec),
            &[invite_view(LOOKUP, InviteState::Open)]
        )
        .is_empty());
        assert_eq!(
            super::approvals_to_complete(
                std::slice::from_ref(&rec),
                &[invite_view(LOOKUP, InviteState::Revoked)]
            ),
            vec![("g".to_string(), "0xclaimant".to_string(), T.to_string())]
        );
        let kept =
            super::settle_revocations(vec![rec], &[invite_view(LOOKUP, InviteState::Revoked)], now);
        assert_eq!(kept.len(), 1, "kept until the approval is completed");
    }

    /// If the link was used through the rendezvous after all, the pending approval is dropped.
    #[test]
    fn a_pending_approval_is_dropped_when_the_link_was_used() {
        use crate::rendezvous::InviteState;
        for state in [
            InviteState::Adding,
            InviteState::Added,
            InviteState::PickedUp,
        ] {
            let mut recs = vec![approval_record("0xclaimant")];
            super::drop_superseded_approvals(&mut recs, &[invite_view(LOOKUP, state)]);
            assert!(recs[0].approved_member.is_empty(), "{state:?}");
        }
        let mut recs = vec![approval_record("0xclaimant")];
        super::drop_superseded_approvals(&mut recs, &[invite_view(LOOKUP, InviteState::Open)]);
        assert_eq!(recs[0].approved_member, "0xclaimant");
    }

    /// The tick runs the approval steps.
    #[test]
    fn the_tick_completes_and_drops_approvals() {
        let src = include_str!("invites.rs");
        let body = &src[src
            .find("pub(crate) fn settle_revocations_after_tick")
            .expect("fn")..];
        let body = &body[..body.find("\n}\n").expect("end")];
        assert!(body.contains("approvals_to_complete("));
        assert!(body.contains("drop_superseded_approvals("));
        assert!(body.contains("crate::comms::add_member("));
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
            let wire = wire.as_hex();
            assert!(
                wire.chars().all(|c| c.is_ascii_hexdigit()),
                "the daemon hex-decodes it"
            );
            let bytes = hex::decode(wire).expect("hex");
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
        assert_eq!(
            super::redeem_token_wire(T).map(|w| w.into_hex()).as_deref(),
            Ok(TOKEN_UTF8_HEX)
        );
        assert_eq!(super::blake3_token_hash(T), TOKEN_HASH);
        let bytes = hex::decode(TOKEN_UTF8_HEX).expect("hex");
        assert_eq!(hex::encode(blake3::hash(&bytes).as_bytes()), TOKEN_HASH);
    }

    /// The §13 token `T` itself is not hex, even cut to an even length: sending it raw is what made
    /// the daemon answer "token must be hex". (`T[..42]` keeps base64url letters outside `0-9a-f`.)
    #[test]
    fn the_raw_contract_token_is_not_hex_even_at_even_length() {
        const T: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
        assert!(hex::decode(&T[..42]).is_err());
        assert!(hex::decode(T).is_err());
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
            let err = match super::redeem_token_wire(bad) {
                Ok(_) => panic!("accepted a malformed token"),
                Err(e) => e,
            };
            if !bad.is_empty() {
                assert!(!err.contains(bad), "the error never repeats the token");
            }
        }
    }

    /// A link in the exact share format `group_invite_create` builds, with a real minted token.
    fn real_link() -> (String, String) {
        let token = crate::connections::random_state();
        let link = format!(
            "citrate://invite?g={}&t={token}&k={}&n={}",
            "ab".repeat(16),
            "02".repeat(33),
            hex::encode("Garden club")
        );
        (link, token)
    }

    /// What reaches the daemon for a real link is hex of the `t=` bytes, and BLAKE3 of those bytes is
    /// the hash the owner published, so the relay finds the invite. Fails for any variant that sends
    /// the raw token.
    #[test]
    fn redeem_request_sends_hex_of_the_link_token_bytes() {
        for _ in 0..32 {
            let (link, t) = real_link();
            let req = super::redeem_request(&link).expect("a real link is accepted");
            assert_eq!(req.group, "ab".repeat(16));
            assert_eq!(req.name, "Garden club");
            let sent = hex::decode(req.token.as_hex()).ok();
            assert_eq!(sent.as_deref(), Some(t.as_bytes()), "hex of the t= bytes");
            let sent = sent.unwrap_or_default();
            assert_eq!(
                hex::encode(blake3::hash(&sent).as_bytes()),
                super::blake3_token_hash(&t),
                "the relay's BLAKE3(bytes) is the published hash"
            );
            assert_eq!(req.token_hash, super::blake3_token_hash(&t));
        }
    }

    #[test]
    fn redeem_request_matches_the_relay_contract_vector() {
        const T: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
        const TOKEN_UTF8_HEX: &str = "41414543417751464267634943516f4c4441304f4478415245684d554652595847426b6147787764486838";
        let link = format!(
            "citrate://invite?g={}&t={T}&k={}",
            "11".repeat(32),
            "02".repeat(33)
        );
        let req = super::redeem_request(&link).expect("the contract token is accepted");
        assert_eq!(req.token.as_hex(), TOKEN_UTF8_HEX);
        assert_eq!(req.name, "Invited group", "no n= gives the generic label");
    }

    /// Fail closed: a link whose token isn't the minted shape, or with no group or token, never
    /// becomes a request, and the error never repeats the token.
    #[test]
    fn redeem_request_refuses_bad_links_without_echoing_the_token() {
        let (_, good) = real_link();
        let g = "ab".repeat(16);
        let cases = [
            format!("citrate://invite?g={g}&t={}", &good[..42]),
            format!("citrate://invite?g={g}&t={good}A"),
            format!("citrate://invite?g={g}&t={}%2B", &good[..42]),
            format!("citrate://invite?g={g}&t={}=", &good[..42]),
            format!("citrate://invite?g={g}&t={good}#frag"),
            format!("citrate://invite?g={g}&t=tok123"),
            format!("citrate://invite?g={g}&t=deadbeef"),
            format!("citrate://invite?g={g}&t="),
            format!("citrate://invite?g={g}"),
            format!("citrate://invite?t={good}"),
            format!("citrate://invite?g=&t={good}"),
            "citrate://invite".to_string(),
        ];
        for link in &cases {
            let err = match super::redeem_request(link) {
                Ok(_) => panic!("accepted {link}"),
                Err(e) => e,
            };
            assert!(!err.contains(&good), "the error never repeats the token");
        }
    }

    /// Tripwire for this bug class: the daemon's `RedeemInvite` request is built in exactly one place
    /// (`comms::redeem_invite`), from a [`super::WireToken`]. A second construction site, or one that
    /// passes a plain string, fails here (and the type makes a raw token fail to compile).
    #[test]
    fn redeem_invite_request_is_built_only_from_a_wire_token() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sites = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("src dir").flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap_or_default();
            let prod = src.split("\n#[cfg(test)]\nmod tests").next().unwrap_or("");
            for (i, _) in prod.match_indices("Request::RedeemInvite") {
                sites.push((
                    name.clone(),
                    prod[i..].chars().take(160).collect::<String>(),
                ));
            }
        }
        assert_eq!(sites.len(), 1, "one RedeemInvite construction: {sites:?}");
        assert_eq!(sites[0].0, "comms.rs");
        let comms = include_str!("comms.rs");
        let f = &comms[comms.find("pub(crate) fn redeem_invite").expect("fn")..];
        let f = &f[..f.find("\n}\n").expect("end")];
        assert!(
            f.contains("token: crate::invites::WireToken"),
            "takes a WireToken"
        );
        assert!(f.contains("token: token.into_hex()"), "sends its hex");
    }

    fn pending(group: &str, token: &str) -> super::PendingInvite {
        super::PendingInvite {
            group: group.into(),
            token: token.into(),
            for_handle: String::new(),
            created_at: 1,
            priv_key: crate::invite_seal::InvitePrivKey::default(),
            link: format!("citrate://invite?g={group}&t={token}&k=02ab"),
            lookup: String::new(),
            revoking: false,
            consumed: false,
            approved_member: String::new(),
            join_code: String::new(),
            relay_closed: false,
        }
    }

    #[test]
    fn a_minted_code_is_linked_to_the_relay_invite_it_seals() {
        let t1 = crate::connections::random_state();
        let t2 = crate::connections::random_state();
        let mut v = vec![pending("grp_a", &t1), pending("grp_a", &t2)];
        assert!(super::tag_join_code(&mut v, &t2, "Ab3dE7gh"));
        assert_eq!(v[1].join_code, "Ab3dE7gh");
        assert!(v[0].join_code.is_empty());
        assert!(!super::tag_join_code(&mut v, "not-a-token", "Zz9yX8wv"));
    }

    #[test]
    fn revoking_a_code_closes_its_relay_invite_once_and_only_its_own() {
        let t1 = crate::connections::random_state();
        let t2 = crate::connections::random_state();
        let path = store_with(
            "close-once",
            &[pending("grp_a", &t1), tagged("grp_a", &t2, "Ab3dE7gh")],
        );
        let mut sent = Vec::new();
        let closed = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| panic!("no rendezvous recorded"),
            |h| {
                sent.push(h);
                Ok(())
            },
        );
        assert_eq!(closed, Ok(true));
        assert_eq!(
            sent,
            vec![super::blake3_token_hash(&t2)],
            "the sealed invite's hash"
        );
        let v = super::view_pending_at(&path).unwrap();
        assert!(v[1].relay_closed && !v[0].relay_closed);
        // A retry (the service revoke failed) doesn't call the relay again.
        let again = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| panic!("no rendezvous recorded"),
            |_| panic!("the relay was called twice"),
        );
        assert_eq!(again, Ok(true));
        // A code this device has no record of changes nothing at the relay.
        let none = super::close_relay_for_code_at(
            &path,
            "Zz9yX8wv",
            |_| panic!("no rendezvous recorded"),
            |_| panic!("no record"),
        );
        assert_eq!(none, Ok(false));
        super::update_pending_at(&path, |v| super::forget_join_code(v, "Ab3dE7gh")).unwrap();
        let v = super::view_pending_at(&path).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].token, t1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_relay_failure_stops_the_revoke_and_keeps_the_record_open() {
        let t = crate::connections::random_state();
        let path = store_with("close-fail", &[tagged("grp_a", &t, "Ab3dE7gh")]);
        let out = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| panic!("no rendezvous recorded"),
            |_| Err("comms: relay unreachable".to_string()),
        );
        assert_eq!(
            out,
            Err("couldn't revoke the invite: comms: relay unreachable".to_string())
        );
        assert!(!super::view_pending_at(&path).unwrap()[0].relay_closed);
        super::update_pending_at(&path, |v| super::forget_join_code(v, "Ab3dE7gh")).unwrap();
        assert_eq!(
            super::view_pending_at(&path).unwrap().len(),
            1,
            "an open relay invite is never forgotten"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The scenario: an invitee resolved the direct link (Review) and holds the opened relay
    /// invite; the inviter then revokes the link; the invitee presses Join. The relay (modelled as
    /// the set of live invite hashes) must refuse it.
    #[test]
    fn review_then_revoke_then_join_is_refused_by_the_relay() {
        let t = crate::connections::random_state();
        let group = "ab".repeat(16);
        let mut relay: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Inviter: create the relay invite (published by hash) and mint a short code for it.
        relay.insert(super::blake3_token_hash(&t));
        let mut v = vec![pending(&group, &t)];
        super::tag_join_code(&mut v, &t, "Ab3dE7gh");
        let path = store_with("review-revoke-join", &v);
        // Invitee: Review opened the envelope; it now holds the relay invite link.
        let opened = format!("citrate://invite?g={group}&t={t}&k={}", "02".repeat(33));
        // Inviter: Revoke from "Your invite links".
        let closed = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| panic!("no rendezvous recorded"),
            |h| {
                relay.remove(&h);
                Ok(())
            },
        );
        // Invitee: Join sends hex of the token's bytes; the relay hashes them and looks it up.
        let req = super::redeem_request(&opened).expect("request");
        let bytes = hex::decode(req.token.as_hex()).expect("hex");
        let hash = hex::encode(blake3::hash(&bytes).as_bytes());
        assert!(!relay.contains(&hash), "the revoked invite still redeems");
        assert_eq!(closed, Ok(true), "the inviter is told it was closed");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// #373 + #355: revoking a short code whose relay invite has a rendezvous closes the rendezvous
    /// too (someone holding the opened link could otherwise still be admitted through it), and the
    /// record is kept, revoking and keyless, until the relay confirms the close.
    #[test]
    fn revoking_a_code_closes_the_rendezvous_of_its_relay_invite() {
        let t = crate::connections::random_state();
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let mut rec = minted("g", &t, d);
        rec.join_code = "Ab3dE7gh".into();
        let path = store_with("code-rendezvous", &[rec]);
        let closed_lookup = std::cell::RefCell::new(None);
        let r = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |l| {
                *closed_lookup.borrow_mut() = Some(l);
                Ok(())
            },
            |_| Ok(()),
        );
        assert_eq!(r, Ok(true));
        assert_eq!(
            closed_lookup.into_inner().as_deref(),
            Some(LOOKUP),
            "the rendezvous was not closed"
        );
        let v = super::view_pending_at(&path).unwrap();
        assert!(
            v[0].revoking,
            "the record must stay revoking until the relay confirms"
        );
        assert!(
            v[0].priv_key.is_empty(),
            "the key is dropped with the claimability"
        );
        // The code's forget step keeps it for the tick; a confirmed close then drops it.
        super::update_pending_at(&path, |v| super::forget_join_code(v, "Ab3dE7gh")).unwrap();
        let v = super::view_pending_at(&path).unwrap();
        assert_eq!(
            v.len(),
            1,
            "a rendezvous record is settled by the tick, not forgotten"
        );
        let settled = super::settle_revocations(
            v,
            &[invite_view(LOOKUP, crate::rendezvous::InviteState::Revoked)],
            2,
        );
        assert!(settled.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A rendezvous close that fails stops the code revoke and leaves the invite as it was.
    #[test]
    fn a_failed_rendezvous_close_stops_the_code_revoke() {
        let t = crate::connections::random_state();
        let d = super::decide_mint(Ok(()), opened(LOOKUP), LOOKUP).expect("link");
        let mut rec = minted("g", &t, d);
        rec.join_code = "Ab3dE7gh".into();
        let path = store_with("code-rendezvous-fail", &[rec]);
        let r = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| Err(RvError::DaemonUnsupported),
            |_| Ok(()),
        );
        assert!(r.is_err(), "{r:?}");
        let v = super::view_pending_at(&path).unwrap();
        assert!(
            !v[0].relay_closed && !v[0].revoking,
            "nothing is marked closed"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Tripwire: `join_revoke` closes the relay invite, then revokes the code at the service, then
    /// forgets the closed record (relay -> service -> forget), and tags a minted code with the
    /// invite it seals.
    #[test]
    fn join_revoke_closes_the_relay_invite_first() {
        let src = include_str!("join.rs");
        let f = &src[src.find("pub async fn join_revoke").expect("fn")..];
        let f = &f[..f.find("\n}\n").expect("end")];
        let relay = f.find("close_relay_for_join_code(").expect("relay step");
        let service = f.find("join.revoke(").expect("service step");
        let forget = f.find("forget_join_code(").expect("forget step");
        assert!(relay < service, "relay first");
        assert!(
            service < forget,
            "the record is forgotten only after the service revoke"
        );
        assert_eq!(
            f.matches("comms::revoke_invite(").count(),
            1,
            "one relay call"
        );
        assert!(src.contains("crate::invites::tag_join_code(v, &invite.t, &view.code)"));
    }

    /// The production part of this file (everything before the test module).
    fn prod_source() -> &'static str {
        let src = include_str!("invites.rs");
        &src[..src.find("\n#[cfg(test)]\nmod tests").expect("test module")]
    }

    /// The text of the function whose signature starts with `sig`, up to its closing brace.
    fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
        let at = src.find(sig).unwrap_or_else(|| panic!("{sig} not found"));
        &src[at..at + src[at..].find("\n}\n").expect("end")]
    }

    /// Tripwire (core#355 B-1): every write of `pending.json` is one locked read-modify-write
    /// (`update_pending_at`), and the relay call of a revoke runs outside any of them, so an invite
    /// created or consumed while the relay answers is never overwritten by a stale copy.
    #[test]
    fn pending_writes_are_locked_and_the_relay_call_is_outside_them() {
        let prod = prod_source();
        // One writer: `write_pending` is called only from `update_pending_at`.
        let writes: Vec<usize> = prod
            .match_indices("write_pending(")
            .map(|(i, _)| i)
            .collect();
        let def = prod.find("fn write_pending(").expect("write_pending") + "fn ".len();
        let upd = fn_body(prod, "fn update_pending_at");
        let upd_at = prod.find(upd).expect("update_pending_at");
        for w in writes {
            assert!(
                w == def || (w > upd_at && w < upd_at + upd.len()),
                "pending.json is written outside update_pending_at: {}",
                &prod[w.saturating_sub(80)..w + 20]
            );
        }
        assert!(
            upd.contains("PENDING_LOCK") && upd.contains(".lock()"),
            "update_pending_at holds the store lock"
        );
        assert!(!prod.contains("fn save("), "no unlocked load/save pair");
        // The relay call runs between the locked read and the locked write, never inside one.
        let close = fn_body(prod, "pub(crate) fn close_relay_for_code_at");
        let relay = close.find("revoke_relay(").expect("relay call");
        let rendezvous = close.find("close(lookup").expect("rendezvous close call");
        for (c, _) in close.match_indices("_pending_at(") {
            let end = c + close[c..].find(";\n").expect("statement end");
            for call in [relay, rendezvous] {
                assert!(
                    !(c..end).contains(&call),
                    "a relay call runs inside a locked pending-store update"
                );
            }
        }
    }

    /// A temp pending-invite store holding `v`.
    fn store_with(tag: &str, v: &[super::PendingInvite]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("citrate-invites-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pending.json");
        super::write_pending(&path, v).unwrap();
        path
    }

    fn tagged(group: &str, token: &str, code: &str) -> super::PendingInvite {
        let mut i = pending(group, token);
        i.join_code = code.into();
        i
    }

    /// core#355 B-1: an invite minted while the relay is closing a revoked code's invite survives.
    #[test]
    fn an_invite_created_during_the_relay_close_is_kept() {
        let t = crate::connections::random_state();
        let path = store_with("b1-create", &[tagged("grp_a", &t, "Ab3dE7gh")]);
        let fresh = crate::connections::random_state();
        let closed = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| panic!("no rendezvous recorded"),
            |_| super::update_pending_at(&path, |v| v.push(pending("grp_b", &fresh))),
        );
        assert_eq!(closed, Ok(true));
        let v = super::view_pending_at(&path).unwrap();
        assert!(
            v.iter().any(|i| i.token == fresh),
            "an invite created during the relay call was overwritten"
        );
        assert!(
            v.iter().any(|i| i.token == t && i.relay_closed),
            "the closed invite is marked"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// core#355 B-1: an invite consumed while the relay answers (the closed one or another) is not
    /// brought back by the revoke.
    #[test]
    fn an_invite_consumed_during_the_relay_close_stays_consumed() {
        let (t, other) = (
            crate::connections::random_state(),
            crate::connections::random_state(),
        );
        let path = store_with(
            "b1-consume",
            &[tagged("grp_a", &t, "Ab3dE7gh"), pending("grp_a", &other)],
        );
        let closed = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| panic!("no rendezvous recorded"),
            |_| super::update_pending_at(&path, |v| v.retain(|i| i.token != t && i.token != other)),
        );
        assert_eq!(closed, Ok(true), "the relay invite was closed");
        assert!(
            super::view_pending_at(&path).unwrap().is_empty(),
            "a consumed invite came back"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// core#355 B-1: a relay failure changes nothing it owns and keeps what changed meanwhile.
    #[test]
    fn a_relay_failure_keeps_changes_made_meanwhile() {
        let t = crate::connections::random_state();
        let path = store_with("b1-fail", &[tagged("grp_a", &t, "Ab3dE7gh")]);
        let fresh = crate::connections::random_state();
        let out = super::close_relay_for_code_at(
            &path,
            "Ab3dE7gh",
            |_| panic!("no rendezvous recorded"),
            |_| {
                super::update_pending_at(&path, |v| v.push(pending("grp_b", &fresh)))?;
                Err("comms: relay unreachable".to_string())
            },
        );
        assert_eq!(
            out,
            Err("couldn't revoke the invite: comms: relay unreachable".to_string())
        );
        let v = super::view_pending_at(&path).unwrap();
        assert!(
            v.iter().any(|i| i.token == fresh),
            "a change made meanwhile was overwritten"
        );
        assert!(
            v.iter().any(|i| i.token == t && !i.relay_closed),
            "the record stays open"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Concurrent writers (create, consume, revoke, tag) each re-read the store under the lock, so
    /// none of them drops another's change.
    #[test]
    fn concurrent_pending_writers_lose_nothing() {
        let path = store_with("b1-threads", &[]);
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let (path, start) = (path.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    for k in 0..25 {
                        let tok = format!("tok-{n}-{k}");
                        super::update_pending_at(&path, |v| v.push(pending("grp_t", &tok)))
                            .unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(
            super::view_pending_at(&path).unwrap().len(),
            200,
            "a concurrent write was lost"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
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
            priv_key: super::InvitePrivKey::from_hex(
                "5ec2e7ba5eba11deadbeefc0ffee0123456789abcdef0123456789abcdef0123".into(),
            ),
            link: "citrate://invite?g=grp_abc&t=tok123&k=pub".into(),
            lookup: String::new(),
            revoking: false,
            consumed: false,
            approved_member: String::new(),
            join_code: String::new(),
            relay_closed: false,
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

    /// A pending invite's `Debug` output never carries its private key, and the stored form
    /// round-trips (the key is kept, a pre-S1 record without one still loads).
    #[test]
    fn pending_invite_debug_hides_the_private_key_and_round_trips() {
        let (priv_key, _pub) = crate::invite_seal::new_invite_keypair();
        let hex = priv_key.expose_hex().to_string();
        let inv = super::PendingInvite {
            group: "grp_abc".into(),
            token: "tok123".into(),
            for_handle: "@alice".into(),
            created_at: 1000,
            priv_key,
            link: "citrate://invite?g=grp_abc&t=tok123&k=pub".into(),
            lookup: String::new(),
            revoking: false,
            consumed: false,
            approved_member: String::new(),
            join_code: String::new(),
            relay_closed: false,
        };
        let dbg = format!("{inv:?}");
        assert!(!dbg.contains(&hex[..16]), "private key in Debug: {dbg}");
        assert!(
            dbg.contains("tok123"),
            "the rest of the record is still visible: {dbg}"
        );
        let json = serde_json::to_vec(&vec![inv]).unwrap();
        let back: Vec<super::PendingInvite> = serde_json::from_slice(&json).unwrap();
        assert_eq!(back[0].priv_key.expose_hex(), hex);
        let legacy: Vec<super::PendingInvite> =
            serde_json::from_str(r#"[{"group":"g","token":"t","forHandle":"","createdAt":1}]"#)
                .unwrap();
        assert!(legacy[0].priv_key.is_empty());
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
            priv_key: super::InvitePrivKey::from_hex("deadbeef".into()),
            link: "citrate://invite?g=grp_abc&t=tok123&k=pub".into(),
            lookup: String::new(),
            revoking: false,
            consumed: false,
            approved_member: String::new(),
            join_code: String::new(),
            relay_closed: false,
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
