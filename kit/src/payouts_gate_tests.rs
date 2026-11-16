// The FL payouts gate: one refusal per money class while payouts are dark, the code-hash pin
// (match, mismatch, no code, not pinned, unreadable), the payout path, distinctive selectors to an
// address that is not the pinned contract, and the calls the gate leaves alone.

use super::*;
use std::cell::Cell;

const WM: [u8; 20] = [0x11; 20];
const VERIFIER: [u8; 20] = [0x22; 20];
const FRL: [u8; 20] = [0x33; 20];
const LORA: [u8; 20] = [0x44; 20];
const OTHER: [u8; 20] = [0x55; 20];

const WM_CODE: &[u8] = b"workmarket runtime code";
const VERIFIER_CODE: &[u8] = b"flroundverifier runtime code";
const FRL_CODE: &[u8] = b"frl runtime code";
const LORA_CODE: &[u8] = b"lora runtime code";

fn keccak(b: &[u8]) -> [u8; 32] {
    Keccak256::digest(b).into()
}

fn pin(contract: FlContract, address: [u8; 20], code: &[u8]) -> FlPin {
    FlPin {
        contract,
        address: Some(address),
        code_hash: Some(keccak(code)),
        pending: None,
    }
}

/// A network where all four pinned contracts are deployed, with `open` as its flag.
fn policy(open: bool) -> PayoutsPolicy {
    PayoutsPolicy {
        network: "testnet".into(),
        payouts_open: open,
        pins: vec![
            pin(FlContract::WorkMarket, WM, WM_CODE),
            pin(FlContract::FlRoundVerifier, VERIFIER, VERIFIER_CODE),
            pin(FlContract::FederatedRoundLedger, FRL, FRL_CODE),
            pin(FlContract::LoraFactory, LORA, LORA_CODE),
        ],
    }
}

/// The r1007 shape: WorkMarket and FLRoundVerifier not deployed, the ledgers pinned.
fn r1007_like(open: bool) -> PayoutsPolicy {
    PayoutsPolicy {
        network: "r1007".into(),
        payouts_open: open,
        pins: vec![
            FlPin {
                contract: FlContract::WorkMarket,
                address: None,
                code_hash: None,
                pending: Some("not deployed on r1007".into()),
            },
            FlPin {
                contract: FlContract::FlRoundVerifier,
                address: None,
                code_hash: None,
                pending: Some("not deployed on r1007".into()),
            },
            pin(FlContract::FederatedRoundLedger, FRL, FRL_CODE),
            pin(FlContract::LoraFactory, LORA, LORA_CODE),
        ],
    }
}

/// The live chain: the code each address holds.
fn chain(address: &str) -> Result<Vec<u8>, String> {
    let code: &[u8] = match address {
        a if a == hex_addr(&WM) => WM_CODE,
        a if a == hex_addr(&VERIFIER) => VERIFIER_CODE,
        a if a == hex_addr(&FRL) => FRL_CODE,
        a if a == hex_addr(&LORA) => LORA_CODE,
        _ => b"",
    };
    Ok(code.to_vec())
}

/// Calldata: the selector of `signature` plus one zero word.
fn call(signature: &str) -> Vec<u8> {
    let mut d = selector_of(signature).to_vec();
    d.extend_from_slice(&[0u8; 32]);
    d
}

fn sig_of(contract: FlContract, class: MoneyClass) -> &'static str {
    MONEY_SELECTORS
        .iter()
        .find(|m| m.contract == contract && m.class == class)
        .map(|m| m.signature)
        .expect("a selector of that class")
}

/// A reader that must not be called (dark needs no code read).
fn no_read(_: &str) -> Result<Vec<u8>, String> {
    panic!("the gate read code while payouts are dark")
}

// --- selectors --------------------------------------------------------------------------------

