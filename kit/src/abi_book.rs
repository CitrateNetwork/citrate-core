//! HUP-S6 US-6.1 (g3-e2e prep) — **calls to a gated contract, decoded from its own ABI.**
//!
//! The SignatureCeremony shows a transaction as a legible action only when it understands the
//! call ([`crate::txdecode`]); anything else needs the member's raw-data acknowledgement
//! (B1.2-ADV-5). A hello-mint page's `mint(uint256)` was such a call. This book lets the ceremony
//! decode calls to one class of contract it can vouch for: a contract the member deployed from a
//! bytecode the D-4 deploy gate judged READY, whose code on chain is exactly that artifact's
//! runtime code. Core registers the artifact's ABI under the deployed address only after both
//! checks (`postdeploy`), so the names shown come from the very code being called.
//!
//! Decoding is deliberately narrow, and anything outside it falls back to raw data:
//! - only `function` entries that change state (`nonpayable`, `payable`);
//! - only static elementary arguments (`uintN`, `intN`, `address`, `bool`, `bytesN`), so the
//!   calldata is exactly `4 + 32 * n` bytes and every word is checked to be canonical for its type;
//! - the selector must match exactly one such function of the registered ABI;
//! - value sent to a `nonpayable` function is not decoded (the call would revert anyway).
//!
//! Nothing here signs or holds a key (Rule 3).

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::Value;

/// Contracts kept (one member's deployments; the oldest registration is dropped past this).
pub const MAX_CONTRACTS: usize = 256;
/// Functions read from one ABI.
const MAX_FUNCTIONS: usize = 512;
/// The longest contract or function name shown.
const MAX_NAME: usize = 64;

/// A static elementary ABI type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiType {
    Uint(u16),
    Int(u16),
    Address,
    Bool,
    FixedBytes(u8),
}

impl AbiType {
    /// Parse a canonical ABI type name; `None` for anything dynamic or composite.
    pub fn parse(s: &str) -> Option<AbiType> {
        let bits = |rest: &str| -> Option<u16> {
            let n: u16 = if rest.is_empty() {
                256
            } else {
                rest.parse().ok()?
            };
            ((8..=256).contains(&n) && n.is_multiple_of(8)).then_some(n)
        };
        match s {
            "address" => Some(AbiType::Address),
            "bool" => Some(AbiType::Bool),
            _ if s.starts_with("uint") => bits(&s[4..]).map(AbiType::Uint),
            _ if s.starts_with("int") => bits(&s[3..]).map(AbiType::Int),
            _ if s.starts_with("bytes") && s.len() > 5 => {
                let n: u8 = s[5..].parse().ok()?;
                (1..=32).contains(&n).then_some(AbiType::FixedBytes(n))
            }
            _ => None,
        }
    }

    fn canonical(self) -> String {
        match self {
            AbiType::Uint(n) => format!("uint{n}"),
            AbiType::Int(n) => format!("int{n}"),
            AbiType::Address => "address".into(),
            AbiType::Bool => "bool".into(),
            AbiType::FixedBytes(n) => format!("bytes{n}"),
        }
    }

    /// The word as this type, or `None` when it is not a canonical encoding of it.
    fn decode(self, w: &[u8]) -> Option<String> {
        match self {
            AbiType::Address => w[..12]
                .iter()
                .all(|b| *b == 0)
                .then(|| format!("0x{}", hex::encode(&w[12..]))),
            AbiType::Bool => {
                (w[..31].iter().all(|b| *b == 0) && w[31] <= 1).then(|| (w[31] == 1).to_string())
            }
            AbiType::FixedBytes(n) => {
                let n = usize::from(n);
                w[n..]
                    .iter()
                    .all(|b| *b == 0)
                    .then(|| format!("0x{}", hex::encode(&w[..n])))
            }
            AbiType::Uint(bits) => {
                let pad = 32 - usize::from(bits / 8);
                if !w[..pad].iter().all(|b| *b == 0) {
                    return None;
                }
                Some(word_decimal(w))
            }
            AbiType::Int(bits) => {
                let pad = 32 - usize::from(bits / 8);
                let neg = w[pad] & 0x80 != 0;
                let fill = if neg { 0xff } else { 0x00 };
                if !w[..pad].iter().all(|b| *b == fill) {
                    return None;
                }
                if !neg {
                    return Some(word_decimal(w));
                }
                // Two's complement magnitude.
                let mut m = [0u8; 32];
                let mut carry = 1u16;
                for i in (0..32).rev() {
                    let v = u16::from(!w[i]) + carry;
                    m[i] = (v & 0xff) as u8;
                    carry = v >> 8;
                }
                Some(format!("-{}", word_decimal(&m)))
            }
        }
    }
}

