//! v2 invite links (server-blind envelope) — the in-app client for the citrate.ai join service.
//!
//! GROW-S1 task 10. The join service (citrate-landing, `https://www.citrate.ai/api/join`) stores a
//! short code plus inviter-approved display data. For a **direct** invite it also stores an opaque
//! sealed envelope that only the holder of the share link can open:
//!
//! 1. **Reserve.** `POST /api/join` (bearer = the member's auth.citrate.ai access token, via
//!    [`BearerSource`]) → `{code, status:"pending", aad:{v, kind, code}}`.
//! 2. **Seal.** The app draws a random 128-bit link key `K`, derives the AEAD key with
//!    `HKDF-SHA256(ikm = K, salt = none, info = "citrate-join-envelope-v1")`, and seals the relay invite
//!    `{g,t,k,n}` with XChaCha20-Poly1305 under a random 192-bit nonce. The additional
//!    authenticated data is the UTF-8 string `citrate-join-v1|<kind>|<code>` ([`aad_bytes`]).
//! 3. **Attach.** `PUT /api/join/<code>` `{env, envVersion:1}` → `{status:"valid", envHash}`.
//! 4. **Share.** `https://citrate.ai/join/<code>#<K>` ([`share_url`]). The fragment is never sent
//!    to a server, so the service never holds anything that admits a member.
//!
//! The invitee resolves the code (`GET /api/join/<code>`), verifies the Ed25519 resolve signature
//! against a **pinned set of key ids** ([`JOIN_PINNED_KIDS`]), checks every response field against
//! the signed claims (including `envHash`), and opens the envelope with `K` from the link.
//!
//! ## Envelope wire format (version 1)
//! `env` = unpadded base64url of `nonce (24 bytes) || ciphertext || Poly1305 tag (16 bytes)`; the
//! plaintext is the UTF-8 JSON object `{"g":…,"t":…,"k":…,"n":…}` (`n` omitted when empty).
//! `envHash` = unpadded base64url of `SHA-256(env bytes)` (the decoded bytes, not the text).
//!
//! ## Secrets
//! `K`, the envelope plaintext, the relay token and the bearer never appear in a log line, an error
//! string or a `Debug` rendering ([`log_line`], [`redact_code`]). The only value that crosses
//! `invoke` and contains `K` is the final share URL returned by a successful mint.
//!
//! Data sources (Rule 7): the citrate.ai join service (citrate-landing `src/app/api/join/*`, rows
//! in the landing Postgres) for mint / attach / resolve / revoke / list; `GET /api/join/jwks` for the
//! verify keys, accepted only under a pinned kid; the relay invite comes from the caller
//! (`group_invite_create` in citrate-core).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::oidc::{classify_ureq, AuthError, BearerSource, HttpClient};

// ---------------------------------------------------------------------------
// Pinned contract constants
// ---------------------------------------------------------------------------

/// The join API base. `www` on purpose: the apex 307-redirects, and a redirect is never followed
/// with a bearer attached.
pub const JOIN_API_BASE: &str = "https://www.citrate.ai/api/join";
/// The share-link base (the public join page). The fragment `#K` is appended by [`share_url`].
pub const JOIN_SHARE_BASE: &str = "https://citrate.ai/join";
/// The `iss` of a resolve signature (citrate-landing `JOIN_ISSUER`).
pub const JOIN_ISSUER: &str = "https://citrate.ai/join";

/// The resolve-signature key ids this release trusts: the CURRENT kid and the NEXT kid.
///
/// ROTATION NOTE. A fetched JWKS key is used only when its `kid` is in this set, so a compromised
/// or swapped JWKS can't introduce a new signer. To rotate without breaking shipped apps: the
/// service publishes `join-2` as `JOIN_SIGNING_JWK_NEXT`, a release ships with
/// `["join-2", "join-3"]` once `join-2` is current, and the service only signs with a kid every
/// supported release pins. Never sign with a kid that is not in the oldest supported release's set.
pub const JOIN_PINNED_KIDS: &[&str] = &["join-1", "join-2"];

/// The envelope format version this client seals and opens (`envVersion`).
pub const ENVELOPE_VERSION: u32 = 1;
/// HKDF `info` for envelope version 1: a domain label of its own, deliberately distinct from the
/// AAD prefix (`citrate-join-v<envVersion>|…`, see [`aad_bytes`]), so the key-derivation context and
/// the AEAD binding can never be confused with each other.
pub const ENVELOPE_HKDF_INFO: &str = "citrate-join-envelope-v1";
/// The service's envelope cap (decoded bytes).
pub const ENVELOPE_MAX_BYTES: usize = 2048;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// The link key `K`: 128 bits, 22 base64url characters.
pub const LINK_KEY_LEN: usize = 16;
const LINK_KEY_B64_LEN: usize = 22;

/// Short-code alphabet and length (citrate-landing `src/lib/join.ts` `CODE_RE`).
const CODE_ALPHABET: &str = "23456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
pub const CODE_LEN: usize = 8;

/// The goal enum the service accepts.
pub const JOIN_GOALS: &[&str] = &["storage", "training", "inference", "dapps", "general"];

