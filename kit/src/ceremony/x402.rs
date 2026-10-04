//! HUP-S1.5 (core half): x402 payment authorizations for registry escalation.
//!
//! ADR-2026-09-30-rule3-budgetable-signatures, D3 fixes the one EIP-712 form an escalation may pay
//! with: the EIP-3009 `TransferWithAuthorization(from, to, value, validAfter, validBefore, nonce)`
//! that the x402 `exact` EVM scheme uses, on an asset whose EIP-712 domain is pinned. This module
//! holds that form and nothing else:
//!
//! - **Core builds the bytes it signs.** [`build_authorization`] takes the structured request
//!   (payer, payee, amount) and sets the validity window itself; the 32-byte nonce comes from the OS
//!   CSPRNG ([`fresh_nonce`]). No caller-supplied typed-data JSON is ever hashed here.
//! - **One type, one hasher.** [`signing_digest`] is `keccak256(0x1901 ‖ domainSeparator ‖
//!   structHash)` for exactly the pinned type. Its golden vectors are checked against the deployed
//!   WrappedSALT on chain 40204 (`DOMAIN_SEPARATOR()`) and against `cast` (see the tests).
//! - **HIC-1 only in this build.** The ceremony signs one of these only after the member approves
//!   that ceremony id ([`super::SignatureCeremony::request_x402`]). The B-2 budget (auto-approval)
//!   stays inert while the asset allowlist is empty (ADR owner decision O-1).
//!
//! Nothing in this module signs. The only signer call is in `ceremony.rs` (`approve`), which the
//! structural tripwire in `ceremony_tests.rs` enforces.

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

/// The pinned primary type (ADR D3). Byte-identical to WrappedSALT's
/// `TRANSFER_WITH_AUTHORIZATION_TYPEHASH` preimage.
pub const TRANSFER_WITH_AUTHORIZATION_TYPE: &str = "TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)";
/// The EIP-712 domain type WrappedSALT uses.
pub const EIP712_DOMAIN_TYPE: &str =
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
/// ADR D3 cap `validity_max`: an authorization is valid for at most ten minutes after it is built.
pub const VALIDITY_MAX_SECS: u64 = 600;
/// `validAfter` is set this far in the past so a chain clock slightly behind ours still accepts it
/// (EIP-3009 requires `block.timestamp > validAfter`).
pub const VALID_AFTER_SKEW_SECS: u64 = 60;
/// The shortest validity the builder accepts (a window too short to deliver is a mistake).
pub const VALIDITY_MIN_SECS: u64 = 30;

/// Why an authorization could not be built or checked. Never carries key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X402Error {
    BadAddress(&'static str),
    BadAmount,
    BadWindow,
    BadDomain,
    BadSignature,
}

impl std::fmt::Display for X402Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            X402Error::BadAddress(w) => write!(f, "x402: the {w} is not a 20-byte hex address"),
            X402Error::BadAmount => f.write_str("x402: the amount must be a positive whole number of base units"),
            X402Error::BadWindow => write!(
                f,
                "x402: the validity window must be {VALIDITY_MIN_SECS} to {VALIDITY_MAX_SECS} seconds"
            ),
            X402Error::BadDomain => f.write_str("x402: the asset's EIP-712 domain is incomplete"),
            X402Error::BadSignature => f.write_str("x402: the signature does not recover to the payer"),
        }
    }
}

impl std::error::Error for X402Error {}

/// The EIP-712 domain of an x402 asset (`{name, version, chainId, verifyingContract}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X402Domain {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    /// Lower-case `0x` + 40 hex.
    pub verifying_contract: String,
}

impl X402Domain {
    /// The domain of a pinned allowlist entry (`web_budget::X402_ASSET_ALLOWLIST`).
    pub fn from_asset(a: &crate::web_budget::X402Asset) -> Result<X402Domain, X402Error> {
        X402Domain::new(a.name, a.version, a.chain_id, a.verifying_contract)
    }

