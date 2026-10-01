//! HUP-S8.2 (US-8.1) — fleet pairing tokens: short-lived, single-use, signed.
//!
//! "Connect my machines" offers a link (and the same link as a QR code) that another machine
//! running Citrate Core opens to pair with this one. The link carries a [`PairClaim`] signed by
//! this app's **pairing key**: an ed25519 key generated in memory when the app starts, never
//! written to disk, never derived from the wallet, and never used for anything else. It proves
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
//!
//! Link format (v1): `citrate://pair?c=<base64url(claim_json)>&s=<base64url(sig64)>`. The verifier
//! checks the signature over the exact `c` bytes it received (never a re-serialization).

use std::collections::HashMap;
use std::net::SocketAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};

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

/// This app's pairing issuer: the in-memory pairing key and the live token set.
pub struct PairIssuer {
    key: SigningKey,
    /// nonce → expires_at, for tokens issued and not yet used.
    outstanding: HashMap<String, u64>,
    /// nonce → expires_at, for tokens already redeemed (kept until they would have expired).
    used: HashMap<String, u64>,
}

impl PairIssuer {
    /// A fresh random pairing key (production: once per app run).
    pub fn new_random() -> Self {
        let mut seed = zeroize::Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(seed.as_mut());
        Self::from_seed(*seed)
    }

    /// A pairing key from a fixed seed (tests).
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(&seed),
            outstanding: HashMap::new(),
            used: HashMap::new(),
        }
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
        let link = encode_link(&json, &sig.to_bytes());
        Ok((claim, link))
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
        let exp = self
            .outstanding
            .remove(&claim.nonce)
            .ok_or(PairError::UnknownToken)?;
        self.used.insert(claim.nonce.clone(), exp);
        self.prune(now);
        Ok(claim)
    }
}

#[cfg(test)]
mod tests {
    include!("fleet_pairing_tests.rs");
}
