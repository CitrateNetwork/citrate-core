//! citrate-core — native SALT transfer intent builder + the `wallet_send` command
//! (@rule8 — a value-bearing write; it signs NOTHING).
//!
//! SALT is the chain's native currency (`chain.ts` nativeCurrency), so a user
//! "Send" is a plain value transfer (no calldata). This module builds the
//! ceremony [`SignatureIntent`] for that transfer and submits it to the
//! [`crate::ceremony::SignatureCeremony`] as a PENDING request — mirroring
//! `agent::user_claim` (which does the same for `claimRewards()`): the command
//! returns only the decoded [`CeremonyView`], and the human then approves it via
//! the ceremony's own `sign_and_broadcast` (B1.4) which fetches nonce+gas from the
//! live 40204 RPC, signs the real EIP-155 tx with the vault key, and broadcasts.
//!
//! HONESTY (Rule 1) / @rule8: this command reads only the wallet's PUBLIC address
//! (never key material — `wallet::address` derives in-process and zeroizes), and
//! returns only the decoded pending view. No signing happens here; a locked vault
//! fails closed. The `raw` payload is the JSON tx-object shape B1.4's
//! `txdecode::decode_transaction` consumes (`{from,to,value,data,gas,chainId}`),
//! identical to the node-agent path so the human sees the real decoded action.

use crate::ceremony::{CeremonyView, IntentKind, SignatureIntent};

/// The Citrate chain id (40204).
const CITRATE_CHAIN_ID: u64 = 40204;
/// Standard gas for a native (no-calldata) value transfer — the fixed 21,000.
const TRANSFER_GAS: u64 = 21_000;
/// The origin surfaced to the human for a user-initiated Send (displayed verbatim).
const LOCAL_USER_ORIGIN: &str = "local-user";

/// Validate a `0x`-prefixed 20-byte hex address; return the lowercased canonical
/// form, or an error (fail closed rather than build a transfer to a malformed to).
fn validate_address(addr: &str) -> Result<String, String> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    if stripped.len() != 40 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "wallet: recipient is not a 20-byte 0x address: {addr}"
        ));
    }
    Ok(format!("0x{}", stripped.to_ascii_lowercase()))
}

/// Build the ceremony tx-JSON for a native SALT transfer. Same object shape the
/// node-agent path emits (`{from,to,value,data,gas,chainId}`); `data` is empty
/// (`0x`) because a native transfer carries no calldata, and `gas` is the fixed
/// 21,000. Values are `0x`-hex quantities. Returned as a JSON string — for a
/// `Transaction` intent the ceremony reads `raw` as JSON (not hex bytes).
fn encode_transfer_json(from: &str, to: &str, value_wei: u128) -> String {
    serde_json::json!({
        "from": from,
        "to": to,
        "value": format!("0x{value_wei:x}"),
        "data": "0x",
        "gas": format!("0x{TRANSFER_GAS:x}"),
        "chainId": format!("0x{CITRATE_CHAIN_ID:x}"),
    })
    .to_string()
}

/// `wallet_send` command — submit a native SALT transfer as a PENDING ceremony
/// and return the decoded view for human approval. Signs NOTHING (the human
/// approves via `sign_and_broadcast`). Requires the vault UNLOCKED to read the
/// sender's public address; a locked/absent vault fails closed. `amount_wei` is a
/// decimal wei string; a zero/garbage amount or malformed recipient is rejected
/// before any ceremony state is created.
#[tauri::command]
pub fn wallet_send(
    to: String,
    amount_wei: String,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<CeremonyView, String> {
    let to = validate_address(&to)?;
    let value_wei: u128 = amount_wei
        .parse()
        .map_err(|_| "wallet: amount is not a u128 wei value".to_string())?;
    if value_wei == 0 {
        return Err("wallet: transfer amount must be greater than zero".to_string());
    }
    // The sender = THIS vault's wallet (public address only; never the key).
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let intent = SignatureIntent {
        origin: LOCAL_USER_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: CITRATE_CHAIN_ID,
        raw: encode_transfer_json(&wallet.address, &to, value_wei),
    };
    // Store PENDING + return the decoded view; the human approves via B1.4.
    Ok(ceremony.0.request(intent))
}

#[cfg(test)]
mod tests {
    include!("transfer_tests.rs");
}