/// Clock leeway for the resolve signature's `exp`.
const JWT_LEEWAY_SECS: u64 = 60;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A join error. Display strings are safe to log and to show: they never contain a code, a link
/// key, an envelope, a relay token or a bearer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinError {
    /// No auth.citrate.ai session.
    NotSignedIn,
    /// 401: the session token was rejected (refresh / sign in again).
    Unauthorized,
    /// 503 or an identity outage behind the service.
    Unavailable,
    /// 422: the cluster name is not allowed: a reserved or confusable term, letters from a script
    /// other than Latin, Greek, Cyrillic or Cherokee, or a mix of scripts (landing `checkClusterName`).
    /// Distinct so the inviter UI can say what to change.
    NameNotAllowed,
    /// 429: quota or rate limit.
    RateLimited,
    /// The uniform 404: unknown, expired, revoked, exhausted, pending or not yours. Deliberately ONE
    /// variant so nothing upstream can tell those apart.
    NotFound,
    /// 409: a different envelope is already attached to this code.
    Conflict,
    /// The request was malformed (client-side check, 400 or 413).
    Invalid,
    /// The resolve signature, its key, or a signed field failed verification.
    Signature,
    /// The envelope did not open (wrong key, wrong code/kind binding, or tampered bytes).
    Envelope,
    /// The service answered something that does not match the contract.
    BadResponse,
    /// The service could not be reached.
    Network,
    /// Some other HTTP status (carries only the status).
    Rejected(u16),
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let JoinError::Rejected(s) = self {
            return write!(
                f,
                "join: the invite service rejected the request (HTTP {s})"
            );
        }
        f.write_str(match self {
            JoinError::NotSignedIn => "join: sign in to create invite links",
            JoinError::Unauthorized => "join: session expired or not authorized, sign in again",
            JoinError::Unavailable => "join: invite links are temporarily unavailable",
            JoinError::NameNotAllowed => {
                "join: that cluster name isn't allowed on an invite. Use letters from one of Latin, \
                 Greek, Cyrillic or Cherokee (not mixed), digits and spaces, and avoid reserved \
                 words such as \"official\" or \"support\""
            }
            JoinError::RateLimited => "join: too many invite requests, try again later",
            JoinError::NotFound => "join: invite not found",
            JoinError::Conflict => "join: this invite already has a different envelope",
            JoinError::Invalid => "join: invalid invite request",
            JoinError::Signature => "join: the invite could not be verified",
            JoinError::Envelope => "join: the invite could not be opened",
            JoinError::BadResponse => "join: unexpected reply from the invite service",
            JoinError::Network => "join: could not reach the invite service",
            JoinError::Rejected(_) => "join: the invite service rejected the request",
        })
    }
}
impl std::error::Error for JoinError {}

/// Map the HTTP seam's error to a join error. The status code is the only thing carried out of a
/// response; bodies are never read on failure.
pub fn map_http_error(e: AuthError) -> JoinError {
    match e {
        AuthError::NotSignedIn => JoinError::NotSignedIn,
        AuthError::Unauthorized => JoinError::Unauthorized,
        AuthError::Rejected(404) => JoinError::NotFound,
        AuthError::Rejected(409) => JoinError::Conflict,
        AuthError::Rejected(422) => JoinError::NameNotAllowed,
        AuthError::Rejected(429) => JoinError::RateLimited,
        AuthError::Rejected(503) | AuthError::Unavailable => JoinError::Unavailable,
        AuthError::Rejected(400) | AuthError::Rejected(413) => JoinError::Invalid,
        AuthError::Rejected(403) => JoinError::Unauthorized,
        AuthError::Rejected(s) => JoinError::Rejected(s),
        _ => JoinError::Network,
    }
}

// ---------------------------------------------------------------------------
// Log redaction
// ---------------------------------------------------------------------------

/// Log-safe form of a short code: the first two characters, the tail masked. A string that is not
/// code-shaped is never echoed.
pub fn redact_code(code: &str) -> String {
    if is_code(code) {
        format!("{}******", &code[..2])
    } else {
        "<code>".to_string()
    }
}

/// Log-safe form of a link: scheme and host only, no path (the code), query or fragment (the key).
/// Copy-adapted from the deep-link delivery redaction in citrate-core (core#346 `log_redact`); it
/// lives here so the join client does not depend on that branch.
pub fn redact_link(link: &str) -> String {
    match url::Url::parse(link.trim()) {
        Ok(u) => format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")),
        Err(_) => "<link>".to_string(),
    }
}

/// Build the one log line format the join client emits: an event name, the redacted code and the
/// error's safe Display. Nothing else is interpolated, so no secret can reach a log.
pub fn log_line(event: &str, code: Option<&str>, err: Option<&JoinError>) -> String {
    let mut s = format!("[join] {event}");
    if let Some(c) = code {
        s.push_str(" code=");
        s.push_str(&redact_code(c));
    }
    if let Some(e) = err {
        s.push_str(" err=");
        s.push_str(&e.to_string());
    }
    s
}

fn emit(event: &str, code: Option<&str>, err: Option<&JoinError>) {
    eprintln!("{}", log_line(event, code, err));
}

// ---------------------------------------------------------------------------
// Codes, kinds, keys, links
// ---------------------------------------------------------------------------

/// True when `s` is exactly a short code (8 characters from the unambiguous alphabet).
pub fn is_code(s: &str) -> bool {
    s.len() == CODE_LEN && s.chars().all(|c| CODE_ALPHABET.contains(c))
}

fn check_code(code: &str) -> Result<(), JoinError> {
    if is_code(code) {
        Ok(())
    } else {
        Err(JoinError::Invalid)
    }
}

/// Invite kind: `direct` (actionable: carries an envelope whose relay invite redeems once) or
/// `referral` (display only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JoinKind {
    Direct,
    Referral,
}

impl JoinKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JoinKind::Direct => "direct",
            JoinKind::Referral => "referral",
        }
    }
}

/// The 128-bit link key `K`. Wiped on drop; `Debug` never prints it.
#[derive(Clone)]
pub struct LinkKey(Zeroizing<[u8; LINK_KEY_LEN]>);

impl LinkKey {
    /// A fresh random key from the OS RNG.
    pub fn generate() -> Self {
        let mut k = Zeroizing::new([0u8; LINK_KEY_LEN]);
        OsRng.fill_bytes(&mut k[..]);
        LinkKey(k)
    }

