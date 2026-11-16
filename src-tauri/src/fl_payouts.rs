//! G-SEC PAY-14 — whether FL payouts are open on this network, as Rust decides it.
//!
//! The webview shows payout and claim surfaces from this answer, never from the profile JSON
//! alone: `open` is true only when the active profile says `payoutsOpen: true` AND WorkMarket and
//! FLRoundVerifier are pinned and their live `eth_getCode` hashes to the pins
//! ([`crate::payouts_gate::status`]). The signer enforces the same rule on every FL transaction
//! whatever the webview shows.
//!
//! | Value | Source |
//! |---|---|
//! | `payoutsOpenFlag` | `src-tauri/network/profiles.json`, active profile, `payoutsOpen` |
//! | pins | the same profile's `flCodePins` (from the network's root book) |
//! | `deployed`, `codeHashMatches` | `eth_getCode` on the 40204 RPC, read only while the flag is set |

/// **fl_payouts_status** — whether FL payouts are open on the active network, and why not.
#[tauri::command]
pub async fn fl_payouts_status(
    app_h: tauri::AppHandle,
) -> std::result::Result<crate::payouts_gate::PayoutsStatus, String> {
    crate::blocking::off_main(move || {
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        Ok(status_with(
            ceremony.0.payouts_policy(),
            &crate::rpc::RpcClient::citrate(),
        ))
    })
    .await
}

/// The status for `policy`, reading code through `client`.
pub fn status_with<T: crate::rpc::RpcTransport>(
    policy: &crate::payouts_gate::PayoutsPolicy,
    client: &crate::rpc::RpcClient<T>,
) -> crate::payouts_gate::PayoutsStatus {
    crate::payouts_gate::status(policy, &mut |a: &str| {
        client.get_code(a).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    include!("fl_payouts_tests.rs");
}
