// HUP-S7.4 (US-7.1) — AgentSBT mint at onboarding: calldata, reads, revert decoding, readiness.
//
// Reroll 2026-10-05 (owner decision 2026-10-04): the member mints their own AgentSBT with
// `mintAgentAsMember(bytes32 did, bytes32 pubkey_fingerprint)`, gated on the membership SBT;
// the parent org is the contract's owner-set `memberOrgId()`. No registrar mints.
//
// Golden vectors were produced with foundry `cast` 1.5.1:
//
//   M=0x1111111111111111111111111111111111111111
//   PK=d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a   (RFC 8032 test 1 pubkey)
//   FP=$(printf $PK | xxd -r -p | shasum -a 256)
//   DID=$(cast keccak "did:citrate:agent:$M")
//   cast calldata "mintAgentAsMember(bytes32,bytes32)" $DID 0x$FP
//   cast sig "memberOrgId()"
//
// The OrgNotActive revert payload is a REAL anvil 1.5.1 answer against AgentSBT built from the
// chain repo source.
use super::*;
use serde_json::json;

const MEMBER: &str = "0x1111111111111111111111111111111111111111";
const RFC8032_PK: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const FP_HEX: &str = "21fe31dfa154a261626bf854046fd2271b7bed4b6abe45aa58877ef47f9721b9";
const DID_HASH_HEX: &str = "8aca81ccce51576b4a0f6e60d53c72b960f786d54bfbcd326be98a7ab37a7c8f";
const MINT_CAST: &str = "6bb964058aca81ccce51576b4a0f6e60d53c72b960f786d54bfbcd326be98a7ab37a7c8f21fe31dfa154a261626bf854046fd2271b7bed4b6abe45aa58877ef47f9721b9";
/// The canonical membership SBT the scripted transport answers for (any address works).
const MEMBER_SBT: &str = "0x2222222222222222222222222222222222222222";

fn arr32(h: &str) -> [u8; 32] {
    let v = hex::decode(h).expect("hex");
    let mut a = [0u8; 32];
    a.copy_from_slice(&v);
    a
}

fn member20() -> [u8; 20] {
    crate::validator::parse_address_20(MEMBER).expect("address")
}

// ------------------------------------------------------------------ identity + calldata

#[test]
fn mint_selector_is_the_member_mint_selector() {
    assert_eq!(
        hex::encode(crate::model_registry::selector(MINT_AGENT_AS_MEMBER_SIG)),
        "6bb96405"
    );
    assert_eq!(MINT_AGENT_AS_MEMBER_SIG, "mintAgentAsMember(bytes32,bytes32)");
}

/// The registrar path is gone: nothing in the member flow builds the owner-only `mintAgent`.
#[test]
fn the_member_flow_never_builds_the_owner_only_mint() {
    let cd = mint_agent_as_member_calldata(&arr32(DID_HASH_HEX), &arr32(FP_HEX));
    assert_ne!(&cd[..4], &crate::model_registry::selector("mintAgent(address,uint256,bytes32,bytes32)"));
    // Two static words after the selector; the recipient is msg.sender, never an argument.
    assert_eq!(cd.len(), 4 + 64);
}

#[test]
fn fingerprint_is_sha256_of_the_ed25519_pubkey_like_the_runtime() {
    // agent/core/src/hitl/signing.rs::signer_id_from_pubkey = hex(sha256(pubkey)).
    assert_eq!(hex::encode(pubkey_fingerprint(&arr32(RFC8032_PK))), FP_HEX);
}

#[test]
fn agent_did_is_lowercase_and_hashes_with_keccak() {
    let did = agent_did("0x1111111111111111111111111111111111111111").expect("did");
    assert_eq!(
        did,
        "did:citrate:agent:0x1111111111111111111111111111111111111111"
    );
    // Checksummed input normalises to the same DID (one member, one DID string).
    let mixed = agent_did("0xAbCdEf0000000000000000000000000000000001").expect("did");
    assert_eq!(
        mixed,
        "did:citrate:agent:0xabcdef0000000000000000000000000000000001"
    );
    assert_eq!(hex::encode(did_hash(&did)), DID_HASH_HEX);
}

#[test]
fn agent_did_rejects_a_malformed_address() {
    assert!(agent_did("0x1234").is_err());
    assert!(agent_did("not an address").is_err());
}

#[test]
fn mint_calldata_matches_cast_byte_for_byte() {
    let got = mint_agent_as_member_calldata(&arr32(DID_HASH_HEX), &arr32(FP_HEX));
    assert_eq!(hex::encode(got), MINT_CAST);
}