    /// Parse the link fragment: exactly 22 canonical unpadded base64url characters (16 bytes).
    pub fn from_fragment(s: &str) -> Result<Self, JoinError> {
        if s.len() != LINK_KEY_B64_LEN || !s.bytes().all(is_b64url_byte) {
            return Err(JoinError::Invalid);
        }
        let raw = Zeroizing::new(URL_SAFE_NO_PAD.decode(s).map_err(|_| JoinError::Invalid)?);
        if raw.len() != LINK_KEY_LEN {
            return Err(JoinError::Invalid);
        }
        let mut k = Zeroizing::new([0u8; LINK_KEY_LEN]);
        k.copy_from_slice(&raw);
        let key = LinkKey(k);
        // Canonical only (no non-zero trailing bits), so one key has one spelling.
        if key.to_fragment().as_str() != s {
            return Err(JoinError::Invalid);
        }
        Ok(key)
    }

    /// The 22-character fragment form.
    pub fn to_fragment(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(&self.0[..]))
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(b: [u8; LINK_KEY_LEN]) -> Self {
        LinkKey(Zeroizing::new(b))
    }
}

impl std::fmt::Debug for LinkKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LinkKey(<redacted>)")
    }
}

fn is_b64url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// The share link for a minted code: `https://citrate.ai/join/<code>` plus `#K` for a direct invite.
pub fn share_url(code: &str, key: Option<&LinkKey>) -> Result<Zeroizing<String>, JoinError> {
    check_code(code)?;
    Ok(Zeroizing::new(match key {
        Some(k) => format!("{JOIN_SHARE_BASE}/{code}#{}", k.to_fragment().as_str()),
        None => format!("{JOIN_SHARE_BASE}/{code}"),
    }))
}

/// Parse an invite link into its code and optional key. Accepts
/// `https://citrate.ai/join/<code>[#K]`, `https://www.citrate.ai/join/<code>[#K]`,
/// `citrate://join/<code>[#K]` and a bare `<code>[#K]`. A query string is ignored. A present but
/// malformed key is an error, never silently dropped.
pub fn parse_link(link: &str) -> Result<(String, Option<LinkKey>), JoinError> {
    let link = link.trim();
    let (pre, frag) = match link.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (link, None),
    };
    let code = if is_code(pre) {
        pre.to_string()
    } else {
        let u = url::Url::parse(pre).map_err(|_| JoinError::Invalid)?;
        let segs: Vec<&str> = u
            .path_segments()
            .map(|s| s.filter(|p| !p.is_empty()).collect())
            .unwrap_or_default();
        let code = match (u.scheme(), u.host_str()) {
            ("https", Some("citrate.ai" | "www.citrate.ai")) => match segs.as_slice() {
                ["join", c] => *c,
                _ => return Err(JoinError::Invalid),
            },
            ("citrate", Some("join")) => match segs.as_slice() {
                [c] => *c,
                _ => return Err(JoinError::Invalid),
            },
            _ => return Err(JoinError::Invalid),
        };
        if u.port().is_some() || !u.username().is_empty() || u.password().is_some() {
            return Err(JoinError::Invalid);
        }
        code.to_string()
    };
    check_code(&code)?;
    let key = match frag {
        None | Some("") => None,
        Some(f) => Some(LinkKey::from_fragment(f)?),
    };
    Ok((code, key))
}

// ---------------------------------------------------------------------------
// The sealed relay invite
// ---------------------------------------------------------------------------

/// The relay invite sealed inside a direct envelope: relay group `g`, single-use token `t`, claims
/// public key `k`, and the hex group name `n` (optional). Wiped on drop; `Debug` is redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct InvitePayload {
    pub g: String,
    pub t: String,
    pub k: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub n: String,
}

impl std::fmt::Debug for InvitePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InvitePayload(<redacted>)")
    }
}

const INVITE_FIELD_MAX: usize = 512;

fn invite_field_ok(v: &str, required: bool) -> bool {
    if v.is_empty() {
        return !required;
    }
    v.len() <= INVITE_FIELD_MAX
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'%'))
}

impl InvitePayload {
    fn validate(&self) -> Result<(), JoinError> {
        if invite_field_ok(&self.g, true)
            && invite_field_ok(&self.t, true)
            && invite_field_ok(&self.k, true)
            && invite_field_ok(&self.n, false)
        {
            Ok(())
        } else {
            Err(JoinError::Invalid)
        }
    }

    /// Parse the relay link `citrate://invite?g=…&t=…&k=…[&n=…]` that `group_invite_create` returns.
    pub fn from_invite_link(link: &str) -> Result<Self, JoinError> {
        let rest = link
            .trim()
            .strip_prefix("citrate://invite?")
            .ok_or(JoinError::Invalid)?;
        let (mut g, mut t, mut k, mut n) = (None, None, None, None);
        for pair in rest.split('&') {
            let (name, value) = pair.split_once('=').ok_or(JoinError::Invalid)?;
            let slot = match name {
                "g" => &mut g,
                "t" => &mut t,
                "k" => &mut k,
                "n" => &mut n,
                _ => continue,
            };
            if slot.is_some() {
                return Err(JoinError::Invalid);
            }
            *slot = Some(value.to_string());
        }
        let p = InvitePayload {
            g: g.ok_or(JoinError::Invalid)?,
            t: t.ok_or(JoinError::Invalid)?,
            k: k.ok_or(JoinError::Invalid)?,
            n: n.unwrap_or_default(),
        };
        p.validate()?;
        Ok(p)
    }

    /// Rebuild the relay link the existing redeem path consumes.
    pub fn to_invite_link(&self) -> Zeroizing<String> {
        Zeroizing::new(if self.n.is_empty() {
            format!("citrate://invite?g={}&t={}&k={}", self.g, self.t, self.k)
        } else {
            format!(
                "citrate://invite?g={}&t={}&k={}&n={}",
                self.g, self.t, self.k, self.n
            )
        })
    }
}

