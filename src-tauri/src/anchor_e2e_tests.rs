//! HUP-S7.3 + S7.5: the anvil rehearsal of the whole anchor path (gate g4-anchor, local evidence).
//!
//! Real pieces only, end to end, against a local anvil on chain id 40204:
//!
//! - `AnchorRegistry` and `BenchmarkRegistry` compiled from the citrate-chain source and deployed
//!   on the anvil (no live chain is touched);
//! - the real Hermes sidecar binary, spawned by core's own `HermesManager` over a Hermes folder
//!   whose decision records and metering log were written by the runtime's own writers
//!   (citrate-agent-runtime `agent-sidecar/tests/anchor_e2e_seed.rs`), including the HIC records
//!   the anchor exists for (HUP-S2.6): core's own `HicOutbox` records (written first by
//!   [`core_hic_outbox_for_the_seed`] and handed to the seed in the `/records/core` wire form),
//!   a `shell_run` command decision and a ceremony-bridge chain-effect decision;
//! - core's HIC outbox exported live to the running sidecar (`HicOutbox::export`), landing in the
//!   open day of the same records directory;
//! - core's nightly tick raising the approval card, the anchor ceremony signing with a fresh anchor
//!   key (funded on the anvil only) behind an unlocked custody vault, the in-flight record written
//!   before the send, the settle step writing the confirmation back to the sidecar;
//! - core's own proof check (`anchor_proof::verdict` + `check_chain`) for every record of the day;
//! - the opt-in benchmark payload rechecked by core (`benchmark_share::validate`) and the exact
//!   transaction a wallet card would carry, broadcast by an anvil dev account standing in for the
//!   member's approved signature, then read back from the registry.
//!
//! Opt-in (`--ignored`); run it with `scripts/anvil-anchor-e2e.sh`, which builds the contracts and
//! the sidecar, seeds the folder and sets the environment below. Nothing here can reach 40204: the
//! RPC is the test's own anvil, refused unless it answers `anvil` to `web3_clientVersion`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use citrate_core_kit::custody::{CustodyError, CustodyVault, Keyring};
use serde_json::{json, Value};

use super::*;
use crate::ceremony::anchor::{
    ensure_anchor_key, receipt_confirms, AnchorCeremony, AnchorGuards, AnchorTxConfig,
};
use crate::chain_agent::{
    after_broadcast, days_to_anchor, nightly_tick_with, AnchorGate, AnchorPort, InFlightAnchors,
};
use crate::rpc::{HttpTransport, RpcClient};

#[derive(Default)]
struct MemKeyring(Mutex<std::collections::HashMap<String, Vec<u8>>>);
impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), CustodyError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), CustodyError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account);
        Ok(())
    }
}

struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct StopOnDrop<'a>(&'a crate::hermes::HermesManager);
impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

fn env_path(k: &str) -> PathBuf {
    PathBuf::from(
        std::env::var(k)
            .unwrap_or_else(|_| panic!("set {k} (run scripts/anvil-anchor-e2e.sh, which sets it)")),
    )
}

fn env_port(k: &str, default: u16) -> u16 {
    std::env::var(k)
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(default)
}

// anvil's well-known unlocked dev accounts (public test keys, no value anywhere else).
const DEPLOYER: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
const MEMBER: &str = "0x70997970c51812dc3a010c7d01b50e0d17dc79c8";
const STRANGER: &str = "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc";

struct Chain {
    rpc: RpcClient<HttpTransport>,
}

impl Chain {
    fn raw(&self, method: &str, params: Value) -> Value {
        let body = self.rpc.build_request(method, params);
        crate::rpc::RpcTransport::call(self.rpc.transport(), body).expect("transport")
    }