    /// Check and normalize a domain. Name and version must be non-empty printable text.
    pub fn new(
        name: &str,
        version: &str,
        chain_id: u64,
        verifying_contract: &str,
    ) -> Result<X402Domain, X402Error> {
        let printable = |s: &str| !s.is_empty() && s.len() <= 64 && !s.chars().any(char::is_control);
        if !printable(name) || !printable(version) || chain_id == 0 {
            return Err(X402Error::BadDomain);
        }
        let vc = parse_address(verifying_contract, "asset contract")?;
        Ok(X402Domain {
            name: name.to_string(),
            version: version.to_string(),
            chain_id,
            verifying_contract: fmt_address(&vc),
        })
    }
}

/// One `TransferWithAuthorization`, as core built it. Addresses are lower-case hex; `value` is a
/// decimal string of base units (it fits `u128`, which is checked); the nonce is `0x` + 64 hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X402Authorization {
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: u64,
    pub valid_before: u64,
    pub nonce: String,
}

/// Parse `0x` + 40 hex (any case) into 20 bytes.
pub fn parse_address(s: &str, what: &'static str) -> Result<[u8; 20], X402Error> {
    let h = s.strip_prefix("0x").ok_or(X402Error::BadAddress(what))?;
    if h.len() != 40 {
        return Err(X402Error::BadAddress(what));
    }
    let v = hex::decode(h).map_err(|_| X402Error::BadAddress(what))?;
    let mut out = [0u8; 20];
    out.copy_from_slice(&v);
    if out == [0u8; 20] {
        return Err(X402Error::BadAddress(what));
    }
    Ok(out)
}

/// Lower-case `0x` form.
pub fn fmt_address(a: &[u8; 20]) -> String {
    format!("0x{}", hex::encode(a))
}

/// A positive decimal amount of base units that fits `u128` (wSALT has 18 decimals, so that is
/// far beyond any supply).
pub fn parse_amount(s: &str) -> Result<u128, X402Error> {
    if s.is_empty() || s.len() > 39 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(X402Error::BadAmount);
    }
    match s.parse::<u128>() {
        Ok(0) | Err(_) => Err(X402Error::BadAmount),
        Ok(v) => Ok(v),
    }
}

fn parse_nonce(s: &str) -> Result<[u8; 32], X402Error> {
    let h = s.strip_prefix("0x").ok_or(X402Error::BadAmount)?;
    let v = hex::decode(h).map_err(|_| X402Error::BadAmount)?;
    v.as_slice().try_into().map_err(|_| X402Error::BadAmount)
}

/// 32 fresh bytes from the OS CSPRNG, as `0x` + 64 hex.
pub fn fresh_nonce() -> String {
    use rand::RngCore;
    let mut n = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut n);
    format!("0x{}", hex::encode(n))
}

/// Build an authorization core will ask the member to approve. `now_secs` is Unix seconds;
/// `validity_secs` must be within [`VALIDITY_MIN_SECS`]..=[`VALIDITY_MAX_SECS`].
pub fn build_authorization(
    from: &str,
    to: &str,
    value: &str,
    now_secs: u64,
    validity_secs: u64,
    nonce: String,
) -> Result<X402Authorization, X402Error> {
    let f = parse_address(from, "payer")?;
    let t = parse_address(to, "payee")?;
    let v = parse_amount(value)?;
    if !(VALIDITY_MIN_SECS..=VALIDITY_MAX_SECS).contains(&validity_secs) {
        return Err(X402Error::BadWindow);
    }
    parse_nonce(&nonce)?;
    Ok(X402Authorization {
        from: fmt_address(&f),
        to: fmt_address(&t),
        value: v.to_string(),
        valid_after: now_secs.saturating_sub(VALID_AFTER_SKEW_SECS),
        valid_before: now_secs.saturating_add(validity_secs),
        nonce: nonce.to_ascii_lowercase(),
    })
}

fn keccak(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Keccak256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn word_u128(v: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&v.to_be_bytes());
    w
}

fn word_u64(v: u64) -> [u8; 32] {
    word_u128(u128::from(v))
}

