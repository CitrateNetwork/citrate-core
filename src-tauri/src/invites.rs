//! Group invites: one flow, request and approve (D-12, 0.5.2).
//!
//! **Inviter** (a group owner or admin). [`group_invite_create`] makes a one-time link
//! `citrate://invite?g=<group>&t=<token>&k=<key>&e=<expires, unix s>[&n=<group name>][&by=<inviter>]`
//! (`n` and `by` are hex of UTF-8, display only). The link's `k` is the public half of a fresh
//! keypair; the private half stays in this device's `invites/pending.json` and nowhere else.
//!
//! **Invitee.** Opening the link (deep link) or pasting it shows "Join <group>?". Join
//! ([`group_invite_request`]) seals `{group, token, address, reply}` to `k` and leaves it in the
//! relay's server-blind claims inbox under `SHA-256(token)`. Only the inviter's device can open it.
//!
//! **Approve.** The inviter sees the request under Pending ([`group_invite_requests`]) and approves
//! it ([`group_invite_approve`]: the normal AddMember / Welcome path) or declines it. Nothing joins
//! without that approval. The invitee's app finishes the join from the Welcome
//! ([`group_invite_my_requests`]).
//!
//! **Revoke and expiry.** The relay holds no invite record. Revoking an invite, or its expiry,
//! deletes its private key here, so no request under it can be opened or approved again; an
//! invite that was used to add someone is removed the same way (single use). A decline or a revoke
//! is also sealed back to the requester's one-time `reply` key, under
//! `SHA-256(domain ‖ token ‖ reply key)`, which only the inviter and that requester know, so the
//! requester's app can say so. That answer is best-effort; without it the request reads as not
//! approved once the link expires.
//!
//! **Store.** `invites/pending.json`: owner-only (`0600`) in an owner-only directory (`0700`),
//! written atomically ([`crate::atomic_file`]), in one format ([`STORE_FORMAT`]). A file in another
//! format (0.5.1 and earlier wrote a bare array) is set aside, kept, and a new store starts. A
//! damaged file is never read as "no invites": a read fails, and the next write sets it aside.
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::connections::random_state;
use crate::invite_seal::InvitePrivKey;

/// How long a link can be used: 14 days.
pub(crate) const INVITE_TTL_SECS: u64 = 14 * 24 * 60 * 60;

/// The one `pending.json` format this build reads and writes.
pub(crate) const STORE_FORMAT: &str = "citrate-invites/request-approve-1";

/// Display labels (group name, inviter, who an invite is for) are cut to this many characters.
const LABEL_MAX_CHARS: usize = 64;

/// Domain tag of the reply inbox key.
const REPLY_DOMAIN: &[u8] = b"citrate-invite-reply-v1";

/// Length of a link token as minted ([`random_state`]: 32 bytes, unpadded base64url).
const INVITE_TOKEN_LEN: usize = 43;

/// The longest link accepted.
const MAX_LINK_LEN: usize = 2048;

/// An invite this device made (inviter side).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Invite {
    pub group: String,
    pub token: String,
    /// Opens the requests sealed to the link's key. Owner-only; never crosses `invoke`.
    pub priv_key: InvitePrivKey,
    pub link: String,
    /// Who it is for, a label only ("" when none was given).
    pub label: String,
    pub created_at: u64,
    pub expires_at: u64,
    /// Requesters the inviter declined (lowercase addresses).
    #[serde(default)]
    pub declined: Vec<String>,
    /// The requester being added now ("" when none). Set before the add, so a second approval or a
    /// revoke can't race it; the invite is removed once the add is confirmed.
    #[serde(default)]
    pub approving: String,
}

/// A request this device sent (invitee side).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MyRequest {
    pub group: String,
    pub group_name: String,
    pub inviter: String,
    pub token: String,
    /// Opens the inviter's answer (declined / revoked). Owner-only; never crosses `invoke`.
    pub reply_key: InvitePrivKey,
    /// The public half, sent inside the request; it addresses the reply inbox.
    pub reply_pub: String,
    pub requested_at: u64,
    pub expires_at: u64,
}

/// `invites/pending.json`.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Store {
    format: String,
    #[serde(default)]
    pub invites: Vec<Invite>,
    #[serde(default)]
    pub requests: Vec<MyRequest>,
}

impl Store {
    fn fresh() -> Self {
        Store {
            format: STORE_FORMAT.to_string(),
            invites: Vec::new(),
            requests: Vec::new(),
        }
    }
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

/// The public id of an invite: the first 16 hex chars of `BLAKE3(token)`. The app, the agent and
/// the MCP tools name an invite by this id; the token itself stays in the link.
pub(crate) fn invite_id(token: &str) -> String {
    hex::encode(blake3::hash(token.as_bytes()).as_bytes())[..16].to_string()
}

// ---------------------------------------------------------------------------
// The link.
// ---------------------------------------------------------------------------

/// What an invite link carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InviteLink {
    pub group: String,
    pub token: String,
    /// The public key requests are sealed to (hex, compressed SEC1).
    pub key: String,
    pub expires_at: u64,
    /// Display only ("" when the link has none).
    pub group_name: String,
    /// Display only ("" when the link has none).
    pub inviter: String,
}

/// The refusal for a link made by an earlier version (no key, or no expiry).
pub(crate) const EARLIER_LINK: &str =
    "This invite link is from an earlier version of Citrate and can't be used. Ask for a new link.";

/// The refusal for anything that isn't an invite link. Never echoes the input (it may hold a token).
pub(crate) const NOT_A_LINK: &str = "That isn't a complete invite link. Ask for the full link.";

