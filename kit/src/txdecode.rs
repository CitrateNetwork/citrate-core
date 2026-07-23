//! citrate-core — legacy EIP-155 transaction intent decode (CORE-B1.4).
//!
//! B1.2 treated every `transaction` intent as undecodable (raw-ack gated),
//! because citrate-core had no tx decoder. B1.4 adds a real one: it parses the
//! EIP-1193 `eth_sendTransaction` tx object (the JSON the wagmi ceremony
//! connector marshals into `SignatureIntent.raw`) into
//!   * a [`ParsedTx`] — the fields we will sign (nonce/gasPrice/gasLimit may be
//!     absent; the broadcast path fills them from the live RPC), and
//!   * a human-readable `{action, cost, destination}` for the approval UI.
//!
//! ## Honest gating (Rule 1 / B1.2-ADV-5)
//! Only a payload we can actually decode to a legible tx surfaces a real action.
//! A payload that is NOT a JSON tx object, or that carries opaque calldata with
//! no recognizable transfer/creation shape we can summarize, STILL returns
//! `None` here so the ceremony marks it `Unrecognized` and raw-ack gates it — a
//! blind approval of undecodable calldata remains impossible. We never fabricate
//! a benign summary.
//!
//! ## No U256 (lean discipline mirror)
//! `value` is a `u128` — same choice `citrate_wallet_core::LegacyTxFields` makes.
//! A value beyond `u128` (>~3.4e20 SALT) is rejected as undecodable rather than
//! silently truncated.

// B1.4 seam: consumed by the ceremony transaction path; the finalize/into-fields
// helpers are also reached by tests directly.
#![allow(dead_code)]

use citrate_wallet_core::LegacyTxFields;
use serde_json::Value;

/// A decoded transaction intent, prior to nonce/gas resolution. The dApp may
/// omit `nonce`/`gas_price`/`gas_limit` (wagmi often does — the wallet fills
/// them), so those are `Option`; [`ParsedTx::finalize`] merges the RPC-fetched
/// values before signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTx {
    /// The `from` address the dApp names (used to fetch the pending nonce). Kept
    /// for the broadcast path; NOT part of the signed fields (recovered from v/r/s).
    pub from: Option<String>,
    /// Recipient 20-byte address, or `None` for contract creation.
    pub to: Option<[u8; 20]>,
    /// Value in wei.
    pub value: u128,
    /// Call data / init code.
    pub data: Vec<u8>,
    /// Nonce, if the dApp supplied it (else fetched from the RPC).
    pub nonce: Option<u64>,
    /// Gas price in wei, if supplied (else fetched from the RPC).
    pub gas_price: Option<u64>,
    /// Gas limit, if supplied (else defaulted for a simple transfer).
    pub gas_limit: Option<u64>,
}

/// The human-readable decode surfaced for approval (mirrors `DecodedAction`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxDisplay {
    pub action: String,
    pub cost: String,
    pub destination: String,
}

/// The default gas limit for a plain value transfer with empty calldata (the
/// canonical 21000). Only used when the dApp omits `gas` AND there is no
/// calldata; a tx WITH calldata that omits gas is left to the RPC/estimate path.
pub const DEFAULT_TRANSFER_GAS: u64 = 21_000;

/// Try to decode a `transaction` intent's raw payload into `(ParsedTx, TxDisplay)`.
/// Returns `None` for anything we cannot legibly summarize — the caller then
/// raw-ack gates it (undecodable calldata stays gated; Rule 1 / B1.2-ADV-5).
///
/// `raw` is the intent payload: the EIP-1193 tx object serialized to JSON by the
/// connector (`JSON.stringify(tx)`). We also accept an already-parsed object.
pub fn decode_transaction(raw: &str) -> Option<(ParsedTx, TxDisplay)> {
    let json: Value = serde_json::from_str(raw).ok()?;
    let obj = json.as_object()?;

    // A tx object must have at least a recipient OR be a contract creation with
    // init code. Anything else is not a legible tx → raw-gate.
    let to = match obj.get("to") {
        Some(Value::String(s)) if !s.is_empty() && s != "0x" => Some(parse_address(s)?),
        // Explicit null or absent `to` == contract creation (only legible if
        // there is init code in `data`/`input`).
        Some(Value::Null) | None => None,
        _ => return None,
    };

    let data = parse_data(obj);
    // Contract creation with NO init code is not a legible action → raw-gate.
    if to.is_none() && data.is_empty() {
        return None;
    }

    let value = parse_u128_quantity(obj.get("value"))?;
    let nonce = parse_u64_opt(obj.get("nonce"))?;
    let gas_price = parse_u64_opt(obj.get("gasPrice"))?;
    let gas_limit = parse_u64_opt(obj.get("gas"))?;
    let from = obj.get("from").and_then(Value::as_str).map(str::to_string);

    let display = build_display(&to, value, &data);

    Some((
        ParsedTx {
            from,
            to,
            value,
            data,
            nonce,
            gas_price,
            gas_limit,
        },
        display,
    ))
}