fn word_addr(a: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a);
    w
}

/// `keccak256(TRANSFER_WITH_AUTHORIZATION_TYPE)`.
pub fn type_hash() -> [u8; 32] {
    keccak(&[TRANSFER_WITH_AUTHORIZATION_TYPE.as_bytes()])
}

/// The EIP-712 domain separator, as WrappedSALT computes it.
pub fn domain_separator(d: &X402Domain) -> Result<[u8; 32], X402Error> {
    let vc = parse_address(&d.verifying_contract, "asset contract")?;
    Ok(keccak(&[
        &keccak(&[EIP712_DOMAIN_TYPE.as_bytes()]),
        &keccak(&[d.name.as_bytes()]),
        &keccak(&[d.version.as_bytes()]),
        &word_u64(d.chain_id),
        &word_addr(&vc),
    ]))
}

/// The struct hash of one authorization.
pub fn struct_hash(a: &X402Authorization) -> Result<[u8; 32], X402Error> {
    let from = parse_address(&a.from, "payer")?;
    let to = parse_address(&a.to, "payee")?;
    let value = parse_amount(&a.value)?;
    let nonce = parse_nonce(&a.nonce)?;
    Ok(keccak(&[
        &type_hash(),
        &word_addr(&from),
        &word_addr(&to),
        &word_u128(value),
        &word_u64(a.valid_after),
        &word_u64(a.valid_before),
        &nonce,
    ]))
}

/// `keccak256(0x1901 ‖ domainSeparator ‖ structHash)`: the bytes the wallet key signs.
pub fn signing_digest(d: &X402Domain, a: &X402Authorization) -> Result<[u8; 32], X402Error> {
    let ds = domain_separator(d)?;
    let sh = struct_hash(a)?;
    Ok(keccak(&[&[0x19, 0x01], &ds, &sh]))
}

/// Recover the address that produced `sig` (`r ‖ s ‖ v`, `v` in {27, 28} or {0, 1}) over `digest`.
/// High-s signatures are refused, matching WrappedSALT's malleability-safe recovery.
pub fn recover_signer(digest: &[u8; 32], sig: &[u8]) -> Result<String, X402Error> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    if sig.len() != 65 {
        return Err(X402Error::BadSignature);
    }
    let signature = Signature::from_slice(&sig[..64]).map_err(|_| X402Error::BadSignature)?;
    if signature.normalize_s().is_some() {
        return Err(X402Error::BadSignature);
    }
    let v = if sig[64] >= 27 { sig[64] - 27 } else { sig[64] };
    let rec = RecoveryId::from_byte(v).ok_or(X402Error::BadSignature)?;
    let vk = VerifyingKey::recover_from_prehash(digest, &signature, rec)
        .map_err(|_| X402Error::BadSignature)?;
    let enc = vk.to_encoded_point(false);
    let h = keccak(&[&enc.as_bytes()[1..]]);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h[12..]);
    Ok(fmt_address(&a))
}

/// Check that `sig_hex` is the payer's signature over this exact authorization and domain.
pub fn verify(d: &X402Domain, a: &X402Authorization, sig_hex: &str) -> Result<(), X402Error> {
    let raw = hex::decode(sig_hex.strip_prefix("0x").unwrap_or(sig_hex))
        .map_err(|_| X402Error::BadSignature)?;
    let signer = recover_signer(&signing_digest(d, a)?, &raw)?;
    if signer.eq_ignore_ascii_case(&a.from) {
        Ok(())
    } else {
        Err(X402Error::BadSignature)
    }
}

/// `1234500000000000000` base units with 18 decimals -> `"1.2345"`.
pub fn format_units(value: u128, decimals: u32) -> String {
    let scale = 10u128.pow(decimals);
    let whole = value / scale;
    let frac = value % scale;
    if frac == 0 {
        return whole.to_string();
    }
    let f = format!("{:0width$}", frac, width = decimals as usize);
    format!("{whole}.{}", f.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    include!("x402_tests.rs");
}
