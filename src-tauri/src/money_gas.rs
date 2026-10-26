//! citrate-core: the gas limit of a money send. @rule8 · T1.
//!
//! A money send (a SALT transfer, a bond activation, a refund claim, a fee-paying registration)
//! carries the gas limit the node estimates for that exact call, plus a margin, and never a fixed
//! number. A fixed limit below what the contract really costs is mined and reverts, and the member
//! pays the gas for nothing: `LiquidStakingPool.requestWithdrawal` went out with 150,000 while the
//! pool needs about 160,500 (citrate-core#501). An estimate can only come from the node that will
//! run the call, so it moves with the contracts.
//!
//! A failed estimate (the call would revert, or the node cannot be reached) refuses the send
//! before any ceremony exists: nothing is proposed that the node already says will fail. An
//! estimate above [`MAX_SEND_GAS`] is refused too, so a broken or hostile node cannot make the
//! member sign a runaway gas limit.
//!
//! The tripwire in `money_gas_tests.rs` fails CI when a fixed gas limit comes back into a
//! transaction core builds.

use crate::rpc::{RpcClient, RpcTransport};

/// Margin over the node's estimate: state can move between the estimate and inclusion.
pub const MARGIN_PCT: u64 = 25;

/// The highest gas limit core signs a money send with. Every money send core builds costs well
/// under it (a model registration with long metadata is the largest, under 1,000,000).
pub const MAX_SEND_GAS: u64 = 3_000_000;

/// The gas limit for a send the node estimated at `estimate`: the estimate plus [`MARGIN_PCT`],
/// at most [`MAX_SEND_GAS`]. An estimate of zero, or above the cap, is refused.
pub fn with_margin(estimate: u64) -> Result<u64, String> {
    if estimate == 0 {
        return Err(
            "the node estimated 0 gas for this transaction, which is not a gas limit, so nothing \
             was proposed"
                .to_string(),
        );
    }
    if estimate > MAX_SEND_GAS {
        return Err(format!(
            "the node estimates {estimate} gas for this transaction, above the {MAX_SEND_GAS} \
             core signs a money send with, so nothing was proposed"
        ));
    }
    // estimate <= MAX_SEND_GAS, so the product cannot overflow.
    Ok((estimate + estimate * MARGIN_PCT / 100).min(MAX_SEND_GAS))
}

/// Ask the node what the send `from` → `to` with `value_wei` and `data` costs, and add the margin.
pub fn estimate<T: RpcTransport>(
    rpc: &RpcClient<T>,
    from: &str,
    to: &str,
    value_wei: u128,
    data: &[u8],
) -> Result<u64, String> {
    let est = rpc
        .estimate_gas(serde_json::json!({
            "from": from,
            "to": to,
            "value": format!("0x{value_wei:x}"),
            "data": format!("0x{}", hex::encode(data)),
        }))
        .map_err(|e| {
            format!("the node could not estimate this transaction, so nothing was proposed: {e}")
        })?;
    with_margin(est)
}

#[cfg(test)]
mod tests {
    include!("money_gas_tests.rs");
}