fn label_hex(s: &str) -> String {
    hex::encode(s.as_bytes())
}

/// A display label: control and direction-override characters dropped, trimmed, cut to
/// [`LABEL_MAX_CHARS`].
pub(crate) fn clean_label(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .collect::<String>()
        .trim()
        .chars()
        .take(LABEL_MAX_CHARS)
        .collect()
}

fn label_from_hex(h: &str) -> String {
    hex::decode(h)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| clean_label(&s))
        .unwrap_or_default()
}

/// Build the share link for a minted invite.
pub(crate) fn build_link(l: &InviteLink) -> String {
    let mut s = format!(
        "citrate://invite?g={}&t={}&k={}&e={}",
        l.group, l.token, l.key, l.expires_at
    );
    if !l.group_name.is_empty() {
        s.push_str(&format!("&n={}", label_hex(&l.group_name)));
    }
    if !l.inviter.is_empty() {
        s.push_str(&format!("&by={}", label_hex(&l.inviter)));
    }
    s
}

fn link_params(link: &str) -> Option<Vec<(&str, &str)>> {
    let q = link.strip_prefix("citrate://invite?")?;
    Some(
        q.split('&')
            .filter_map(|pair| pair.split_once('='))
            .collect(),
    )
}

fn param<'a>(params: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn group_id_ok(g: &str) -> bool {
    !g.is_empty()
        && g.len() <= 128
        && g.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn token_ok(t: &str) -> bool {
    use base64::Engine as _;
    t.len() == INVITE_TOKEN_LEN
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(t)
            .map(|raw| raw.len() == 32)
            .unwrap_or(false)
}

fn key_ok(k: &str) -> bool {
    k.len() == 66
        && (k.starts_with("02") || k.starts_with("03"))
        && k.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Read an invite link. Fails closed on anything malformed; a link from an earlier version (no
/// key or no expiry) gets [`EARLIER_LINK`]. Errors never contain the token.
pub(crate) fn parse_link(link: &str) -> Result<InviteLink, String> {
    let link = link.trim();
    if link.len() > MAX_LINK_LEN {
        return Err(NOT_A_LINK.to_string());
    }
    let params = link_params(link).ok_or(NOT_A_LINK)?;
    let group = param(&params, "g")
        .filter(|g| group_id_ok(g))
        .ok_or(NOT_A_LINK)?;
    let token = param(&params, "t")
        .filter(|t| token_ok(t))
        .ok_or(NOT_A_LINK)?;
    let (Some(key), Some(e)) = (param(&params, "k"), param(&params, "e")) else {
        return Err(EARLIER_LINK.to_string());
    };
    if !key_ok(key) {
        return Err(NOT_A_LINK.to_string());
    }
    let expires_at = e
        .parse::<u64>()
        .ok()
        .filter(|_| e.bytes().all(|b| b.is_ascii_digit()))
        .ok_or(NOT_A_LINK)?;
    Ok(InviteLink {
        group: group.to_string(),
        token: token.to_string(),
        key: key.to_ascii_lowercase(),
        expires_at,
        group_name: param(&params, "n").map(label_from_hex).unwrap_or_default(),
        inviter: param(&params, "by").map(label_from_hex).unwrap_or_default(),
    })
}

// ---------------------------------------------------------------------------
// The sealed messages.
// ---------------------------------------------------------------------------

/// The relay inbox key a request is left under: `SHA-256(token)`, hex.
fn request_inbox(token: &str) -> String {
    hex::encode(crate::invite_seal::token_hash(token))
}

/// The relay inbox key the inviter's answer to one requester is left under:
/// `SHA-256(REPLY_DOMAIN ‖ token ‖ reply key)`, hex. The reply key is inside the sealed request,
/// so nobody else holding the link can address (or flood) this inbox.
fn reply_inbox(token: &str, reply_pub: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(REPLY_DOMAIN);
    h.update(token.as_bytes());
    h.update(reply_pub.to_ascii_lowercase().as_bytes());
    hex::encode(h.finalize())
}

/// What a requester sends, sealed to the link's key.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestBody {
    v: u32,
    group: String,
    token: String,
    address: String,
    reply: String,
}

/// The inviter's answer, sealed to the requester's reply key.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Declined,
    Revoked,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyBody {
    v: u32,
    group: String,
    token: String,
    decision: Decision,
}

fn address_ok(a: &str) -> bool {
    a.len() == 42 && a.starts_with("0x") && a[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// A request opened with the invite's key.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenedRequest {
    /// Lowercase.
    address: String,
    reply: String,
}

/// Open the requests left for `inv`: those sealed to its key, for its group and token, with a
/// well-formed address and reply key. One per address (the first). Anything else is skipped,
/// never shown as a request.
fn open_requests(inv: &Invite, ciphertexts: &[String]) -> Vec<OpenedRequest> {
    let mut out: Vec<OpenedRequest> = Vec::new();
    for ct in ciphertexts {
        let Ok(sealed) = hex::decode(ct) else {
            continue;
        };
        let Ok(plain) = crate::invite_seal::open_with(&inv.priv_key, &sealed) else {
            continue;
        };
        let plain = zeroize::Zeroizing::new(plain);
        let Ok(b) = serde_json::from_slice::<RequestBody>(&plain) else {
            continue;
        };
        let address = b.address.to_ascii_lowercase();
        if b.v == 1
            && b.group == inv.group
            && b.token == inv.token
            && address_ok(&address)
            && key_ok(&b.reply)
            && !out.iter().any(|r| r.address == address)
        {
            out.push(OpenedRequest {
                address,
                reply: b.reply.to_ascii_lowercase(),
            });
        }
    }
    out
}

/// The inviter's answer in the requester's reply inbox, if one opens with `req`'s reply key.
fn read_reply(req: &MyRequest, ciphertexts: &[String]) -> Option<Decision> {
    ciphertexts.iter().find_map(|ct| {
        let sealed = hex::decode(ct).ok()?;
        let plain = crate::invite_seal::open_with(&req.reply_key, &sealed).ok()?;
        let b: ReplyBody = serde_json::from_slice(&plain).ok()?;
        (b.v == 1 && b.group == req.group && b.token == req.token).then_some(b.decision)
    })
}

/// Seal `decision` to one requester and leave it in their reply inbox. Best-effort: the caller
/// reports whether it was sent.
fn send_reply(
    inv: &Invite,
    to: &OpenedRequest,
    decision: Decision,
    submit: &mut impl FnMut(String, String) -> Result<(), String>,
) -> Result<(), String> {
    let body = serde_json::to_vec(&ReplyBody {
        v: 1,
        group: inv.group.clone(),
        token: inv.token.clone(),
        decision,
    })
    .map_err(|e| e.to_string())?;
    let sealed = crate::invite_seal::seal_to(&to.reply, &body)?;
    submit(reply_inbox(&inv.token, &to.reply), hex::encode(sealed))
}

// ---------------------------------------------------------------------------
// The store.
// ---------------------------------------------------------------------------

fn store_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("invites");
    citrate_core_kit::fsutil::ensure_private_dir(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("pending.json"))
}

/// What reading `pending.json` found.
enum Loaded {
    /// No file: no invites, no requests.
    Missing,
    Current(Store),
    /// Valid JSON in another format (an earlier version's file, or a later one's). Set aside.
    OtherFormat,
}

/// Why `pending.json` can't be used.
enum ReadError {
    /// The file doesn't parse (or is in this format but malformed). The next write sets it aside.
    Damaged,
    /// The file couldn't be read at all. Nothing is moved: the error may pass.
    Io(String),
}

impl ReadError {
    fn message(&self) -> String {
        match self {
            ReadError::Damaged => "Your saved invites are damaged and can't be read. Making a new \
                invite sets the damaged file aside (kept, not deleted) and starts a new list."
                .to_string(),
            ReadError::Io(e) => format!("couldn't read your saved invites: {e}"),
        }
    }
}

/// Only the `format` field, read without keeping any other value.
#[derive(Deserialize)]
struct FormatProbe {
    #[serde(default)]
    format: Option<String>,
}

/// Read `pending.json`. Call it only under [`STORE_LOCK`].
fn read_store(path: &std::path::Path) -> Result<Loaded, ReadError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => zeroize::Zeroizing::new(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Loaded::Missing),
        Err(e) => return Err(ReadError::Io(e.to_string())),
    };
    match serde_json::from_slice::<FormatProbe>(&bytes) {
        Ok(p) if p.format.as_deref() == Some(STORE_FORMAT) => serde_json::from_slice(&bytes)
            .map(Loaded::Current)
            .map_err(|_| ReadError::Damaged),
        Ok(_) => Ok(Loaded::OtherFormat),
        // Valid JSON of another shape (0.5.1's bare array): another format, not damage.
        Err(e) if e.classify() == serde_json::error::Category::Data => Ok(Loaded::OtherFormat),
        Err(_) => Err(ReadError::Damaged),
    }
}

