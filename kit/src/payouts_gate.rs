//! The FL payouts gate: a signing-side check on transactions to the federated-learning money
//! contracts (WorkMarket, FLRoundVerifier, FederatedRoundLedger, DatasetRegistry, PatronageLedger,
//! LoRAFactory).
//!
//! # What it enforces
//!
//! Payouts ship dark in 0.5.4. Whether they are open is decided here, in Rust, never only by the
//! webview: a call is a *payout call* when its `to` is a pinned FL contract and its selector is one
//! of the money classes ([`MoneyClass`]: join, open, postPayout, settle, withdraw, dispute,
//! claimBond, refund, challenge). Every write to WorkMarket counts, including one whose selector
//! is unknown. A payout call is signed only when ALL of these hold:
//!
//! 1. the active network profile says `payoutsOpen: true`;
//! 2. the profile pins the contract's address AND `keccak256(runtime code)` (from the network's
//!    root book, CV-12);
//! 3. the live `eth_getCode` at that address, and at WorkMarket and FLRoundVerifier (the payout
//!    path, [`PAYOUT_PATH`]), hashes to the pin, read immediately before the signature.
//!
//! Not pinned or no code means not open. A different code hash is refused with both hashes named.
//! A code read that fails is refused (never assumed to match). With payouts dark no code is read
//! at all.
//!
//! A call whose selector is distinctive to an FL contract (it occurs in no other system contract,
//! [`MoneySelector::distinctive`]) and whose `to` is NOT that contract's pinned address is also
//! refused: a WorkMarket call to an address this build does not pin is not this network's
//! WorkMarket, and an unpinned WorkMarket is not open.
//!
//! Read-only calls (`eth_call`) never reach a signer and are not affected. Calls to any other
//! contract, and contract creations, pass unchanged. The HIC-1 review (the ceremony card with
//! the exact wei) still applies to every call that passes.
//!
//! # Where it runs
//!
//! [`crate::ceremony::SignatureCeremony`] checks every transaction against the policy after the
//! network gate and before the nonce read and the signature, so every signing entry (the
//! contract reader, `sign_request` from the webview, Hermes, agents) goes through it. Proposal
//! paths may call [`check_intent`] first so the member sees the refusal before a card opens.
//!
//! The policy is installed once at app start ([`install_payouts_gate`]) from the active network
//! profile. A process that installs none uses [`PayoutsPolicy::closed`]: nothing pinned, payouts
//! dark, so every distinctive FL call is refused.

use serde::Serialize;
use sha3::{Digest as _, Keccak256};
use std::sync::OnceLock;

/// The FL contracts the gate knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum FlContract {
    WorkMarket,
    #[serde(rename = "FLRoundVerifier")]
    FlRoundVerifier,
    FederatedRoundLedger,
    DatasetRegistry,
    PatronageLedger,
    #[serde(rename = "LoRAFactory")]
    LoraFactory,
}

impl FlContract {
    /// Every FL contract, in the order the profile lists them.
    pub const ALL: [FlContract; 6] = [
        FlContract::WorkMarket,
        FlContract::FlRoundVerifier,
        FlContract::FederatedRoundLedger,
        FlContract::DatasetRegistry,
        FlContract::PatronageLedger,
        FlContract::LoraFactory,
    ];

    /// The contract's name in the address book and the root book.
    pub fn name(self) -> &'static str {
        match self {
            FlContract::WorkMarket => "WorkMarket",
            FlContract::FlRoundVerifier => "FLRoundVerifier",
            FlContract::FederatedRoundLedger => "FederatedRoundLedger",
            FlContract::DatasetRegistry => "DatasetRegistry",
            FlContract::PatronageLedger => "PatronageLedger",
            FlContract::LoraFactory => "LoRAFactory",
        }
    }

    /// The contract named `name` (the book's spelling), if it is an FL contract.
    pub fn from_name(name: &str) -> Option<FlContract> {
        FlContract::ALL.into_iter().find(|c| c.name() == name)
    }
}

impl std::fmt::Display for FlContract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The money classes of an FL call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MoneyClass {
    Join,
    Open,
    PostPayout,
    Settle,
    Withdraw,
    Dispute,
    ClaimBond,
    Refund,
    Challenge,
    /// Any other write to WorkMarket (an unknown selector, or a plain transfer): WorkMarket holds
    /// escrow, so every write to it is gated.
    OtherWorkMarketWrite,
}

