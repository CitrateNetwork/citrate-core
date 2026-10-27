//! G7 (0.5.2 RC): **citrate-core's money paths, end to end, on the genesis contract set.**
//!
//! `scripts/e2e/moneypath-anvil.sh` deploys citrate-chain's genesis contract set (#320 → #324 →
//! #331: governance, dormant ledgers, Track M membership) on a throwaway anvil with chain id 40204
//! and runs [`anvil_moneypath_genesis_set`] against it. The test drives core's own code, never a
//! re-implementation of it:
//!
//! - the grant-status read (`grant_status::read_grant_status`) and the Activate-validator read
//!   (`validator_activation::read_activation_status`), the read model the UI's truth labels come
//!   from (`src/onboarding/activateValidator.ts`, checked against these goldens by
//!   `src/onboarding/moneypathGoldens.test.ts`);
//! - the activation intent (`node::bond_activation_intent`, the body of `node_register_validator`),
//!   signed with the proposer key through `validator::sign_registration_from_data_dir`;
//! - the SignatureCeremony (`request` + `approve_and_broadcast`) signing EIP-155 transactions from
//!   a vault holding anvil's public test mnemonic.
//!
//! D-9 (#519): the liquid staking pool is retired and not in the genesis set, so there is no pool
//! flow and no pending-withdrawals read here.
//!
//! What only the test does, because core has no code for it: the treasury's side (mint the
//! membership SBT, place the grant, attest KYC, record the release), mining blocks and moving the
//! clock, and the member's `MemberBond.requestRelease` / `withdrawToMember` calldata (core has no
//! builder for the Track M exit; that is a listed gap). Those member transactions still go through
//! the ceremony, so they are signed and sent the way the app would send them.
//!
//! Each step records the events its transactions emitted, the chain state read directly, and
//! core's read model, normalized (addresses become role names, hashes and gas become
//! placeholders, timestamps become offsets) and compares it with
//! `src-tauri/tests/fixtures/moneypath/<flow>.golden.json`. Any drift fails. With
//! `CITRATE_E2E_MP_UPDATE=1` the goldens are rewritten instead (review the diff).
//!
//! Without `CITRATE_E2E_MP_RPC` the e2e is skipped; the golden-shape checks below always run.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use sha3::{Digest as _, Keccak256};

use crate::ceremony::{BroadcastConfig, BroadcastResult, SignatureCeremony, SignatureIntent};
use crate::custody::{CustodyError, CustodyVault, Keyring};
use crate::rpc::{HttpTransport, RpcClient, RpcTransport};

/// Anvil's public default mnemonic. The member wallet is its account 0 (`m/44'/60'/0'/0/0`).
const MNEMONIC: &str = "test test test test test test test test test test test junk";
const CHAIN_ID: u64 = 40204;
const SALT: u128 = 1_000_000_000_000_000_000;
const GRANT_WEI: u128 = 32_000 * SALT;
/// The test node's ed25519 proposer seed (a throwaway test key, written to a temp data dir).
const PROPOSER_SEED: [u8; 32] = [0x6d; 32];
/// The node's ed25519-verify precompile; the script installs a whitelist stand-in there.
const ED25519_PRECOMPILE: &str = "0x0000000000000000000000000000000000000120";
/// The membership term the treasury mints (fixed, so the goldens do not move with the clock).
const TERM_START: u64 = 1;
const TERM_END: u64 = 4_102_444_800;
const LOCK_SECONDS: u64 = 364 * 24 * 3600;
const EPOCH_BLOCKS: u64 = 1000;
const EXIT_LOCK_EPOCHS: u64 = 6;

/// The flows, in the order the e2e runs them on one chain. One golden file each.
const FLOWS: [&str; 2] = ["membership-activation", "bond-exit"];

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/moneypath")
}

/// The event signatures the flows may emit, with parameter names (the golden shows them).
/// A log whose topic0 is not here is recorded as `UNKNOWN`, which no golden expects.
const EVENTS: &[&str] = &[
    "Transfer(address indexed from,address indexed to,uint256 indexed tokenId)",
    "MemberMinted(uint256 indexed tokenId,address indexed member,bytes32 indexed subHash,uint64 termStart,uint64 termEnd)",
    "KycVerifiedSet(uint256 indexed tokenId,bool verified)",
    "Initialized(uint64 version)",
    "BondInitialized(address indexed member,uint256 indexed memberTokenId,uint256 principal,uint64 unlockBlock,uint64 unlockTimestamp)",
    "Granted(uint256 indexed grantId,address indexed member,address indexed bond,uint256 principal,uint64 unlockBlock,uint64 unlockTimestamp)",
    "ValidatorRegistered(bytes32 indexed pubkey,address indexed staker,uint256 stake,uint64 activationEpoch)",
    "BondActivated(address indexed member,bytes32 indexed pubkey,uint256 principal)",
    "ReleaseRequested(address indexed member,bytes32 indexed pubkey,uint256 amount)",
    "UnbondInitiated(bytes32 indexed pubkey,uint256 amount,uint256 remaining,uint64 unlockEpoch)",
    "GrantReleased(uint256 indexed grantId,address indexed member)",
    "Withdrawn(bytes32 indexed pubkey,address indexed staker,uint256 amount)",
    "PrincipalWithdrawn(address indexed member,uint256 amount)",
];

// ------------------------------------------------------------------------ small ABI helpers

fn keccak(b: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(b);
    h.finalize().into()
}

fn selector(sig: &str) -> Vec<u8> {
    keccak(sig.as_bytes())[..4].to_vec()
}

fn word_u(n: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&n.to_be_bytes());
    w
}

fn word_addr(a: &str) -> Result<[u8; 32], String> {
    let b = hex::decode(a.trim_start_matches("0x")).map_err(|e| format!("address {a}: {e}"))?;
    if b.len() != 20 {
        return Err(format!("address {a} is not 20 bytes"));
    }
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(&b);
    Ok(w)
}

