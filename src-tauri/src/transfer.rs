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

// The chain id this build targets (the kit constant pinned to the active network profile).
use crate::rpc::CITRATE_CHAIN_ID;
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
/// (`0x`) because a native transfer carries no calldata, and `gas` is the node's
/// estimate plus the margin (`money_gas`): 21,000 only covers a transfer to a
/// plain account, and a recipient that is a contract costs more. Values are
/// `0x`-hex quantities. Returned as a JSON string — for a `Transaction` intent the
/// ceremony reads `raw` as JSON (not hex bytes).
fn encode_transfer_json(from: &str, to: &str, value_wei: u128, gas: u64) -> String {
    serde_json::json!({
        "from": from,
        "to": to,
        "value": format!("0x{value_wei:x}"),
        "data": "0x",
        "gas": format!("0x{gas:x}"),
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
pub async fn wallet_send(
    app_h: tauri::AppHandle,
    to: String,
    amount_wei: String,
) -> std::result::Result<CeremonyView, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st2 = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st3 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        wallet_send_sync(to, amount_wei, st2, st3)
    })
    .await
}

/// Blocking body of [`wallet_send`]; reached only through [`crate::blocking::off_main`].
pub fn wallet_send_sync(
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
    let rpc = crate::rpc::RpcClient::citrate();
    let intent = transfer_intent(&rpc, &wallet.address, &to, value_wei)?;
    // Store PENDING + return the decoded view; the human approves via B1.4.
    Ok(ceremony.0.request(intent))
}

/// The body of [`wallet_send`]: the native transfer of `value_wei` from `from` to `to` (both
/// already validated), with the gas the node estimates for it (`money_gas`).
pub(crate) fn transfer_intent<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    from: &str,
    to: &str,
    value_wei: u128,
) -> Result<SignatureIntent, String> {
    let gas = crate::money_gas::estimate(rpc, from, to, value_wei, &[])?;
    Ok(SignatureIntent {
        origin: LOCAL_USER_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: CITRATE_CHAIN_ID,
        raw: encode_transfer_json(from, to, value_wei, gas),
    })
}

#[cfg(test)]
mod tests {
    include!("transfer_tests.rs");
}
