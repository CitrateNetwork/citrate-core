//! HUP-S1.5 — the EIP-712 hasher (ADR-2026-09-30 D3 precondition).
//!
//! The lean `crypto` build had no domain-separated typed-data hasher, so `approve` refuses
//! `TypedData` outright (PBA-L4-007). This module supplies the hashing half the ADR asks for and
//! nothing else: it builds the bytes core would sign from **pinned templates**. It never parses
//! caller-supplied typed data and it never signs. Signing stays in [`crate::ceremony`] behind the
//! gated signer; the `TypedData` refusal there is unchanged.
//!
//! What is here:
//!
//! - the EIP-712 primitives: `typeHash`, `hashStruct` over pre-encoded words, the domain
//!   separator for `EIP712Domain(string name,string version,uint256 chainId,address
//!   verifyingContract)`, and the final `keccak256("\x19\x01" ‖ domainSeparator ‖ structHash)`;
//! - the one budgetable primary type (ADR D3, kind B-2): EIP-3009
//!   `TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256
//!   validBefore,bytes32 nonce)`, the form the x402 `exact` EVM scheme uses;
//! - a decoder view for the approval card (amount, asset, payee, validity), so the member sees the
//!   numbers that are hashed, never a hex blob.
//!
//! Tested against the published EIP-712 "Ether Mail" vectors and the EIP-3009 type hash, and
//! proven against the chain's `WrappedSALT` contract on a local anvil deploy
//! (`scripts/anvil-registry-dryrun.sh` in citrate-core).

use sha3::{Digest as _, Keccak256};

/// The EIP-712 domain type every allowlisted asset must use (name, version, chainId, contract).
pub const EIP712_DOMAIN_TYPE: &str =
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";

/// The one budgetable primary type (ADR D3). Any other type stays HIC-1.
pub const TRANSFER_WITH_AUTHORIZATION_TYPE: &str = "TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)";

/// A 32-byte word.
pub type Word = [u8; 32];

/// Why a typed-data value could not be built. Messages never carry key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eip712Error {
    /// Not a `0x` + 40 hex digit address.
    BadAddress,
    /// Not a decimal whole number, or larger than `2^256 - 1`.
    BadUint,
    /// `validBefore <= validAfter`.
    EmptyWindow,
}

impl std::fmt::Display for Eip712Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Eip712Error::BadAddress => write!(f, "not a 20-byte hex address"),
            Eip712Error::BadUint => {
                write!(f, "not a whole number in the uint256 range")
            }
            Eip712Error::EmptyWindow => {
                write!(
                    f,
                    "the validity window is empty (validBefore must be after validAfter)"
                )
            }
        }
    }
}

impl std::error::Error for Eip712Error {}