#[test]
fn read_calldata_matches_cast() {
    assert_eq!(
        hex::encode(balance_of_calldata(&member20())),
        "70a082310000000000000000000000001111111111111111111111111111111111111111"
    );
    assert_eq!(
        hex::encode(get_agent_calldata(3)),
        "2de5aaf70000000000000000000000000000000000000000000000000000000000000003"
    );
    assert_eq!(
        hex::encode(is_active_calldata(0)),
        "82afd23b0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(hex::encode(org_contract_calldata()), "6607f9d6");
    assert_eq!(hex::encode(member_org_id_calldata()), "1e3e1e8b");
}

#[test]
fn transfer_topic_is_the_erc721_transfer_event() {
    assert_eq!(
        TRANSFER_TOPIC,
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
    );
    // did_hash is plain keccak256 over the bytes, so it also derives the event topic.
    let t = format!(
        "0x{}",
        hex::encode(did_hash("Transfer(address,address,uint256)"))
    );
    assert_eq!(t, TRANSFER_TOPIC);
}

#[test]
fn minted_to_filter_pins_from_zero_and_to_member() {
    let f = minted_to_filter("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b", &member20());
    assert_eq!(f["address"], "0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b");
    assert_eq!(f["fromBlock"], "0x0");
    assert_eq!(f["topics"][0], TRANSFER_TOPIC);
    assert_eq!(f["topics"][1], format!("0x{}", "0".repeat(64)));
    assert_eq!(
        f["topics"][2],
        "0x0000000000000000000000001111111111111111111111111111111111111111"
    );
}

// ------------------------------------------------------------------ decoding

#[test]
fn decodes_get_agent_return() {
    // cast abi-encode "f((uint256,bytes32,bytes32,bool))" "(7,$DID,0x$FP,true)"
    let ret = hex::decode(format!("{:064x}{DID_HASH_HEX}{FP_HEX}{:064x}", 7, 1)).expect("hex");
    let a = decode_agent(5, &ret).expect("decodes");
    assert_eq!(a.token_id, "5");
    assert_eq!(a.parent_org_id, "7");
    assert_eq!(a.did, format!("0x{DID_HASH_HEX}"));
    assert_eq!(a.pubkey_fingerprint, format!("0x{FP_HEX}"));
    assert!(a.quarantined);
}

#[test]
fn short_get_agent_return_is_an_error_not_a_guess() {
    assert!(decode_agent(0, &[0u8; 64]).is_err());
}

#[test]
fn decodes_words() {
    let mut w = vec![0u8; 32];
    w[31] = 2;
    assert_eq!(decode_u128(&w).expect("u128"), 2);
    assert!(decode_u128(&[0u8; 31]).is_err());
    let mut big = vec![0u8; 32];
    big[0] = 1;
    assert!(
        decode_u128(&big).is_err(),
        "beyond u128 is refused, never truncated"
    );
    let mut b = vec![0u8; 32];
    b[31] = 1;
    assert!(decode_bool(&b).expect("bool"));
    let mut addr = vec![0u8; 32];
    addr[12..].copy_from_slice(&member20());
    assert_eq!(decode_address(&addr).expect("addr"), MEMBER);
}

#[test]
fn token_id_from_a_transfer_log() {
    let log = crate::rpc::LogEntry {
        topics: vec![
            TRANSFER_TOPIC.to_string(),
            format!("0x{}", "0".repeat(64)),
            "0x0000000000000000000000001111111111111111111111111111111111111111".into(),
            format!("0x{:064x}", 12),
        ],
        data: "0x".into(),
        block_number: 9,
    };
    assert_eq!(token_id_from_log(&log).expect("id"), 12);
    let bad = crate::rpc::LogEntry {
        topics: vec![TRANSFER_TOPIC.to_string()],
        data: "0x".into(),
        block_number: 9,
    };
    assert!(token_id_from_log(&bad).is_err());
}

// ------------------------------------------------------------------ revert classification

#[test]
fn classifies_the_org_not_active_revert_from_data_or_message() {
    let org = json!({"code":3,"message":"execution reverted: custom error 0xa4dde45e","data":"0xa4dde45e"});
    assert_eq!(classify_revert(&org), Preflight::OrgNotActive);
    let citrate = json!({"code":-32000,"message":"execution reverted: Execution reverted: Contract call reverted: 0xa4dde45e (gas used: 24955)"});
    assert_eq!(classify_revert(&citrate), Preflight::OrgNotActive);
}

/// An owner-only revert is no longer special: the member path never calls an issuer-only
/// function, so such a revert is shown verbatim instead of claiming a registrar exists.
#[test]
fn an_owner_only_revert_is_not_read_as_a_registrar_gate() {
    let not_owner = json!({"code":3,"message":"execution reverted: custom error 0x118cdaa7","data":"0x118cdaa7000000000000000000000000000000000000000000000000000000000000dead"});
    assert!(matches!(classify_revert(&not_owner), Preflight::Reverted(_)));
}

#[test]
fn an_unknown_revert_is_reported_verbatim() {
    let other = json!({"code":-32000,"message":"execution reverted: revm execution failed: Transaction(RejectCallerWithCode)"});
    match classify_revert(&other) {
        Preflight::Reverted(m) => assert!(m.contains("RejectCallerWithCode")),
        p => panic!("expected Reverted, got {p:?}"),
    }
}

// ------------------------------------------------------------------ readiness

fn ready_facts() -> Facts {
    Facts {
        contract: Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b".into()),
        has_code: Ok(true),
        held: Ok(0),
        member_org: Ok(Some(3)),
        is_member: Ok(true),
        org_active: Ok(true),
        identity_key: Ok([7u8; 32]),
        preflight: Ok(Preflight::Ok),
    }
}

#[test]
fn ready_only_when_every_check_passes() {
    let r = assess(&ready_facts());
    assert_eq!(r.state, MintState::Ready);
    assert!(r.available);
    assert!(r.message.contains("Signature Ceremony"));
}

#[test]
fn absent_from_the_book_is_after_the_network_upgrade() {
    let mut f = ready_facts();
    f.contract = None;
    let r = assess(&f);
    assert_eq!(r.state, MintState::NotInBook);
    assert!(!r.available);
    assert!(r.message.contains("available after the network upgrade"));
}

#[test]
fn no_code_is_after_the_network_upgrade() {
    let mut f = ready_facts();
    f.has_code = Ok(false);
    let r = assess(&f);
    assert_eq!(r.state, MintState::NoCode);
    assert!(!r.available);
    assert!(r.message.contains("available after the network upgrade"));
}

#[test]
fn an_unreachable_chain_never_claims_ready() {
    let mut f = ready_facts();
    f.has_code = Err("rpc transport error: timeout".into());
    let r = assess(&f);
    assert_eq!(r.state, MintState::ChainUnreachable);
    assert!(!r.available);
    assert!(r.message.contains("timeout"));
}

#[test]
fn already_holding_one_is_minted_not_offered_again() {
    let mut f = ready_facts();
    f.held = Ok(1);
    // Even if the rest would fail, a member who holds one sees it.
    f.org_active = Ok(false);
    let r = assess(&f);
    assert_eq!(r.state, MintState::Minted);
    assert!(!r.available, "no second mint is offered");
}

#[test]
fn inactive_parent_org_is_after_the_network_upgrade() {
    let mut f = ready_facts();
    f.org_active = Ok(false);
    let r = assess(&f);
    assert_eq!(r.state, MintState::OrgNotActive);
    assert!(!r.available);
    assert!(r.message.contains("available after the network upgrade"));
}

#[test]
fn missing_identity_key_says_what_unlocks_it() {
    let mut f = ready_facts();
    f.identity_key = Err("proposer key not minted yet".into());
    let r = assess(&f);
    assert_eq!(r.state, MintState::IdentityKeyMissing);
    assert!(!r.available);
    assert!(r.message.contains("node"));
}

#[test]
fn a_contract_without_member_issuance_is_after_the_network_upgrade() {
    let mut f = ready_facts();
    f.member_org = Ok(None);
    let r = assess(&f);
    assert_eq!(r.state, MintState::MemberMintUnavailable);
    assert!(!r.available);
    assert!(r.message.contains("available after the network upgrade"));
    assert!(!r.message.to_ascii_lowercase().contains("registrar"));
}

#[test]
fn a_wallet_without_the_membership_sbt_is_told_why() {
    let mut f = ready_facts();
    f.is_member = Ok(false);
    let r = assess(&f);
    assert_eq!(r.state, MintState::NotMember);
    assert!(!r.available);
    assert!(r.message.contains("membership"));
}

#[test]
fn unreadable_member_issuance_or_membership_is_unreachable_not_ready() {
    let mut f = ready_facts();
    f.member_org = Err("timeout".into());
    assert_eq!(assess(&f).state, MintState::ChainUnreachable);
    let mut f = ready_facts();
    f.is_member = Err("timeout".into());
    assert_eq!(assess(&f).state, MintState::ChainUnreachable);
}

#[test]
fn no_message_mentions_a_registrar_or_an_issuer() {
    for st in [
        MintState::Ready,
        MintState::Minted,
        MintState::NotInBook,
        MintState::NoCode,
        MintState::MemberMintUnavailable,
        MintState::NotMember,
        MintState::OrgNotActive,
        MintState::IdentityKeyMissing,
        MintState::ChainUnreachable,
        MintState::Reverted,
    ] {
        let m = message_for(st, "detail").to_ascii_lowercase();
        assert!(!m.contains("registrar") && !m.contains("issuer"), "{st:?}: {m}");
    }
}

#[test]
fn a_preflight_org_revert_maps_to_org_not_active() {
    let mut f = ready_facts();
    f.preflight = Ok(Preflight::OrgNotActive);
    assert_eq!(assess(&f).state, MintState::OrgNotActive);
}

#[test]
fn any_other_revert_blocks_with_the_reason() {
    let mut f = ready_facts();
    f.preflight = Ok(Preflight::Reverted("boom".into()));
    let r = assess(&f);
    assert_eq!(r.state, MintState::Reverted);
    assert!(!r.available);
    assert!(r.message.contains("boom"));
}

#[test]
fn mint_state_serialises_as_kebab_codes() {
    assert_eq!(
        serde_json::to_value(MintState::NotInBook).expect("json"),
        json!("not-in-book")
    );
    assert_eq!(
        serde_json::to_value(MintState::IdentityKeyMissing).expect("json"),
        json!("identity-key-missing")
    );
    assert_eq!(
        serde_json::to_value(MintState::MemberMintUnavailable).expect("json"),
        json!("member-mint-unavailable")
    );
    assert_eq!(
        serde_json::to_value(MintState::NotMember).expect("json"),
        json!("not-member")
    );
}

#[test]
fn user_facing_messages_have_no_em_dashes() {
    for st in [
        MintState::Ready,
        MintState::Minted,
        MintState::NotInBook,
        MintState::NoCode,
        MintState::MemberMintUnavailable,
        MintState::NotMember,
        MintState::OrgNotActive,
        MintState::IdentityKeyMissing,
        MintState::ChainUnreachable,
        MintState::Reverted,
    ] {
        let m = message_for(st, "detail");
        assert!(!m.contains('\u{2014}'), "{st:?}: {m}");
    }
}

// ------------------------------------------------------------------ the ceremony tx

#[test]
fn mint_tx_json_is_from_and_to_the_member_with_zero_value() {
    let raw = mint_tx_json(
        MEMBER,
        "0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b",
        &[0x6b, 0xb9, 0x64, 0x05],
        120_000,
    );
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(v["from"], MEMBER);
    assert_eq!(v["to"], "0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b");
    assert_eq!(v["value"], "0x0");
    assert_eq!(v["data"], "0x6bb96405");
    assert_eq!(v["gas"], "0x1d4c0");
    assert_eq!(v["chainId"], "0x9d0c");
}

fn status_with(state: MintState, available: bool, contract: Option<&str>) -> AgentSbtStatus {
    AgentSbtStatus {
        contract: contract.map(str::to_string),
        member: MEMBER.to_string(),
        did: agent_did(MEMBER).ok(),
        parent_org_id: Some("3".into()),
        balance: Some("0".into()),
        tokens: Some(Vec::new()),
        tokens_note: None,
        state,
        available,
        message: message_for(state, ""),
    }
}

/// Review fix (S7.4): the mint command refuses, with the member-facing reason and without
/// asking the node for gas, whenever readiness says the mint is not available.
#[test]
fn mint_request_refuses_when_not_available_and_never_estimates() {
    let c = "0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b";
    for (state, contract) in [
        (MintState::OrgNotActive, Some(c)),
        (MintState::MemberMintUnavailable, Some(c)),
        (MintState::NotMember, Some(c)),
        (MintState::Minted, Some(c)),
        (MintState::NotInBook, None),
        (MintState::ChainUnreachable, Some(c)),
    ] {
        let st = status_with(state, false, contract);
        let mut asked = false;
        let r = prepare_mint_tx(&st, &arr32(RFC8032_PK), |_| {
            asked = true;
            Ok(100_000)
        });
        assert_eq!(r, Err(message_for(state, "")), "{state:?}");
        assert!(
            !asked,
            "{state:?}: no gas estimate when the mint is refused"
        );
    }
    // `available` is the gate even if a state were inconsistent with it.
    let st = status_with(MintState::Ready, false, Some(c));
    assert!(prepare_mint_tx(&st, &arr32(RFC8032_PK), |_| Ok(1)).is_err());
}

#[test]
fn mint_request_when_ready_builds_the_exact_tx_with_margin() {
    let c = "0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b";
    let st = status_with(MintState::Ready, true, Some(c));
    let mut seen = None;
    let raw = prepare_mint_tx(&st, &arr32(RFC8032_PK), |q| {
        seen = Some(q);
        Ok(100_000)
    })
    .expect("ready");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(v["from"], MEMBER);
    assert_eq!(v["to"], c);
    assert_eq!(v["data"], format!("0x{MINT_CAST}"));
    assert_eq!(v["gas"], format!("0x{:x}", 125_000));
    let q = seen.expect("estimated");
    assert_eq!(q["data"], format!("0x{MINT_CAST}"));
    assert_eq!(q["from"], MEMBER);
    // A failed estimate is surfaced, never a guessed gas limit.
    let e = prepare_mint_tx(&st, &arr32(RFC8032_PK), |_| Err("boom".into()));
    assert!(e.expect_err("estimate failed").contains("boom"));
}

#[test]
fn gas_margin_adds_a_quarter_and_never_overflows() {
    assert_eq!(with_gas_margin(100_000), 125_000);
    assert_eq!(with_gas_margin(u64::MAX), u64::MAX);
}

#[test]
fn member_org_id_decodes_and_an_old_contract_reads_as_no_member_issuance() {
    // A word: the owner-set member org.
    assert_eq!(decode_member_org(&ok(json!(word(3)))), Ok(Some(3)));
    // The pre-reroll AgentSBT has no memberOrgId(): the call reverts (no fallback) or returns
    // no data. Both mean "this contract has no member issuance", not a chain failure.
    let reverted = json!({"jsonrpc":"2.0","id":1,"error":{"code":3,"message":"execution reverted"}});
    assert_eq!(decode_member_org(&reverted), Ok(None));
    assert_eq!(decode_member_org(&ok(json!("0x"))), Ok(None));
    // A malformed answer is an error, never a guessed org.
    assert!(decode_member_org(&json!({"jsonrpc":"2.0","id":1})).is_err());
}

#[test]
fn proposer_pubkey_hex_parses_to_32_bytes() {
    let k = parse_pubkey_hex(&format!("0x{RFC8032_PK}")).expect("32 bytes");
    assert_eq!(hex::encode(k), RFC8032_PK);
    assert!(parse_pubkey_hex("0x1234").is_err());
}

// ------------------------------------------------------------------ the gather over a transport

/// A scripted JSON-RPC transport for the gather tests: answers by method, records requests.
struct Scripted {
    answers: std::collections::HashMap<&'static str, serde_json::Value>,
    seen: std::cell::RefCell<Vec<serde_json::Value>>,
}

impl crate::rpc::RpcTransport for Scripted {
    fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, crate::rpc::RpcError> {
        self.seen.borrow_mut().push(body.clone());
        let method = body["method"].as_str().unwrap_or_default().to_string();
        let data = body["params"][0]["data"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let key: &str = match (method.as_str(), &data[..data.len().min(10)]) {
            ("eth_getCode", _) => "code",
            ("eth_getLogs", _) => "logs",
            ("eth_call", "0x70a08231") if body["params"][0]["to"] == MEMBER_SBT => "membership",
            ("eth_call", "0x70a08231") => "balance",
            ("eth_call", "0x1e3e1e8b") => "member_org",
            ("eth_call", "0x6607f9d6") => "org",
            ("eth_call", "0x82afd23b") => "active",
            ("eth_call", "0x6bb96405") => "preflight",
            ("eth_call", "0x2de5aaf7") => "agent",
            _ => "other",
        };
        let ans = self
            .answers
            .get(key)
            .cloned()
            .unwrap_or_else(|| json!({"error": {"message": format!("unscripted {key}")}}));
        Ok(ans)
    }
}

fn ok(v: serde_json::Value) -> serde_json::Value {
    json!({"jsonrpc":"2.0","id":1,"result": v})
}

fn word(n: u128) -> String {
    format!("0x{n:064x}")
}

fn live_like() -> std::collections::HashMap<&'static str, serde_json::Value> {
    let mut m = std::collections::HashMap::new();
    m.insert("code", ok(json!("0x6080")));
    m.insert("balance", ok(json!(word(0))));
    m.insert(
        "org",
        ok(json!(
            "0x000000000000000000000000b1bb65fc3f2188ff1209845cbe64eba985461689"
        )),
    );
    m.insert("active", ok(json!(word(0))));
    m.insert("member_org", ok(json!(word(3))));
    m.insert("membership", ok(json!(word(1))));
    m.insert("logs", ok(json!([])));
    m
}

#[test]
fn gather_reports_the_live_40204_state_honestly() {
    // A member of the new book whose member org is not active yet: isActive(3) is false.
    let t = Scripted {
        answers: live_like(),
        seen: Default::default(),
    };
    let rpc = crate::rpc::RpcClient::with_transport(t);
    let st = gather(&rpc, Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b"), Some(MEMBER_SBT), MEMBER, Ok([7u8; 32]));
    assert_eq!(st.state, MintState::OrgNotActive);
    assert!(!st.available);
    assert_eq!(st.balance.as_deref(), Some("0"));
    assert_eq!(st.tokens.as_ref().map(Vec::len), Some(0));
    assert_eq!(
        st.did.as_deref(),
        Some("did:citrate:agent:0x1111111111111111111111111111111111111111")
    );
    assert_eq!(st.parent_org_id.as_deref(), Some("3"), "the parent org is the contract's memberOrgId");
    // The org read went to the contract the AgentSBT itself names, not a guessed address.
    let seen = rpc.transport().seen.borrow();
    let active_call = seen
        .iter()
        .find(|b| {
            b["params"][0]["data"]
                .as_str()
                .unwrap_or("")
                .starts_with("0x82afd23b")
        })
        .expect("isActive was read");
    assert_eq!(
        active_call["params"][0]["to"],
        "0xb1bb65fc3f2188ff1209845cbe64eba985461689"
    );
}

#[test]
fn gather_without_a_book_entry_makes_no_rpc_calls() {
    let t = Scripted {
        answers: live_like(),
        seen: Default::default(),
    };
    let rpc = crate::rpc::RpcClient::with_transport(t);
    let st = gather(&rpc, None, Some(MEMBER_SBT), MEMBER, Ok([7u8; 32]));
    assert_eq!(st.state, MintState::NotInBook);
    assert!(rpc.transport().seen.borrow().is_empty());
}

#[test]
fn gather_on_the_pre_reroll_contract_says_after_the_upgrade_and_never_preflights() {
    let mut a = live_like();
    a.insert(
        "member_org",
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"execution reverted"}}),
    );
    let rpc = crate::rpc::RpcClient::with_transport(Scripted {
        answers: a,
        seen: Default::default(),
    });
    let st = gather(&rpc, Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b"), Some(MEMBER_SBT), MEMBER, Ok([7u8; 32]));
    assert_eq!(st.state, MintState::MemberMintUnavailable);
    assert_eq!(st.parent_org_id, None, "no member org is guessed");
    let seen = rpc.transport().seen.borrow();
    assert!(!seen.iter().any(|b| b["params"][0]["data"].as_str().unwrap_or("").starts_with("0x6bb96405")));
}