impl MoneyClass {
    fn words(self) -> &'static str {
        match self {
            MoneyClass::Join => "join",
            MoneyClass::Open => "open",
            MoneyClass::PostPayout => "postPayout",
            MoneyClass::Settle => "settle",
            MoneyClass::Withdraw => "withdraw",
            MoneyClass::Dispute => "dispute",
            MoneyClass::ClaimBond => "claimBond",
            MoneyClass::Refund => "refund",
            MoneyClass::Challenge => "challenge",
            MoneyClass::OtherWorkMarketWrite => "a write",
        }
    }
}

/// One gated function: the canonical signature its selector is computed from, the contract it
/// belongs to, its class, and whether its selector occurs in no other system contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoneySelector {
    pub contract: FlContract,
    pub signature: &'static str,
    pub class: MoneyClass,
    /// The selector occurs in no other contract of the system ABI set
    /// (`src-tauri/abis/signatures.json`), so a call carrying it to any address but the pinned
    /// contract is refused. `false` for generic selectors (`withdraw()`, `refund(uint256)`,
    /// `resolveDispute(uint256,bool)`) other contracts share.
    pub distinctive: bool,
}

const fn sel(
    contract: FlContract,
    signature: &'static str,
    class: MoneyClass,
    distinctive: bool,
) -> MoneySelector {
    MoneySelector {
        contract,
        signature,
        class,
        distinctive,
    }
}

/// The gated functions. Signatures are the canonical ABI forms of citrate-chain
/// `contracts/src/work/WorkMarket.sol`, `FederatedRoundLedger.sol`, `data/DatasetRegistry.sol`
/// and `LoRAFactory.sol` (origin/main b2e91167; WorkMarket checked against `forge inspect
/// methodIdentifiers`, the others against `src-tauri/abis/signatures.json`).
pub const MONEY_SELECTORS: &[MoneySelector] = {
    use FlContract::*;
    use MoneyClass::*;
    &[
        sel(
            WorkMarket,
            "open((uint8,address,bytes32,bytes32,bytes32,string,bytes32,bytes32,uint128,uint64,uint32,uint32,uint128,address,uint128,address))",
            Open,
            true,
        ),
        sel(WorkMarket, "join(uint256,uint128,bytes32[])", Join, true),
        sel(WorkMarket, "postPayout(uint256,bytes32,uint64,uint8)", PostPayout, true),
        sel(
            WorkMarket,
            "settle(uint256,address,uint64,uint64,uint32,bytes32[])",
            Settle,
            true,
        ),
        sel(WorkMarket, "withdraw()", Withdraw, false),
        sel(WorkMarket, "dispute(uint256,bytes)", Dispute, true),
        sel(WorkMarket, "commitDispute(uint256,bytes32)", Dispute, true),
        sel(WorkMarket, "revealDispute(uint256,bytes,bytes32)", Dispute, true),
        sel(WorkMarket, "resolveDispute(uint256,bool)", Dispute, false),
        sel(WorkMarket, "claimBond(uint256)", ClaimBond, true),
        sel(WorkMarket, "releaseChallengeBond(uint256,address)", ClaimBond, true),
        sel(WorkMarket, "releaseIndexChallengeBond(uint256,uint32)", ClaimBond, true),
        sel(WorkMarket, "refund(uint256)", Refund, false),
        sel(WorkMarket, "sweepReserve(uint256)", Refund, true),
        sel(
            WorkMarket,
            "challengeInclusion(uint256,(address,bytes32,bytes32,bytes)[])",
            Challenge,
            true,
        ),
        sel(WorkMarket, "challengeIndex(uint256,uint32)", Challenge, true),
        sel(
            WorkMarket,
            "answerChallenge(uint256,(uint32,address,uint64,uint64,bytes32[]))",
            Challenge,
            true,
        ),
        sel(WorkMarket, "resolveChallenge(uint256,address)", Challenge, true),
        sel(
            WorkMarket,
            "answerIndexChallenge(uint256,(uint32,address,uint64,uint64,bytes32[]))",
            Challenge,
            true,
        ),
        sel(WorkMarket, "resolveIndexChallenge(uint256,uint32)", Challenge, true),
        sel(
            FederatedRoundLedger,
            "challengeOutput(bytes32,uint32,bytes,bytes32[],bytes32,bytes32[])",
            Challenge,
            true,
        ),
        sel(
            FederatedRoundLedger,
            "challengeRow(bytes32,uint32,bytes,bytes32[],(uint16,address,bytes32,bytes32[],bytes32,bytes32[]))",
            Challenge,
            true,
        ),
        sel(
            FederatedRoundLedger,
            "challengeRules(bytes32,uint32,bytes,bytes32[])",
            Challenge,
            true,
        ),
        sel(
            FederatedRoundLedger,
            "challengeOrder(bytes32,(uint16,address,bytes32,bytes32[],bytes32,bytes32[]),(uint16,address,bytes32,bytes32[],bytes32,bytes32[]))",
            Challenge,
            true,
        ),
        sel(DatasetRegistry, "withdraw(bytes32,uint32,uint16)", Withdraw, true),
        sel(LoraFactory, "withdrawFees()", Withdraw, false),
    ]
};

