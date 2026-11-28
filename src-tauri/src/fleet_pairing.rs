//! HUP-S8.2 (US-8.1) — fleet pairing tokens: short-lived, single-use, signed.
//!
//! "Connect my machines" offers a link (and the same link as a QR code) that another machine
//! running Citrate Core opens to pair with this one. The link carries a [`PairClaim`] signed by
//! this app's **pairing key**: an ed25519 key kept in the OS keychain (L7-WP02, account
//! [`PAIRING_KEY_ACCOUNT`], separate from the device key and the comms key), never written to any
//! other file, never derived from the wallet, and never used for anything else. When the keychain
//! cannot be used the app falls back to a key held in memory for this run and says so. It proves
//! "this link was minted by the Citrate Core you are about to talk to"; it is NOT a wallet
//! signature and it moves no value (Rule 3 is untouched: the wallet-signed `DeviceLink` that
//! binds a device to the member is S8.1, through the SignatureCeremony).
//!
//! Properties (each pinned by `fleet_pairing_tests.rs`):
//! - **Signed.** `sig = ed25519(pairing_key, DOMAIN ‖ claim_json)`. The joining device verifies it
//!   offline against the embedded `issuer_pub` (integrity of the hints + expiry it is about to
//!   use); the issuer verifies against its OWN key, so a link minted elsewhere is unknown to it.
//! - **Short-lived.** [`PAIR_TTL_SECS`] (10 minutes; a default pending owner sign-off). A claim
//!   that asks for longer is refused even when validly signed.
//! - **Single use.** The issuer keeps the nonce of every outstanding token; a redeem consumes it,
//!   a second redeem is `AlreadyUsed`. At most [`MAX_OUTSTANDING`] tokens are live at once.
//! - **Survives a restart (L7-WP02).** The outstanding and used nonces (with their expiry, no key
//!   material) are written to the ledger [`LEDGER_FILE`] (owner-only). A redeem is written before
//!   it is accepted, so a used link stays used after a restart; an unreadable ledger starts empty,
//!   which refuses every earlier link (fail closed).
//!
//! Link format (v1): `citrate://pair?c=<base64url(claim_json)>&s=<base64url(sig64)>`. The verifier
//! checks the signature over the exact `c` bytes it received (never a re-serialization).

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::custody::Keyring;

/// The keychain account of the pairing key (L7-WP02). Not the device key
/// (`device_link::DEVICE_KEY_ACCOUNT`) and not the comms key.
pub const PAIRING_KEY_ACCOUNT: &str = "fleet-pairing-key-v1";
/// The single-use ledger beside the fleet roster, in the app data dir (L7-WP02).
pub const LEDGER_FILE: &str = "fleet-pair-ledger.json";
/// Largest ledger read; a bigger file is treated as unreadable.
const MAX_LEDGER_BYTES: u64 = 64 * 1024;

/// The link scheme + host every pairing link starts with.
pub const LINK_PREFIX: &str = "citrate://pair?";
/// Pairing token lifetime. DEFAULT PENDING OWNER SIGN-OFF (10 minutes).
pub const PAIR_TTL_SECS: u64 = 600;
/// Clock skew tolerated between two of the member's machines when checking `issued_at`.
pub const CLOCK_SKEW_SECS: u64 = 120;
/// Live (unexpired, unused) tokens an issuer holds at once.
pub const MAX_OUTSTANDING: usize = 8;
/// Longest link accepted (a v1 link is well under 1 KiB).
pub const MAX_LINK_LEN: usize = 2048;
/// Longest device label.
pub const MAX_LABEL_LEN: usize = 64;
/// Most address hints a link carries.
pub const MAX_HINTS: usize = 6;

/// Domain separation for the pairing signature.
const DOMAIN: &[u8] = b"citrate/fleet-pair/v1\0";

/// Why a pairing link was refused. Coarse and secret-free; shown to the member as plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PairError {
    /// Not a v1 pairing link, or a field is out of bounds.
    Malformed,
    /// The signature does not match the claim.
    BadSignature,
    /// The token's lifetime is over.
    Expired,
    /// `issued_at` is further in the future than the tolerated clock skew.
    NotYetValid,
    /// The token was already used.
    AlreadyUsed,
    /// This device did not issue the token (or forgot it after it expired).
    UnknownToken,
    /// Too many live tokens; wait for one to be used or to expire.
    TooManyOutstanding,
    /// The link was created on the device trying to use it.
    SameDevice,
    /// L7-WP02: the issuer could not record the redeem, so it refused it (the link stays open).
    Unavailable,
}