#[test]
fn selectors_are_the_deployed_abi() {
    // WorkMarket from `forge inspect WorkMarket methodIdentifiers` (citrate-chain contracts/src/
    // work/WorkMarket.sol); FRL, DatasetRegistry and LoRAFactory from `cast sig` and the system
    // ABI set.
    let expected = [
        ("open((uint8,address,bytes32,bytes32,bytes32,string,bytes32,bytes32,uint128,uint64,uint32,uint32,uint128,address,uint128,address))", "f5ed7aa1"),
        ("join(uint256,uint128,bytes32[])", "b46d20fa"),
        ("postPayout(uint256,bytes32,uint64,uint8)", "8db033dd"),
        ("settle(uint256,address,uint64,uint64,uint32,bytes32[])", "ef221216"),
        ("withdraw()", "3ccfd60b"),
        ("dispute(uint256,bytes)", "f82d578a"),
        ("commitDispute(uint256,bytes32)", "002a6c65"),
        ("revealDispute(uint256,bytes,bytes32)", "92cc95eb"),
        ("resolveDispute(uint256,bool)", "34b25ee2"),
        ("claimBond(uint256)", "1c08c08e"),
        ("releaseChallengeBond(uint256,address)", "49310763"),
        ("releaseIndexChallengeBond(uint256,uint32)", "822ca8ed"),
        ("refund(uint256)", "278ecde1"),
        ("sweepReserve(uint256)", "376f7c30"),
        ("challengeInclusion(uint256,(address,bytes32,bytes32,bytes)[])", "fdeb61fb"),
        ("challengeIndex(uint256,uint32)", "53a413e1"),
        ("answerChallenge(uint256,(uint32,address,uint64,uint64,bytes32[]))", "e6a0c48a"),
        ("resolveChallenge(uint256,address)", "5e06ff19"),
        ("answerIndexChallenge(uint256,(uint32,address,uint64,uint64,bytes32[]))", "1b4b74b3"),
        ("resolveIndexChallenge(uint256,uint32)", "db9f558e"),
        ("challengeOutput(bytes32,uint32,bytes,bytes32[],bytes32,bytes32[])", "c3d6f0f0"),
        ("challengeRow(bytes32,uint32,bytes,bytes32[],(uint16,address,bytes32,bytes32[],bytes32,bytes32[]))", "43fcf32f"),
        ("challengeRules(bytes32,uint32,bytes,bytes32[])", "0064f030"),
        ("challengeOrder(bytes32,(uint16,address,bytes32,bytes32[],bytes32,bytes32[]),(uint16,address,bytes32,bytes32[],bytes32,bytes32[]))", "abf7e339"),
        ("withdraw(bytes32,uint32,uint16)", "a8078bd0"),
        ("withdrawFees()", "476343ee"),
    ];
    assert_eq!(expected.len(), MONEY_SELECTORS.len(), "every gated function is pinned here");
    for (signature, hex_sel) in expected {
        assert!(
            MONEY_SELECTORS.iter().any(|m| m.signature == signature),
            "{signature} is gated"
        );
        assert_eq!(hex::encode(selector_of(signature)), hex_sel, "{signature}");
    }
}

#[test]
fn distinctive_selectors_occur_in_no_other_system_contract() {
    let book: serde_json::Value =
        serde_json::from_str(include_str!("../../src-tauri/abis/signatures.json")).expect("json");
    let functions = book["functions"].as_object().expect("functions");
    let fl: Vec<&str> = FlContract::ALL.iter().map(|c| c.name()).collect();
    for m in MONEY_SELECTORS {
        let key = format!("0x{}", hex::encode(selector_of(m.signature)));
        let others: Vec<String> = functions
            .get(&key)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .flat_map(|e| e["from"].as_array().cloned().unwrap_or_default())
            .filter_map(|f| f.as_str().map(str::to_string))
            .filter(|f| !fl.contains(&f.as_str()))
            .collect();
        if m.distinctive {
            assert!(others.is_empty(), "{} is shared with {others:?}", m.signature);
        }
    }
    // The generic ones are generic for a reason.
    for shared in ["withdraw()", "resolveDispute(uint256,bool)", "withdrawFees()"] {
        let key = format!("0x{}", hex::encode(selector_of(shared)));
        assert!(functions.contains_key(&key), "{shared} occurs in another system contract");
        assert!(MONEY_SELECTORS
            .iter()
            .filter(|m| m.signature == shared)
            .all(|m| !m.distinctive));
    }
}

// --- dark: one refusal per selector class -------------------------------------------------------

fn assert_dark(contract: FlContract, to: [u8; 20], class: MoneyClass) {
    let data = call(sig_of(contract, class));
    let got = check_call(&policy(false), Some(to), &data, &mut no_read);
    assert_eq!(
        got,
        Err(PayoutsRefusal::Dark {
            network: "testnet".into(),
            contract,
            class
        }),
        "{contract} {class:?}"
    );
}

#[test]
fn dark_refuses_join() {
    assert_dark(FlContract::WorkMarket, WM, MoneyClass::Join);
}

#[test]
fn dark_refuses_open() {
    assert_dark(FlContract::WorkMarket, WM, MoneyClass::Open);
}

#[test]
fn dark_refuses_post_payout() {
    assert_dark(FlContract::WorkMarket, WM, MoneyClass::PostPayout);
}