/// The 4-byte selector of a canonical function signature.
pub fn selector_of(signature: &str) -> [u8; 4] {
    let h = Keccak256::digest(signature.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// The gated entries whose selector is `selector`.
fn entries_for(selector: [u8; 4]) -> impl Iterator<Item = &'static MoneySelector> {
    MONEY_SELECTORS
        .iter()
        .filter(move |m| selector_of(m.signature) == selector)
}

/// The money class of `data` on `contract`, or `None` when it is not gated there. Every write to
/// WorkMarket is gated ([`MoneyClass::OtherWorkMarketWrite`] for one not listed).
pub fn classify(contract: FlContract, data: &[u8]) -> Option<MoneyClass> {
    let listed = data
        .get(..4)
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .and_then(|s| entries_for(s).find(|m| m.contract == contract))
        .map(|m| m.class);
    match (listed, contract) {
        (Some(c), _) => Some(c),
        (None, FlContract::WorkMarket) => Some(MoneyClass::OtherWorkMarketWrite),
        (None, _) => None,
    }
}

/// One FL contract's pin in the network profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlPin {
    pub contract: FlContract,
    /// The deployed address, or `None` when the contract is not on this network.
    pub address: Option<[u8; 20]>,
    /// `keccak256` of the runtime code at `address`, from the root book.
    pub code_hash: Option<[u8; 32]>,
    /// Why it is not pinned, when it is not.
    pub pending: Option<String>,
}

/// What the gate checks, from the active network profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayoutsPolicy {
    /// The network profile id (for the refusal text).
    pub network: String,
    /// The profile's `payoutsOpen`. Takes effect only where the pinned code hash matches.
    pub payouts_open: bool,
    /// One entry per FL contract the profile pins or names.
    pub pins: Vec<FlPin>,
}

impl PayoutsPolicy {
    /// Nothing pinned, payouts dark: the policy of a process that installed none, and of a
    /// profile that is not ready.
    pub fn closed(network: &str) -> PayoutsPolicy {
        PayoutsPolicy {
            network: network.to_string(),
            payouts_open: false,
            pins: Vec::new(),
        }
    }

    fn pin(&self, contract: FlContract) -> Option<&FlPin> {
        self.pins.iter().find(|p| p.contract == contract)
    }

    /// The pinned FL contract at `address`, if any.
    fn contract_at(&self, address: [u8; 20]) -> Option<FlContract> {
        self.pins
            .iter()
            .find(|p| p.address == Some(address))
            .map(|p| p.contract)
    }
}

/// Why a payout call was refused. Nothing was signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayoutsRefusal {
    /// `payoutsOpen` is not set on this network.
    Dark {
        network: String,
        contract: FlContract,
        class: MoneyClass,
    },
    /// The profile pins no address or no code hash for the contract (not deployed here).
    NotPinned {
        network: String,
        contract: FlContract,
        class: MoneyClass,
        why: String,
    },
    /// A distinctive FL call to an address that is not the contract's pinned address.
    NotThisNetworksContract {
        network: String,
        contract: FlContract,
        class: MoneyClass,
        to: String,
    },
    /// No code at the pinned address.
    NotDeployed {
        contract: FlContract,
        address: String,
    },
    /// The live code hash differs from the pin.
    CodeMismatch {
        contract: FlContract,
        address: String,
        pinned: String,
        live: String,
    },
    /// The code could not be read, so it could not be checked.
    CodeUnread {
        contract: FlContract,
        address: String,
        reason: String,
    },
}