/// Move `path` aside as `<name>.<tag>-<unix ms>` (with a `-<k>` suffix if that name is taken, so
/// an earlier one is never replaced), owner-only, and fsync the directory. The bytes are kept as
/// they were (they may hold invite keys). Returns the new path. Call it only under [`STORE_LOCK`].
fn set_aside(path: &std::path::Path, tag: &str) -> Result<std::path::PathBuf, String> {
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let base = format!("{name}.{tag}-{}", now_unix_ms());
    let aside = (0..1000u32)
        .map(|k| {
            path.with_file_name(if k == 0 {
                base.clone()
            } else {
                format!("{base}-{k}")
            })
        })
        .find(|p| p.symlink_metadata().is_err())
        .ok_or_else(|| "couldn't find a free name to set the invite file aside".to_string())?;
    std::fs::rename(path, &aside).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only a regular file: never follow a link to change the mode of what it points at.
        if aside
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_file())
        {
            std::fs::set_permissions(&aside, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        if let Some(dir) = path.parent() {
            std::fs::File::open(dir)
                .and_then(|d| d.sync_all())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(aside)
}

/// The tag of a file set aside because it is in another format.
const EARLIER_TAG: &str = "earlier";
/// The tag of a file set aside because it is damaged.
const DAMAGED_TAG: &str = "damaged";

/// Serializes every `pending.json` read and read-change-write in this process (Tauri commands,
/// the agent and the MCP tools).
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    STORE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The store as it is now, under [`STORE_LOCK`]. A file in another format is set aside and this
/// reads as a new store. A damaged or unreadable file is an error, never "no invites", and a read
/// never moves it.
fn view_store_at(path: &std::path::Path) -> Result<Store, String> {
    let _guard = lock();
    match read_store(path) {
        Ok(Loaded::Current(s)) => Ok(s),
        Ok(Loaded::Missing) => Ok(Store::fresh()),
        Ok(Loaded::OtherFormat) => {
            set_aside(path, EARLIER_TAG)?;
            Ok(Store::fresh())
        }
        Err(e) => Err(e.message()),
    }
}

/// Read, change and write `pending.json` in one step, under [`STORE_LOCK`]. Every writer goes
/// through here and none holds a copy across a network call. A file in another format is set
/// aside and the change starts a new store. A damaged file is set aside and this write fails,
/// saying so; the next one starts a new store.
fn update_store_at<T>(
    path: &std::path::Path,
    f: impl FnOnce(&mut Store) -> T,
) -> Result<T, String> {
    let _guard = lock();
    let mut store = match read_store(path) {
        Ok(Loaded::Current(s)) => s,
        Ok(Loaded::Missing) => Store::fresh(),
        Ok(Loaded::OtherFormat) => {
            set_aside(path, EARLIER_TAG)?;
            Store::fresh()
        }
        Err(ReadError::Damaged) => {
            let aside = set_aside(path, DAMAGED_TAG)?;
            let kept = aside
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            return Err(format!(
                "Your saved invites were damaged and couldn't be read. They were set aside as \
                 {kept} (kept, not deleted) and nothing was changed. Try again to start a new \
                 list. Links made before this can't be approved from this device."
            ));
        }
        Err(e) => return Err(e.message()),
    };
    let out = f(&mut store);
    write_store(path, &store)?;
    Ok(out)
}

/// Write the store owner-only, atomically: a new `0600` temp file (never one already there),
/// fsynced, renamed over `pending.json`, then the directory fsynced. A crash leaves the old file or
/// the new one, never a cut one.
fn write_store(path: &std::path::Path, store: &Store) -> Result<(), String> {
    let bytes =
        zeroize::Zeroizing::new(serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?);
    let staged = crate::atomic_file::stage_secret(path, &bytes).map_err(|e| e.to_string())?;
    crate::atomic_file::commit_staged(&staged, path).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Inviter.
// ---------------------------------------------------------------------------

/// An invite as the app shows it. No private key (PBA-L4-008: it never crosses `invoke`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InviteView {
    pub id: String,
    pub group: String,
    pub link: String,
    pub label: String,
    pub created_at: u64,
    pub expires_at: u64,
    /// The requester being added now ("" when none).
    pub approving: String,
}

impl From<&Invite> for InviteView {
    fn from(i: &Invite) -> Self {
        InviteView {
            id: invite_id(&i.token),
            group: i.group.clone(),
            link: i.link.clone(),
            label: i.label.clone(),
            created_at: i.created_at,
            expires_at: i.expires_at,
            approving: i.approving.clone(),
        }
    }
}

/// Whether an invite can still take requests: not expired, or mid-approval.
fn is_open(i: &Invite, now: u64) -> bool {
    now < i.expires_at || !i.approving.is_empty()
}

/// Make an invite for `group` and record it. Expired invites are dropped (with their keys) in the
/// same write.
pub(crate) fn create_at(
    path: &std::path::Path,
    group: &str,
    label: &str,
    inviter: &str,
    group_name: &str,
    now: u64,
) -> Result<InviteView, String> {
    if !group_id_ok(group) {
        return Err("that isn't a group id".to_string());
    }
    let token = random_state();
    let (priv_key, key) = crate::invite_seal::new_invite_keypair();
    let expires_at = now.saturating_add(INVITE_TTL_SECS);
    let link = build_link(&InviteLink {
        group: group.to_string(),
        token: token.clone(),
        key,
        expires_at,
        group_name: clean_label(group_name),
        inviter: clean_label(inviter),
    });
    let invite = Invite {
        group: group.to_string(),
        token,
        priv_key,
        link,
        label: clean_label(label),
        created_at: now,
        expires_at,
        declined: Vec::new(),
        approving: String::new(),
    };
    let view = InviteView::from(&invite);
    update_store_at(path, |s| {
        s.invites.retain(|i| is_open(i, now));
        s.invites.push(invite);
    })?;
    Ok(view)
}

/// The open invites for `group`. A store that can't be read is an error, not an empty list.
pub(crate) fn invite_views_at(
    path: &std::path::Path,
    group: &str,
    now: u64,
) -> Result<Vec<InviteView>, String> {
    Ok(view_store_at(path)?
        .invites
        .iter()
        .filter(|i| i.group == group && is_open(i, now))
        .map(InviteView::from)
        .collect())
}

/// A request waiting for the inviter.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IncomingRequest {
    pub invite_id: String,
    pub group: String,
    /// Lowercase comms address the request names (what the request says; not checked against
    /// the requester's relay session).
    pub address: String,
    /// The label of the link it came in on ("" when the link has none).
    pub invite_label: String,
    /// It is being approved now.
    pub approving: bool,
}

/// The requests waiting on `group`'s open invites (declined ones left out). `poll` reads one
/// claims inbox. Fails if the store can't be read or the relay can't be asked: unknown is not
/// "no requests".
pub(crate) fn requests_at(
    path: &std::path::Path,
    group: &str,
    now: u64,
    mut poll: impl FnMut(String) -> Result<Vec<String>, String>,
) -> Result<Vec<IncomingRequest>, String> {
    let open: Vec<Invite> = view_store_at(path)?
        .invites
        .into_iter()
        .filter(|i| i.group == group && is_open(i, now))
        .collect();
    let mut out = Vec::new();
    for inv in &open {
        let cts = poll(request_inbox(&inv.token))
            .map_err(|e| format!("Couldn't check for requests: {e}"))?;
        for r in open_requests(inv, &cts) {
            if inv.declined.contains(&r.address) {
                continue;
            }
            out.push(IncomingRequest {
                invite_id: invite_id(&inv.token),
                group: inv.group.clone(),
                invite_label: String::new(),
                approving: inv.approving == r.address,
                address: r.address,
            });
        }
    }
    Ok(out)
}

/// The refusal when the invite is gone: revoked, already used, or expired and dropped.
pub(crate) const INVITE_GONE: &str =
    "This invite was revoked or already used, so this request can't be approved.";

fn find<'a>(s: &'a mut Store, group: &str, id: &str) -> Option<&'a mut Invite> {
    s.invites
        .iter_mut()
        .find(|i| i.group == group && invite_id(&i.token) == id)
}