#[test]
fn dark_refuses_settle() {
    assert_dark(FlContract::WorkMarket, WM, MoneyClass::Settle);
}

#[test]
fn dark_refuses_withdraw() {
    assert_dark(FlContract::WorkMarket, WM, MoneyClass::Withdraw);
    assert_dark(FlContract::LoraFactory, LORA, MoneyClass::Withdraw);
}

#[test]
fn dark_refuses_every_dispute_entry() {
    for m in MONEY_SELECTORS.iter().filter(|m| m.class == MoneyClass::Dispute) {
        let got = check_call(&policy(false), Some(WM), &call(m.signature), &mut no_read);
        assert!(matches!(got, Err(PayoutsRefusal::Dark { class: MoneyClass::Dispute, .. })), "{}", m.signature);
    }
}

#[test]
fn dark_refuses_claim_bond_and_bond_releases() {
    for m in MONEY_SELECTORS.iter().filter(|m| m.class == MoneyClass::ClaimBond) {
        let got = check_call(&policy(false), Some(WM), &call(m.signature), &mut no_read);
        assert!(matches!(got, Err(PayoutsRefusal::Dark { class: MoneyClass::ClaimBond, .. })), "{}", m.signature);
    }
}

#[test]
fn dark_refuses_refund_and_sweep() {
    for m in MONEY_SELECTORS.iter().filter(|m| m.class == MoneyClass::Refund) {
        let got = check_call(&policy(false), Some(WM), &call(m.signature), &mut no_read);
        assert!(matches!(got, Err(PayoutsRefusal::Dark { class: MoneyClass::Refund, .. })), "{}", m.signature);
    }
}

#[test]
fn dark_refuses_every_challenge_on_work_market_and_the_round_ledger() {
    for m in MONEY_SELECTORS.iter().filter(|m| m.class == MoneyClass::Challenge) {
        let to = if m.contract == FlContract::WorkMarket { WM } else { FRL };
        let got = check_call(&policy(false), Some(to), &call(m.signature), &mut no_read);
        assert_eq!(
            got,
            Err(PayoutsRefusal::Dark {
                network: "testnet".into(),
                contract: m.contract,
                class: MoneyClass::Challenge
            }),
            "{}",
            m.signature
        );
    }
}

#[test]
fn dark_refuses_any_other_work_market_write_and_a_plain_transfer() {
    // setNewOffersPaused(bool), an unknown selector and empty calldata (value only).
    for data in [call("setNewOffersPaused(bool)"), vec![0xde, 0xad, 0xbe, 0xef], vec![]] {
        let got = check_call(&policy(false), Some(WM), &data, &mut no_read);
        assert!(
            matches!(got, Err(PayoutsRefusal::Dark { class: MoneyClass::OtherWorkMarketWrite, .. })),
            "{data:?}: {got:?}"
        );
    }
}

#[test]
fn refusal_text_names_the_network_contract_and_call() {
    let e = check_call(&policy(false), Some(WM), &call(sig_of(FlContract::WorkMarket, MoneyClass::Join)), &mut no_read)
        .expect_err("dark");
    assert_eq!(
        e.to_string(),
        "FL payouts are not open on network \"testnet\", so this WorkMarket join call is not signed"
    );
}

// --- open: the code-hash pin ------------------------------------------------------------------

#[test]
fn open_with_matching_code_signs() {
    for m in MONEY_SELECTORS.iter().filter(|m| m.contract == FlContract::WorkMarket) {
        assert_eq!(check_call(&policy(true), Some(WM), &call(m.signature), &mut chain), Ok(()), "{}", m.signature);
    }
    let frl = call(sig_of(FlContract::FederatedRoundLedger, MoneyClass::Challenge));
    assert_eq!(check_call(&policy(true), Some(FRL), &frl, &mut chain), Ok(()));
}

#[test]
fn open_reads_the_code_of_the_target_and_the_payout_path_before_each_signature() {
    let reads = std::cell::RefCell::new(Vec::new());
    let mut reader = |a: &str| {
        reads.borrow_mut().push(a.to_string());
        chain(a)
    };
    let frl = call(sig_of(FlContract::FederatedRoundLedger, MoneyClass::Challenge));
    check_call(&policy(true), Some(FRL), &frl, &mut reader).expect("open");
    check_call(&policy(true), Some(FRL), &frl, &mut reader).expect("open");
    assert_eq!(
        *reads.borrow(),
        vec![hex_addr(&FRL), hex_addr(&WM), hex_addr(&VERIFIER), hex_addr(&FRL), hex_addr(&WM), hex_addr(&VERIFIER)],
        "every signature reads the code again; nothing is cached"
    );
}