impl PairError {
    /// Plain-language text for the wizard.
    pub fn message(self) -> &'static str {
        match self {
            PairError::Malformed => "That is not a Citrate Core pairing link.",
            PairError::BadSignature => "The pairing link has been altered; ask for a new one.",
            PairError::Expired => "The pairing link has expired; create a new one.",
            PairError::NotYetValid => {
                "The pairing link is dated in the future; check both machines' clocks."
            }
            PairError::AlreadyUsed => "That pairing link was already used; create a new one.",
            PairError::UnknownToken => "This machine did not create that pairing link.",
            PairError::TooManyOutstanding => {
                "Too many open pairing links; wait for one to expire or be used."
            }
            PairError::SameDevice => {
                "That link was created on this machine; open it on the other machine."
            }
            PairError::Unavailable => {
                "The other machine could not save its pairing state; try again in a moment."
            }
        }
    }
}

impl std::fmt::Display for PairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// The signed body of a pairing link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairClaim {
    /// Format version (1).
    pub v: u8,
    /// 16 random bytes, hex. The single-use handle.
    pub nonce: String,
    /// The issuer's pairing public key, hex (32 bytes).
    pub issuer_pub: String,
    /// The issuing device's label, as the member named it.
    pub issuer_label: String,
    /// The issuing device's tier id (`T0`/`T1`/`T2`), when known.
    pub issuer_tier: Option<String>,
    /// Unix seconds.
    pub issued_at: u64,
    /// Unix seconds; `issued_at + PAIR_TTL_SECS`.
    pub expires_at: u64,
    /// `ip:port` addresses where the issuer listens for this pairing (LAN, tailnet).
    pub hints: Vec<String>,
}

/// A QR code as rows of `'1'` (dark) / `'0'` (light) modules, without the quiet zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrMatrix {
    pub size: usize,
    pub rows: Vec<String>,
}

/// The bytes the pairing key signs.
pub fn signing_bytes(claim_json: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(DOMAIN.len() + claim_json.len());
    v.extend_from_slice(DOMAIN);
    v.extend_from_slice(claim_json);
    v
}

/// `citrate://pair?c=…&s=…` for a claim's JSON bytes and its signature.
pub fn encode_link(claim_json: &[u8], sig: &[u8; 64]) -> String {
    format!(
        "{LINK_PREFIX}c={}&s={}",
        URL_SAFE_NO_PAD.encode(claim_json),
        URL_SAFE_NO_PAD.encode(sig)
    )
}

fn label_ok(label: &str) -> bool {
    !label.trim().is_empty()
        && label.chars().count() <= MAX_LABEL_LEN
        && !label.chars().any(char::is_control)
}

fn hints_ok(hints: &[String]) -> bool {
    hints.len() <= MAX_HINTS
        && hints
            .iter()
            .all(|h| h.len() <= 64 && h.parse::<SocketAddr>().is_ok())
}

/// Shape checks on a decoded claim (independent of signature and clock).
fn claim_shape_ok(c: &PairClaim) -> bool {
    c.v == 1
        && c.nonce.len() == 32
        && hex::decode(&c.nonce).is_ok()
        && c.issuer_pub.len() == 64
        && label_ok(&c.issuer_label)
        && c.issuer_tier
            .as_deref()
            .is_none_or(|t| matches!(t, "T0" | "T1" | "T2"))
        && c.expires_at > c.issued_at
        && c.expires_at - c.issued_at <= PAIR_TTL_SECS
        && hints_ok(&c.hints)
}