fn calldata(sig: &str, words: &[[u8; 32]]) -> String {
    let mut out = selector(sig);
    for w in words {
        out.extend_from_slice(w);
    }
    format!("0x{}", hex::encode(out))
}

fn hex_u64(v: &Value) -> Option<u64> {
    u64::from_str_radix(v.as_str()?.trim_start_matches("0x"), 16).ok()
}

fn hex_u128(v: &Value) -> Option<u128> {
    u128::from_str_radix(v.as_str()?.trim_start_matches("0x"), 16).ok()
}

/// A word as u128, `None` beyond (never truncated).
fn word_to_u128(w: &[u8]) -> Option<u128> {
    if w.len() != 32 || w[..16].iter().any(|b| *b != 0) {
        return None;
    }
    let mut lo = [0u8; 16];
    lo.copy_from_slice(&w[16..]);
    Some(u128::from_be_bytes(lo))
}

const GRANT_STATES: &[&str] = &["None", "Attributed", "Lapsed", "Released", "Reclaimed"];
const REGISTRY_STATUSES: &[&str] = &["None", "Active", "Exiting", "Slashed"];

/// A Solidity enum value's name, or `?` for a value the enum does not have.
fn enum_name(names: &[&'static str], v: Option<u128>) -> &'static str {
    v.and_then(|i| usize::try_from(i).ok())
        .and_then(|i| names.get(i).copied())
        .unwrap_or("?")
}

// ------------------------------------------------------------------------ the test keyring

#[derive(Default)]
struct MemKeyring(std::sync::Mutex<BTreeMap<String, Vec<u8>>>);

impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| CustodyError::Io("poisoned".into()))?
            .get(account)
            .cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), CustodyError> {
        self.0
            .lock()
            .map_err(|_| CustodyError::Io("poisoned".into()))?
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), CustodyError> {
        self.0
            .lock()
            .map_err(|_| CustodyError::Io("poisoned".into()))?
            .remove(account);
        Ok(())
    }
}

// ------------------------------------------------------------------------ events

struct EventSpec {
    name: String,
    /// (name, type, indexed)
    params: Vec<(String, String, bool)>,
}

fn parse_event(sig: &str) -> Result<([u8; 32], EventSpec), String> {
    let open = sig.find('(').ok_or("event without (")?;
    let name = sig[..open].to_string();
    let inner = sig[open + 1..].trim_end_matches(')');
    let mut params = Vec::new();
    let mut types = Vec::new();
    for p in inner.split(',').filter(|p| !p.is_empty()) {
        let parts: Vec<&str> = p.split_whitespace().collect();
        let (ty, indexed, pname) = match parts.as_slice() {
            [ty, "indexed", n] => (*ty, true, *n),
            [ty, n] => (*ty, false, *n),
            _ => return Err(format!("bad event param {p:?} in {sig}")),
        };
        types.push(ty.to_string());
        params.push((pname.to_string(), ty.to_string(), indexed));
    }
    let canonical = format!("{name}({})", types.join(","));
    Ok((keccak(canonical.as_bytes()), EventSpec { name, params }))
}

// ------------------------------------------------------------------------ the world

struct E2eEnv {
    rpc: String,
    book: Value,
    update: bool,
}

impl E2eEnv {
    fn from_env() -> Result<Option<E2eEnv>, String> {
        let Ok(rpc) = std::env::var("CITRATE_E2E_MP_RPC") else {
            return Ok(None);
        };
        let path = std::env::var("CITRATE_E2E_MP_BOOK").map_err(|_| "CITRATE_E2E_MP_BOOK unset")?;
        let book: Value = serde_json::from_str(
            &std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?,
        )
        .map_err(|e| format!("{path}: {e}"))?;
        let update = std::env::var("CITRATE_E2E_MP_UPDATE").is_ok_and(|v| v == "1");
        Ok(Some(E2eEnv { rpc, book, update }))
    }
}

struct World {
    chain: RpcClient<HttpTransport>,
    /// For anvil's own methods (mining), which can outlast the app transport's 6 s bound.
    rpc_url: String,
    vault: CustodyVault,
    ceremony: SignatureCeremony,
    data_dir: PathBuf,
    member: String,
    grantor: String,
    sbt: String,
    stake_vault: String,
    registry: String,
    bond: String,
    node_pubkey: [u8; 32],
    /// address (lowercase) -> role name, for normalization.
    roles: BTreeMap<String, String>,
    /// tx hash (lowercase) -> label, for normalization.
    txs: BTreeMap<String, String>,
    events: BTreeMap<[u8; 32], EventSpec>,
    /// The grant block's timestamp: unlock timestamps are recorded relative to it.
    grant_ts: Option<u64>,
}

fn contract(book: &Value, name: &str) -> Result<String, String> {
    book["contracts"][name]
        .as_str()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| format!("moneypath book has no contracts.{name}"))
}