#[test]
fn open_with_other_code_is_refused_with_both_hashes() {
    let mut swapped = |a: &str| {
        if a == hex_addr(&WM) {
            Ok(b"a different build".to_vec())
        } else {
            chain(a)
        }
    };
    let got = check_call(&policy(true), Some(WM), &call(sig_of(FlContract::WorkMarket, MoneyClass::Join)), &mut swapped);
    assert_eq!(
        got,
        Err(PayoutsRefusal::CodeMismatch {
            contract: FlContract::WorkMarket,
            address: hex_addr(&WM),
            pinned: hex32(&keccak(WM_CODE)),
            live: hex32(&keccak(b"a different build")),
        })
    );
    let text = got.expect_err("mismatch").to_string();
    assert!(text.contains(&hex32(&keccak(WM_CODE))) && text.contains("not the pinned build"), "{text}");
}

#[test]
fn open_with_a_mismatched_verifier_refuses_a_work_market_call() {
    let mut bad_verifier = |a: &str| {
        if a == hex_addr(&VERIFIER) {
            Ok(b"another verifier".to_vec())
        } else {
            chain(a)
        }
    };
    let got = check_call(&policy(true), Some(WM), &call(sig_of(FlContract::WorkMarket, MoneyClass::Settle)), &mut bad_verifier);
    assert!(matches!(got, Err(PayoutsRefusal::CodeMismatch { contract: FlContract::FlRoundVerifier, .. })), "{got:?}");
}

#[test]
fn open_with_no_code_is_not_deployed() {
    let mut empty = |_: &str| Ok(Vec::new());
    let got = check_call(&policy(true), Some(WM), &call(sig_of(FlContract::WorkMarket, MoneyClass::Join)), &mut empty);
    assert_eq!(got, Err(PayoutsRefusal::NotDeployed { contract: FlContract::WorkMarket, address: hex_addr(&WM) }));
}

#[test]
fn open_with_an_unreadable_code_is_refused_not_assumed() {
    let mut down = |_: &str| Err("the RPC could not be reached".to_string());
    let got = check_call(&policy(true), Some(WM), &call(sig_of(FlContract::WorkMarket, MoneyClass::Join)), &mut down);
    assert_eq!(
        got,
        Err(PayoutsRefusal::CodeUnread {
            contract: FlContract::WorkMarket,
            address: hex_addr(&WM),
            reason: "the RPC could not be reached".into()
        })
    );
}

#[test]
fn open_on_a_network_without_work_market_refuses_payout_path_calls() {
    // The flag on a network whose WorkMarket is not pinned: a ledger challenge still needs the
    // payout path, which is not here.
    let frl = call(sig_of(FlContract::FederatedRoundLedger, MoneyClass::Challenge));
    let got = check_call(&r1007_like(true), Some(FRL), &frl, &mut chain);
    assert_eq!(
        got,
        Err(PayoutsRefusal::NotPinned {
            network: "r1007".into(),
            contract: FlContract::WorkMarket,
            class: MoneyClass::Challenge,
            why: "not deployed on r1007".into()
        })
    );
}

// --- not the pinned contract ------------------------------------------------------------------

#[test]
fn a_distinctive_work_market_call_to_an_unpinned_address_is_refused() {
    // r1007: no WorkMarket pinned, so a join to any address is not this network's WorkMarket.
    for open in [false, true] {
        let got = check_call(&r1007_like(open), Some(OTHER), &call(sig_of(FlContract::WorkMarket, MoneyClass::Join)), &mut no_read);
        assert_eq!(
            got,
            Err(PayoutsRefusal::NotThisNetworksContract {
                network: "r1007".into(),
                contract: FlContract::WorkMarket,
                class: MoneyClass::Join,
                to: hex_addr(&OTHER)
            })
        );
    }
    // A clone at another address while the real one is pinned.
    let got = check_call(&policy(true), Some(OTHER), &call(sig_of(FlContract::WorkMarket, MoneyClass::Open)), &mut no_read);
    assert!(matches!(got, Err(PayoutsRefusal::NotThisNetworksContract { contract: FlContract::WorkMarket, .. })));
}

#[test]
fn the_closed_default_refuses_distinctive_calls_anywhere() {
    let closed = PayoutsPolicy::closed("not installed");
    for m in MONEY_SELECTORS.iter().filter(|m| m.distinctive) {
        assert!(check_call(&closed, Some(OTHER), &call(m.signature), &mut no_read).is_err(), "{}", m.signature);
    }
}