/// A 32-byte big-endian word in decimal.
fn word_decimal(w: &[u8]) -> String {
    let mut digits: Vec<u8> = vec![0];
    for byte in w {
        let mut carry = u32::from(*byte);
        for d in digits.iter_mut() {
            let v = u32::from(*d) * 256 + carry;
            *d = (v % 10) as u8;
            carry = v / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    digits.iter().rev().map(|d| char::from(b'0' + d)).collect()
}

/// One state-changing function of a registered contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiFunction {
    pub name: String,
    /// `(argument name, type)`.
    pub inputs: Vec<(String, AbiType)>,
    pub payable: bool,
    pub selector: [u8; 4],
}

/// A registered contract: its Solidity name and its decodable functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractAbi {
    pub name: String,
    pub functions: Vec<AbiFunction>,
}

fn safe_name(s: &str) -> Option<String> {
    let ok = !s.is_empty()
        && s.len() <= MAX_NAME
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    ok.then(|| s.to_string())
}

fn selector(sig: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

impl ContractAbi {
    /// Read the decodable functions from a solc / forge ABI array. Functions with a dynamic or
    /// composite argument, view/pure functions and anything malformed are left out (their calls
    /// then show as raw data). Two functions with one selector are both dropped.
    pub fn from_abi_json(name: &str, abi: &Value) -> Result<ContractAbi, String> {
        let name = safe_name(name).ok_or("the contract name is not a plain identifier")?;
        let entries = abi.as_array().ok_or("the ABI is not an array")?;
        let mut functions: Vec<AbiFunction> = Vec::new();
        let mut seen: BTreeMap<[u8; 4], usize> = BTreeMap::new();
        for e in entries.iter().take(MAX_FUNCTIONS) {
            if e.get("type").and_then(Value::as_str) != Some("function") {
                continue;
            }
            let mutability = e.get("stateMutability").and_then(Value::as_str);
            let payable = match mutability {
                Some("payable") => true,
                Some("nonpayable") => false,
                _ => continue,
            };
            let Some(fname) = e.get("name").and_then(Value::as_str).and_then(safe_name) else {
                continue;
            };
            let Some(raw_inputs) = e.get("inputs").and_then(Value::as_array) else {
                continue;
            };
            let mut inputs = Vec::with_capacity(raw_inputs.len());
            let mut ok = true;
            for (i, inp) in raw_inputs.iter().enumerate() {
                let ty = inp
                    .get("type")
                    .and_then(Value::as_str)
                    .and_then(AbiType::parse);
                let arg = inp
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(safe_name)
                    .unwrap_or_else(|| format!("arg{i}"));
                match ty {
                    Some(t) => inputs.push((arg, t)),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            let sig = format!(
                "{fname}({})",
                inputs
                    .iter()
                    .map(|(_, t)| t.canonical())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let sel = selector(&sig);
            *seen.entry(sel).or_default() += 1;
            functions.push(AbiFunction {
                name: fname,
                inputs,
                payable,
                selector: sel,
            });
        }
        functions.retain(|f| seen.get(&f.selector) == Some(&1));
        Ok(ContractAbi { name, functions })
    }

    /// The call as `name(arg=value, ...)`, or `None` when it is not exactly one decodable call
    /// (unknown selector, wrong length, a non-canonical word, value sent to a nonpayable
    /// function).
    pub fn decode_call(&self, data: &[u8], value: u128) -> Option<String> {
        if data.len() < 4 {
            return None;
        }
        let f = self.functions.iter().find(|f| f.selector == data[..4])?;
        if data.len() != 4 + 32 * f.inputs.len() || (value > 0 && !f.payable) {
            return None;
        }
        let mut args = Vec::with_capacity(f.inputs.len());
        for (i, (name, ty)) in f.inputs.iter().enumerate() {
            let w = &data[4 + 32 * i..4 + 32 * (i + 1)];
            args.push(format!("{name}={}", ty.decode(w)?));
        }
        Some(format!("{}({})", f.name, args.join(", ")))
    }
}

/// Registered contracts by address, each with its registration sequence number.
#[derive(Debug, Default)]
struct BookInner {
    seq: u64,
    by_address: BTreeMap<[u8; 20], (u64, ContractAbi)>,
}

/// The ceremony's book of gated contracts: deployed address -> its ABI.
#[derive(Debug, Default)]
pub struct AbiBook {
    inner: Mutex<BookInner>,
}

impl AbiBook {
    /// Register (or replace) `address`'s ABI. Callers register only a contract whose on-chain
    /// code is exactly a READY-gated artifact's runtime code.
    pub fn register(&self, address: [u8; 20], abi: ContractAbi) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.seq += 1;
        let seq = g.seq;
        g.by_address.insert(address, (seq, abi));
        while g.by_address.len() > MAX_CONTRACTS {
            let oldest = g
                .by_address
                .iter()
                .min_by_key(|(_, (s, _))| *s)
                .map(|(k, _)| *k);
            match oldest {
                Some(k) => {
                    g.by_address.remove(&k);
                }
                None => break,
            }
        }
    }

    /// The registered ABI of `address`.
    pub fn get(&self, address: &[u8; 20]) -> Option<ContractAbi> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.by_address.get(address).map(|(_, a)| a.clone())
    }

    /// Contracts registered.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .by_address
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    include!("abi_book_tests.rs");
}