    /// Send from an unlocked anvil dev account and wait for the receipt.
    fn send(&self, tx: Value) -> Value {
        let r = self.raw("eth_sendTransaction", json!([tx]));
        let h = r["result"]
            .as_str()
            .unwrap_or_else(|| panic!("send failed: {r}"))
            .to_string();
        for _ in 0..100 {
            let rc = self.raw("eth_getTransactionReceipt", json!([h]));
            if !rc["result"].is_null() {
                return rc["result"].clone();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("no receipt for {h}");
    }

    fn deploy(&self, artifacts: &Path, name: &str) -> String {
        let p = artifacts.join(format!("{name}.sol/{name}.json"));
        let j: Value =
            serde_json::from_str(&std::fs::read_to_string(&p).expect("artifact")).expect("json");
        let code = j["bytecode"]["object"].as_str().expect("bytecode");
        let rc = self.send(json!({"from": DEPLOYER, "data": code, "gas": "0x1c9c380"}));
        assert_eq!(rc["status"], "0x1", "{name} deploy: {rc}");
        rc["contractAddress"].as_str().expect("address").to_string()
    }

    fn eth_call(&self, to: &str, data: &[u8]) -> Value {
        self.raw(
            "eth_call",
            json!([{"to": to, "data": format!("0x{}", hex::encode(data))}, "latest"]),
        )
    }
}

fn word(n: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&n.to_be_bytes());
    w
}

fn addr_word(a: &str) -> [u8; 32] {
    let mut w = [0u8; 32];
    hex::decode_to_slice(a.trim_start_matches("0x"), &mut w[12..]).expect("address");
    w
}

fn sel(sig: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

fn result_bytes(v: &Value) -> Vec<u8> {
    hex::decode(
        v["result"]
            .as_str()
            .unwrap_or_else(|| panic!("no result: {v}"))
            .trim_start_matches("0x"),
    )
    .expect("hex")
}

/// The kinds of HIC record the closed day must carry and every one of them must be proven.
const HIC_KINDS: &[&str] = &[
    "grant.folder_added",
    "escalation.spend",
    "ceremony.approval",
    "agent.tool_approval",
    "shell.run",
    "ceremony.capsule_effect",
];

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Step 0 of the rehearsal: core's real HIC outbox, with the events core decides (a folder grant,
/// an escalation inside the budget, the member's answers on a ceremony card and a tool card),
/// written in the `/records/core` wire form for the runtime seed to record on the closed day.
#[test]
#[ignore = "step 0 of scripts/anvil-anchor-e2e.sh; needs CITRATE_ANCHOR_E2E_CORE_RECORDS and CITRATE_ANCHOR_E2E_WORK"]
fn core_hic_outbox_for_the_seed() {
    use crate::hic_records::{card_event, escalation_spent, grant_added, wire, HicOutbox};
    let out = env_path("CITRATE_ANCHOR_E2E_CORE_RECORDS");
    let work = env_path("CITRATE_ANCHOR_E2E_WORK");
    let ob = HicOutbox::new(work.join("hic-outbox-seed"));
    let at = now_ms();
    ob.append(grant_added("/work/hello-mint", &["g-1".to_string()]), at)
        .expect("grant");
    let spend = crate::escalation::SpendRecord {
        escalation_id: "esc-1".into(),
        endpoint_id: "ep-1".into(),
        destination: "https://api.example".into(),
        quoted_micros: 2_000,
        charged_micros: 1_500,
        mode: crate::escalation::Mode::Budget,
        outcome: "answered".into(),
        usage_reported: true,
        exceeded_quote: false,
        at_ms: at,
    };
    ob.append(escalation_spent(&spend), at + 1).expect("spend");
    ob.append(
        card_event(
            "ceremony.approval",
            "approved",
            "send 1 SALT to 0x00000000000000000000000000000000000000b2",
            "",
        )
        .expect("card"),
        at + 2,
    )
    .expect("ceremony card");
    ob.append(
        card_event("agent.tool_approval", "denied", "write_file notes.md", "").expect("card"),
        at + 3,
    )
    .expect("tool card");
    let records = ob.records().expect("outbox chain verifies");
    assert_eq!(records.len(), 4);
    let wire: Vec<Value> = records.iter().map(wire).collect();
    std::fs::write(&out, serde_json::to_vec(&wire).expect("encode")).expect("write");
    println!("E2E_CORE_OUTBOX {}", json!({ "records": wire.len() }));
}

#[test]
#[ignore = "needs anvil, the forge artifacts, the sidecar binary and a seeded Hermes folder: run scripts/anvil-anchor-e2e.sh"]
fn anvil_anchor_and_benchmark_end_to_end() {
    let artifacts = env_path("CITRATE_ANCHOR_E2E_ARTIFACTS");
    let sidecar = env_path("CITRATE_ANCHOR_E2E_SIDECAR");
    let hermes_dir = env_path("CITRATE_ANCHOR_E2E_HERMES_DIR");
    let work = env_path("CITRATE_ANCHOR_E2E_WORK");
    let label = std::env::var("CITRATE_ANCHOR_E2E_LABEL").unwrap_or_else(|_| "registry".into());
    let anvil_port = env_port("CITRATE_ANCHOR_E2E_ANVIL_PORT", 18_745);
    let control_port = env_port("CITRATE_ANCHOR_E2E_CONTROL_PORT", 19_745);
    std::fs::create_dir_all(&work).expect("work dir");

    // 1. A private anvil on chain id 40204 (the anchor signer only signs for 40204).
    let url = format!("http://127.0.0.1:{anvil_port}");
    let mut anvil = KillOnDrop(
        Command::new("anvil")
            .args([
                "--port",
                &anvil_port.to_string(),
                "--chain-id",
                "40204",
                "--silent",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("anvil on PATH"),
    );
    let chain = Chain {
        rpc: RpcClient::with_transport(HttpTransport::new(url.clone())),
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while chain.rpc.block_number().is_err() {
        assert!(Instant::now() < deadline, "anvil did not come up on {url}");
        std::thread::sleep(Duration::from_millis(100));
    }
    // Our own anvil must still be running: if it could not bind, whatever answers is not ours.
    assert!(
        matches!(anvil.0.try_wait(), Ok(None)),
        "anvil exited (port {anvil_port} taken?); refusing to use another endpoint"
    );
    let client = chain.raw("web3_clientVersion", json!([]));
    assert!(
        client["result"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains("anvil"),
        "refusing an endpoint that is not this test's anvil: {client}"
    );
    assert_eq!(
        chain.raw("eth_chainId", json!([]))["result"],
        "0x9d0c",
        "chain id must be 40204"
    );

    // 2. The registries, from the chain source.
    let registry = chain.deploy(&artifacts, "AnchorRegistry");
    let bench_registry = chain.deploy(&artifacts, "BenchmarkRegistry");
    let next_version = chain.eth_call(&registry, &get_anchor_by_calldata(&[0u8; 20], &[0u8; 32]))
        ["error"]["data"]
        .as_str()
        .is_some_and(|d| d.starts_with(&format!("0x{}", hex::encode(anchor_not_found_selector()))));

    // 3. The real sidecar, started the way the app starts it.
    let m = crate::hermes::HermesManager::new(
        sidecar.clone(),
        work.join("hermes.token"),
        work.join("hermes.crash"),
    )
    .with_chain_data_dir(hermes_dir.clone())
    .with_control_addr(&format!("127.0.0.1:{control_port}"))
    .with_health_interval(Duration::from_millis(200));
    m.start().expect("sidecar starts");
    let _stop = StopOnDrop(&m);
    let deadline = Instant::now() + Duration::from_secs(30);
    while m.anchor_status().is_err() {
        assert!(Instant::now() < deadline, "the sidecar did not answer");
        std::thread::sleep(Duration::from_millis(200));
    }

    // 4. The sidecar sees one closed day to anchor.
    let status = m.anchor_status().expect("status");
    let days = days_to_anchor(&status);
    assert_eq!(days.len(), 1, "one closed day: {status}");
    let day = days[0];
    let date = crate::ceremony::anchor::date_of_day(day).expect("date");

    // 5. Core's nightly tick raises exactly one card for it.
    let ceremony = AnchorCeremony::new();
    let port: &dyn AnchorPort = &m;
    let tick = nightly_tick_with(
        port,
        &ceremony,
        AnchorGate::Ready,
        Some(&registry),
        &BTreeSet::new(),
    )
    .expect("tick");
    assert_eq!(tick.raised.len(), 1, "{:?}", tick.skipped);
    let card = tick.raised[0].clone();
    assert_eq!(card.day, day);
    assert_eq!(
        card.registry.to_ascii_lowercase(),
        registry.to_ascii_lowercase()
    );

    // 6. A fresh anchor key, funded on this anvil only.
    let keyring = MemKeyring::default();
    let anchor_key = ensure_anchor_key(&keyring).expect("anchor key");
    let rc = chain.send(json!({"from": DEPLOYER, "to": anchor_key, "value": "0xde0b6b3a7640000"}));
    assert_eq!(rc["status"], "0x1");

    // 7. A locked vault signs nothing and keeps the card.
    let vault = CustodyVault::new(Box::new(MemKeyring::default()), work.join("custody.enc"), 0);
    let mut pw = b"anvil-rehearsal-only".to_vec();
    vault.init(&mut pw.clone()).expect("vault init");
    let held = InFlightAnchors::load(Some(work.join(crate::chain_agent::IN_FLIGHT_FILE)));
    let record = |r: &crate::ceremony::anchor::AnchorReceipt| held.record(r);
    vault.lock();
    let locked = ceremony.approve_and_broadcast(
        &keyring,
        &chain.rpc,
        &card.id,
        &registry,
        AnchorTxConfig::with_placeholder_caps(50, Duration::from_millis(100)),
        AnchorGuards {
            vault: &vault,
            before_send: &record,
        },
    );
    assert!(locked.is_err(), "a locked vault must not sign");
    assert_eq!(ceremony.pending().len(), 1, "the card is kept");

    // 8. Unlocked: sign, record in flight, send, settle.
    vault.unlock(&mut pw).expect("unlock");
    let receipt = ceremony
        .approve_and_broadcast(
            &keyring,
            &chain.rpc,
            &card.id,
            &registry,
            AnchorTxConfig::with_placeholder_caps(50, Duration::from_millis(100)),
            AnchorGuards {
                vault: &vault,
                before_send: &record,
            },
        )
        .expect("anchored");
    assert!(receipt_confirms(&receipt), "{receipt:?}");
    let (anchored, line) = after_broadcast(Ok(port), &receipt, &held);
    assert!(anchored, "{line}");
    assert!(held.days().is_empty(), "nothing left in flight");
    let block = receipt.block_number.expect("block");
    let anchor_gas_used = chain.raw("eth_getTransactionReceipt", json!([receipt.tx_hash]))
        ["result"]["gasUsed"]
        .as_str()
        .and_then(|g| u64::from_str_radix(g.trim_start_matches("0x"), 16).ok())
        .expect("gasUsed");
    assert!(
        anchor_gas_used <= crate::ceremony::anchor::PLACEHOLDER_MAX_GAS_LIMIT,
        "{label}: the anchor used {anchor_gas_used} gas"
    );

    // 9. The sidecar recorded it; the next tick raises nothing for the day.
    let status = m.anchor_status().expect("status");
    assert_eq!(status["anchored"][0]["day"], day, "{status}");
    assert_eq!(status["anchored"][0]["txHash"], receipt.tx_hash);
    let again = nightly_tick_with(
        port,
        &ceremony,
        AnchorGate::Ready,
        Some(&registry),
        &held.days(),
    )
    .expect("tick");
    assert!(
        again.raised.is_empty(),
        "no second card: {:?}",
        again.raised
    );

    // 10. Every record of the day is proven by core itself, against the chain.
    let records = m.anchor_records(None, Some(20)).expect("records");
    let list = records["records"].as_array().expect("list").clone();
    let closed: Vec<u64> = list
        .iter()
        .filter(|r| r["day"] == day)
        .map(|r| r["seq"].as_u64().expect("seq"))
        .collect();
    // Five plain records, then the HIC records (core's four as seven decisions and outcomes, the
    // shell_run decision and outcome, the declined chain effect): fifteen, a tree whose size is
    // not a power of two.
    assert_eq!(closed.len(), 15, "{records}");
    let closed_kinds: BTreeSet<String> = list
        .iter()
        .filter(|r| r["day"] == day)
        .filter_map(|r| r["record"]["entry"]["decision"]["kind"].as_str())
        .map(str::to_string)
        .collect();
    for k in HIC_KINDS {
        assert!(
            closed_kinds.contains(*k),
            "{k} missing from {closed_kinds:?}"
        );
    }
    let mut proven = 0;
    for seq in &closed {
        let p = m.anchor_proof(*seq).expect("proof");
        let v = verdict(&p, |c| {
            check_chain(&chain.rpc, Some(&registry), c, Some(&anchor_key))
        });
        assert!(v.proven, "seq {seq}: {}", v.line);
        match &v.chain {
            ChainCheck::Anchored { anchor, .. } => assert_eq!(anchor.block_number, block),
            c => panic!("seq {seq}: {c:?}"),
        }
        proven += 1;
    }
    assert!(list
        .iter()
        .filter(|r| r["day"] == day)
        .all(|r| r["anchored"] == true));
    let hic_proven = list
        .iter()
        .filter(|r| r["day"] == day)
        .filter(|r| {
            r["record"]["entry"]["decision"]["kind"]
                .as_str()
                .is_some_and(|k| HIC_KINDS.contains(&k))
        })
        .count();

    // 10b. Core's HIC outbox, exported live to the running sidecar: the records land in the
    //      same records directory, on the open day (batched and anchored with it once it closes).
    let live = crate::hic_records::HicOutbox::new(work.join("hic-outbox-live"));
    live.append(
        crate::hic_records::card_event("ceremony.approval", "denied", "deploy HelloMint", "")
            .expect("card"),
        now_ms(),
    )
    .expect("append");
    assert_eq!(
        live.export(&m).expect("export"),
        crate::hic_records::ExportResult::Exported(1)
    );
    let after = m.anchor_records(None, Some(5)).expect("records");
    let newest = &after["records"][0];
    assert_ne!(
        newest["day"], day,
        "the live record is on the open day: {newest}"
    );
    assert_eq!(
        newest["record"]["entry"]["decision"]["kind"],
        "ceremony.approval"
    );
    assert_eq!(newest["record"]["entry"]["decision"]["decision"], "denied");
    assert_eq!(newest["batched"], false);
    assert_eq!(newest["anchored"], false);
    let live_seq = newest["seq"].as_u64().expect("seq");

    // 11. The open day's record has no proof yet; a tampered proof is not proven.
    let open = list
        .iter()
        .find(|r| r["day"] != day)
        .and_then(|r| r["seq"].as_u64())
        .expect("open-day record");
    assert!(m.anchor_proof(open).is_err(), "the open day is not batched");
    let mut bad = m.anchor_proof(closed[0]).expect("proof");
    bad.proof.record_hash = hex::encode([0x42u8; 32]);
    let v = verdict(&bad, |c| {
        check_chain(&chain.rpc, Some(&registry), c, Some(&anchor_key))
    });
    assert!(!v.proven, "{}", v.line);

    // 12. Another account's anchors: never reported as this member's.
    let mut stranger_root = [0u8; 32];
    stranger_root[0] = 0x5a;
    let mut cd = sel("anchor(uint8,bytes32)").to_vec();
    cd.extend_from_slice(&word(2));
    cd.extend_from_slice(&stranger_root);
    let rc = chain.send(json!({"from": STRANGER, "to": registry, "data": format!("0x{}", hex::encode(&cd)), "gas": "0x7a120"}));
    assert_eq!(rc["status"], "0x1");
    match check_chain(
        &chain.rpc,
        Some(&registry),
        &stranger_root,
        Some(&anchor_key),
    ) {
        ChainCheck::Anchored { by_you, .. } => assert!(!by_you),
        c => panic!("{c:?}"),
    }
    // The same day value sent again by a stranger: the deployed version refuses it
    // (AlreadyAnchored); the next version records it under the stranger's own address, and the
    // member's proof still reads the member's own record.
    let day_value = hex32(
        m.anchor_proof(closed[0])
            .expect("proof")
            .commitment
            .as_str(),
    )
    .expect("commitment");
    let mut cd = sel("anchor(uint8,bytes32)").to_vec();
    cd.extend_from_slice(&word(2));
    cd.extend_from_slice(&day_value);
    let rc = chain.send(json!({"from": STRANGER, "to": registry, "data": format!("0x{}", hex::encode(&cd)), "gas": "0x7a120"}));
    let stranger_same_root_status = rc["status"].as_str().unwrap_or("").to_string();
    assert_eq!(
        stranger_same_root_status,
        if next_version { "0x1" } else { "0x0" },
        "{label}: {rc}"
    );
    let p = m.anchor_proof(closed[0]).expect("proof");
    let v = verdict(&p, |c| {
        check_chain(&chain.rpc, Some(&registry), c, Some(&anchor_key))
    });
    assert!(v.proven, "{label}: {}", v.line);

    // 13. Benchmark sharing: the sidecar's payload, rechecked by core, sent as the wallet card's
    //     exact transaction from the member's account, and read back from the registry.
    let agent_id: u128 = 7;
    let raw = m
        .metering_benchmark(&date, agent_id, &bench_registry)
        .expect("payload");
    let payload: crate::benchmark_share::Payload =
        serde_json::from_value(raw).expect("payload shape");
    let calls = crate::benchmark_share::validate(&payload, &bench_registry, agent_id, &date)
        .expect("core accepts the payload");
    assert!(!calls.is_empty());
    let capsule = crate::benchmark_share::hermes_capsule_id();
    for c in &calls {
        let gas = chain
            .rpc
            .estimate_gas(json!({"from": MEMBER, "to": bench_registry, "data": format!("0x{}", hex::encode(&c.data))}))
            .expect("estimate");
        let tx: Value = serde_json::from_str(&crate::agent_sbt::mint_tx_json(
            MEMBER,
            &bench_registry,
            &c.data,
            crate::agent_sbt::with_gas_margin(gas),
        ))
        .expect("tx json");
        assert_eq!(tx["chainId"], "0x9d0c");
        let rc = chain.send(json!({"from": tx["from"], "to": tx["to"], "data": tx["data"], "gas": tx["gas"], "value": tx["value"]}));
        assert_eq!(rc["status"], "0x1", "{}: {rc}", c.metric);
    }
    for c in &calls {
        use sha3::{Digest, Keccak256};
        let name: [u8; 32] = Keccak256::digest(c.metric.as_bytes()).into();
        let mut q = sel("getMetric(address,uint256,bytes32,bytes32,uint256,uint256)").to_vec();
        q.extend_from_slice(&addr_word(MEMBER));
        q.extend_from_slice(&word(agent_id));
        q.extend_from_slice(&capsule);
        q.extend_from_slice(&name);
        q.extend_from_slice(&word(0));
        q.extend_from_slice(&word(10));
        let ret = result_bytes(&chain.eth_call(&bench_registry, &q));
        // (offset, length, then one 6-word BenchmarkRecord per entry)
        assert_eq!(ret.len(), 32 * 2 + 32 * 6, "{}: one record", c.metric);
        let rec = &ret[64..];
        assert_eq!(rec[31] as u128, agent_id);
        assert_eq!(&rec[32..64], &capsule);
        assert_eq!(&rec[64..96], &name);
        let mut v = [0u8; 16];
        v.copy_from_slice(&rec[96 + 16..128]);
        assert_eq!(u128::from_be_bytes(v).to_string(), c.value, "{}", c.metric);
        assert_eq!(&rec[160 + 12..192], &addr_word(MEMBER)[12..], "committer");
    }

    println!(
        "E2E_RESULT {}",
        json!({
            "label": label,
            "nextRegistryVersion": next_version,
            "chainId": 40204,
            "anchorRegistry": registry,
            "benchmarkRegistry": bench_registry,
            "day": day,
            "date": date,
            "anchorKey": anchor_key,
            "anchorTx": receipt.tx_hash,
            "anchorBlock": block,
            "anchorGasUsed": anchor_gas_used,
            "recordsProven": proven,
            "hicRecordsProven": hic_proven,
            "liveHicRecordOpenDay": live_seq,
            "openDayRecord": open,
            "strangerSameRootStatus": stranger_same_root_status,
            "benchmarkCalls": calls.len(),
        })
    );
}