/// The AAD bytes for an envelope: the UTF-8 of `citrate-join-v<envVersion>|<kind>|<code>` (for the
/// current version, `citrate-join-v1|direct|<code>`). The same derivation as citrate-landing
/// `joinAadBytes`; both sides assert the shared vectors (`fixtures/join-aad-vectors.json`).
pub fn aad_bytes(kind: JoinKind, code: &str, env_version: u32) -> Result<Vec<u8>, JoinError> {
    check_code(code)?;
    if env_version == 0 {
        return Err(JoinError::Invalid);
    }
    Ok(format!("citrate-join-v{env_version}|{}|{code}", kind.as_str()).into_bytes())
}

fn derive_aead_key(key: &LinkKey) -> Result<Zeroizing<[u8; 32]>, JoinError> {
    let hk = Hkdf::<Sha256>::new(None, &key.0[..]);
    let mut okm = Zeroizing::new([0u8; 32]);
    hk.expand(ENVELOPE_HKDF_INFO.as_bytes(), &mut okm[..])
        .map_err(|_| JoinError::Envelope)?;
    Ok(okm)
}

/// Seal the relay invite under `K`, bound to `kind` and `code`. Returns the base64url envelope.
pub fn seal(
    key: &LinkKey,
    kind: JoinKind,
    code: &str,
    payload: &InvitePayload,
) -> Result<String, JoinError> {
    check_code(code)?;
    payload.validate()?;
    let aead_key = derive_aead_key(key)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&aead_key[..]).map_err(|_| JoinError::Envelope)?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let plaintext = Zeroizing::new(serde_json::to_vec(payload).map_err(|_| JoinError::Invalid)?);
    let aad = aad_bytes(kind, code, ENVELOPE_VERSION)?;
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| JoinError::Envelope)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    if out.len() > ENVELOPE_MAX_BYTES {
        return Err(JoinError::Invalid);
    }
    Ok(URL_SAFE_NO_PAD.encode(out))
}

fn decode_envelope(env: &str) -> Result<Vec<u8>, JoinError> {
    if env.is_empty() || !env.bytes().all(is_b64url_byte) {
        return Err(JoinError::Envelope);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(env)
        .map_err(|_| JoinError::Envelope)?;
    if bytes.len() < NONCE_LEN + TAG_LEN || bytes.len() > ENVELOPE_MAX_BYTES {
        return Err(JoinError::Envelope);
    }
    if URL_SAFE_NO_PAD.encode(&bytes) != env {
        return Err(JoinError::Envelope);
    }
    Ok(bytes)
}

/// Open an envelope with `K`. Fails (`Envelope`) for a wrong key, a wrong `kind` or `code`, or any
/// changed byte.
pub fn open(
    key: &LinkKey,
    kind: JoinKind,
    code: &str,
    env: &str,
) -> Result<InvitePayload, JoinError> {
    check_code(code)?;
    let bytes = decode_envelope(env)?;
    let (nonce, ct) = bytes.split_at(NONCE_LEN);
    let aead_key = derive_aead_key(key)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&aead_key[..]).map_err(|_| JoinError::Envelope)?;
    let aad = aad_bytes(kind, code, ENVELOPE_VERSION)?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: &aad })
            .map_err(|_| JoinError::Envelope)?,
    );
    let p: InvitePayload = serde_json::from_slice(&plaintext).map_err(|_| JoinError::Envelope)?;
    p.validate().map_err(|_| JoinError::Envelope)?;
    Ok(p)
}

/// `envHash`: unpadded base64url of SHA-256 over the DECODED envelope bytes. Matches citrate-landing
/// `envelopeHash` (src/lib/join.ts:198 @ f704313):
/// `createHash("sha256").update(Buffer.from(envB64url, "base64url")).digest("base64url")`.
pub fn envelope_hash(env: &str) -> Result<String, JoinError> {
    let bytes = decode_envelope(env)?;
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes)))
}

// ---------------------------------------------------------------------------
// Resolve signature verification
// ---------------------------------------------------------------------------

/// The resolve response body (`GET /api/join/<code>`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveBody {
    pub ok: bool,
    pub v: u32,
    pub status: String,
    pub code: String,
    pub kind: JoinKind,
    #[serde(default)]
    pub cluster_name: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub inviter: Option<String>,
    pub inviter_verified: bool,
    pub exp: String,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub env_version: Option<u32>,
    #[serde(default)]
    pub env_hash: Option<String>,
    pub sig: String,
}

/// The signed claims (citrate-landing `signResolve`: every response field except `env`, which is
/// covered by `envHash`; the numeric JWT `exp` stands in for the ISO `exp`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveClaims {
    v: u32,
    status: String,
    code: String,
    kind: JoinKind,
    #[serde(default)]
    cluster_name: Option<String>,
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    inviter: Option<String>,
    inviter_verified: bool,
    #[serde(default)]
    env_version: Option<u32>,
    #[serde(default)]
    env_hash: Option<String>,
    exp: u64,
    sub: String,
}

/// One key from `GET /api/join/jwks`.
#[derive(Deserialize, Clone)]
pub struct Jwk {
    pub kty: String,
    #[serde(default)]
    pub crv: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub kid: Option<String>,
    #[serde(default)]
    pub alg: Option<String>,
}

/// The fetched key set.
#[derive(Deserialize, Clone)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

/// The display half of a verified invite: every field here was covered by the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedInvite {
    pub code: String,
    pub kind: JoinKind,
    pub cluster_name: Option<String>,
    pub goal: Option<String>,
    pub inviter: Option<String>,
    pub inviter_verified: bool,
    /// Expiry, unix seconds (from the signed `exp`).
    pub exp: u64,
    /// The ISO expiry string as sent (consistent with `exp`).
    pub exp_iso: String,
}