impl std::fmt::Display for PayoutsRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PayoutsRefusal::Dark {
                network,
                contract,
                class,
            } => write!(
                f,
                "FL payouts are not open on network {network:?}, so this {contract} {} call is \
                 not signed",
                class.words()
            ),
            PayoutsRefusal::NotPinned {
                network,
                contract,
                class,
                why,
            } => write!(
                f,
                "{contract} is not pinned on network {network:?} ({why}), so this {} call is not \
                 signed",
                class.words()
            ),
            PayoutsRefusal::NotThisNetworksContract {
                network,
                contract,
                class,
                to,
            } => write!(
                f,
                "{to} is not the {contract} this build pins for network {network:?}, so this {} \
                 call is not signed",
                class.words()
            ),
            PayoutsRefusal::NotDeployed { contract, address } => write!(
                f,
                "{contract} has no code at its pinned address {address}, so FL payouts are not \
                 open and nothing is signed"
            ),
            PayoutsRefusal::CodeMismatch {
                contract,
                address,
                pinned,
                live,
            } => write!(
                f,
                "the code at {contract} {address} is not the pinned build (pinned code hash \
                 {pinned}, live {live}), so nothing is signed"
            ),
            PayoutsRefusal::CodeUnread {
                contract,
                address,
                reason,
            } => write!(
                f,
                "the code at {contract} {address} could not be read to check its hash \
                 ({reason}), so nothing is signed"
            ),
        }
    }
}

impl std::error::Error for PayoutsRefusal {}

/// Reads the runtime code at an address (`0x` + 40 lowercase hex): the code bytes, empty when
/// there is none.
pub type CodeReader<'a> = dyn FnMut(&str) -> Result<Vec<u8>, String> + 'a;

fn hex_addr(a: &[u8; 20]) -> String {
    format!("0x{}", hex::encode(a))
}

fn hex32(h: &[u8; 32]) -> String {
    format!("0x{}", hex::encode(h))
}

/// Check the pinned code of `contract` against the live code. `Ok` only on an exact hash match.
fn check_code(
    policy: &PayoutsPolicy,
    contract: FlContract,
    class: MoneyClass,
    read_code: &mut CodeReader<'_>,
) -> Result<(), PayoutsRefusal> {
    let pin = policy.pin(contract);
    let (address, pinned) = match pin.map(|p| (p.address, p.code_hash)) {
        Some((Some(a), Some(h))) => (a, h),
        _ => {
            return Err(PayoutsRefusal::NotPinned {
                network: policy.network.clone(),
                contract,
                class,
                why: pin
                    .and_then(|p| p.pending.clone())
                    .unwrap_or_else(|| "not deployed on this network".to_string()),
            })
        }
    };
    let addr = hex_addr(&address);
    let code = read_code(&addr).map_err(|reason| PayoutsRefusal::CodeUnread {
        contract,
        address: addr.clone(),
        reason,
    })?;
    if code.is_empty() {
        return Err(PayoutsRefusal::NotDeployed {
            contract,
            address: addr,
        });
    }
    let live: [u8; 32] = Keccak256::digest(&code).into();
    if live != pinned {
        return Err(PayoutsRefusal::CodeMismatch {
            contract,
            address: addr,
            pinned: hex32(&pinned),
            live: hex32(&live),
        });
    }
    Ok(())
}

/// Check one transaction (`to`, calldata) against `policy`. `Ok` means the gate lets it through
/// (it is not a payout call, or payouts are open and the pinned code matches the live code).
/// `read_code` is called only for a payout call while payouts are open.
pub fn check_call(
    policy: &PayoutsPolicy,
    to: Option<[u8; 20]>,
    data: &[u8],
    read_code: &mut CodeReader<'_>,
) -> Result<(), PayoutsRefusal> {
    // A contract creation calls no FL contract.
    let Some(to) = to else {
        return Ok(());
    };
    let gated = match policy.contract_at(to) {
        Some(contract) => classify(contract, data).map(|class| (contract, class)),
        None => {
            // Not a pinned FL address: refuse a selector only an FL contract has.
            let distinctive = data
                .get(..4)
                .and_then(|s| <[u8; 4]>::try_from(s).ok())
                .and_then(|s| entries_for(s).find(|m| m.distinctive));
            if let Some(m) = distinctive {
                return Err(PayoutsRefusal::NotThisNetworksContract {
                    network: policy.network.clone(),
                    contract: m.contract,
                    class: m.class,
                    to: hex_addr(&to),
                });
            }
            None
        }
    };
    let Some((contract, class)) = gated else {
        return Ok(());
    };
    if !policy.payouts_open {
        return Err(PayoutsRefusal::Dark {
            network: policy.network.clone(),
            contract,
            class,
        });
    }
    // The flag takes effect only where the pins match: the called contract, and the payout path
    // (escrow and verifier) it pays through.
    check_code(policy, contract, class, read_code)?;
    for other in PAYOUT_PATH {
        if other != contract {
            check_code(policy, other, class, read_code)?;
        }
    }
    Ok(())
}

