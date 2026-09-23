//! Hermes P3 / WP3.2 — **contract deploy** (a real contract-creation ceremony).
//!
//! Assembles a compiled contract's init code (deploy bytecode ++ ABI-encoded
//! constructor args) into a `to`-less contract-creation transaction and submits it as
//! a PENDING SignatureCeremony (Rule 3 — nothing signs here; the human approves +
//! broadcasts via the signing surface, which signs the EIP-155 creation tx and returns
//! the tx hash). `txdecode` renders a `to`-less tx as the honest action "contract
//! creation", so the human sees exactly what they approve.
//!
//! Rule 1 / safety: this deploys the bytecode it is GIVEN — it never fabricates or
//! ships bespoke contract bytecode. The caller supplies compiled, audited init code
//! (e.g. from `forge`/`solc`, or an agent that compiled + fork-simulated a contract).
//! Empty bytecode is rejected up-front so the ceremony never carries a no-op creation.
use serde_json::json;

/// Default gas for a contract creation when the caller does not supply an estimate.
/// A calldata/creation tx MUST carry explicit gas; a fork-sim estimate is preferred,
/// this is a generous fallback so a modest contract does not run out of gas.
const DEFAULT_DEPLOY_GAS: u64 = 2_000_000;

/// Assemble the deployment init code: the compiled deploy bytecode followed by the
/// ABI-encoded constructor arguments (empty when the constructor takes none). This is
/// exactly what an EVM contract-creation transaction carries in its `data`.
pub fn deploy_initcode(bytecode: &[u8], constructor_args: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytecode.len() + constructor_args.len());
    out.extend_from_slice(bytecode);
    out.extend_from_slice(constructor_args);
    out
}

/// Parse a `0x`-prefixed-or-bare hex string into bytes; rejects odd length / non-hex.
fn parse_hex(s: &str, what: &str) -> Result<Vec<u8>, String> {
    let t = s.trim().strip_prefix("0x").unwrap_or_else(|| s.trim());
    if t.is_empty() {
        return Ok(Vec::new());
    }
    hex::decode(t).map_err(|e| format!("{what}: not valid hex ({e})"))
}

/// The pending-ceremony tx JSON for a contract creation: NO `to` field (creation),
/// `data` = init code, plus value/gas/chainId. Mirrors the other write paths
/// (`storage.rs`, `model_register.rs`) minus the recipient.
fn encode_deploy_tx_json(from: &str, initcode: &[u8], value_wei: u128, gas: u64) -> String {
    json!({
        "from": from,
        // No "to" → the signer/decoder treats this as a contract creation.
        "value": format!("0x{value_wei:x}"),
        "data": format!("0x{}", hex::encode(initcode)),
        "gas": format!("0x{gas:x}"),
        "chainId": format!("0x{:x}", 40204u64),
    })
    .to_string()
}

/// **Command — contract_deploy.** Propose deploying a compiled contract. Assembles the
/// init code from `bytecode_hex` (+ optional `constructor_args_hex`) and submits a
/// PENDING SignatureCeremony carrying the `to`-less creation tx (Rule 3 — the human
/// approves + broadcasts; nothing signs here). The bytecode is caller-supplied + audited
/// — the app never invents contract code (Rule 1). Empty bytecode is rejected up-front.
#[tauri::command]
pub fn contract_deploy(
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    bytecode_hex: String,
    constructor_args_hex: Option<String>,
    value_wei: Option<String>,
    gas: Option<u64>,
) -> std::result::Result<crate::ceremony::CeremonyView, String> {
    let bytecode = parse_hex(&bytecode_hex, "bytecode")?;
    if bytecode.is_empty() {
        return Err("contract bytecode is required to deploy".into());
    }
    let args = match &constructor_args_hex {
        Some(a) => parse_hex(a, "constructor args")?,
        None => Vec::new(),
    };
    let value: u128 = match value_wei.as_deref() {
        None | Some("") => 0,
        Some(v) => v
            .trim()
            .parse()
            .map_err(|_| "value must be a decimal wei amount".to_string())?,
    };
    let initcode = deploy_initcode(&bytecode, &args);
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let raw = encode_deploy_tx_json(
        &wallet.address,
        &initcode,
        value,
        gas.unwrap_or(DEFAULT_DEPLOY_GAS),
    );
    let intent = crate::ceremony::SignatureIntent {
        origin: "local-user".to_string(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw,
    };
    // Return the decoded view so the UI drives signing.broadcast(view.id) (the money-path
    // pattern) — nothing signs here (Rule 3).
    Ok(ceremony.0.request(intent))
}

#[cfg(test)]
mod tests {
    include!("contract_deploy_tests.rs");
}