/// The invites an approval is adding someone through right now, in this process (`group/id`).
/// While one is here nothing else acts on that invite (a second approval, a check, a decline, a
/// revoke), so AddMember is never sent twice for one invite.
static IN_FLIGHT: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

const IN_FLIGHT_MSG: &str =
    "This invite is being used to add someone right now. Try again in a moment.";

/// The refusal when an approval's outcome is unknown: the hold stays until a check settles it.
const UNKNOWN_OUTCOME: &str =
    "Couldn't confirm whether they were added. Press Check again on the invite, or Cancel it.";

/// Marks one invite as in flight until dropped.
struct InFlight(String);

impl InFlight {
    fn take(group: &str, id: &str) -> Result<Self, String> {
        let key = format!("{group}/{id}");
        let mut v = IN_FLIGHT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if v.contains(&key) {
            return Err(IN_FLIGHT_MSG.to_string());
        }
        v.push(key.clone());
        Ok(InFlight(key))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|k| *k != self.0);
    }
}

/// What checking a held invite found.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeldCheck {
    /// Nothing was held (or the invite is gone): nothing to do.
    NotHeld,
    /// The requester is on the roster: the invite is used and removed.
    Added,
    /// They aren't on the roster: the hold is released and the invite is open again.
    Released,
    /// They aren't on the roster and the link has expired: the invite is removed with its key.
    Expired,
}