/// Verify a resolve response for `expected_code` against the fetched JWKS, accepting a key only
/// under one of `pinned_kids`. Every body field must equal its signed claim; a direct invite's
/// `env` must hash to the signed `envHash`; a referral must carry no envelope.
pub fn verify_resolve(
    body: &ResolveBody,
    expected_code: &str,
    jwks: &Jwks,
    pinned_kids: &[&str],
) -> Result<VerifiedInvite, JoinError> {
    check_code(expected_code)?;
    let header = jsonwebtoken::decode_header(&body.sig).map_err(|_| JoinError::Signature)?;
    if header.alg != Algorithm::EdDSA {
        return Err(JoinError::Signature);
    }
    let kid = header.kid.as_deref().ok_or(JoinError::Signature)?;
    if !pinned_kids.contains(&kid) {
        return Err(JoinError::Signature);
    }
    let jwk = jwks
        .keys
        .iter()
        .find(|k| k.kid.as_deref() == Some(kid))
        .ok_or(JoinError::Signature)?;
    if jwk.kty != "OKP"
        || jwk.crv.as_deref() != Some("Ed25519")
        || jwk.alg.as_deref().is_some_and(|a| a != "EdDSA")
    {
        return Err(JoinError::Signature);
    }
    let x = jwk.x.as_deref().ok_or(JoinError::Signature)?;
    if URL_SAFE_NO_PAD.decode(x).map(|b| b.len()).ok() != Some(32) {
        return Err(JoinError::Signature);
    }
    let dkey = DecodingKey::from_ed_components(x).map_err(|_| JoinError::Signature)?;
    let mut val = Validation::new(Algorithm::EdDSA);
    val.set_issuer(&[JOIN_ISSUER]);
    val.sub = Some(expected_code.to_string());
    val.validate_aud = false;
    val.leeway = JWT_LEEWAY_SECS;
    val.set_required_spec_claims(&["exp", "iss", "sub"]);
    let claims = jsonwebtoken::decode::<ResolveClaims>(&body.sig, &dkey, &val)
        .map_err(|_| JoinError::Signature)?
        .claims;

    // Every body field must be exactly what was signed.
    let same = body.ok
        && claims.sub == expected_code
        && claims.code == expected_code
        && body.code == expected_code
        && claims.v == 2
        && body.v == 2
        && claims.status == "valid"
        && body.status == "valid"
        && claims.kind == body.kind
        && claims.cluster_name == body.cluster_name
        && claims.goal == body.goal
        && claims.inviter == body.inviter
        && claims.inviter_verified == body.inviter_verified
        && claims.env_version == body.env_version
        && claims.env_hash == body.env_hash;
    if !same {
        return Err(JoinError::Signature);
    }
    let iso_secs = parse_iso_utc_secs(&body.exp).ok_or(JoinError::Signature)?;
    if iso_secs != claims.exp {
        return Err(JoinError::Signature);
    }
    match body.kind {
        JoinKind::Direct => {
            let env = body.env.as_deref().ok_or(JoinError::Signature)?;
            if body.env_version != Some(ENVELOPE_VERSION) {
                return Err(JoinError::Signature);
            }
            let h = envelope_hash(env).map_err(|_| JoinError::Signature)?;
            if claims.env_hash.as_deref() != Some(h.as_str()) {
                return Err(JoinError::Signature);
            }
        }
        JoinKind::Referral => {
            if body.env.is_some() || body.env_hash.is_some() || body.env_version.is_some() {
                return Err(JoinError::Signature);
            }
        }
    }
    Ok(VerifiedInvite {
        code: expected_code.to_string(),
        kind: body.kind,
        cluster_name: body.cluster_name.clone(),
        goal: body.goal.clone(),
        inviter: body.inviter.clone(),
        inviter_verified: body.inviter_verified,
        exp: claims.exp,
        exp_iso: body.exp.clone(),
    })
}

/// Parse a JavaScript `toISOString()` value (`YYYY-MM-DDTHH:MM:SS[.fff]Z`) to whole unix seconds
/// (floored, as `jose` floors a `Date` expiry).
pub fn parse_iso_utc_secs(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || *b.last()? != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let t = s.get(r)?;
        if t.bytes().all(|c| c.is_ascii_digit()) {
            t.parse().ok()
        } else {
            None
        }
    };
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let frac = &s[19..s.len() - 1];
    if !(frac.is_empty()
        || (frac.len() >= 2
            && frac.starts_with('.')
            && frac[1..].bytes().all(|c| c.is_ascii_digit())))
    {
        return None;
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    // Days from civil (Howard Hinnant), proleptic Gregorian.
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + se).ok()
}

// ---------------------------------------------------------------------------
// Service DTOs
// ---------------------------------------------------------------------------

/// What the inviter asks to mint.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MintRequest {
    pub kind: JoinKind,
    pub cluster_name: String,
    pub goal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_resolves: Option<u32>,
}

#[derive(Deserialize)]
struct AadObj {
    v: u32,
    kind: JoinKind,
    code: String,
}

#[derive(Deserialize)]
struct ReserveResp {
    ok: bool,
    code: String,
    kind: JoinKind,
    status: String,
    exp: String,
    #[serde(default)]
    aad: Option<AadObj>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachResp {
    ok: bool,
    code: String,
    kind: JoinKind,
    status: String,
    exp: String,
    env_version: u32,
    env_hash: String,
}

#[derive(Deserialize)]
struct RevokeResp {
    ok: bool,
    code: String,
    status: String,
}

/// One of the caller's codes (`GET /api/join/mine`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinCodeEntry {
    pub code: String,
    pub kind: JoinKind,
    /// `pending` | `valid` | `expired` | `revoked` | `exhausted`.
    pub status: String,
    pub exp: String,
    pub resolve_count: u64,
}

