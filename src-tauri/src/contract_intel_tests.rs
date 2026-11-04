// CORE-A1 / CORE-A2 — contract lookup, identification and ABI resolution. Pure: a scripted
// JSON-RPC transport (answers chosen per method and calldata) and a scripted explorer. No network.
use super::*;
use std::cell::RefCell;

type Answer = Result<Value, String>;

/// A transport that answers each request through a closure over (method, params) and records them.
struct FnRpc<F: Fn(&str, &Value) -> Answer> {
    answer: F,
    seen: RefCell<Vec<(String, Value)>>,
}

impl<F: Fn(&str, &Value) -> Answer> crate::rpc::RpcTransport for FnRpc<F> {
    fn call(&self, body: Value) -> Result<Value, crate::rpc::RpcError> {
        let method = body["method"].as_str().unwrap_or_default().to_string();
        let params = body["params"].clone();
        self.seen.borrow_mut().push((method.clone(), params.clone()));
        if method == "__transport_down__" {
            return Err(crate::rpc::RpcError::Transport("down".into()));
        }
        match (self.answer)(&method, &params) {
            Ok(v) => Ok(json!({"jsonrpc": "2.0", "id": body["id"], "result": v})),
            Err(m) if m == "TRANSPORT" => Err(crate::rpc::RpcError::Transport("connection refused".into())),
            Err(m) => Ok(json!({"jsonrpc": "2.0", "id": body["id"], "error": {"code": 3, "message": m}})),
        }
    }
}

fn rpc<F: Fn(&str, &Value) -> Answer>(f: F) -> RpcClient<FnRpc<F>> {
    RpcClient::with_transport(FnRpc {
        answer: f,
        seen: RefCell::new(Vec::new()),
    })
}

/// A scripted explorer: one status + body for every URL, recorded.
struct Scan {
    reply: Result<(u16, String), String>,
    urls: RefCell<Vec<String>>,
}

impl ScanHttp for Scan {
    fn get(&self, url: &str) -> Result<crate::verified_source::ScanReply, String> {
        self.urls.borrow_mut().push(url.to_string());
        self.reply
            .clone()
            .map(|(status, body)| crate::verified_source::ScanReply { status, body })
    }
}

fn scan(status: u16, body: Value) -> Scan {
    Scan {
        reply: Ok((status, body.to_string())),
        urls: RefCell::new(Vec::new()),
    }
}

fn scan_unverified() -> Scan {
    scan(200, json!({"status": "unverified", "verified": false}))
}

fn scan_down() -> Scan {
    Scan {
        reply: Err("CitrateScan request failed: timeout".into()),
        urls: RefCell::new(Vec::new()),
    }
}

const BASE: &str = "https://explorer.example";
const UNKNOWN: &str = "0x1111111111111111111111111111111111111111";
const IMPL: &str = "0x2222222222222222222222222222222222222222";
const BEACON: &str = "0x3333333333333333333333333333333333333333";
const OWNER: &str = "0x4444444444444444444444444444444444444444";

fn book_addr(name: &str) -> String {
    book_entries()
        .expect("book")
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.address.clone())
        .expect("name in the book")
}

fn word_hex(addr: &str) -> String {
    format!("0x{}{}", "0".repeat(24), addr.trim_start_matches("0x"))
}

fn zero_word() -> Value {
    json!(format!("0x{}", "0".repeat(64)))
}

fn uint_word(n: u128) -> String {
    format!("0x{n:064x}")
}

fn data_of(p: &Value) -> String {
    p[0]["data"].as_str().unwrap_or_default().to_string()
}

fn to_of(p: &Value) -> String {
    p[0]["to"].as_str().unwrap_or_default().to_ascii_lowercase()
}

fn sel(sig: &str) -> String {
    format!("0x{}", hex::encode(selector(sig)))
}

/// Runtime code holding a PUSH4 of each selector (each preceded by a DUP1 and followed by EQ).
fn code_with_selectors(sels: &[&str]) -> String {
    let mut c = String::from("0x6080604052");
    for s in sels {
        c.push_str("8063");
        c.push_str(s);
        c.push_str("14");
    }
    c.push_str("00");
    c
}