/// [`check_call`] for a ceremony intent. Only a transaction intent the ceremony can decode is
/// checked here; anything else passes (a message signs no call, and an undecodable transaction
/// is refused by the signer, which runs this check again on the decoded fields).
pub fn check_intent(
    policy: &PayoutsPolicy,
    intent: &crate::ceremony::SignatureIntent,
    read_code: &mut CodeReader<'_>,
) -> Result<(), PayoutsRefusal> {
    if intent.kind != crate::ceremony::IntentKind::Transaction {
        return Ok(());
    }
    match crate::txdecode::decode_transaction(&intent.raw) {
        Some((tx, _)) => check_call(policy, tx.to, &tx.data, read_code),
        None => Ok(()),
    }
}

/// One FL contract in [`PayoutsStatus`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlContractStatus {
    pub contract: FlContract,
    pub address: Option<String>,
    pub pinned_code_hash: Option<String>,
    /// `None` = not read (not pinned); `Some(true)` = code present.
    pub deployed: Option<bool>,
    /// `Some(true)` only when the live code hash equals the pin.
    pub code_hash_matches: Option<bool>,
    /// Why it is not open, when it is not.
    pub reason: Option<String>,
}

/// Whether FL payouts are open on the active network, and why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayoutsStatus {
    pub network: String,
    /// The profile's flag, before the code check.
    pub payouts_open_flag: bool,
    /// Effective: the flag is set AND WorkMarket and FLRoundVerifier (the payout path) are pinned
    /// and their live code matches the pins.
    pub open: bool,
    pub reason: Option<String>,
    pub contracts: Vec<FlContractStatus>,
}

/// The contracts a payout cannot happen without: the escrow and the round verifier it consults.
pub const PAYOUT_PATH: [FlContract; 2] = [FlContract::WorkMarket, FlContract::FlRoundVerifier];

/// Read every pinned FL contract's code and say whether payouts are open. Reads code only while
/// the flag is set (dark needs no read).
pub fn status(policy: &PayoutsPolicy, read_code: &mut CodeReader<'_>) -> PayoutsStatus {
    let mut contracts = Vec::new();
    let mut first_reason: Option<String> = None;
    for contract in FlContract::ALL {
        let pin = policy.pin(contract);
        let address = pin.and_then(|p| p.address).map(|a| hex_addr(&a));
        let pinned_code_hash = pin.and_then(|p| p.code_hash).map(|h| hex32(&h));
        let mut row = FlContractStatus {
            contract,
            address,
            pinned_code_hash,
            deployed: None,
            code_hash_matches: None,
            reason: None,
        };
        if policy.payouts_open {
            match check_code(policy, contract, MoneyClass::Settle, read_code) {
                Ok(()) => {
                    row.deployed = Some(true);
                    row.code_hash_matches = Some(true);
                }
                Err(e) => {
                    match &e {
                        PayoutsRefusal::NotDeployed { .. } => row.deployed = Some(false),
                        PayoutsRefusal::CodeMismatch { .. } => {
                            row.deployed = Some(true);
                            row.code_hash_matches = Some(false);
                        }
                        _ => {}
                    }
                    let text = match &e {
                        PayoutsRefusal::NotPinned { why, .. } => {
                            format!("{contract} is not pinned on this network ({why})")
                        }
                        other => other.to_string(),
                    };
                    if PAYOUT_PATH.contains(&contract) && first_reason.is_none() {
                        first_reason = Some(text.clone());
                    }
                    row.reason = Some(text);
                }
            }
        } else if let Some(why) = pin.and_then(|p| p.pending.clone()) {
            row.reason = Some(why);
        }
        contracts.push(row);
    }
    let reason = if !policy.payouts_open {
        Some(format!(
            "FL payouts are not open on network {:?} (payoutsOpen is false)",
            policy.network
        ))
    } else {
        first_reason
    };
    PayoutsStatus {
        network: policy.network.clone(),
        payouts_open_flag: policy.payouts_open,
        open: reason.is_none(),
        reason,
        contracts,
    }
}

static PAYOUTS_GATE: OnceLock<PayoutsPolicy> = OnceLock::new();

/// Install the process-wide payouts policy (app start, from the active network profile). The
/// first install wins; returns `false` when one was already installed.
pub fn install_payouts_gate(policy: PayoutsPolicy) -> bool {
    PAYOUTS_GATE.set(policy).is_ok()
}

/// The installed policy, or [`PayoutsPolicy::closed`] when none was installed.
pub fn payouts_gate() -> &'static PayoutsPolicy {
    static CLOSED: OnceLock<PayoutsPolicy> = OnceLock::new();
    PAYOUTS_GATE
        .get()
        .unwrap_or_else(|| CLOSED.get_or_init(|| PayoutsPolicy::closed("not installed")))
}

#[cfg(test)]
mod tests {
    include!("payouts_gate_tests.rs");
}