/// Build the human-readable action/cost/destination for the approval UI.
fn build_display(to: &Option<[u8; 20]>, value: u128, data: &[u8]) -> TxDisplay {
    let destination = match to {
        Some(addr) => format!("0x{}", hex::encode(addr)),
        None => "contract creation".to_string(),
    };
    let cost = format!("{} wei", value);
    let action = match to {
        None => format!("Deploy contract ({} bytes init code)", data.len()),
        Some(_) if data.is_empty() => format!("Send {value} wei to {destination}"),
        Some(_) => format!(
            "Call {destination} with {} bytes calldata (value {value} wei)",
            data.len()
        ),
    };
    TxDisplay {
        action,
        cost,
        destination,
    }
}

impl ParsedTx {
    /// Merge the RPC-resolved `nonce`/`gas_price` (and a default gas limit for a
    /// plain transfer) into concrete [`LegacyTxFields`] ready to sign. The
    /// dApp-supplied values win when present; otherwise the fetched values are
    /// used. Gas limit for a value transfer with empty calldata defaults to
    /// 21000 when the dApp omits it.
    ///
    /// Returns `None` if a gas limit is still unknown (calldata present but no
    /// `gas` supplied) — the broadcast path must not guess execution gas.
    pub fn finalize(&self, fetched_nonce: u64, fetched_gas_price: u64) -> Option<LegacyTxFields> {
        let nonce = self.nonce.unwrap_or(fetched_nonce);
        let gas_price = self.gas_price.unwrap_or(fetched_gas_price);
        let gas_limit = match self.gas_limit {
            Some(g) => g,
            None if self.data.is_empty() => DEFAULT_TRANSFER_GAS,
            None => return None, // calldata present, no gas supplied → cannot guess
        };
        Some(LegacyTxFields {
            nonce,
            gas_price,
            gas_limit,
            to: self.to,
            value: self.value,
            data: self.data.clone(),
        })
    }
}

/// Parse a `0x`-prefixed 20-byte EVM address.
fn parse_address(s: &str) -> Option<[u8; 20]> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(stripped).ok()?;
    if bytes.len() != 20 {
        return None;
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Some(out)
}

/// Extract calldata from `data` or `input` (both are used in the wild).
fn parse_data(obj: &serde_json::Map<String, Value>) -> Vec<u8> {
    let field = obj
        .get("data")
        .or_else(|| obj.get("input"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let stripped = field.strip_prefix("0x").unwrap_or(field);
    hex::decode(stripped).unwrap_or_default()
}

/// Parse an optional `0x`-hex quantity into a `u128`. Absent/null → 0 (a tx with
/// no `value` moves nothing). A present-but-malformed value → `None` (undecodable).
/// A value that overflows `u128` → `None` (rejected, never truncated).
fn parse_u128_quantity(v: Option<&Value>) -> Option<u128> {
    match v {
        None | Some(Value::Null) => Some(0),
        Some(Value::String(s)) => {
            let stripped = s.strip_prefix("0x").unwrap_or(s);
            if stripped.is_empty() {
                return Some(0);
            }
            u128::from_str_radix(stripped, 16).ok()
        }
        // A numeric literal (some libs send small numbers) fits if it is a u64.
        Some(Value::Number(n)) => n.as_u64().map(u128::from),
        _ => None,
    }
}

/// Parse an optional `0x`-hex quantity into an `Option<u64>`: absent/null → `Ok(None)`
/// (the field will be RPC-resolved), present-and-valid → `Ok(Some(n))`,
/// present-but-malformed → the OUTER `None` (undecodable). The double-option is
/// flattened by the caller via `?`.
fn parse_u64_opt(v: Option<&Value>) -> Option<Option<u64>> {
    match v {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(s)) => {
            let stripped = s.strip_prefix("0x").unwrap_or(s);
            if stripped.is_empty() {
                return Some(None);
            }
            u64::from_str_radix(stripped, 16).ok().map(Some)
        }
        Some(Value::Number(n)) => n.as_u64().map(Some),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    include!("txdecode_tests.rs");
}