/// Split a link into (claim JSON bytes, parsed claim, signature). Shape-checked, NOT verified.
pub fn decode_link(link: &str) -> Result<(Vec<u8>, PairClaim, [u8; 64]), PairError> {
    if link.len() > MAX_LINK_LEN {
        return Err(PairError::Malformed);
    }
    let query = link.strip_prefix(LINK_PREFIX).ok_or(PairError::Malformed)?;
    let mut c = None;
    let mut s = None;
    for part in query.split('&') {
        match part.split_once('=') {
            Some(("c", v)) if c.is_none() => c = Some(v),
            Some(("s", v)) if s.is_none() => s = Some(v),
            _ => return Err(PairError::Malformed),
        }
    }
    let json = URL_SAFE_NO_PAD
        .decode(c.ok_or(PairError::Malformed)?)
        .map_err(|_| PairError::Malformed)?;
    let sig_v = URL_SAFE_NO_PAD
        .decode(s.ok_or(PairError::Malformed)?)
        .map_err(|_| PairError::Malformed)?;
    let sig: [u8; 64] = sig_v.try_into().map_err(|_| PairError::Malformed)?;
    let claim: PairClaim = serde_json::from_slice(&json).map_err(|_| PairError::Malformed)?;
    if !claim_shape_ok(&claim) {
        return Err(PairError::Malformed);
    }
    Ok((json, claim, sig))
}

fn verify_sig(key: &VerifyingKey, json: &[u8], sig: &[u8; 64]) -> Result<(), PairError> {
    key.verify_strict(&signing_bytes(json), &Signature::from_bytes(sig))
        .map_err(|_| PairError::BadSignature)
}

fn check_clock(c: &PairClaim, now: u64) -> Result<(), PairError> {
    if c.issued_at > now.saturating_add(CLOCK_SKEW_SECS) {
        return Err(PairError::NotYetValid);
    }
    if now >= c.expires_at {
        return Err(PairError::Expired);
    }
    Ok(())
}

/// **Offline check on the joining device:** shape, signature against the embedded issuer key,
/// and the clock. Says the link is intact and current; the issuer decides single use.
pub fn verify_link(link: &str, now: u64) -> Result<PairClaim, PairError> {
    let (json, claim, sig) = decode_link(link)?;
    let pub_bytes: [u8; 32] = hex::decode(&claim.issuer_pub)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(PairError::Malformed)?;
    let key = VerifyingKey::from_bytes(&pub_bytes).map_err(|_| PairError::Malformed)?;
    verify_sig(&key, &json, &sig)?;
    check_clock(&claim, now)?;
    Ok(claim)
}

/// Domain separation for the issuer's signed join reply.
const REPLY_DOMAIN: &[u8] = b"citrate/fleet-pair-reply/v1\0";
/// Domain separation for the confirmation code both screens show.
const CODE_DOMAIN: &[u8] = b"citrate/fleet-pair-code/v1\0";

/// What the issuer signs when it accepts a join: the token, who joined, and who it is.
pub fn reply_bytes(
    nonce: &str,
    joiner_id: &str,
    issuer_id: &str,
    issuer_label: &str,
    issuer_tier: Option<&str>,
) -> Vec<u8> {
    let body = serde_json::json!([nonce, joiner_id, issuer_id, issuer_label, issuer_tier]);
    let mut v = REPLY_DOMAIN.to_vec();
    v.extend_from_slice(body.to_string().as_bytes());
    v
}

/// The joiner's check that a reply came from the holder of the link's pairing key.
pub fn verify_reply(issuer_pub_hex: &str, proof_hex: &str, bytes: &[u8]) -> bool {
    let Some(pub_bytes) = hex::decode(issuer_pub_hex)
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
    else {
        return false;
    };
    let Some(sig) = hex::decode(proof_hex)
        .ok()
        .and_then(|b| <[u8; 64]>::try_from(b).ok())
    else {
        return false;
    };
    VerifyingKey::from_bytes(&pub_bytes)
        .map(|k| k.verify_strict(bytes, &Signature::from_bytes(&sig)).is_ok())
        .unwrap_or(false)
}

/// Six digits both machines compute for one pairing (token, issuer key, joining device) and show,
/// so the member can see the two screens are talking to each other.
pub fn confirmation_code(nonce: &str, issuer_pub: &str, joiner_id: &str) -> String {
    use sha2::Digest as _;
    let mut h = sha2::Sha256::new();
    h.update(CODE_DOMAIN);
    h.update(nonce.as_bytes());
    h.update(b"\0");
    h.update(issuer_pub.as_bytes());
    h.update(b"\0");
    h.update(joiner_id.as_bytes());
    let d = h.finalize();
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{n:06}")
}