/// Settle a held approval by the roster (`is_member`), whatever happened to the add: a crash
/// after the hold, a lost answer, a relay that no longer serves the request. Present: the invite
/// is used. Absent: the hold is released (or, past expiry, the invite is removed with its key). A
/// roster that can't be read keeps the hold and is an error.
pub(crate) fn check_held_at(
    _path: &std::path::Path,
    _group: &str,
    _id: &str,
    _now: u64,
    _is_member: impl FnOnce(&str, &str) -> Result<bool, String>,
) -> Result<HeldCheck, String> {
    Ok(HeldCheck::NotHeld)
}

/// At start-up: check every held approval against the roster ([`check_held_at`]). One that can't
/// be settled now stays held, shown with Check again and Cancel.
pub(crate) fn recheck_held_at(
    _path: &std::path::Path,
    _now: u64,
    _is_member: impl FnMut(&str, &str) -> Result<bool, String>,
) -> Result<Vec<(String, Result<HeldCheck, String>)>, String> {
    Ok(Vec::new())
}

/// Approve `address`'s request on invite `id`: add them (`add`, the AddMember / Welcome path) and
/// remove the invite (single use). The approval must match a request opened under this invite
/// (`poll` reads its inbox). Refused when the invite is gone (revoked or used), expired, the
/// request was declined, the invite is held for someone else, or an approval on it is in flight.
///
/// AddMember is sent at most once per hold: Approve again on a held invite asks the roster
/// (`is_member`) first and adds only if they aren't there. After a failed add the roster decides;
/// if it can't be read, the hold stays (Check again or Cancel on the invite settle it).
#[allow(clippy::too_many_arguments)]
pub(crate) fn approve_at(
    path: &std::path::Path,
    group: &str,
    id: &str,
    address: &str,
    now: u64,
    _poll: impl FnOnce(String) -> Result<Vec<String>, String>,
    add: impl FnOnce(&str, &str) -> Result<(), String>,
    mut is_member: impl FnMut(&str, &str) -> Result<bool, String>,
) -> Result<(), String> {
    let member = address.to_ascii_lowercase();
    if !address_ok(&member) {
        return Err("that isn't a member address".to_string());
    }
    update_store_at(path, |s| {
        let inv = find(s, group, id).ok_or_else(|| INVITE_GONE.to_string())?;
        if inv.approving.is_empty() || inv.approving != member {
            refuse_approval(inv, &member, now)?;
        }
        inv.approving = member.clone();
        Ok::<(), String>(())
    })??;
    let added = match add(group, &member) {
        Ok(()) => Ok(()),
        Err(e) => match is_member(group, &member) {
            Ok(true) => Ok(()),
            Ok(false) => Err((e, true)),
            Err(_) => Err((format!("{e}. {UNKNOWN_OUTCOME}"), false)),
        },
    };
    match added {
        Ok(()) => update_store_at(path, |s| {
            s.invites.retain(|i| {
                !(i.group == group && invite_id(&i.token) == id && i.approving == member)
            });
        }),
        Err((e, not_added)) => {
            if not_added {
                update_store_at(path, |s| {
                    if let Some(i) = find(s, group, id).filter(|i| i.approving == member) {
                        i.approving.clear();
                    }
                })?;
            }
            Err(e)
        }
    }
}