#[derive(Deserialize)]
struct MineResp {
    ok: bool,
    codes: Vec<JoinCodeEntry>,
}

/// The webview view of a mint. `share_url` is the ONLY field that contains `K`, and it is present
/// only once the envelope is attached (`status == "valid"`). `attach_error` explains a pending
/// direct code; retry with [`JoinClient::attach`].
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MintView {
    pub code: String,
    pub kind: JoinKind,
    pub status: String,
    pub exp: String,
    pub share_url: Option<String>,
    pub attach_error: Option<String>,
}

impl std::fmt::Debug for MintView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MintView")
            .field("code", &redact_code(&self.code))
            .field("kind", &self.kind)
            .field("status", &self.status)
            .field("share_url", &self.share_url.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// The webview view of a resolved invite: signed display data only. Never the envelope, the key
/// or the relay token. `actionable` = a direct invite that opened and is held for redeem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedView {
    pub code: String,
    pub kind: JoinKind,
    pub cluster_name: Option<String>,
    pub goal: Option<String>,
    pub inviter: Option<String>,
    pub inviter_verified: bool,
    pub exp: String,
    pub actionable: bool,
    pub key_missing: bool,
}

struct PendingAttach {
    key: LinkKey,
    env: String,
    exp: Option<u64>,
}

struct Opened {
    payload: InvitePayload,
    exp: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// HTTP: a redirect-free client for the join service
// ---------------------------------------------------------------------------

/// Per-request deadline for join-service calls.
const JOIN_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// The production HTTP client for the join service: blocking `ureq` with **redirects disabled**
/// (`max_redirects(0)`). A 3xx is never followed: the bearer and the request body are never replayed
/// to another location, and any non-2xx (including a 3xx) surfaces as `Rejected(status)`.
pub struct JoinHttp;

impl JoinHttp {
    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(JOIN_HTTP_TIMEOUT))
            .max_redirects(0)
            .build()
            .into()
    }

    fn run(
        method: &str,
        url: &str,
        bearer: Option<&str>,
        body: Option<&str>,
    ) -> Result<String, AuthError> {
        let mut b = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("Accept", "application/json");
        if body.is_some() {
            b = b.header("Content-Type", "application/json");
        }
        if let Some(tok) = bearer {
            b = b.header("Authorization", format!("Bearer {tok}"));
        }
        let req = b
            .body(body.unwrap_or("").to_string())
            .map_err(|_| AuthError::Network)?;
        let mut resp = Self::agent().run(req).map_err(|e| classify_ureq(&e))?;
        let status = resp.status().as_u16();
        // With redirects off, ureq hands a 3xx back as a response: refuse it.
        if !(200..300).contains(&status) {
            return Err(if status == 401 {
                AuthError::Unauthorized
            } else {
                AuthError::Rejected(status)
            });
        }
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AuthError::Network)
    }
}