#[test]
fn gather_reads_membership_from_the_canonical_member_sbt() {
    let mut a = live_like();
    a.insert("membership", ok(json!(word(0))));
    let rpc = crate::rpc::RpcClient::with_transport(Scripted {
        answers: a,
        seen: Default::default(),
    });
    let st = gather(&rpc, Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b"), Some(MEMBER_SBT), MEMBER, Ok([7u8; 32]));
    assert_eq!(st.state, MintState::NotMember);
    assert!(!st.available);
    // Without a membership SBT in the book the gather cannot claim membership.
    let rpc = crate::rpc::RpcClient::with_transport(Scripted {
        answers: live_like(),
        seen: Default::default(),
    });
    let st = gather(&rpc, Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b"), None, MEMBER, Ok([7u8; 32]));
    assert_eq!(st.state, MintState::ChainUnreachable);
}

#[test]
fn gather_ready_preflights_the_member_mint_from_the_member() {
    let mut a = live_like();
    a.insert("active", ok(json!(word(1))));
    a.insert("preflight", ok(json!(word(0))));
    let rpc = crate::rpc::RpcClient::with_transport(Scripted {
        answers: a,
        seen: Default::default(),
    });
    let st = gather(&rpc, Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b"), Some(MEMBER_SBT), MEMBER, Ok(arr32(RFC8032_PK)));
    assert_eq!(st.state, MintState::Ready);
    assert!(st.available);
    let seen = rpc.transport().seen.borrow();
    let pf = seen
        .iter()
        .find(|b| {
            b["params"][0]["data"]
                .as_str()
                .unwrap_or("")
                .starts_with("0x6bb96405")
        })
        .expect("preflight ran");
    assert_eq!(pf["params"][0]["from"], MEMBER);
    assert_eq!(pf["params"][0]["data"], format!("0x{MINT_CAST}"));
}

#[test]
fn gather_lists_held_tokens_with_their_records() {
    let mut a = live_like();
    a.insert("balance", ok(json!(word(1))));
    a.insert(
        "logs",
        ok(json!([{
            "topics": [TRANSFER_TOPIC, format!("0x{}", "0".repeat(64)),
                "0x0000000000000000000000001111111111111111111111111111111111111111", word(4)],
            "data": "0x",
            "blockNumber": "0x10"
        }])),
    );
    a.insert(
        "agent",
        ok(json!(format!(
            "0x{:064x}{DID_HASH_HEX}{FP_HEX}{:064x}",
            0, 0
        ))),
    );
    let rpc = crate::rpc::RpcClient::with_transport(Scripted {
        answers: a,
        seen: Default::default(),
    });
    let st = gather(&rpc, Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b"), Some(MEMBER_SBT), MEMBER, Ok([7u8; 32]));
    assert_eq!(st.state, MintState::Minted);
    let toks = st.tokens.expect("tokens listed");
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].token_id, "4");
    assert_eq!(toks[0].did, format!("0x{DID_HASH_HEX}"));
}

#[test]
fn a_failed_log_scan_keeps_the_balance_and_says_tokens_are_unknown() {
    let mut a = live_like();
    a.insert("balance", ok(json!(word(1))));
    a.insert(
        "logs",
        json!({"jsonrpc":"2.0","id":1,"error":{"message":"range too large"}}),
    );
    let rpc = crate::rpc::RpcClient::with_transport(Scripted {
        answers: a,
        seen: Default::default(),
    });
    let st = gather(&rpc, Some("0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b"), Some(MEMBER_SBT), MEMBER, Ok([7u8; 32]));
    assert_eq!(st.state, MintState::Minted);
    assert_eq!(st.balance.as_deref(), Some("1"));
    assert!(st.tokens.is_none(), "unknown, never an empty list");
    assert!(st
        .tokens_note
        .as_deref()
        .unwrap_or("")
        .contains("range too large"));
}

// ------------------------------------------------------------------ anvil (real contract)

/// Deploys OrganizationSBT + AgentSBT built from the citrate-chain source on a local anvil and
/// checks the member path against it through [`gather`] and [`mint_agent_as_member_calldata`].
///
/// Needs anvil and the forge artifacts, so it is opt-in. Run it with
/// `scripts/anvil-agent-sbt.sh`, which compiles the chain source into a scratch dir and sets
/// `CITRATE_AGENT_SBT_ARTIFACTS` before running this test with `--ignored`.
#[test]
#[ignore = "needs anvil + AgentSBT artifacts: run scripts/anvil-agent-sbt.sh"]
fn anvil_pre_reroll_agent_sbt_reads_as_no_member_issuance() {
    use std::process::{Command, Stdio};

    let artifacts = std::env::var("CITRATE_AGENT_SBT_ARTIFACTS").expect(
        "set CITRATE_AGENT_SBT_ARTIFACTS to the forge out dir (see scripts/anvil-agent-sbt.sh)",
    );
    let port: u16 = std::env::var("CITRATE_AGENT_SBT_ANVIL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(18_645);
    let url = format!("http://127.0.0.1:{port}");

    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _anvil = Kill(
        Command::new("anvil")
            .args(["--port", &port.to_string(), "--silent"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("anvil on PATH"),
    );

    let rpc = crate::rpc::RpcClient::with_transport(crate::rpc::HttpTransport::new(url.clone()));
    // Wait for anvil to answer.
    let mut up = false;
    for _ in 0..50 {
        if rpc.block_number().is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(up, "anvil did not come up on {url}");

    // anvil's well-known unlocked dev accounts (public test keys, no value).
    let owner = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
    let member = "0x70997970c51812dc3a010c7d01b50e44c4ccb3a8";

    let raw = |method: &str, params: serde_json::Value| -> serde_json::Value {
        let body = rpc.build_request(method, params);
        crate::rpc::RpcTransport::call(rpc.transport(), body).expect("transport")
    };
    let send = |tx: serde_json::Value| -> serde_json::Value {
        let r = raw("eth_sendTransaction", json!([tx]));
        let h = r["result"]
            .as_str()
            .unwrap_or_else(|| panic!("send failed: {r}"))
            .to_string();
        // anvil mines on submit, but the receipt can lag the RPC answer by a moment.
        let mut rc = json!(null);
        for _ in 0..50 {
            rc = raw("eth_getTransactionReceipt", json!([h]));
            if !rc["result"].is_null() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(
            rc["result"]["status"], "0x1",
            "tx reverted or not mined: {rc}"
        );
        rc["result"].clone()
    };
    let creation = |name: &str| -> String {
        let p = std::path::Path::new(&artifacts).join(format!("{name}.sol/{name}.json"));
        let j: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).expect("artifact")).expect("json");
        j["bytecode"]["object"]
            .as_str()
            .expect("bytecode")
            .to_string()
    };
    let pad = |a: &str| format!("{:0>64}", a.trim_start_matches("0x"));

    // OrganizationSBT(owner) then AgentSBT(owner, org).
    let org = send(json!({"from": owner, "data": format!("{}{}", creation("OrganizationSBT"), pad(owner)), "gas": "0x1c9c380"}))
        ["contractAddress"].as_str().expect("org addr").to_string();
    let agent = send(json!({"from": owner, "data": format!("{}{}{}", creation("AgentSBT"), pad(owner), pad(&org)), "gas": "0x1c9c380"}))
        ["contractAddress"].as_str().expect("agent addr").to_string();

    let member_key = parse_pubkey_hex(&format!("0x{RFC8032_PK}")).expect("key");
    // Any address with no code stands in for the membership SBT: on this contract the gather
    // stops before the membership read.
    let member_sbt = "0x2222222222222222222222222222222222222222";

    // 1. Before any org exists, on the pre-reroll AgentSBT (owner-only mintAgent, no
    //    memberOrgId): the member path is honestly unavailable, nothing is offered and the
    //    member mint is never preflighted.
    let st = gather(&rpc, Some(agent.as_str()), Some(member_sbt), member, Ok(member_key));
    assert_eq!(st.state, MintState::MemberMintUnavailable, "{st:?}");
    assert!(!st.available);
    assert_eq!(st.parent_org_id, None);

    // 2. The owner mints org 0: mintOrg(owner, keccak("did:citrate:org:test"), owner, []).
    let org_did = did_hash("did:citrate:org:test");
    let mut mint_org =
        crate::model_registry::selector("mintOrg(address,bytes32,address,bytes32[])").to_vec();
    mint_org.extend(hex::decode(pad(owner)).expect("hex"));
    mint_org.extend(org_did);
    mint_org.extend(hex::decode(pad(owner)).expect("hex"));
    mint_org.extend(hex::decode(format!("{:064x}{:064x}", 128, 0)).expect("hex"));
    send(
        json!({"from": owner, "to": org, "data": format!("0x{}", hex::encode(&mint_org)), "gas": "0x7a120"}),
    );

    // 3. An active org does not change that: the contract still has no member issuance, and
    //    the app never falls back to an issuer-only mint (no registrar path).
    let st = gather(&rpc, Some(agent.as_str()), Some(member_sbt), member, Ok(member_key));
    assert_eq!(st.state, MintState::MemberMintUnavailable, "{st:?}");
    let st = gather(&rpc, Some(agent.as_str()), Some(member_sbt), owner, Ok(member_key));
    assert_eq!(st.state, MintState::MemberMintUnavailable, "{st:?}");

    // 4. The exact member-mint calldata reverts on this contract (unknown selector), so a
    //    build that skipped the readiness check would still never mint here.
    let calldata = mint_agent_as_member_calldata(
        &did_hash(&agent_did(member).expect("did")),
        &pubkey_fingerprint(&member_key),
    );
    let r = raw("eth_call", json!([{"from": member, "to": agent, "data": format!("0x{}", hex::encode(&calldata))}, "latest"]));
    assert!(r.get("error").is_some(), "mintAgentAsMember must not exist on the pre-reroll contract: {r}");
    // The member-path mint itself (Ready, then Minted with parent org = memberOrgId) runs
    // against the reroll AgentSBT once the chain lane's mintAgentAsMember lands on
    // citrate-chain reroll/panic-s1; this script then builds it from that branch.
}