/// Why a first approval of `member` on `inv` is refused, if it is.
fn refuse_approval(inv: &Invite, member: &str, now: u64) -> Result<(), String> {
    if !inv.approving.is_empty() && inv.approving != member {
        return Err("This invite was already used to add someone else.".to_string());
    }
    if now >= inv.expires_at {
        return Err(
            "This invite expired, so this request can't be approved. Send them a new link."
                .to_string(),
        );
    }
    if inv.declined.iter().any(|d| d == member) {
        return Err(
            "You declined this request. Send them a new link if you change your mind.".to_string(),
        );
    }
    Ok(())
}

/// Whether the requester(s) were told (best-effort).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub notified: bool,
}

/// Decline `address`'s request on invite `id`: it is not shown again and can't be approved. Their
/// app is told if the relay can be reached (`poll` finds their request, `submit` leaves the answer).
pub(crate) fn decline_at(
    path: &std::path::Path,
    group: &str,
    id: &str,
    address: &str,
    poll: impl FnOnce(String) -> Result<Vec<String>, String>,
    mut submit: impl FnMut(String, String) -> Result<(), String>,
) -> Result<Notice, String> {
    let member = address.to_ascii_lowercase();
    let inv = update_store_at(path, |s| {
        let inv = find(s, group, id).ok_or_else(|| INVITE_GONE.to_string())?;
        if inv.approving == member {
            return Err("They're being added now, so this request can't be declined.".to_string());
        }
        if !inv.declined.contains(&member) {
            inv.declined.push(member.clone());
        }
        Ok(inv.clone())
    })??;
    let notified = poll(request_inbox(&inv.token))
        .ok()
        .and_then(|cts| {
            open_requests(&inv, &cts)
                .into_iter()
                .find(|r| r.address == member)
        })
        .is_some_and(|r| send_reply(&inv, &r, Decision::Declined, &mut submit).is_ok());
    Ok(Notice { notified })
}

/// Revoke invite `id` (also Cancel on a held approval): remove it with its key, so no request
/// under it can be opened or approved. Refused only while an approval on it is in flight. Waiting requesters are told if the relay can be
/// reached (`notified` is false if any of them couldn't be).
pub(crate) fn revoke_at(
    path: &std::path::Path,
    group: &str,
    id: &str,
    poll: impl FnOnce(String) -> Result<Vec<String>, String>,
    mut submit: impl FnMut(String, String) -> Result<(), String>,
) -> Result<Notice, String> {
    // Refused only while an approval on it is in flight. A held approval whose outcome is unknown
    // can be revoked: that is Cancel, the way out when the roster can't settle it.
    let inv = {
        let mut s = view_store_at(path)?;
        let i = find(&mut s, group, id)
            .ok_or("no such invite (already revoked or used)")?
            .clone();
        if !i.approving.is_empty() {
            return Err(
                "This invite is being used to add someone, so it can't be revoked now.".into(),
            );
        }
        i
    };
    // Read the waiting requests while the key still exists (outside the lock: a network call).
    let waiting = poll(request_inbox(&inv.token)).map(|cts| open_requests(&inv, &cts));
    update_store_at(path, |s| {
        find(s, group, id).ok_or("no such invite (already revoked or used)")?;
        s.invites
            .retain(|i| !(i.group == group && invite_id(&i.token) == id));
        Ok::<(), &str>(())
    })??;
    // Tell every waiting requester, even after one send fails; not the one a held approval may
    // already have added.
    let mut notified = waiting.is_ok();
    for r in waiting.iter().flatten() {
        if !inv.declined.contains(&r.address)
            && r.address != inv.approving
            && send_reply(&inv, r, Decision::Revoked, &mut submit).is_err()
        {
            notified = false;
        }
    }
    Ok(Notice { notified })
}

// ---------------------------------------------------------------------------
// Invitee.
// ---------------------------------------------------------------------------

/// What the "Join <group>?" screen shows for a link.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvitePreview {
    pub group: String,
    pub group_name: String,
    pub inviter: String,
    pub expires_at: u64,
    pub expired: bool,
    /// This device already asked to join with this link.
    pub requested: bool,
}

/// Read a link for the Join screen (nothing is sent).
pub(crate) fn preview_at(
    path: &std::path::Path,
    link: &str,
    now: u64,
) -> Result<InvitePreview, String> {
    let l = parse_link(link)?;
    let requested = view_store_at(path)?
        .requests
        .iter()
        .any(|r| r.group == l.group && r.token == l.token);
    Ok(InvitePreview {
        expired: now >= l.expires_at,
        group: l.group,
        group_name: l.group_name,
        inviter: l.inviter,
        expires_at: l.expires_at,
        requested,
    })
}

/// Where a request this device sent stands.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    /// Sent; no answer yet.
    Waiting,
    /// Approved and joined (the Welcome was used).
    Joined,
    Declined,
    /// The inviter revoked the link.
    Revoked,
    /// The link expired before an approval arrived.
    Expired,
}

/// A request this device sent, as the app shows it. No reply key.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MyRequestView {
    pub id: String,
    pub group: String,
    pub group_name: String,
    pub inviter: String,
    pub requested_at: u64,
    pub expires_at: u64,
    pub state: RequestState,
    /// Why the state couldn't be checked just now ("" when it was).
    pub note: String,
}

fn my_view(r: &MyRequest, state: RequestState, note: String) -> MyRequestView {
    MyRequestView {
        id: invite_id(&r.token),
        group: r.group.clone(),
        group_name: r.group_name.clone(),
        inviter: r.inviter.clone(),
        requested_at: r.requested_at,
        expires_at: r.expires_at,
        state,
        note,
    }
}