/// Render a link as a QR module matrix.
pub fn qr_matrix(link: &str) -> Result<QrMatrix, String> {
    let code = qrcode::QrCode::new(link.as_bytes()).map_err(|e| e.to_string())?;
    let size = code.width();
    let colors = code.to_colors();
    let rows = colors
        .chunks(size)
        .map(|row| {
            row.iter()
                .map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' })
                .collect::<String>()
        })
        .collect();
    Ok(QrMatrix { size, rows })
}

/// This app's pairing issuer: the pairing key and the live token set.
pub struct PairIssuer {
    key: SigningKey,
    /// nonce → expires_at, for tokens issued and not yet used.
    outstanding: HashMap<String, u64>,
    /// nonce → expires_at, for tokens already redeemed (kept until they would have expired).
    used: HashMap<String, u64>,
    /// L7-WP02: where the token sets are persisted; `None` for an in-memory issuer.
    ledger: Option<PathBuf>,
}

/// The ledger file: nonces and expiry times only.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    v: u8,
    outstanding: BTreeMap<String, u64>,
    used: BTreeMap<String, u64>,
}

/// Read the ledger. Missing is empty; unreadable, oversized or malformed is also empty and the
/// file is left as it is until the next write (fail closed: every earlier link is unknown).
fn read_ledger(path: &Path) -> (HashMap<String, u64>, HashMap<String, u64>) {
    let read = || -> Option<Ledger> {
        let f = std::fs::File::open(path).ok()?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(f, MAX_LEDGER_BYTES + 1), &mut buf)
            .ok()?;
        if buf.len() as u64 > MAX_LEDGER_BYTES {
            return None;
        }
        serde_json::from_slice::<Ledger>(&buf)
            .ok()
            .filter(|l| l.v == 1)
    };
    match read() {
        Some(l) => (
            l.outstanding.into_iter().collect(),
            l.used.into_iter().collect(),
        ),
        None => (HashMap::new(), HashMap::new()),
    }
}

/// Write the ledger, owner-only, through a unique temp file and an atomic rename.
fn write_ledger(
    path: &Path,
    outstanding: &HashMap<String, u64>,
    used: &HashMap<String, u64>,
) -> std::io::Result<()> {
    let l = Ledger {
        v: 1,
        outstanding: outstanding.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        used: used.iter().map(|(k, v)| (k.clone(), *v)).collect(),
    };
    let bytes = serde_json::to_vec(&l).map_err(std::io::Error::other)?;
    let staged = crate::atomic_file::stage_secret(path, &bytes)?;
    crate::atomic_file::commit_staged(&staged, path)
}

/// This machine's pairing key seed from the keychain, minting and storing a fresh random one on
/// first use. A stored value of the wrong length is an error, never silently replaced.
fn load_or_mint_pairing_seed(keyring: &dyn Keyring) -> Result<Zeroizing<[u8; 32]>, String> {
    let stored = keyring
        .get(PAIRING_KEY_ACCOUNT)
        .map_err(|e| format!("pairing key keychain error: {e}"))?;
    let mut seed = Zeroizing::new([0u8; 32]);
    match stored {
        Some(mut bytes) => {
            let ok = bytes.len() == 32;
            if ok {
                seed.copy_from_slice(&bytes);
            }
            bytes.zeroize();
            if !ok {
                return Err("the stored pairing key is not 32 bytes".into());
            }
        }
        None => {
            rand::rngs::OsRng.fill_bytes(seed.as_mut());
            keyring
                .set(PAIRING_KEY_ACCOUNT, seed.as_ref())
                .map_err(|e| format!("pairing key keychain error: {e}"))?;
        }
    }
    Ok(seed)
}

impl PairIssuer {
    /// A fresh random pairing key held in memory for this run only (the fallback when the
    /// keychain cannot be used; nothing is persisted).
    pub fn new_random() -> Self {
        let mut seed = zeroize::Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(seed.as_mut());
        Self::from_seed_ref(&seed)
    }