// --- what the gate leaves alone ---------------------------------------------------------------

#[test]
fn generic_selectors_to_other_contracts_pass() {
    for signature in ["withdraw()", "refund(uint256)", "resolveDispute(uint256,bool)", "withdrawFees()"] {
        assert_eq!(check_call(&r1007_like(false), Some(OTHER), &call(signature), &mut no_read), Ok(()), "{signature}");
    }
}

#[test]
fn non_money_calls_to_fl_ledgers_pass() {
    // FRL.finalize(bytes32) and LoRAFactory.setPublicStatus(bytes32,bool): not money classes.
    assert_eq!(check_call(&policy(false), Some(FRL), &call("finalize(bytes32)"), &mut no_read), Ok(()));
    assert_eq!(check_call(&policy(false), Some(LORA), &call("setPublicStatus(bytes32,bool)"), &mut no_read), Ok(()));
}

#[test]
fn unrelated_calls_and_creations_pass() {
    assert_eq!(check_call(&policy(false), Some(OTHER), &call("transfer(address,uint256)"), &mut no_read), Ok(()));
    assert_eq!(check_call(&policy(false), Some(OTHER), &[], &mut no_read), Ok(()));
    assert_eq!(check_call(&policy(false), None, &call(sig_of(FlContract::WorkMarket, MoneyClass::Join)), &mut no_read), Ok(()));
}

#[test]
fn a_message_intent_is_not_a_call() {
    let intent = crate::ceremony::SignatureIntent {
        origin: "https://example.org".into(),
        kind: crate::ceremony::IntentKind::PersonalSign,
        chain_id: 40204,
        raw: format!("0x{}", hex::encode(call(sig_of(FlContract::WorkMarket, MoneyClass::Join)))),
    };
    assert_eq!(check_intent(&policy(false), &intent, &mut no_read), Ok(()));
}

#[test]
fn a_transaction_intent_is_checked_on_its_decoded_call() {
    let intent = crate::ceremony::SignatureIntent {
        origin: "https://example.org".into(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": "0x9858effd232b4033e47d90003d41ec34ecaeda94",
            "to": hex_addr(&WM),
            "value": "0x0",
            "data": format!("0x{}", hex::encode(call(sig_of(FlContract::WorkMarket, MoneyClass::Settle)))),
            "gas": "0x30000",
        })
        .to_string(),
    };
    assert!(matches!(check_intent(&policy(false), &intent, &mut no_read), Err(PayoutsRefusal::Dark { class: MoneyClass::Settle, .. })));
}

// --- status -----------------------------------------------------------------------------------

#[test]
fn status_dark_reads_nothing_and_says_why() {
    let reads = Cell::new(0);
    let s = status(&r1007_like(false), &mut |_: &str| {
        reads.set(reads.get() + 1);
        Ok(Vec::new())
    });
    assert_eq!(reads.get(), 0);
    assert!(!s.open && !s.payouts_open_flag);
    assert_eq!(s.reason.as_deref(), Some("FL payouts are not open on network \"r1007\" (payoutsOpen is false)"));
    let wm = &s.contracts[0];
    assert_eq!((wm.contract, wm.address.as_deref(), wm.reason.as_deref()), (FlContract::WorkMarket, None, Some("not deployed on r1007")));
    assert_eq!(s.contracts.len(), 6);
}

#[test]
fn status_open_needs_the_payout_path_pinned_and_matching() {
    let s = status(&policy(true), &mut chain);
    assert!(s.open, "{:?}", s.reason);
    assert_eq!(s.contracts[0].code_hash_matches, Some(true));

    let s = status(&r1007_like(true), &mut chain);
    assert!(!s.open);
    assert_eq!(s.reason.as_deref(), Some("WorkMarket is not pinned on this network (not deployed on r1007)"));

    let mut swapped = |a: &str| if a == hex_addr(&VERIFIER) { Ok(b"x".to_vec()) } else { chain(a) };
    let s = status(&policy(true), &mut swapped);
    assert!(!s.open);
    assert_eq!(s.contracts[1].code_hash_matches, Some(false));
    assert!(s.reason.as_deref().is_some_and(|r| r.contains("FLRoundVerifier")), "{:?}", s.reason);
}

#[test]
fn the_process_default_is_closed() {
    // No test installs a process-wide policy, so the default is what an app that forgot to install
    // one would sign against.
    let p = payouts_gate();
    assert!(!p.payouts_open && p.pins.is_empty());
}