fn erc20_code() -> String {
    code_with_selectors(&ERC20_SELECTORS.map(|(_, s)| s))
}

fn enc_string(s: &str) -> String {
    let b = s.as_bytes();
    let pad = (32 - b.len() % 32) % 32;
    format!("0x{:064x}{:064x}{}{}", 32, b.len(), hex::encode(b), "00".repeat(pad))
}

// ---- the embedded book and the bundles ----

#[test]
fn the_embedded_book_lists_the_full_canonical_book_not_the_app_subset() {
    let entries = book_entries().expect("book");
    for name in [
        "WrappedSALT",
        "ModelRegistry",
        "SkillRegistry",
        "AgentSBT",
        "CapsuleRegistry",
        "EntryPoint",
        "CitrateWalletFactory",
        "GovernanceTimelock",
        "GOVERNANCE",
        "OrangeSeed",
        "ModelInference",
        "CitrateMemberSBT",
    ] {
        assert!(entries.iter().any(|e| e.name == name), "{name} is missing from the book");
    }
    // The generated 40204.json carries 15 names; the canonical book is several times that.
    assert!(entries.len() > 80, "only {} entries", entries.len());
    for e in entries {
        assert_eq!(e.address, e.address.to_ascii_lowercase());
        assert_eq!(e.address.len(), 42);
    }
    // An alias shares its address and is listed under both names.
    assert_eq!(book_addr("GOVERNANCE"), book_addr("GovernanceTimelock"));
}

#[test]
fn every_contract_in_the_book_has_a_bundled_abi_or_an_explicit_none() {
    // Reroll tripwire: a book that gains a contract without regenerating the bundle fails here.
    let abis: Value = serde_json::from_str(SYSTEM_ABIS_JSON).expect("abis");
    let names = abis["contracts"].as_object().expect("contracts");
    let ids = abis["abis"].as_object().expect("abis");
    for e in book_entries().expect("book") {
        if e.section == "precompiles" {
            assert!(e.abi_contract.is_none());
            continue;
        }
        let entry = names.get(&e.name).unwrap_or_else(|| {
            panic!("{} has no entry in abis/40204.system-abis.json; re-run scripts/gen-system-abis.mjs", e.name)
        });
        match entry["contract"].as_str() {
            Some(id) => assert!(ids.contains_key(id), "{}: ABI {id} is not bundled", e.name),
            None => assert!(entry["note"].is_string(), "{}: an ABI-less entry needs a note", e.name),
        }
    }
}

#[test]
fn the_bundles_are_generated_not_hand_edited() {
    for (file, src) in [
        ("40204.system-abis.json", SYSTEM_ABIS_JSON),
        ("signatures.json", SIGNATURES_JSON),
    ] {
        assert!(
            src.contains("GENERATED by scripts/gen-system-abis.mjs"),
            "{file} lost its generated marker; re-run the script rather than editing it"
        );
    }
}