impl World {
    fn new(env: &E2eEnv, dir: &Path) -> Result<World, String> {
        let chain = RpcClient::with_transport(HttpTransport::new(env.rpc.clone()));
        let vault = CustodyVault::new(Box::new(MemKeyring::default()), dir.join("vault.enc"), 0);
        let pass = || b"moneypath e2e vault passphrase".to_vec();
        vault.init(&mut pass()).map_err(|e| e.to_string())?;
        vault.unlock(&mut pass()).map_err(|e| e.to_string())?;
        let member = crate::wallet::import(&vault, MNEMONIC)
            .map_err(|e| e.to_string())?
            .address
            .to_ascii_lowercase();
        let data_dir = dir.join("node");
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        std::fs::write(
            data_dir.join(crate::validator::PROPOSER_KEY_FILE),
            PROPOSER_SEED,
        )
        .map_err(|e| e.to_string())?;
        let book = &env.book;
        let mut roles = BTreeMap::new();
        let grantor = book["grantor"]
            .as_str()
            .ok_or("book has no grantor")?
            .to_ascii_lowercase();
        let deployer = book["deployer"]
            .as_str()
            .ok_or("book has no deployer")?
            .to_ascii_lowercase();
        roles.insert(member.clone(), "member".to_string());
        roles.insert(grantor.clone(), "grantor".to_string());
        roles.insert(deployer, "deployer".to_string());
        roles.insert(format!("0x{}", "0".repeat(40)), "zero".to_string());
        for name in [
            "GovernanceTimelock",
            "GovernanceMultisig",
            "CitrateMemberSBT",
            "MemberBond",
            "MembershipStakeVault",
            "ValidatorRegistry",
        ] {
            roles.insert(contract(book, name)?, name.to_string());
        }
        let mut events = BTreeMap::new();
        for sig in EVENTS {
            let (topic, spec) = parse_event(sig)?;
            events.insert(topic, spec);
        }
        let mut w = World {
            chain,
            rpc_url: env.rpc.clone(),
            vault,
            ceremony: SignatureCeremony::new(),
            data_dir,
            member,
            grantor,
            sbt: contract(book, "CitrateMemberSBT")?,
            stake_vault: contract(book, "MembershipStakeVault")?,
            registry: contract(book, "ValidatorRegistry")?,
            bond: String::new(),
            node_pubkey: crate::validator::proposer_pubkey_from_seed(&PROPOSER_SEED),
            roles,
            txs: BTreeMap::new(),
            events,
            grant_ts: None,
        };
        // The member's bond: CREATE2-deterministic, read from the vault before it exists.
        let ret = w.call(
            &w.stake_vault,
            &calldata("bondOf(address)", &[word_addr(&w.member)?]),
        )?;
        w.bond = format!("0x{}", hex::encode(ret.get(12..32).ok_or("short bondOf")?));
        w.roles.insert(w.bond.clone(), "bond".to_string());
        Ok(w)
    }

    /// The app's book for this chain, as the generated book's shape: the thread-local override
    /// makes core's production readers and builders target the anvil deployment.
    fn core_book(&self) -> Result<String, String> {
        let genesis = self.rpc("eth_getBlockByNumber", json!(["0x0", false]))?["hash"]
            .as_str()
            .ok_or("no block 0 hash")?
            .to_string();
        let zero = format!("0x{}", "0".repeat(40));
        Ok(json!({
            "chainId": CHAIN_ID,
            "genesisHash": genesis,
            "addresses": {
                "CitrateMemberSBT": self.sbt,
                "MembershipStakeVault": self.stake_vault,
                "ValidatorRegistry": self.registry,
                "CitrateWalletFactory": zero,
                "ContributionAccounting": zero,
                "IPFSIncentivesV3": zero,
            }
        })
        .to_string())
    }

    fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let resp = self
            .chain
            .transport()
            .call(json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
            .map_err(|e| format!("{method}: {e}"))?;
        if let Some(err) = resp.get("error") {
            return Err(format!("{method}: {err}"));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    fn call(&self, to: &str, data: &str) -> Result<Vec<u8>, String> {
        let v = self.rpc("eth_call", json!([{"to": to, "data": data}, "latest"]))?;
        hex::decode(v.as_str().unwrap_or("0x").trim_start_matches("0x")).map_err(|e| e.to_string())
    }

    fn call_u128(&self, to: &str, sig: &str, args: &[[u8; 32]]) -> Result<u128, String> {
        let r = self.call(to, &calldata(sig, args))?;
        r.get(..32)
            .and_then(word_to_u128)
            .ok_or_else(|| format!("{sig}: not a u128 word"))
    }

    fn balance(&self, a: &str) -> Result<u128, String> {
        hex_u128(&self.rpc("eth_getBalance", json!([a, "latest"]))?).ok_or("bad balance".into())
    }

    fn head(&self) -> Result<u64, String> {
        hex_u64(&self.rpc("eth_blockNumber", json!([]))?).ok_or("bad block number".into())
    }

    /// Mine `n` empty blocks (anvil's `anvil_mine`, with a long timeout of its own).
    fn mine(&self, n: u64) -> Result<(), String> {
        println!("moneypath e2e: mining {n} blocks");
        if n == 0 {
            return Ok(());
        }
        let resp: Value = ureq::post(&self.rpc_url)
            .config()
            .timeout_global(Some(Duration::from_secs(600)))
            .build()
            .send_json(json!({"jsonrpc": "2.0", "id": 1, "method": "anvil_mine",
                              "params": [format!("0x{n:x}")]}))
            .map_err(|e| format!("anvil_mine: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("anvil_mine: {e}"))?;
        match resp.get("error") {
            Some(e) => Err(format!("anvil_mine: {e}")),
            None => Ok(()),
        }
    }

    /// The mined receipt (anvil automines, but the receipt can lag the send by a moment).
    fn receipt(&self, hash: &str) -> Result<Value, String> {
        for _ in 0..100 {
            let r = self.rpc("eth_getTransactionReceipt", json!([hash]))?;
            if !r.is_null() {
                return Ok(r);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!("no receipt for {hash}"))
    }

    /// A treasury-side transaction from an unlocked anvil account (the grantor): not core's.
    fn treasury_send(
        &mut self,
        label: &str,
        to: &str,
        value: u128,
        data: &str,
    ) -> Result<Value, String> {
        let hash = self.rpc(
            "eth_sendTransaction",
            json!([{"from": self.grantor, "to": to, "value": format!("0x{value:x}"),
                    "data": data, "gas": "0x7a1200"}]),
        )?;
        let hash = hash.as_str().ok_or("no tx hash")?.to_ascii_lowercase();
        self.txs.insert(hash.clone(), label.to_string());
        let r = self.receipt(&hash)?;
        if hex_u64(&r["status"]) != Some(1) {
            return Err(format!("treasury tx {label} reverted"));
        }
        Ok(r)
    }

    /// A member transaction through core's SignatureCeremony: `request` shows the decoded
    /// intent, the approval passes the raw-mode ack only when the view demands one (as the UI
    /// does), and `approve_and_broadcast` signs the EIP-155 tx from the vault and sends it.
    fn ceremony_send(
        &mut self,
        label: &str,
        intent: SignatureIntent,
    ) -> Result<(Value, BroadcastResult, Value), String> {
        let view = self.ceremony.request(intent);
        let view_json = serde_json::to_value(&view).map_err(|e| e.to_string())?;
        let result = self
            .ceremony
            .approve_and_broadcast(
                &self.vault,
                &self.chain,
                &view.id,
                view.requires_raw_ack,
                BroadcastConfig {
                    chain_id: CHAIN_ID,
                    poll_attempts: 100,
                    poll_interval: Duration::from_millis(50),
                },
            )
            .map_err(|e| format!("ceremony {label}: {e}"))?;
        self.txs
            .insert(result.tx_hash.to_ascii_lowercase(), label.to_string());
        let receipt = self.receipt(&result.tx_hash)?;
        Ok((view_json, result, receipt))
    }

    /// The member's gas for a receipt, so balance deltas can be stated exactly.
    fn gas_cost(receipt: &Value) -> Result<u128, String> {
        let used = hex_u128(&receipt["gasUsed"]).ok_or("no gasUsed")?;
        let price = hex_u128(&receipt["effectiveGasPrice"]).ok_or("no effectiveGasPrice")?;
        Ok(used * price)
    }

    // -------------------------------------------------------------------- normalization

    fn label_addr(&self, a: &str) -> String {
        let a = a.to_ascii_lowercase();
        self.roles.get(&a).cloned().unwrap_or(a)
    }

    fn block_ts(&self, block: &Value) -> Result<u64, String> {
        let b = self.rpc("eth_getBlockByNumber", json!([block, false]))?;
        hex_u64(&b["timestamp"]).ok_or("no block timestamp".into())
    }

    /// The events of a receipt: emitter role, name, named and normalized args.
    fn events_of(&self, receipt: &Value) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let ts = self.block_ts(&receipt["blockNumber"])?;
        for log in receipt["logs"].as_array().ok_or("receipt without logs")? {
            let emitter = self.label_addr(log["address"].as_str().unwrap_or(""));
            let topics: Vec<Vec<u8>> = log["topics"]
                .as_array()
                .ok_or("log without topics")?
                .iter()
                .map(|t| hex::decode(t.as_str().unwrap_or("").trim_start_matches("0x")))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?;
            let data = hex::decode(
                log["data"]
                    .as_str()
                    .unwrap_or("0x")
                    .trim_start_matches("0x"),
            )
            .map_err(|e| e.to_string())?;
            let mut topic0 = [0u8; 32];
            if let Some(t) = topics.first().filter(|t| t.len() == 32) {
                topic0.copy_from_slice(t);
            }
            let Some(spec) = self.events.get(&topic0) else {
                out.push(json!({"emitter": emitter, "event": "UNKNOWN", "topic0": format!("0x{}", hex::encode(topic0))}));
                continue;
            };
            let mut args = serde_json::Map::new();
            let (mut ti, mut di) = (1usize, 0usize);
            for (name, ty, indexed) in &spec.params {
                let w: Vec<u8> = if *indexed {
                    ti += 1;
                    topics.get(ti - 1).cloned().unwrap_or_default()
                } else {
                    di += 1;
                    data.get((di - 1) * 32..di * 32)
                        .map(<[u8]>::to_vec)
                        .unwrap_or_default()
                };
                args.insert(name.clone(), self.norm_word(&w, ty, name, ts));
            }
            out.push(json!({"emitter": emitter, "event": spec.name, "args": args}));
        }
        Ok(out)
    }

    fn norm_word(&self, w: &[u8], ty: &str, name: &str, block_ts: u64) -> Value {
        if w.len() != 32 {
            return json!("<missing>");
        }
        match ty {
            "address" => json!(self.label_addr(&format!("0x{}", hex::encode(&w[12..])))),
            "bool" => json!(w[31] == 1),
            "bytes32" if w == self.node_pubkey => json!("<nodePubkey>"),
            "bytes32" => json!(format!("0x{}", hex::encode(w))),
            _ => match word_to_u128(w) {
                Some(v) if name.ends_with("Timestamp") => {
                    json!(format!("blockTs+{}", (v as i128) - (block_ts as i128)))
                }
                Some(v) => json!(v.to_string()),
                None => json!(format!("0x{}", hex::encode(w))),
            },
        }
    }

    /// Core's serialized read model, normalized: addresses become roles, tx hashes labels, the
    /// node key `<nodePubkey>`, measured or gas-dependent numbers placeholders, and the unlock
    /// timestamp an offset from the grant block.
    fn norm_core(&self, v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut out = serde_json::Map::new();
                for (k, x) in m {
                    let nv = match (k.as_str(), x) {
                        // Measured over the last 100 blocks' timestamps: anvil's mined blocks make
                        // it run-dependent, and no flow decides on it.
                        ("blockTimeMs", _) => json!("<not compared>"),
                        (_, Value::Null) => Value::Null,
                        ("gasUsed" | "effectiveGasPriceWei", _) => json!("<gas>"),
                        ("id", Value::String(_)) if m.contains_key("origin") => {
                            json!("<ceremonyId>")
                        }
                        ("unlockTimestamp", Value::Number(n)) => {
                            match (n.as_u64(), self.grant_ts) {
                                (Some(t), Some(g)) => {
                                    json!(format!("grantTs+{}", (t as i128) - (g as i128)))
                                }
                                _ => x.clone(),
                            }
                        }
                        _ => self.norm_core(x),
                    };
                    out.insert(k.clone(), nv);
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(|x| self.norm_core(x)).collect()),
            Value::String(s) => {
                let l = s.to_ascii_lowercase();
                if l.len() == 42 && l.starts_with("0x") {
                    json!(self.label_addr(&l))
                } else if l == format!("0x{}", hex::encode(self.node_pubkey)) {
                    json!("<nodePubkey>")
                } else if let Some(t) = self.txs.get(&l) {
                    json!(format!("<tx:{t}>"))
                } else {
                    // Addresses inside messages (the ceremony's decoded action, core's errors).
                    let mut out = s.clone();
                    for (addr, role) in &self.roles {
                        out = out.replace(addr.as_str(), &format!("<{role}>"));
                    }
                    json!(out)
                }
            }
            _ => v.clone(),
        }
    }

    fn broadcast_json(&self, r: &BroadcastResult) -> Result<Value, String> {
        Ok(self.norm_core(&serde_json::to_value(r).map_err(|e| e.to_string())?))
    }

    // -------------------------------------------------------------------- core read model

    fn core_grant_status(&self) -> Value {
        match crate::grant_status::read_grant_status(&self.chain, &self.member) {
            Ok(g) => self.norm_core(&serde_json::to_value(g).unwrap_or(Value::Null)),
            Err(e) => json!({"error": e.to_string()}),
        }
    }

    fn core_activation_status(&self) -> Value {
        let targets = crate::validator_activation::Targets {
            vault: crate::addresses::membership_stake_vault(),
            sbt: crate::addresses::citrate_member_sbt(),
            registry: crate::addresses::validator_registry(),
        };
        let st = crate::validator_activation::read_activation_status(
            self.chain.transport(),
            None::<&HttpTransport>,
            &targets,
            &self.member,
            Some(self.node_pubkey),
            None,
            &|_| None,
        );
        match st {
            Ok(s) => self.norm_core(&serde_json::to_value(s).unwrap_or(Value::Null)),
            Err(e) => json!({"error": e}),
        }
    }

    fn core_activation_intent(&self) -> Result<SignatureIntent, String> {
        let dir = self.data_dir.clone();
        crate::node::bond_activation_intent(
            &self.chain,
            &self.member,
            &|chain_id, registry, staker, nonce| {
                crate::validator::sign_registration_from_data_dir(
                    &dir, chain_id, registry, staker, nonce,
                )
            },
        )
    }

    // -------------------------------------------------------------------- chain state

    fn pubkey_word(&self) -> [u8; 32] {
        self.node_pubkey
    }

    /// The chain state the flows move, read directly with `eth_call` (not through core).
    fn chain_state(&self) -> Result<Value, String> {
        let m = word_addr(&self.member)?;
        let pk = self.pubkey_word();
        let bond_code = self.rpc("eth_getCode", json!([self.bond, "latest"]))?;
        let bond_deployed = bond_code.as_str().is_some_and(|c| c.len() > 2);
        let grant = match self.call(
            &self.stake_vault,
            &calldata("getGrant(uint256)", &[word_u(0)]),
        ) {
            Ok(r) if r.len() >= 7 * 32 => json!({
                "member": self.label_addr(&format!("0x{}", hex::encode(&r[12..32]))),
                "bond": self.label_addr(&format!("0x{}", hex::encode(&r[44..64]))),
                "principalWei": word_to_u128(&r[64..96]).map(|v| v.to_string()),
                "memberTokenId": word_to_u128(&r[96..128]).map(|v| v.to_string()),
                "state": enum_name(GRANT_STATES, word_to_u128(&r[192..224])),
            }),
            _ => json!("none"),
        };
        let bond = if bond_deployed {
            json!({
                "activated": self.call_u128(&self.bond, "activated()", &[])? == 1,
                "releaseRequested": self.call_u128(&self.bond, "releaseRequested()", &[])? == 1,
                "isUnlocked": self.call_u128(&self.bond, "isUnlocked()", &[])? == 1,
                "isKycVerified": self.call_u128(&self.bond, "isKycVerified()", &[])? == 1,
                "balanceWei": self.balance(&self.bond)?.to_string(),
            })
        } else {
            json!("not deployed")
        };
        let info = self.call(&self.registry, &calldata("validatorInfo(bytes32)", &[pk]))?;
        let w = |i: usize| info.get(i * 32..(i + 1) * 32).and_then(word_to_u128);
        let staker_of = self.call(
            &self.registry,
            &calldata("pubkeyOfStaker(address)", &[word_addr(&self.bond)?]),
        )?;
        Ok(json!({
            "vault": {
                "grant0": grant,
                "attributedPrincipalWei": self.call_u128(&self.stake_vault, "attributedPrincipal(address)", &[m])?.to_string(),
                "balanceWei": self.balance(&self.stake_vault)?.to_string(),
            },
            "bond": bond,
            "registry": {
                "pubkeyOfStaker(bond)": if staker_of == pk { json!("<nodePubkey>") } else { json!(format!("0x{}", hex::encode(&staker_of))) },
                "isActive": self.call_u128(&self.registry, "isActive(bytes32)", &[pk])? == 1,
                "stakeOfWei": self.call_u128(&self.registry, "stakeOf(bytes32)", &[pk])?.to_string(),
                "status": enum_name(REGISTRY_STATUSES, w(8)),
                "escrowWei": w(3).map(|v| v.to_string()),
                "activationEpoch": w(5).map(|v| v.to_string()),
                "exitEpoch": w(6).map(|v| v.to_string()),
                "escrowUnlockEpoch": w(7).map(|v| v.to_string()),
                "currentEpoch": self.call_u128(&self.registry, "currentEpoch()", &[])?.to_string(),
                "activeCount": self.call_u128(&self.registry, "activeCount()", &[])?.to_string(),
                "balanceWei": self.balance(&self.registry)?.to_string(),
            },
            "head": self.head()?,
        }))
    }

    /// Whitelist one ed25519-verify input on the precompile stand-in, after checking it here with
    /// ed25519-dalek: `pubkey || sig || digest`, the digest being the one core computes.
    fn admit_registration_sig(&self, intent: &SignatureIntent) -> Result<(), String> {
        let tx: Value = serde_json::from_str(&intent.raw).map_err(|e| e.to_string())?;
        let data = hex::decode(
            tx["data"]
                .as_str()
                .ok_or("no data")?
                .trim_start_matches("0x"),
        )
        .map_err(|e| e.to_string())?;
        let pubkey: [u8; 32] = data
            .get(4..36)
            .and_then(|s| s.try_into().ok())
            .ok_or("no pubkey in activate calldata")?;
        let sig: [u8; 64] = data
            .get(100..164)
            .and_then(|s| s.try_into().ok())
            .ok_or("no signature in activate calldata")?;
        let registry: [u8; 20] = crate::validator::parse_address_20(&self.registry)?;
        let staker: [u8; 20] = crate::validator::parse_address_20(&self.bond)?;
        let nonce =
            crate::validator::read_registration_nonce(&self.chain, &self.registry, &staker)?;
        let digest =
            crate::validator::registration_digest(CHAIN_ID, &registry, &staker, &pubkey, nonce);
        let vk = ed25519_dalek::VerifyingKey::from_bytes(&pubkey).map_err(|e| e.to_string())?;
        vk.verify_strict(&digest, &ed25519_dalek::Signature::from_bytes(&sig))
            .map_err(|e| format!("core's registration signature does not verify: {e}"))?;
        let mut input = Vec::with_capacity(128);
        input.extend_from_slice(&pubkey);
        input.extend_from_slice(&sig);
        input.extend_from_slice(&digest);
        self.rpc(
            "anvil_setStorageAt",
            json!([
                ED25519_PRECOMPILE,
                format!("0x{}", hex::encode(keccak(&input))),
                format!("0x{}", hex::encode(word_u(1)))
            ]),
        )?;
        Ok(())
    }

    /// A member transaction core has no builder for (the Track M exit), as the ceremony's raw
    /// `Transaction` JSON: `to`, value 0, explicit gas.
    fn raw_member_intent(&self, to: &str, data: &str, gas: u64) -> SignatureIntent {
        SignatureIntent {
            origin: "local-user".to_string(),
            kind: crate::ceremony::IntentKind::Transaction,
            chain_id: CHAIN_ID,
            raw: json!({"from": self.member, "to": to, "value": "0x0", "data": data,
                        "gas": format!("0x{gas:x}"), "chainId": format!("0x{CHAIN_ID:x}")})
            .to_string(),
        }
    }
}

// ------------------------------------------------------------------------ steps and goldens

/// One recorded step: what it did, the events, the chain state and core's read model.
fn step(name: &str, does: &str, events: Vec<Value>, chain: Value, core: Value) -> Value {
    println!("moneypath e2e: step {name} recorded");
    json!({"step": name, "does": does, "events": events, "chain": chain, "core": core})
}

fn run_membership_activation(w: &mut World) -> Result<Vec<Value>, String> {
    let mut steps = Vec::new();

    // 0. Before any grant: nothing deployed, and core refuses to build an activation.
    let refused = match w.core_activation_intent() {
        Ok(_) => json!("built an intent before the grant"),
        Err(e) => json!(w.norm_core(&json!(e))),
    };
    steps.push(step(
        "before-grant",
        "no grant yet: core reads an undeployed bond and refuses the activation intent",
        vec![],
        w.chain_state()?,
        json!({"grantStatus": w.core_grant_status(), "activationStatus": w.core_activation_status(),
               "activationIntent": refused}),
    ));

    // 1. The treasury mints the membership SBT and places the 32,000 SALT grant (not core's code).
    let sub_hash = keccak(b"moneypath-e2e-member-sub");
    let mint = w.treasury_send(
        "mintMember",
        &w.sbt.clone(),
        0,
        &calldata(
            "mintMember(address,bytes32,uint64,uint64)",
            &[
                word_addr(&w.member)?,
                sub_hash,
                word_u(TERM_START.into()),
                word_u(TERM_END.into()),
            ],
        ),
    )?;
    let grant = w.treasury_send(
        "grant",
        &w.stake_vault.clone(),
        GRANT_WEI,
        &calldata(
            "grant(address,uint256,uint256)",
            &[word_addr(&w.member)?, word_u(GRANT_WEI), word_u(0)],
        ),
    )?;
    w.grant_ts = Some(w.block_ts(&grant["blockNumber"])?);
    let mut events = w.events_of(&mint)?;
    events.extend(w.events_of(&grant)?);
    steps.push(step(
        "grant",
        "treasury mints the SBT and grants 32,000 SALT into the member's MemberBond (locked 364 days)",
        events,
        w.chain_state()?,
        json!({"grantStatus": w.core_grant_status(), "activationStatus": w.core_activation_status()}),
    ));

    // 2. Activation: core reads, guards, signs the registration digest with the node key and
    //    builds MemberBond.activate; the member approves it in the ceremony.
    let intent = w.core_activation_intent()?;
    w.admit_registration_sig(&intent)?;
    let (view, result, receipt) = w.ceremony_send("activate", intent)?;
    steps.push(step(
        "activate",
        "core builds MemberBond.activate (node key signs the registry digest); the ceremony signs and sends it",
        w.events_of(&receipt)?,
        w.chain_state()?,
        json!({"ceremony": w.norm_core(&view), "broadcast": w.broadcast_json(&result)?,
               "grantStatus": w.core_grant_status(), "activationStatus": w.core_activation_status()}),
    ));

    // 3. A second activation: core refuses before any ceremony exists.
    let again = match w.core_activation_intent() {
        Ok(_) => json!("built a second activation intent"),
        Err(e) => json!(w.norm_core(&json!(e))),
    };
    steps.push(step(
        "activate-again",
        "core refuses a second activation (the bond already has a validator)",
        vec![],
        w.chain_state()?,
        json!({"activationIntent": again}),
    ));

    // 4. The activation epoch arrives: isActive and actEpoch <= currentEpoch.
    let act_epoch: u64 = w.chain_state()?["registry"]["activationEpoch"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("no activation epoch")?;
    let head = w.head()?;
    w.mine((act_epoch * EPOCH_BLOCKS).saturating_sub(head))?;
    steps.push(step(
        "epoch-reached",
        "blocks are mined to the activation epoch: the validator is in the active set",
        vec![],
        w.chain_state()?,
        json!({"activationStatus": w.core_activation_status()}),
    ));
    Ok(steps)
}

fn run_bond_exit(w: &mut World) -> Result<Vec<Value>, String> {
    let mut steps = Vec::new();
    let release = calldata("requestRelease()", &[]);
    let withdraw = calldata("withdrawToMember()", &[]);

    // 0. The treasury attests KYC (money out is gated on it; not core's code).
    let kyc = w.treasury_send(
        "setKycVerified",
        &w.sbt.clone(),
        0,
        &calldata("setKycVerified(uint256,bool)", &[word_u(0), word_u(1)]),
    )?;
    steps.push(step(
        "kyc-attested",
        "treasury attests KYC on the member's SBT",
        w.events_of(&kyc)?,
        w.chain_state()?,
        json!({"grantStatus": w.core_grant_status()}),
    ));

    // 1. Release inside the lock: the bond refuses (StillLocked); the tx is mined and reverts.
    let intent = w.raw_member_intent(&w.bond.clone(), &release, 400_000);
    let (view, result, receipt) = w.ceremony_send("requestRelease-locked", intent)?;
    steps.push(step(
        "release-inside-lock",
        "member sends MemberBond.requestRelease inside the lock: mined, reverted, nothing moves",
        w.events_of(&receipt)?,
        w.chain_state()?,
        json!({"ceremony": w.norm_core(&view), "broadcast": w.broadcast_json(&result)?,
               "activationStatus": w.core_activation_status()}),
    ));

    // 2. The lock's wall-clock leg elapses (364 days).
    w.rpc("evm_increaseTime", json!([LOCK_SECONDS]))?;
    w.mine(1)?;
    steps.push(step(
        "lock-elapsed",
        "364 days pass: the bond is unlocked; nothing moves until the member acts",
        vec![],
        w.chain_state()?,
        json!({"grantStatus": w.core_grant_status(), "activationStatus": w.core_activation_status()}),
    ));

    // 3. Release: the member starts the exit; anyone records it on the vault.
    let intent = w.raw_member_intent(&w.bond.clone(), &release, 400_000);
    let (view, result, receipt) = w.ceremony_send("requestRelease", intent)?;
    let mut events = w.events_of(&receipt)?;
    let recorded = w.treasury_send(
        "recordRelease",
        &w.stake_vault.clone(),
        0,
        &calldata("recordRelease(uint256)", &[word_u(0)]),
    )?;
    events.extend(w.events_of(&recorded)?);
    steps.push(step(
        "release",
        "member sends MemberBond.requestRelease (registry full unbond); the vault records the release",
        events,
        w.chain_state()?,
        json!({"ceremony": w.norm_core(&view), "broadcast": w.broadcast_json(&result)?,
               "grantStatus": w.core_grant_status(), "activationStatus": w.core_activation_status()}),
    ));

    // 4. Withdraw before the registry's exit lock: reverts (Locked).
    let intent = w.raw_member_intent(&w.bond.clone(), &withdraw, 400_000);
    let (_, result, receipt) = w.ceremony_send("withdrawToMember-early", intent)?;
    steps.push(step(
        "withdraw-too-early",
        "member sends MemberBond.withdrawToMember before the registry exit lock: mined, reverted",
        w.events_of(&receipt)?,
        w.chain_state()?,
        json!({"broadcast": w.broadcast_json(&result)?}),
    ));

    // 5. The exit lock (6 epochs) passes; the principal comes back to the member.
    let unlock: u64 = w.chain_state()?["registry"]["escrowUnlockEpoch"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("no escrow unlock epoch")?;
    let head = w.head()?;
    w.mine((unlock * EPOCH_BLOCKS).saturating_sub(head))?;
    let before = w.balance(&w.member)?;
    let intent = w.raw_member_intent(&w.bond.clone(), &withdraw, 400_000);
    let (_, result, receipt) = w.ceremony_send("withdrawToMember", intent)?;
    let received = (w.balance(&w.member)? + World::gas_cost(&receipt)?)
        .checked_sub(before)
        .ok_or("member balance went down")?;
    steps.push(step(
        "exit-matured",
        &format!(
            "after {EXIT_LOCK_EPOCHS} epochs the member withdraws: the principal returns to the member"
        ),
        w.events_of(&receipt)?,
        json!({"state": w.chain_state()?, "memberReceivedWei": received.to_string()}),
        json!({"broadcast": w.broadcast_json(&result)?, "grantStatus": w.core_grant_status(),
               "activationStatus": w.core_activation_status()}),
    ));
    Ok(steps)
}

/// The first difference between two JSON values, as a path and both sides.
fn first_diff(path: &str, want: &Value, got: &Value) -> Option<String> {
    match (want, got) {
        (Value::Object(a), Value::Object(b)) => {
            for k in a.keys().chain(b.keys()) {
                let p = format!("{path}.{k}");
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => {
                        if let Some(d) = first_diff(&p, x, y) {
                            return Some(d);
                        }
                    }
                    (x, y) => return Some(format!("{p}: golden {x:?}, run {y:?}")),
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            for i in 0..a.len().max(b.len()) {
                let p = format!("{path}[{i}]");
                match (a.get(i), b.get(i)) {
                    (Some(x), Some(y)) => {
                        if let Some(d) = first_diff(&p, x, y) {
                            return Some(d);
                        }
                    }
                    (x, y) => return Some(format!("{p}: golden {x:?}, run {y:?}")),
                }
            }
            None
        }
        _ if want == got => None,
        _ => Some(format!("{path}: golden {want}, run {got}")),
    }
}

fn golden_path(flow: &str) -> PathBuf {
    golden_dir().join(format!("{flow}.golden.json"))
}

fn check_or_update(flow: &str, steps: Vec<Value>, update: bool) -> Result<(), String> {
    let got = json!({"flow": flow, "steps": steps});
    let path = golden_path(flow);
    if update {
        let text = serde_json::to_string_pretty(&got).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("moneypath e2e: rewrote {}", path.display());
        return Ok(());
    }
    let want: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    match first_diff(flow, &want, &got) {
        None => {
            println!("moneypath e2e: {flow} matches its golden");
            Ok(())
        }
        Some(d) => Err(format!(
            "moneypath e2e: {flow} drifted from {}\n  first difference: {d}",
            path.display()
        )),
    }
}

/// One flow: its steps, recorded on the shared chain.
type FlowRun = fn(&mut World) -> Result<Vec<Value>, String>;

/// **The e2e.** Run by `scripts/e2e/moneypath-anvil.sh`; skipped without `CITRATE_E2E_MP_RPC`.
#[test]
#[ignore = "needs the genesis contract set on anvil: scripts/e2e/moneypath-anvil.sh"]
fn anvil_moneypath_genesis_set() {
    let env = match E2eEnv::from_env() {
        Ok(Some(e)) => e,
        Ok(None) => {
            println!("moneypath e2e: CITRATE_E2E_MP_RPC unset, skipped");
            return;
        }
        Err(e) => panic!("moneypath e2e: {e}"),
    };
    let dir = std::env::temp_dir().join(format!("citrate-moneypath-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let outcome = (|| -> Result<Vec<String>, String> {
        let mut w = World::new(&env, &dir)?;
        crate::addresses::set_thread_test_book(&w.core_book()?)?;
        let mut failures = Vec::new();
        let runs: [(&str, FlowRun); 2] = [
            (FLOWS[0], run_membership_activation),
            (FLOWS[1], run_bond_exit),
        ];
        for (flow, run) in runs {
            // A flow that cannot finish leaves the chain in a state the later flows were not
            // recorded from, so the run stops there, keeping the drift already found.
            let steps = match run(&mut w) {
                Ok(steps) => steps,
                Err(e) => {
                    failures.push(format!("moneypath e2e: {flow} could not finish: {e}"));
                    break;
                }
            };
            if let Err(e) = check_or_update(flow, steps, env.update) {
                failures.push(e);
            }
        }
        Ok(failures)
    })();
    crate::addresses::clear_thread_test_book();
    let _ = std::fs::remove_dir_all(&dir);
    match outcome {
        Ok(failures) if failures.is_empty() => {}
        Ok(failures) => panic!("{}", failures.join("\n")),
        Err(e) => panic!("moneypath e2e: {e}"),
    }
}

// ------------------------------------------------------------------------ always-on checks

/// Every flow has a golden, each golden names its flow and has steps, and every step carries the
/// four parts the e2e compares. A golden emptied or truncated by hand fails here, without anvil.
#[test]
fn moneypath_goldens_are_complete() {
    for flow in FLOWS {
        let path = golden_path(flow);
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let g: Value = serde_json::from_str(&text).expect("golden is JSON");
        assert_eq!(
            g["flow"],
            json!(flow),
            "{} names another flow",
            path.display()
        );
        let steps = g["steps"].as_array().expect("steps array");
        assert!(!steps.is_empty(), "{flow} has no steps");
        for s in steps {
            for key in ["step", "does", "events", "chain", "core"] {
                assert!(s.get(key).is_some(), "{flow}: a step lacks {key}: {s}");
            }
            for e in s["events"].as_array().expect("events array") {
                assert_ne!(
                    e["event"],
                    json!("UNKNOWN"),
                    "{flow}: an unknown event is golden"
                );
            }
        }
    }
}

/// Every event signature the e2e decodes parses, and no two share a topic0.
#[test]
fn moneypath_event_table_is_well_formed() {
    let mut seen = std::collections::BTreeSet::new();
    for sig in EVENTS {
        let (topic, spec) = parse_event(sig).expect("parses");
        assert!(seen.insert(topic), "duplicate topic0 for {sig}");
        assert!(!spec.params.is_empty(), "{sig} has no params");
    }
    // Pinned against a known topic0: the standard ERC-721 Transfer event the SBT mint emits.
    let (topic, _) =
        parse_event("Transfer(address indexed from,address indexed to,uint256 indexed tokenId)")
            .expect("parses");
    assert_eq!(
        format!("0x{}", hex::encode(topic)),
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
    );
}

/// The golden comparison reports the first difference with its path, and none when equal.
#[test]
fn moneypath_first_diff_names_the_path() {
    let a = json!({"steps": [{"chain": {"isActive": true}}]});
    let b = json!({"steps": [{"chain": {"isActive": false}}]});
    assert_eq!(first_diff("f", &a, &a), None);
    let d = first_diff("f", &a, &b).expect("a difference");
    assert!(d.starts_with("f.steps[0].chain.isActive"), "{d}");
}