    /// A pairing key from a fixed seed, in memory only (tests).
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self::from_seed_ref(&seed)
    }

    fn from_seed_ref(seed: &[u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(seed),
            outstanding: HashMap::new(),
            used: HashMap::new(),
            ledger: None,
        }
    }

    /// L7-WP02, production: the pairing key from the keychain (minted there on first use) and the
    /// token sets from `ledger`, so links survive a restart and stay single use across it.
    pub fn from_keyring(keyring: &dyn Keyring, ledger: &Path) -> Result<Self, String> {
        let seed = load_or_mint_pairing_seed(keyring)?;
        let mut me = Self::from_seed_ref(&seed);
        let (outstanding, used) = read_ledger(ledger);
        me.outstanding = outstanding;
        me.used = used;
        me.ledger = Some(ledger.to_path_buf());
        Ok(me)
    }

    /// Whether the token sets are persisted (false for the in-memory fallback).
    pub fn persistent(&self) -> bool {
        self.ledger.is_some()
    }

    /// The pairing public key, hex.
    pub fn public_hex(&self) -> String {
        hex::encode(self.key.verifying_key().to_bytes())
    }

    fn prune(&mut self, now: u64) {
        self.outstanding.retain(|_, exp| *exp > now);
        self.used.retain(|_, exp| *exp > now);
    }

    /// Live (unexpired, unused) tokens.
    pub fn outstanding(&self, now: u64) -> usize {
        self.outstanding.values().filter(|exp| **exp > now).count()
    }

    /// Mint a signed pairing link valid for [`PAIR_TTL_SECS`].
    pub fn issue(
        &mut self,
        label: &str,
        tier: Option<&str>,
        hints: Vec<String>,
        now: u64,
    ) -> Result<(PairClaim, String), PairError> {
        self.prune(now);
        if self.outstanding.len() >= MAX_OUTSTANDING {
            return Err(PairError::TooManyOutstanding);
        }
        let mut nonce = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let claim = PairClaim {
            v: 1,
            nonce: hex::encode(nonce),
            issuer_pub: self.public_hex(),
            issuer_label: label.trim().to_string(),
            issuer_tier: tier.map(str::to_string),
            issued_at: now,
            expires_at: now + PAIR_TTL_SECS,
            hints,
        };
        if !claim_shape_ok(&claim) {
            return Err(PairError::Malformed);
        }
        let json = serde_json::to_vec(&claim).map_err(|_| PairError::Malformed)?;
        let sig = self.key.sign(&signing_bytes(&json));
        self.outstanding
            .insert(claim.nonce.clone(), claim.expires_at);
        // Best effort: if this write fails the link still works in this run, and after a restart
        // it is unknown (refused), never redeemable twice.
        if let Some(p) = &self.ledger {
            let _ = write_ledger(p, &self.outstanding, &self.used);
        }
        let link = encode_link(&json, &sig.to_bytes());
        Ok((claim, link))
    }

    /// Sign the join reply for a redeemed token (see [`reply_bytes`]), hex.
    pub fn sign_reply(&self, bytes: &[u8]) -> String {
        hex::encode(self.key.sign(bytes).to_bytes())
    }

    /// **Authoritative redeem on the issuer:** the link must be signed by THIS issuer's key,
    /// current, and unused. Success consumes the token.
    pub fn redeem(&mut self, link: &str, now: u64) -> Result<PairClaim, PairError> {
        let (json, claim, sig) = decode_link(link)?;
        verify_sig(&self.key.verifying_key(), &json, &sig).map_err(|e| {
            if claim.issuer_pub != self.public_hex() {
                PairError::UnknownToken
            } else {
                e
            }
        })?;
        if self.used.contains_key(&claim.nonce) {
            return Err(PairError::AlreadyUsed);
        }
        if let Err(e) = check_clock(&claim, now) {
            self.outstanding.remove(&claim.nonce);
            return Err(e);
        }
        let exp = *self
            .outstanding
            .get(&claim.nonce)
            .ok_or(PairError::UnknownToken)?;
        let mut outstanding = self.outstanding.clone();
        let mut used = self.used.clone();
        outstanding.remove(&claim.nonce);
        used.insert(claim.nonce.clone(), exp);
        outstanding.retain(|_, e| *e > now);
        used.retain(|_, e| *e > now);
        // L7-WP02: the redeem is recorded before it is accepted. If it cannot be recorded it is
        // refused and the token stays open, so a restart can never make a used link usable again.
        if let Some(p) = &self.ledger {
            write_ledger(p, &outstanding, &used).map_err(|_| PairError::Unavailable)?;
        }
        self.outstanding = outstanding;
        self.used = used;
        Ok(claim)
    }
}

#[cfg(test)]
mod tests {
    include!("fleet_pairing_tests.rs");
}