/// Ask to join with `link` as `address` (this device's comms address): seal the request to the
/// link's key and leave it in the relay inbox (`submit`), then record it. Refused for an expired
/// link. Asking again with the same link sends nothing new.
pub(crate) fn request_at(
    path: &std::path::Path,
    link: &str,
    address: &str,
    now: u64,
    submit: impl FnOnce(String, String) -> Result<(), String>,
) -> Result<MyRequestView, String> {
    let l = parse_link(link)?;
    if now >= l.expires_at {
        return Err("This invite has expired. Ask for a new link.".to_string());
    }
    let address = address.to_ascii_lowercase();
    if !address_ok(&address) {
        return Err("this device's group address isn't ready yet; try again in a moment".into());
    }
    if let Some(r) = view_store_at(path)?
        .requests
        .iter()
        .find(|r| r.group == l.group && r.token == l.token)
    {
        return Ok(my_view(r, RequestState::Waiting, String::new()));
    }
    let (reply_key, reply) = crate::invite_seal::new_invite_keypair();
    let body = zeroize::Zeroizing::new(
        serde_json::to_vec(&RequestBody {
            v: 1,
            group: l.group.clone(),
            token: l.token.clone(),
            address,
            reply: reply.clone(),
        })
        .map_err(|e| e.to_string())?,
    );
    let sealed = crate::invite_seal::seal_to(&l.key, &body)?;
    submit(request_inbox(&l.token), hex::encode(sealed))
        .map_err(|e| format!("Couldn't send your request: {e}"))?;
    let req = MyRequest {
        group: l.group,
        group_name: l.group_name,
        inviter: l.inviter,
        token: l.token,
        reply_key,
        reply_pub: reply,
        requested_at: now,
        expires_at: l.expires_at,
    };
    let view = my_view(&req, RequestState::Waiting, String::new());
    update_store_at(path, |s| {
        if !s
            .requests
            .iter()
            .any(|r| r.group == req.group && r.token == req.token)
        {
            s.requests.push(req);
        }
    })?;
    Ok(view)
}

/// The daemon's answer when no Welcome has arrived for a group yet.
fn no_welcome_yet(e: &str) -> bool {
    e.contains("no pending welcome")
}

/// Check each request this device sent: `join` finishes a join from a Welcome if one arrived
/// (approved); otherwise the reply inbox (`poll`) may hold a decline or revoke; otherwise it is
/// waiting, or expired once the link has. A joined request is removed. The relay or daemon being
/// unreachable is said in `note`, never read as an answer.
pub(crate) fn check_requests_at(
    path: &std::path::Path,
    now: u64,
    mut join: impl FnMut(&str) -> Result<(), String>,
    mut poll: impl FnMut(String) -> Result<Vec<String>, String>,
) -> Result<Vec<MyRequestView>, String> {
    let requests = view_store_at(path)?.requests;
    let mut out = Vec::new();
    let mut joined: Vec<(String, String)> = Vec::new();
    for r in &requests {
        let view = match join(&r.group) {
            Ok(()) => {
                joined.push((r.group.clone(), r.token.clone()));
                my_view(r, RequestState::Joined, String::new())
            }
            Err(e) if no_welcome_yet(&e) => match poll(reply_inbox(&r.token, &r.reply_pub)) {
                Ok(cts) => match read_reply(r, &cts) {
                    Some(Decision::Declined) => my_view(r, RequestState::Declined, String::new()),
                    Some(Decision::Revoked) => my_view(r, RequestState::Revoked, String::new()),
                    None if now >= r.expires_at => my_view(r, RequestState::Expired, String::new()),
                    None => my_view(r, RequestState::Waiting, String::new()),
                },
                Err(e) => my_view(
                    r,
                    RequestState::Waiting,
                    format!("Can't reach the relay right now ({e})."),
                ),
            },
            Err(e) => my_view(
                r,
                RequestState::Waiting,
                format!("Can't check right now ({e})."),
            ),
        };
        out.push(view);
    }
    if !joined.is_empty() {
        update_store_at(path, |s| {
            s.requests
                .retain(|r| !joined.iter().any(|(g, t)| *g == r.group && *t == r.token));
        })?;
    }
    Ok(out)
}

/// Forget a request this device sent (the member dismissed it, or cancelled while waiting).
pub(crate) fn dismiss_at(path: &std::path::Path, id: &str) -> Result<(), String> {
    update_store_at(path, |s| s.requests.retain(|r| invite_id(&r.token) != id))
}

// ---------------------------------------------------------------------------
// Commands.
// ---------------------------------------------------------------------------

/// The group's name from this device's own group list, for the link's display label ("" when the
/// daemon can't be asked: the link then shows a generic label).
async fn group_name_of(app: &tauri::AppHandle, group: &str) -> String {
    crate::comms::groups_list(app.clone())
        .await
        .ok()
        .and_then(|v| v.into_iter().find(|(id, _)| id == group).map(|(_, n)| n))
        .unwrap_or_default()
}

/// Make an invite for `group` (inviter: an owner or admin; the relay enforces who may add).
/// `label` says who it is for ("" for none); `inviter` is how the inviter is named on the
/// invitee's screen ("" for none). Returns the invite with its link to share.
pub(crate) async fn create(
    app: &tauri::AppHandle,
    group: &str,
    label: &str,
    inviter: &str,
) -> Result<InviteView, String> {
    let name = group_name_of(app, group).await;
    create_at(&store_path(app)?, group, label, inviter, &name, now_unix())
}