/// The canonical type list of an ABI parameter array (tuples expanded), as selectors hash it.
fn canonical_types(params: &Value) -> String {
    params
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    let t = p["type"].as_str().unwrap_or_default();
                    match t.strip_prefix("tuple") {
                        Some(suffix) => format!("({}){suffix}", canonical_types(&p["components"])),
                        None => t.to_string(),
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default()
}

fn keccak_hex(s: &str) -> String {
    format!("0x{}", hex::encode(Keccak256::digest(s.as_bytes())))
}

#[test]
fn every_bundled_signature_hashes_to_its_key() {
    let sigs: Value = serde_json::from_str(SIGNATURES_JSON).expect("signatures");
    let mut n = 0;
    for (section, len) in [("functions", 10), ("errors", 10), ("events", 66)] {
        for (key, list) in sigs[section].as_object().expect("section") {
            for f in list.as_array().expect("list") {
                let sig = f["sig"].as_str().expect("sig");
                assert_eq!(&keccak_hex(sig)[..len], key, "{section} {sig}");
                n += 1;
            }
        }
    }
    assert!(n > 1000, "only {n} signatures");
}

#[test]
fn the_signature_subset_covers_every_bundled_abi_and_the_standard_set() {
    let abis: Value = serde_json::from_str(SYSTEM_ABIS_JSON).expect("abis");
    let b = bundles().expect("bundles");
    for (id, abi) in abis["abis"].as_object().expect("abis") {
        for item in abi.as_array().expect("abi") {
            let name = item["name"].as_str().unwrap_or_default();
            let sig = format!("{name}({})", canonical_types(&item["inputs"]));
            match item["type"].as_str() {
                Some("function") => assert!(b.functions.contains_key(&keccak_hex(&sig)[..10]), "{id} {sig}"),
                Some("event") if item["anonymous"] != json!(true) => {
                    assert!(b.events.contains_key(&keccak_hex(&sig)), "{id} {sig}")
                }
                _ => {}
            }
        }
    }
    // The curated standard set: ERC-20 transfer and the shared ERC-20/721 Transfer topic.
    assert!(b.functions.contains_key("0xa9059cbb"));
    let transfer = &b.events["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"];
    assert!(transfer.iter().any(|f| f.contains("uint256 value")));
    assert!(transfer.iter().any(|f| f.contains("uint256 indexed tokenId")));
}

#[test]
fn signature_lookup_answers_known_keys_and_reports_unknown_ones_as_empty() {
    let m = lookup_signatures(
        &["0xA9059CBB".into(), "0xdeadbeef".into()],
        &["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into()],
    )
    .expect("lookup");
    assert!(m.functions["0xa9059cbb"][0].starts_with("function transfer(address"));
    assert!(m.functions["0xdeadbeef"].is_empty(), "an unknown selector is never guessed");
    assert_eq!(m.events.values().next().map(Vec::len), Some(2));
    assert!(m.errors["0xa9059cbb"].is_empty());
    let err = lookup_signatures(&["0x08c379a0".into()], &[]).expect("lookup");
    assert_eq!(err.errors["0x08c379a0"], vec!["error Error(string message)".to_string()]);
    assert!(lookup_signatures(&["0x123".into()], &[]).is_err());
    assert!(lookup_signatures(&[], &["0x12".into()]).is_err());
    let many: Vec<String> = (0..65).map(|i| format!("0x{i:08x}")).collect();
    assert!(lookup_signatures(&many, &[]).is_err());
}

// ---- name search ----

#[test]
fn name_search_ranks_exact_then_prefix_then_substring_and_ignores_case_and_spacing() {
    let entries = book_entries().expect("book");
    let hits = search_book(entries, "model registry");
    assert_eq!(hits.first().map(|e| e.name.as_str()), Some("ModelRegistry"));
    let hits = search_book(entries, "wrappedsalt");
    assert_eq!(hits.first().map(|e| e.name.as_str()), Some("WrappedSALT"));
    let hits = search_book(entries, "Registry");
    assert!(hits.len() > 3);
    assert!(hits.len() <= MAX_BOOK_MATCHES);
    let hits = search_book(entries, "ipfs incentives");
    assert!(hits.iter().any(|e| e.name == "IPFSIncentivesV3"));
    assert!(search_book(entries, "no such contract anywhere").is_empty());
    assert!(search_book(entries, "  ").is_empty());
}

#[test]
fn address_search_returns_every_name_at_that_address() {
    let entries = book_entries().expect("book");
    let gov = book_addr("GovernanceTimelock");
    let names: Vec<String> = search_book(entries, &gov.to_ascii_uppercase().replacen("0X", "0x", 1))
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(names.contains(&"GovernanceTimelock".to_string()));
    assert!(names.contains(&"GOVERNANCE".to_string()));
    assert!(search_book(entries, UNKNOWN).is_empty());
}

// ---- address_book_lookup with the registries ----

fn skill_registry_answers(method: &str, p: &Value) -> Answer {
    if method != "eth_call" {
        return Err(format!("unexpected {method}"));
    }
    let to = to_of(p);
    let data = data_of(p);
    if to == book_addr("SkillRegistry") && data == sel("getAllSkillHashes()") {
        return Ok(json!(format!("0x{:064x}{:064x}{}", 32, 1, "ab".repeat(32))));
    }
    if to == book_addr("SkillRegistry") && data.starts_with(&sel("getSkill(bytes32)")) {
        // (owner, name, version, manifestCID, description, isActive)
        let s = |x: &str| {
            let b = x.as_bytes();
            format!("{:064x}{}{}", b.len(), hex::encode(b), "00".repeat((32 - b.len() % 32) % 32))
        };
        let (n, v, c, d) = (s("contract-deploy"), s("1.0.0"), s(""), s("Deploy a contract."));
        let head = 6 * 32;
        let o1 = head;
        let o2 = o1 + n.len() / 2;
        let o3 = o2 + v.len() / 2;
        let o4 = o3 + c.len() / 2;
        return Ok(json!(format!(
            "0x{}{:064x}{:064x}{:064x}{:064x}{:064x}{n}{v}{c}{d}",
            &word_hex(OWNER)[2..],
            o1,
            o2,
            o3,
            o4,
            1
        )));
    }
    if to == book_addr("ModelRegistry") {
        return Err("TRANSPORT".into());
    }
    Err("execution reverted".into())
}

#[test]
fn a_name_lookup_searches_the_book_and_the_skill_and_model_registries() {
    let r = rpc(skill_registry_answers);
    let out = address_book_lookup(&r, "contract deploy").expect("lookup");
    assert_eq!(out.kind, "name");
    assert_eq!(out.chain_id, 40204);
    let skills = out.registries.iter().find(|x| x.registry == "SkillRegistry").expect("skills");
    assert_eq!(skills.status, "read");
    assert_eq!(skills.matches.len(), 1);
    assert_eq!(skills.matches[0]["name"], "contract-deploy");
    assert_eq!(skills.matches[0]["owner"], OWNER);
    // A registry that cannot be read is unavailable, never an empty "read".
    let models = out.registries.iter().find(|x| x.registry == "ModelRegistry").expect("models");
    assert_eq!(models.status, "unavailable");
    assert!(models.matches.is_empty());
    assert!(models.note.contains("Unknown, not empty"));
    let agents = out.registries.iter().find(|x| x.registry == "AgentSBT").expect("agents");
    assert_eq!(agents.status, "not-searchable");
    let capsules = out.registries.iter().find(|x| x.registry == "CapsuleRegistry").expect("capsules");
    assert_eq!(capsules.status, "not-searchable");
    assert!(out.book.is_empty(), "no book contract is named contract-deploy");
    assert!(out.note.contains("not in the book"));
}

#[test]
fn an_address_lookup_reads_owner_indexes_and_the_agent_sbt_balance() {
    let r = rpc(|method, p| {
        assert_eq!(method, "eth_call");
        let data = data_of(p);
        let to = to_of(p);
        if data.starts_with(&sel("getModelsByOwner(address)")) {
            assert!(data.ends_with(&OWNER[2..]));
            return Ok(json!(format!("0x{:064x}{:064x}{}{}", 32, 2, "11".repeat(32), "22".repeat(32))));
        }
        if data.starts_with(&sel("getSkillsByOwner(address)")) {
            return Ok(json!(format!("0x{:064x}{:064x}", 32, 0)));
        }
        if to == book_addr("AgentSBT") && data.starts_with(&sel("balanceOf(address)")) {
            return Ok(json!(uint_word(1)));
        }
        Err("execution reverted".into())
    });
    let out = address_book_lookup(&r, OWNER).expect("lookup");
    assert_eq!(out.kind, "address");
    let by = |n: &str| out.registries.iter().find(|x| x.registry == n).expect("registry").clone();
    assert_eq!(by("ModelRegistry").matches[0]["modelsOwned"], 2);
    assert!(by("SkillRegistry").matches.is_empty());
    assert_eq!(by("SkillRegistry").status, "read");
    assert_eq!(by("AgentSBT").matches[0]["agentTokens"], "1");
}

#[test]
fn a_lookup_refuses_an_empty_or_overlong_query() {
    let r = rpc(|_, _| Err("unused".into()));
    assert!(address_book_lookup(&r, "   ").is_err());
    assert!(address_book_lookup(&r, &"x".repeat(MAX_QUERY_CHARS + 1)).is_err());
}

// ---- get_code and proxies ----

#[test]
fn eip1167_minimal_proxy_code_names_its_implementation() {
    let code = hex::decode(format!(
        "363d3d373d3d3d363d73{}5af43d82803e903d91602b57fd5bf3",
        &IMPL[2..]
    ))
    .expect("hex");
    assert_eq!(eip1167_target(&code).as_deref(), Some(IMPL));
    assert_eq!(eip1167_target(&code[..44]), None);
    assert_eq!(eip1167_target(&[0u8; 45]), None);
}

#[test]
fn push4_scan_reads_opcodes_and_never_push_data() {
    // PUSH32 whose data contains 0x63 aabbccdd must not yield a selector; a real PUSH4 must.
    let code = hex::decode(format!("7f{}{}63a9059cbb00", "63aabbccdd", "00".repeat(27))).expect("hex");
    let consts = push4_constants(&code);
    assert!(consts.contains("a9059cbb"));
    assert!(!consts.contains("aabbccdd"));
    // A truncated PUSH4 at the end of the code is ignored, not read past the end.
    assert!(push4_constants(&[0x63, 0x01, 0x02]).is_empty());
}

#[test]
fn an_account_without_code_is_not_a_contract_and_the_explorer_is_not_asked() {
    let r = rpc(|m, _| match m {
        "eth_getCode" => Ok(json!("0x")),
        other => Err(format!("unexpected {other}")),
    });
    let s = scan_unverified();
    let info = get_code(&r, &s, BASE, UNKNOWN).expect("code");
    assert!(!info.is_contract);
    assert_eq!(info.code_size, 0);
    assert_eq!(info.code_hash, None);
    assert_eq!(info.proxy, None);
    assert!(s.urls.borrow().is_empty());
}

#[test]
fn get_code_reports_size_hash_an_eip1967_proxy_and_the_explorer_status() {
    let impl_addr = book_addr("WrappedSALT");
    let ia = impl_addr.clone();
    let r = rpc(move |m, p| match m {
        "eth_getCode" if p[0] == json!(UNKNOWN) => Ok(json!("0x6080604052")),
        "eth_getCode" => Ok(json!(code_with_selectors(&["a9059cbb"]))),
        "eth_getStorageAt" if p[1] == json!(EIP1967_IMPLEMENTATION_SLOT) => Ok(json!(word_hex(&ia))),
        "eth_getStorageAt" => Ok(zero_word()),
        other => Err(format!("unexpected {other}")),
    });
    let s = scan_unverified();
    let info = get_code(&r, &s, BASE, UNKNOWN).expect("code");
    assert!(info.is_contract);
    assert_eq!(info.code_size, 5);
    assert_eq!(
        info.code_hash.as_deref(),
        Some(keccak_hex_bytes(&[0x60, 0x80, 0x60, 0x40, 0x52]).as_str())
    );
    let p = info.proxy.expect("proxy");
    assert_eq!(p.kind, "eip-1967");
    assert_eq!(p.implementation, impl_addr);
    assert_eq!(p.implementation_book_names, vec!["WrappedSALT".to_string()]);
    assert!(p.implementation_code_size.is_some());
    assert_eq!(info.verified.status, "unverified");
    assert_eq!(s.urls.borrow().len(), 1);
}

fn keccak_hex_bytes(b: &[u8]) -> String {
    format!("0x{}", hex::encode(Keccak256::digest(b)))
}

#[test]
fn a_beacon_proxy_resolves_through_the_beacons_implementation() {
    let r = rpc(|m, p| match m {
        "eth_getCode" => Ok(json!("0x60806040")),
        "eth_getStorageAt" if p[1] == json!(EIP1967_BEACON_SLOT) => Ok(json!(word_hex(BEACON))),
        "eth_getStorageAt" => Ok(zero_word()),
        "eth_call" if to_of(p) == BEACON && data_of(p) == sel("implementation()") => Ok(json!(word_hex(IMPL))),
        other => Err(format!("unexpected {other}")),
    });
    let info = get_code(&r, &scan_unverified(), BASE, UNKNOWN).expect("code");
    let p = info.proxy.expect("proxy");
    assert_eq!(p.kind, "eip-1967-beacon");
    assert_eq!(p.beacon.as_deref(), Some(BEACON));
    assert_eq!(p.implementation, IMPL);
}

#[test]
fn an_unreadable_explorer_is_unavailable_never_unverified() {
    let r = rpc(|m, _| match m {
        "eth_getCode" => Ok(json!("0x60806040")),
        "eth_getStorageAt" => Ok(zero_word()),
        other => Err(format!("unexpected {other}")),
    });
    let info = get_code(&r, &scan_down(), BASE, UNKNOWN).expect("code");
    assert_eq!(info.verified.status, "unavailable");
    assert!(info.verified.note.contains("unknown, not unverified"));
    assert_eq!(info.proxy, None);
}

#[test]
fn unreadable_proxy_storage_is_an_error_not_a_plain_contract() {
    let r = rpc(|m, _| match m {
        "eth_getCode" => Ok(json!("0x60806040")),
        _ => Err("TRANSPORT".into()),
    });
    assert!(get_code(&r, &scan_unverified(), BASE, UNKNOWN).is_err());
}

// ---- identify_contract ----

fn token_answers(m: &str, p: &Value) -> Answer {
    match m {
        "eth_getCode" => Ok(json!(erc20_code())),
        "eth_getStorageAt" => Ok(zero_word()),
        "eth_call" => {
            let d = data_of(p);
            if d.starts_with(&sel("supportsInterface(bytes4)")) {
                Err("execution reverted".into())
            } else if d == sel("totalSupply()") {
                Ok(json!(uint_word(1_000_000)))
            } else if d == sel("decimals()") {
                Ok(json!(uint_word(18)))
            } else if d == sel("symbol()") {
                Ok(json!(enc_string("TKN")))
            } else if d == sel("name()") {
                Ok(json!(enc_string("Token")))
            } else {
                Err("execution reverted".into())
            }
        }
        other => Err(format!("unexpected {other}")),
    }
}

#[test]
fn an_unverified_token_outside_the_book_is_identified_from_its_code_and_reads() {
    let r = rpc(token_answers);
    let id = identify(&r, &scan_unverified(), BASE, UNKNOWN).expect("identify");
    assert!(id.book_names.is_empty());
    assert_eq!(id.abi_source, "none");
    assert_eq!(id.erc165, "no");
    assert!(id.interfaces.is_empty());
    assert_eq!(id.erc20.verdict, "likely");
    assert!(id.erc20.selectors_missing.is_empty());
    assert_eq!(id.erc20.total_supply.as_deref(), Some("1000000"));
    assert_eq!(id.erc20.decimals, Some(18));
    assert_eq!(id.erc20.symbol.as_deref(), Some("TKN"));
    assert!(id
        .known_selectors
        .iter()
        .any(|s| s.selector == "0xa9059cbb" && s.signatures[0].contains("transfer(address")));
    assert!(id.note.contains("clues, not proof"));
}

#[test]
fn an_erc721_is_found_by_erc165_and_is_not_called_an_erc20() {
    let r = rpc(|m, p| match m {
        "eth_getCode" => Ok(json!(code_with_selectors(&["6352211e", "70a08231", "23b872dd", "095ea7b3"]))),
        "eth_getStorageAt" => Ok(zero_word()),
        "eth_call" => {
            let d = data_of(p);
            let id = d.get(10..18).unwrap_or_default().to_string();
            if d.starts_with(&sel("supportsInterface(bytes4)")) {
                Ok(json!(uint_word(u128::from(matches!(id.as_str(), "01ffc9a7" | "80ac58cd" | "5b5e139f")))))
            } else {
                Err("execution reverted".into())
            }
        }
        other => Err(format!("unexpected {other}")),
    });
    let id = identify(&r, &scan_unverified(), BASE, UNKNOWN).expect("identify");
    assert_eq!(id.erc165, "yes");
    assert_eq!(id.interfaces["erc721"], "yes");
    assert_eq!(id.interfaces["erc721Metadata"], "yes");
    assert_eq!(id.interfaces["erc1155"], "no");
    assert_eq!(id.erc20.verdict, "no");
}

#[test]
fn an_unreachable_node_leaves_erc165_unknown() {
    let r = rpc(|m, p| match m {
        "eth_getCode" => Ok(json!("0x60806040")),
        "eth_getStorageAt" => Ok(zero_word()),
        "eth_call" if data_of(p).starts_with(&sel("supportsInterface(bytes4)")) => Err("TRANSPORT".into()),
        _ => Err("execution reverted".into()),
    });
    let id = identify(&r, &scan_unverified(), BASE, UNKNOWN).expect("identify");
    assert_eq!(id.erc165, "unknown");
}

#[test]
fn a_book_contract_without_verified_source_uses_its_bundled_abi() {
    let weth = book_addr("WrappedSALT");
    let r = rpc(token_answers);
    let id = identify(&r, &scan_unverified(), BASE, &weth).expect("identify");
    assert_eq!(id.book_names, vec!["WrappedSALT".to_string()]);
    assert_eq!(id.abi_source, "book");
    assert!(id.abi_contract.as_deref().is_some_and(|c| c.ends_with(":WrappedSALT")));
}

#[test]
fn identifying_an_account_without_code_says_so() {
    let r = rpc(|m, _| match m {
        "eth_getCode" => Ok(json!("0x")),
        other => Err(format!("unexpected {other}")),
    });
    let id = identify(&r, &scan_unverified(), BASE, UNKNOWN).expect("identify");
    assert!(!id.is_contract);
    assert_eq!(id.abi_source, "none");
    assert_eq!(id.erc20.verdict, "no");
}

// ---- ABI resolution ----

fn plain_contract(m: &str, _p: &Value) -> Answer {
    match m {
        "eth_getCode" => Ok(json!("0x60806040")),
        "eth_getStorageAt" => Ok(zero_word()),
        other => Err(format!("unexpected {other}")),
    }
}

#[test]
fn a_verified_abi_comes_first() {
    let weth = book_addr("WrappedSALT");
    let s = scan(
        200,
        json!({"status": "verified", "verified": true, "contractName": "Verified", "abi": [{"type": "function", "name": "v", "inputs": [], "outputs": [], "stateMutability": "view"}]}),
    );
    let out = resolve_abi(&rpc(plain_contract), &s, BASE, &weth).expect("resolve");
    assert_eq!(out.source, "verified");
    assert_eq!(out.contract_name.as_deref(), Some("Verified"));
}

#[test]
fn an_unverified_book_contract_resolves_to_its_bundled_abi() {
    let weth = book_addr("WrappedSALT");
    let out = resolve_abi(&rpc(plain_contract), &scan_unverified(), BASE, &weth).expect("resolve");
    assert_eq!(out.source, "book");
    assert_eq!(out.contract_name.as_deref(), Some("WrappedSALT"));
    let abi = out.abi.expect("abi");
    assert!(abi.as_array().is_some_and(|a| a.iter().any(|i| i["name"] == "deposit")));
    assert!(out.note.contains("bundled system ABI"));
}

#[test]
fn a_proxy_resolves_to_its_implementations_bundled_abi() {
    let weth = book_addr("WrappedSALT");
    let w = weth.clone();
    let r = rpc(move |m, p| match m {
        "eth_getCode" => Ok(json!("0x60806040")),
        "eth_getStorageAt" if p[1] == json!(EIP1967_IMPLEMENTATION_SLOT) => Ok(json!(word_hex(&w))),
        "eth_getStorageAt" => Ok(zero_word()),
        other => Err(format!("unexpected {other}")),
    });
    let out = resolve_abi(&r, &scan_unverified(), BASE, UNKNOWN).expect("resolve");
    assert_eq!(out.source, "book");
    assert_eq!(out.abi_address.as_deref(), Some(weth.as_str()));
    assert_eq!(out.proxy.map(|p| p.kind).as_deref(), Some("eip-1967"));
}

#[test]
fn a_partial_match_is_used_only_after_the_book_and_is_labelled() {
    let s = scan(
        200,
        json!({"status": "partial-match", "verified": false, "contractName": "P", "abi": [], "source": "contract P {}"}),
    );
    let out = resolve_abi(&rpc(plain_contract), &s, BASE, UNKNOWN).expect("resolve");
    assert_eq!(out.source, "partial-match");
    let weth = book_addr("WrappedSALT");
    let out = resolve_abi(&rpc(plain_contract), &s, BASE, &weth).expect("resolve");
    assert_eq!(out.source, "book");
}

#[test]
fn no_abi_is_reported_as_none_with_the_explorer_failure_named() {
    let out = resolve_abi(&rpc(plain_contract), &scan_down(), BASE, UNKNOWN).expect("resolve");
    assert_eq!(out.source, "none");
    assert_eq!(out.abi, None);
    assert!(out.note.contains("could not be read"));
    let out = resolve_abi(&rpc(plain_contract), &scan_unverified(), BASE, UNKNOWN).expect("resolve");
    assert_eq!(out.source, "none");
    assert!(out.note.contains("signature subset"));
}

// ---- chain_tx_fetch ----

#[test]
fn a_transaction_and_its_receipt_are_fetched_with_book_names() {
    let weth = book_addr("WrappedSALT");
    let w = weth.clone();
    let hash = format!("0x{}", "ab".repeat(32));
    let r = rpc(move |m, _| match m {
        "eth_getTransactionByHash" => Ok(json!({
            "from": OWNER, "to": w.to_ascii_uppercase().replacen("0X", "0x", 1), "value": "0xde0b6b3a7640000",
            "input": "0xd0e30db0", "nonce": "0x7", "blockNumber": "0x10"
        })),
        "eth_getTransactionReceipt" => Ok(json!({
            "status": "0x1", "gasUsed": "0x5208", "contractAddress": null,
            "logs": [{"address": w, "topics": ["0xE1FFFCC4923D04B559F4D29A8BFC6CDA04EB5B0D3C460751C2402C5C5CC9109C", word_hex(OWNER)], "data": uint_word(1), "logIndex": "0x0"}]
        })),
        other => Err(format!("unexpected {other}")),
    });
    let tx = fetch_tx(&r, &hash).expect("tx");
    assert!(tx.found);
    assert_eq!(tx.status, "success");
    assert_eq!(tx.to.as_deref(), Some(weth.as_str()));
    assert_eq!(tx.value_wei.as_deref(), Some("1000000000000000000"));
    assert_eq!(tx.nonce.as_deref(), Some("7"));
    assert_eq!(tx.gas_used.as_deref(), Some("21000"));
    assert_eq!(tx.logs.len(), 1);
    assert!(tx.logs[0].topics[0].starts_with("0xe1fffcc4"));
    assert_eq!(tx.names[&weth], vec!["WrappedSALT".to_string()]);
    assert!(!tx.names.contains_key(OWNER));
}

#[test]
fn a_pending_or_unknown_transaction_is_reported_as_such() {
    let hash = format!("0x{}", "cd".repeat(32));
    let r = rpc(|m, _| match m {
        "eth_getTransactionByHash" => Ok(json!({"from": OWNER, "to": null, "value": "0x0", "input": "0x6080"})),
        "eth_getTransactionReceipt" => Ok(Value::Null),
        other => Err(format!("unexpected {other}")),
    });
    let tx = fetch_tx(&r, &hash).expect("tx");
    assert!(tx.found);
    assert_eq!(tx.status, "pending");
    assert_eq!(tx.to, None, "a creation has no recipient");
    let r = rpc(|_, _| Ok(Value::Null));
    let tx = fetch_tx(&r, &hash).expect("tx");
    assert!(!tx.found);
    assert!(fetch_tx(&r, "0x1234").is_err());
}