impl HttpClient for JoinHttp {
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<String, AuthError> {
        Self::run("GET", url, bearer, None)
    }
    fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<String, AuthError> {
        // The join service takes JSON only.
        Err(AuthError::Network)
    }
    fn post_json(&self, url: &str, bearer: Option<&str>, body: &str) -> Result<String, AuthError> {
        Self::run("POST", url, bearer, Some(body))
    }
    fn put_json(&self, url: &str, bearer: Option<&str>, body: &str) -> Result<String, AuthError> {
        Self::run("PUT", url, bearer, Some(body))
    }
    fn delete_json(
        &self,
        url: &str,
        bearer: Option<&str>,
        body: &str,
    ) -> Result<String, AuthError> {
        Self::run("DELETE", url, bearer, Some(body))
    }
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

/// Endpoint configuration. Production is fixed ([`JoinConfig::production`]); tests point it at a
/// loopback stub.
#[derive(Debug, Clone)]
pub struct JoinConfig {
    pub(crate) api_base: String,
    pub(crate) pinned_kids: Vec<&'static str>,
}

impl JoinConfig {
    pub fn production() -> Self {
        JoinConfig {
            api_base: JOIN_API_BASE.to_string(),
            pinned_kids: JOIN_PINNED_KIDS.to_vec(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_tests(api_base: &str) -> Self {
        JoinConfig {
            api_base: api_base.trim_end_matches('/').to_string(),
            pinned_kids: JOIN_PINNED_KIDS.to_vec(),
        }
    }
}

/// The in-app join client. Holds, in memory only: direct codes whose envelope is sealed but not yet
/// attached (so a retry re-sends the identical, idempotent body), and opened invites waiting for
/// the member's explicit Join.
pub struct JoinClient {
    cfg: JoinConfig,
    http: Box<dyn HttpClient>,
    pending: Mutex<HashMap<String, PendingAttach>>,
    opened: Mutex<HashMap<String, Opened>>,
}

impl JoinClient {
    pub fn new(cfg: JoinConfig, http: Box<dyn HttpClient>) -> Self {
        JoinClient {
            cfg,
            http,
            pending: Mutex::new(HashMap::new()),
            opened: Mutex::new(HashMap::new()),
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingAttach>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn opened(&self) -> std::sync::MutexGuard<'_, HashMap<String, Opened>> {
        self.opened.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn prune(&self) {
        let now = now_secs();
        self.pending()
            .retain(|_, p| p.exp.map(|e| e > now).unwrap_or(true));
        self.opened().retain(|_, o| o.exp > now);
    }

    /// Mint an invite. `direct` needs the relay invite (`invite`) and returns the share link with
    /// `#K` once attached; `referral` takes no invite and returns the plain link.
    pub fn mint(
        &self,
        auth: &dyn BearerSource,
        req: &MintRequest,
        invite: Option<&InvitePayload>,
    ) -> Result<MintView, JoinError> {
        self.prune();
        let out = self.mint_inner(auth, req, invite);
        match &out {
            Ok(v) => emit(
                &format!("mint {} {}", v.kind.as_str(), v.status),
                Some(&v.code),
                None,
            ),
            Err(e) => emit("mint failed", None, Some(e)),
        }
        out
    }

    fn mint_inner(
        &self,
        auth: &dyn BearerSource,
        req: &MintRequest,
        invite: Option<&InvitePayload>,
    ) -> Result<MintView, JoinError> {
        if req.cluster_name.trim().is_empty() || !JOIN_GOALS.contains(&req.goal.as_str()) {
            return Err(JoinError::Invalid);
        }
        match (req.kind, invite) {
            (JoinKind::Direct, Some(p)) => p.validate()?,
            (JoinKind::Referral, None) => {}
            _ => return Err(JoinError::Invalid),
        }
        let bearer = auth.bearer().map_err(map_http_error)?;
        let body = serde_json::to_string(req).map_err(|_| JoinError::Invalid)?;
        let resp = self
            .http
            .post_json(&self.cfg.api_base, Some(bearer.as_str()), &body)
            .map_err(map_http_error)?;
        let r: ReserveResp = serde_json::from_str(&resp).map_err(|_| JoinError::BadResponse)?;
        if !r.ok || !is_code(&r.code) || r.kind != req.kind {
            return Err(JoinError::BadResponse);
        }
        match (req.kind, invite) {
            (JoinKind::Referral, _) => {
                if r.status != "valid" {
                    return Err(JoinError::BadResponse);
                }
                Ok(MintView {
                    share_url: Some(share_url(&r.code, None)?.to_string()),
                    code: r.code,
                    kind: JoinKind::Referral,
                    status: r.status,
                    exp: r.exp,
                    attach_error: None,
                })
            }
            (JoinKind::Direct, Some(payload)) => {
                // The service names the AAD it expects; it must be exactly ours for this code.
                let a = r.aad.as_ref().ok_or(JoinError::BadResponse)?;
                if r.status != "pending"
                    || a.v != ENVELOPE_VERSION
                    || a.kind != JoinKind::Direct
                    || a.code != r.code
                {
                    return Err(JoinError::BadResponse);
                }
                let key = LinkKey::generate();
                let env = seal(&key, JoinKind::Direct, &r.code, payload)?;
                self.pending().insert(
                    r.code.clone(),
                    PendingAttach {
                        key,
                        env,
                        exp: parse_iso_utc_secs(&r.exp),
                    },
                );
                match self.attach_inner(bearer.as_str(), &r.code) {
                    Ok(v) => Ok(v),
                    // Reserved but not attached: report it honestly as pending so the caller can
                    // retry `attach` with the identical body (or revoke).
                    Err(e) => Ok(MintView {
                        code: r.code,
                        kind: JoinKind::Direct,
                        status: "pending".to_string(),
                        exp: r.exp,
                        share_url: None,
                        attach_error: Some(e.to_string()),
                    }),
                }
            }
            (JoinKind::Direct, None) => Err(JoinError::Invalid),
        }
    }

    /// Attach (or re-attach, idempotently) the sealed envelope of a pending direct code minted by
    /// this client. `NotFound` when this client holds no envelope for the code.
    pub fn attach(&self, auth: &dyn BearerSource, code: &str) -> Result<MintView, JoinError> {
        self.prune();
        let out = check_code(code)
            .and_then(|_| auth.bearer().map_err(map_http_error))
            .and_then(|b| self.attach_inner(b.as_str(), code));
        emit("attach", Some(code), out.as_ref().err());
        out
    }

    fn attach_inner(&self, bearer: &str, code: &str) -> Result<MintView, JoinError> {
        let (env, local_hash) = {
            let p = self.pending();
            let entry = p.get(code).ok_or(JoinError::NotFound)?;
            (entry.env.clone(), envelope_hash(&entry.env)?)
        };
        let body = serde_json::json!({ "env": env, "envVersion": ENVELOPE_VERSION }).to_string();
        let url = format!("{}/{code}", self.cfg.api_base);
        let resp = self
            .http
            .put_json(&url, Some(bearer), &body)
            .map_err(map_http_error)?;
        let r: AttachResp = serde_json::from_str(&resp).map_err(|_| JoinError::BadResponse)?;
        // The service must have stored exactly what we sealed.
        if !r.ok
            || r.code != code
            || r.kind != JoinKind::Direct
            || r.status != "valid"
            || r.env_version != ENVELOPE_VERSION
            || r.env_hash != local_hash
        {
            return Err(JoinError::BadResponse);
        }
        let entry = self.pending().remove(code).ok_or(JoinError::NotFound)?;
        Ok(MintView {
            share_url: Some(share_url(code, Some(&entry.key))?.to_string()),
            code: code.to_string(),
            kind: JoinKind::Direct,
            status: r.status,
            exp: r.exp,
            attach_error: None,
        })
    }

    /// Fetch the published resolve keys, keeping only pinned kids.
    fn fetch_jwks(&self) -> Result<Jwks, JoinError> {
        let url = format!("{}/jwks", self.cfg.api_base);
        let body = self.http.get(&url, None).map_err(map_http_error)?;
        let mut jwks: Jwks = serde_json::from_str(&body).map_err(|_| JoinError::BadResponse)?;
        jwks.keys.retain(|k| {
            k.kid
                .as_deref()
                .is_some_and(|kid| self.cfg.pinned_kids.contains(&kid))
                && k.kty == "OKP"
                && k.crv.as_deref() == Some("Ed25519")
                && k.x
                    .as_deref()
                    .and_then(|x| URL_SAFE_NO_PAD.decode(x).ok())
                    .is_some_and(|b| b.len() == 32)
        });
        // No usable pinned key: nothing could verify a resolve, so don't spend one.
        if jwks.keys.is_empty() {
            return Err(JoinError::Signature);
        }
        Ok(jwks)
    }

    /// Resolve an invite link. Verifies the signature against pinned keys and, for a direct invite
    /// with its key, opens the envelope and holds the relay invite in memory for
    /// [`JoinClient::take_opened`]. Unauthenticated: no bearer is sent.
    ///
    /// The JWKS is fetched and checked FIRST, so a key outage or an unusable key set fails before
    /// the service counts a resolve. Resolve is not redeem: the service allows a code up to its
    /// `maxResolves` (direct 1..=50, default 50; referral up to 5,000) and the relay redeem is the
    /// single-use step. Each call still counts, so resolve on the member's action, not on a preview.
    pub fn resolve(&self, link: &str) -> Result<ResolvedView, JoinError> {
        self.prune();
        let (code, key) = match parse_link(link) {
            Ok(v) => v,
            Err(e) => {
                emit(
                    &format!("resolve rejected link={}", redact_link(link)),
                    None,
                    Some(&e),
                );
                return Err(e);
            }
        };
        let out = self.resolve_inner(&code, key.as_ref());
        emit("resolve", Some(&code), out.as_ref().err());
        out
    }

    fn resolve_inner(&self, code: &str, key: Option<&LinkKey>) -> Result<ResolvedView, JoinError> {
        let jwks = self.fetch_jwks()?;
        let url = format!("{}/{code}", self.cfg.api_base);
        let resp = self.http.get(&url, None).map_err(map_http_error)?;
        let body: ResolveBody = serde_json::from_str(&resp).map_err(|_| JoinError::BadResponse)?;
        let v = verify_resolve(&body, code, &jwks, &self.cfg.pinned_kids)?;
        let mut actionable = false;
        let mut key_missing = false;
        if v.kind == JoinKind::Direct {
            match (key, body.env.as_deref()) {
                (Some(k), Some(env)) => {
                    let payload = open(k, JoinKind::Direct, code, env)?;
                    self.opened().insert(
                        code.to_string(),
                        Opened {
                            payload,
                            exp: v.exp,
                        },
                    );
                    actionable = true;
                }
                (None, _) => key_missing = true,
                (Some(_), None) => return Err(JoinError::Signature),
            }
        }
        Ok(ResolvedView {
            code: v.code,
            kind: v.kind,
            cluster_name: v.cluster_name,
            goal: v.goal,
            inviter: v.inviter,
            inviter_verified: v.inviter_verified,
            exp: v.exp_iso,
            actionable,
            key_missing,
        })
    }

    /// Hand over (and forget) the opened relay invite for `code`, for the member's explicit Join.
    pub fn take_opened(&self, code: &str) -> Option<InvitePayload> {
        self.prune();
        self.opened().remove(code).map(|o| o.payload)
    }

    /// Drop the opened relay invite for `code` without handing it over: the member cancelled the
    /// invite, or it was replaced or expired. A later `redeem_opened(code)` then fails NotFound
    /// until the link is resolved again (which needs its key).
    pub fn forget_opened(&self, code: &str) {
        self.opened().remove(code);
    }

    /// Redeem the opened relay invite for `code` with `redeem` (the self-admit call). The invite is
    /// forgotten only when `redeem` succeeds; on an error (relay unreachable, daemon down) it stays
    /// held until it expires, so the member can retry. `NotFound` (as its message) when nothing is
    /// held for the code.
    pub fn redeem_opened(
        &self,
        code: &str,
        redeem: impl FnOnce(&InvitePayload) -> Result<(), String>,
    ) -> Result<(), String> {
        self.prune();
        let payload = self
            .opened()
            .get(code)
            .map(|o| o.payload.clone())
            .ok_or_else(|| JoinError::NotFound.to_string())?;
        let out = redeem(&payload);
        match &out {
            Ok(()) => {
                self.opened().remove(code);
                emit("redeem", Some(code), None);
            }
            Err(_) => emit("redeem failed, invite kept for retry", Some(code), None),
        }
        out
    }

    /// Revoke one of the caller's codes. Idempotent at the service; drops any local state for it.
    pub fn revoke(&self, auth: &dyn BearerSource, code: &str) -> Result<(), JoinError> {
        let out = (|| {
            check_code(code)?;
            let bearer = auth.bearer().map_err(map_http_error)?;
            let url = format!("{}/{code}", self.cfg.api_base);
            let resp = self
                .http
                .delete_json(&url, Some(bearer.as_str()), "{}")
                .map_err(map_http_error)?;
            let r: RevokeResp = serde_json::from_str(&resp).map_err(|_| JoinError::BadResponse)?;
            if !r.ok || r.code != code || r.status != "revoked" {
                return Err(JoinError::BadResponse);
            }
            self.pending().remove(code);
            self.opened().remove(code);
            Ok(())
        })();
        emit("revoke", Some(code), out.as_ref().err());
        out
    }

    /// The caller's codes, newest first.
    pub fn list(&self, auth: &dyn BearerSource) -> Result<Vec<JoinCodeEntry>, JoinError> {
        let out = (|| {
            let bearer = auth.bearer().map_err(map_http_error)?;
            let url = format!("{}/mine", self.cfg.api_base);
            let resp = self
                .http
                .get(&url, Some(bearer.as_str()))
                .map_err(map_http_error)?;
            let r: MineResp = serde_json::from_str(&resp).map_err(|_| JoinError::BadResponse)?;
            if !r.ok || r.codes.iter().any(|c| !is_code(&c.code)) {
                return Err(JoinError::BadResponse);
            }
            Ok(r.codes)
        })();
        emit("list", None, out.as_ref().err());
        out
    }
}

#[cfg(test)]
mod tests {
    include!("join_tests.rs");
}