/// `group_invite_create` — make an invite link for a group.
#[tauri::command]
pub async fn group_invite_create(
    app: tauri::AppHandle,
    group: String,
    label: String,
    inviter: String,
) -> Result<InviteView, String> {
    create(&app, &group, &label, &inviter).await
}

/// `group_invites` — the group's open invites made on this device.
#[tauri::command]
pub async fn group_invites(
    app: tauri::AppHandle,
    group: String,
) -> Result<Vec<InviteView>, String> {
    invite_views_at(&store_path(&app)?, &group, now_unix())
}

/// Revoke an invite on this device (also used by the MCP tool). Blocking: relay calls.
pub(crate) fn revoke(app: &tauri::AppHandle, group: &str, id: &str) -> Result<Notice, String> {
    revoke_at(
        &store_path(app)?,
        group,
        id,
        |th| crate::comms::poll_claims(app, th),
        |th, ct| crate::comms::submit_claim(app, th, ct),
    )
}

/// `group_invite_revoke` — revoke an invite.
#[tauri::command]
pub async fn group_invite_revoke(
    app: tauri::AppHandle,
    group: String,
    id: String,
) -> Result<Notice, String> {
    crate::blocking::off_main(move || revoke(&app, &group, &id)).await
}

/// `group_invite_requests` — the requests waiting on the group's invites (from the relay inbox).
#[tauri::command]
pub async fn group_invite_requests(
    app: tauri::AppHandle,
    group: String,
) -> Result<Vec<IncomingRequest>, String> {
    crate::blocking::off_main(move || {
        requests_at(&store_path(&app)?, &group, now_unix(), |th| {
            crate::comms::poll_claims(&app, th)
        })
    })
    .await
}

/// `group_invite_approve` — approve a request: add the requester, and use up the invite.
#[tauri::command]
pub async fn group_invite_approve(
    app: tauri::AppHandle,
    group: String,
    id: String,
    address: String,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        approve_at(
            &store_path(&app)?,
            &group,
            &id,
            &address,
            now_unix(),
            |th| crate::comms::poll_claims(&app, th),
            |g, m| crate::comms::add_member(&app, g.to_string(), m.to_string()),
            |g, m| crate::comms::is_member(&app, g, m),
        )
    })
    .await
}

/// `group_invite_check` — settle a held approval by the roster (Check again on the invite row).
#[tauri::command]
pub async fn group_invite_check(
    app: tauri::AppHandle,
    group: String,
    id: String,
) -> Result<HeldCheck, String> {
    crate::blocking::off_main(move || {
        check_held_at(&store_path(&app)?, &group, &id, now_unix(), |g, m| {
            crate::comms::is_member(&app, g, m)
        })
    })
    .await
}

/// At app start: settle held approvals by the roster (best-effort; one that can't be settled
/// stays held and the invite row offers Check again and Cancel). Starts the group daemon only
/// when there is a held approval.
pub(crate) fn recheck_held_on_start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let Ok(path) = store_path(&app) else {
            return;
        };
        let _ = recheck_held_at(&path, now_unix(), |g, m| {
            crate::comms::is_member(&app, g, m)
        });
    });
}

/// `group_invite_decline` — decline a request.
#[tauri::command]
pub async fn group_invite_decline(
    app: tauri::AppHandle,
    group: String,
    id: String,
    address: String,
) -> Result<Notice, String> {
    crate::blocking::off_main(move || {
        decline_at(
            &store_path(&app)?,
            &group,
            &id,
            &address,
            |th| crate::comms::poll_claims(&app, th),
            |th, ct| crate::comms::submit_claim(&app, th, ct),
        )
    })
    .await
}

/// `group_invite_preview` — read a link for the Join screen. Sends nothing.
#[tauri::command]
pub async fn group_invite_preview(
    app: tauri::AppHandle,
    link: String,
) -> Result<InvitePreview, String> {
    preview_at(&store_path(&app)?, &link, now_unix())
}

/// `group_invite_request` — ask to join with a link (the invitee's Join).
#[tauri::command]
pub async fn group_invite_request(
    app: tauri::AppHandle,
    link: String,
) -> Result<MyRequestView, String> {
    crate::blocking::off_main(move || {
        let address = crate::comms::device_identity(&app)?.address;
        request_at(&store_path(&app)?, &link, &address, now_unix(), |th, ct| {
            crate::comms::submit_claim(&app, th, ct)
        })
    })
    .await
}

/// `group_invite_my_requests` — where this device's requests stand; finishes a join once approved.
#[tauri::command]
pub async fn group_invite_my_requests(app: tauri::AppHandle) -> Result<Vec<MyRequestView>, String> {
    crate::blocking::off_main(move || {
        check_requests_at(
            &store_path(&app)?,
            now_unix(),
            |g| crate::comms::join_group(&app, g.to_string()),
            |th| crate::comms::poll_claims(&app, th),
        )
    })
    .await
}

/// `group_invite_dismiss` — forget a request this device sent.
#[tauri::command]
pub async fn group_invite_dismiss(app: tauri::AppHandle, id: String) -> Result<(), String> {
    dismiss_at(&store_path(&app)?, &id)
}

/// The open invites for `group`, for the agent and MCP tools (ids and labels only, no links). A
/// store that can't be read is an error.
pub(crate) fn open_invites(app: &tauri::AppHandle, group: &str) -> Result<Vec<InviteView>, String> {
    invite_views_at(&store_path(app)?, group, now_unix())
}

#[cfg(test)]
mod tests {
    include!("invites_tests.rs");
}