/// `keccak256(bytes)`.
pub fn keccak256(bytes: &[u8]) -> Word {
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

/// `typeHash = keccak256(encodeType)`. The caller passes the full canonical type string
/// (referenced struct types appended in alphabetical order, as EIP-712 requires).
pub fn type_hash(encode_type: &str) -> Word {
    keccak256(encode_type.as_bytes())
}

/// `hashStruct = keccak256(typeHash ‖ encodeData)`, where `fields` are the already-encoded 32-byte
/// members in declaration order (atomic values padded, dynamic values and nested structs hashed).
pub fn hash_struct(type_hash: &Word, fields: &[Word]) -> Word {
    let mut buf = Vec::with_capacity(32 * (1 + fields.len()));
    buf.extend_from_slice(type_hash);
    for f in fields {
        buf.extend_from_slice(f);
    }
    keccak256(&buf)
}

/// The signing digest: `keccak256("\x19\x01" ‖ domainSeparator ‖ structHash)`.
pub fn typed_data_digest(domain_separator: &Word, struct_hash: &Word) -> Word {
    let mut buf = [0u8; 66];
    buf[0] = 0x19;
    buf[1] = 0x01;
    buf[2..34].copy_from_slice(domain_separator);
    buf[34..66].copy_from_slice(struct_hash);
    keccak256(&buf)
}

/// An address left-padded to a word.
pub fn address_word(addr: &[u8; 20]) -> Word {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(addr);
    w
}

/// A `u64` as a big-endian uint256 word.
pub fn u64_word(v: u64) -> Word {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

/// A `string` member: `keccak256(utf8 bytes)`.
pub fn string_word(s: &str) -> Word {
    keccak256(s.as_bytes())
}

/// Parse `0x` + 40 hex digits (any case; no checksum is required or checked).
pub fn parse_address(s: &str) -> Result<[u8; 20], Eip712Error> {
    let hex_part = s.strip_prefix("0x").ok_or(Eip712Error::BadAddress)?;
    if hex_part.len() != 40 {
        return Err(Eip712Error::BadAddress);
    }
    let bytes = hex::decode(hex_part).map_err(|_| Eip712Error::BadAddress)?;
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Parse a decimal whole number into a big-endian uint256 word. Overflow past `2^256 - 1`, an
/// empty string, a sign, or any non-digit fails closed.
pub fn parse_u256_dec(s: &str) -> Result<Word, Eip712Error> {
    if s.is_empty() || s.len() > 78 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Eip712Error::BadUint);
    }
    // Four big-endian u64 limbs; multiply by ten and add each digit with carry.
    let mut limbs = [0u64; 4];
    for b in s.bytes() {
        let mut carry = u128::from(b - b'0');
        for limb in limbs.iter_mut().rev() {
            let v = u128::from(*limb) * 10 + carry;
            *limb = v as u64; // low 64 bits; the high bits carry on
            carry = v >> 64;
        }
        if carry != 0 {
            return Err(Eip712Error::BadUint);
        }
    }
    let mut w = [0u8; 32];
    for (i, limb) in limbs.iter().enumerate() {
        w[i * 8..i * 8 + 8].copy_from_slice(&limb.to_be_bytes());
    }
    Ok(w)
}

/// A big-endian uint256 word as a decimal string (the inverse of [`parse_u256_dec`]).
pub fn u256_to_dec(w: &Word) -> String {
    let mut limbs = [0u64; 4];
    for (i, limb) in limbs.iter_mut().enumerate() {
        let mut b = [0u8; 8];
        b.copy_from_slice(&w[i * 8..i * 8 + 8]);
        *limb = u64::from_be_bytes(b);
    }
    if limbs.iter().all(|&l| l == 0) {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while limbs.iter().any(|&l| l != 0) {
        // Divide by ten, collecting the remainder.
        let mut rem: u128 = 0;
        for limb in limbs.iter_mut() {
            let cur = (rem << 64) | u128::from(*limb);
            *limb = (cur / 10) as u64;
            rem = cur % 10;
        }
        digits.push(b'0' + rem as u8);
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

/// `amount` base units shown with `decimals` places, trailing zeros trimmed
/// (`1500000000000000000`, 18 → `1.5`).
pub fn format_units(amount: &Word, decimals: u32) -> String {
    let dec = u256_to_dec(amount);
    let d = decimals as usize;
    if d == 0 {
        return dec;
    }
    let padded = if dec.len() <= d {
        format!("{}{}", "0".repeat(d + 1 - dec.len()), dec)
    } else {
        dec
    };
    let (int, frac) = padded.split_at(padded.len() - d);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        int.to_string()
    } else {
        format!("{int}.{frac}")
    }
}

/// An EIP-712 domain with all four fields (the only shape an allowlisted asset may have).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    pub verifying_contract: [u8; 20],
}

impl Domain {
    /// `hashStruct(EIP712Domain)`.
    pub fn separator(&self) -> Word {
        hash_struct(
            &type_hash(EIP712_DOMAIN_TYPE),
            &[
                string_word(&self.name),
                string_word(&self.version),
                u64_word(self.chain_id),
                address_word(&self.verifying_contract),
            ],
        )
    }
}

/// EIP-3009 `TransferWithAuthorization`, built by core from a pinned template (ADR D3). Core
/// chooses `nonce` (OS CSPRNG) and the validity window; the sidecar never supplies either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferWithAuthorization {
    pub from: [u8; 20],
    pub to: [u8; 20],
    /// Base units of the asset (uint256, big-endian).
    pub value: Word,
    /// Unix seconds; the token accepts it strictly after this.
    pub valid_after: u64,
    /// Unix seconds; the token accepts it strictly before this.
    pub valid_before: u64,
    pub nonce: Word,
}

impl TransferWithAuthorization {
    /// Checked constructor: rejects an empty validity window.
    pub fn new(
        from: [u8; 20],
        to: [u8; 20],
        value: Word,
        valid_after: u64,
        valid_before: u64,
        nonce: Word,
    ) -> Result<Self, Eip712Error> {
        if valid_before <= valid_after {
            return Err(Eip712Error::EmptyWindow);
        }
        Ok(TransferWithAuthorization {
            from,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
        })
    }

    /// `hashStruct(TransferWithAuthorization)`.
    pub fn struct_hash(&self) -> Word {
        hash_struct(
            &type_hash(TRANSFER_WITH_AUTHORIZATION_TYPE),
            &[
                address_word(&self.from),
                address_word(&self.to),
                self.value,
                u64_word(self.valid_after),
                u64_word(self.valid_before),
                self.nonce,
            ],
        )
    }

    /// The digest the member's key signs for this authorization on `domain`.
    pub fn digest(&self, domain: &Domain) -> Word {
        typed_data_digest(&domain.separator(), &self.struct_hash())
    }

    /// What the approval card shows: the hashed numbers, readable (ADR D3 decoder precondition).
    pub fn view(&self, domain: &Domain, decimals: u32) -> AuthorizationView {
        AuthorizationView {
            primary_type: "TransferWithAuthorization".to_string(),
            asset_name: domain.name.clone(),
            asset_contract: format!("0x{}", hex::encode(domain.verifying_contract)),
            chain_id: domain.chain_id,
            from: format!("0x{}", hex::encode(self.from)),
            payee: format!("0x{}", hex::encode(self.to)),
            amount_base_units: u256_to_dec(&self.value),
            amount: format_units(&self.value, decimals),
            valid_after: self.valid_after,
            valid_before: self.valid_before,
            nonce: format!("0x{}", hex::encode(self.nonce)),
        }
    }
}

/// The `(v, r, s)` an on-chain canonical `ecrecover` accepts, from a recoverable secp256k1
/// signature: `s` is normalized to the lower half of the curve order (EIP-2; the chain's
/// `WrappedSALT._recoverCanonical` rejects a high `s`), the recovery parity is flipped to match,
/// and `v` is 27 or 28. A signer that skips this is refused by the contract about half the time.
pub fn canonical_vrs(
    sig: &k256::ecdsa::Signature,
    recid: k256::ecdsa::RecoveryId,
) -> (u8, Word, Word) {
    let (sig, y_odd) = match sig.normalize_s() {
        Some(low) => (low, !recid.is_y_odd()),
        None => (*sig, recid.is_y_odd()),
    };
    let bytes = sig.to_bytes();
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&bytes[..32]);
    s.copy_from_slice(&bytes[32..]);
    (27 + u8::from(y_odd), r, s)
}

/// The approval-card view of an authorization.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationView {
    pub primary_type: String,
    pub asset_name: String,
    pub asset_contract: String,
    pub chain_id: u64,
    pub from: String,
    pub payee: String,
    pub amount_base_units: String,
    pub amount: String,
    pub valid_after: u64,
    pub valid_before: u64,
    pub nonce: String,
}

#[cfg(test)]
mod tests {
    include!("eip712_tests.rs");
}
