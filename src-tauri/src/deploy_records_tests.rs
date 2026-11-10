// CORE-A3 — durable, encrypted deploy records. Pure: temp dirs, a fixture chain, an in-memory
// keyring; nothing reaches a network or the OS keychain.
use super::*;
use crate::custody::CustodyError;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

const ADDR: &str = "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed";
const FROM: &str = "0x1111111111111111111111111111111111111111";
const TX: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CREATION_HEX: &str = "6080604052348015600f57600080fd5b50603f80601d6000396000f3fe6080604052";
const ARGS_HEX: &str = "000000000000000000000000000000000000000000000000000000000000002a";

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "citrate-deployrec-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("temp dir");
        TestDir(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct MemKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
}

impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

struct DeadKeyring;

impl Keyring for DeadKeyring {
    fn get(&self, _a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> std::result::Result<(), CustodyError> {
        Ok(())
    }
}

/// A fixture 40204 node answering the receipt and the transaction of one creation.
struct ChainRpc {
    receipt: serde_json::Value,
    tx: serde_json::Value,
}

impl ChainRpc {
    fn creation(input_hex: &str) -> Self {
        ChainRpc {
            receipt: serde_json::json!({
                "transactionHash": TX, "blockNumber": "0x2a", "status": "0x1",
                "contractAddress": ADDR,
            }),
            tx: serde_json::json!({
                "hash": TX, "blockNumber": "0x2a", "from": FROM, "to": null,
                "input": format!("0x{input_hex}"),
            }),
        }
    }
}

impl crate::rpc::RpcTransport for ChainRpc {
    fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, crate::rpc::RpcError> {
        let result = match body["method"].as_str() {
            Some("eth_getTransactionReceipt") => self.receipt.clone(),
            Some("eth_getTransactionByHash") => self.tx.clone(),
            m => panic!("unexpected RPC method {m:?}"),
        };
        Ok(serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": result}))
    }
}

fn client(rpc: ChainRpc) -> crate::rpc::RpcClient<ChainRpc> {
    crate::rpc::RpcClient::with_transport(rpc)
}

/// A rendered hello-mint project with a compiled artifact.
fn project(d: &TestDir) -> HelloMintProject {
    let root = d.path().join("project");
    std::fs::create_dir_all(root.join("contracts/out/Token.sol")).unwrap();
    let lock = crate::postdeploy::LOCK_FILE;
    std::fs::write(
        root.join(lock),
        serde_json::json!({"schema": 1, "template": "hello-mint", "solc": "0.8.36",
            "params": {"name": "Lemon Drops", "contract": "LemonDrops"}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        root.join("contracts").join(lock),
        serde_json::json!({"schema": 1, "template": "erc721", "solc": "0.8.36",
            "params": {"contract": "LemonDrops"}})
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("contracts/out/Token.sol/LemonDrops.json"),
        serde_json::json!({
            "abi": [{"type": "function", "name": "mint", "inputs": [{"name": "quantity", "type": "uint256"}],
                     "outputs": [], "stateMutability": "payable"}],
            "bytecode": {"object": format!("0x{CREATION_HEX}")},
            "deployedBytecode": {"object": "0x6080604052"},
        })
        .to_string(),
    )
    .unwrap();
    crate::postdeploy::open_project(&root).expect("project")
}

fn initcode() -> Vec<u8> {
    hex::decode(format!("{CREATION_HEX}{ARGS_HEX}")).unwrap()
}

fn gate_with(verdict: Verdict) -> GateStore {
    let gate = GateStore::default();
    gate.record(GateRecord {
        initcode_hash: crate::deploy_gate::initcode_hash(&initcode()),
        binding_hash: "0xbb".into(),
        compiler: CompilerSettings {
            solc_version: "0.8.36".into(),
            optimizer: true,
            optimizer_runs: 200,
            evm_version: "cancun".into(),
            via_ir: false,
        },
        verdict,
        items: vec![],
        evaluated_at_ms: 7,
    })
    .unwrap();
    gate
}

const STD_JSON: &str = r#"{"language":"Solidity","sources":{"src/Token.sol":{"content":"contract LemonDrops {}"}}}"#;

fn save(
    store: &DeployRecordStore<'_>,
    p: &HelloMintProject,
    gate: &GateStore,
    rpc: ChainRpc,
    sj: Result<String, String>,
) -> Result<DeployRecordSaved, String> {
    save_deploy_record(store, &client(rpc), p, gate, TX, move |_| sj, 1_000)
}

fn sealed_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|r| r.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default()
}

#[test]
fn a_confirmed_deploy_is_kept_with_every_field_from_its_source() {
    let d = TestDir::new("fields");
    let p = project(&d);
    let kr = MemKeyring::default();
    let store = DeployRecordStore::new(d.path().join(RECORDS_DIR), &kr);
    let gate = gate_with(Verdict::Ready);

    let out = save(&store, &p, &gate, ChainRpc::creation(&format!("{CREATION_HEX}{ARGS_HEX}")), Ok(STD_JSON.into())).unwrap();
    assert!(!out.already_saved);
    assert_eq!(out.count, 1);

    let recs = store.load(&project_id(&p.root)).unwrap();
    assert_eq!(recs.len(), 1);
    let r = &recs[0];
    assert_eq!(r.chain_id, 40204);
    assert_eq!(r.address, ADDR);
    assert_eq!(r.tx_hash, TX);
    assert_eq!(r.block_number, 42);
    assert_eq!(r.deployer, FROM);
    assert_eq!(r.initcode_hash, crate::deploy_gate::initcode_hash(&initcode()));
    assert_eq!(r.contract_name, "LemonDrops");
    assert_eq!(r.abi[0]["name"], "mint");
    assert_eq!(r.constructor_args_hex, format!("0x{ARGS_HEX}"));
    assert_eq!(r.compiler.solc_version, "0.8.36");
    assert_eq!(r.compiler.optimizer_runs, 200);
    assert_eq!(r.standard_json_input.as_deref(), Some(STD_JSON));
    assert_eq!(r.standard_json_note, None);
    assert_eq!(r.gate.verdict, Verdict::Ready);
    assert_eq!(r.gate.evaluated_at_ms, 7);
    assert_eq!(r.recorded_at_ms, 1_000);

    let s = &out.record;
    assert_eq!(s.abi_entries, 1);
    assert_eq!(s.standard_json_bytes, Some(STD_JSON.len()));
    assert_eq!(s.gate_verdict, Verdict::Ready);
}

#[test]
fn the_record_survives_a_restart_and_is_ciphertext_on_disk() {
    let d = TestDir::new("durable");
    let p = project(&d);
    let kr = MemKeyring::default();
    let dir = d.path().join(RECORDS_DIR);
    save(
        &DeployRecordStore::new(&dir, &kr),
        &p,
        &gate_with(Verdict::Ready),
        ChainRpc::creation(&format!("{CREATION_HEX}{ARGS_HEX}")),
        Ok(STD_JSON.into()),
    )
    .unwrap();

    // A new store (a restarted app) with the same keychain reads it back.
    let again = DeployRecordStore::new(&dir, &kr).load(&project_id(&p.root)).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].address, ADDR);

    let files = sealed_files(&dir);
    assert_eq!(files.len(), 1, "one sealed file for the project: {files:?}");
    let bytes = std::fs::read(&files[0]).unwrap();
    assert_eq!(&bytes[..8], b"CITRDPL\0");
    let text = String::from_utf8_lossy(&bytes);
    for plain in ["LemonDrops", "5aaeb6053f3e94c9b9a09f33669435e7ef1beaed", "mint", "Solidity", "cancun"] {
        assert!(!text.contains(plain), "{plain} is readable on disk");
    }
    assert!(kr.get(KEY_ACCOUNT).unwrap().is_some_and(|k| k.len() == 32));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&files[0]), 0o600);
        assert_eq!(mode(&dir), 0o700);
    }
}

#[test]
fn the_same_transaction_is_recorded_once() {
    let d = TestDir::new("once");
    let p = project(&d);
    let kr = MemKeyring::default();
    let store = DeployRecordStore::new(d.path().join(RECORDS_DIR), &kr);
    let gate = gate_with(Verdict::Ready);
    let input = format!("{CREATION_HEX}{ARGS_HEX}");
    save(&store, &p, &gate, ChainRpc::creation(&input), Ok(STD_JSON.into())).unwrap();
    // Even with the gate record gone (as after a restart), saving it again is a no-op success.
    let out = save(&store, &p, &GateStore::default(), ChainRpc::creation(&input), Err("forge missing".into())).unwrap();
    assert!(out.already_saved);
    assert_eq!(out.count, 1);
    let recs = store.load(&project_id(&p.root)).unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].standard_json_input.as_deref(), Some(STD_JSON), "the first record is kept as it was");
}

#[test]
fn only_a_successful_creation_is_recorded() {
    let d = TestDir::new("creation");
    let p = project(&d);
    let kr = MemKeyring::default();
    let dir = d.path().join(RECORDS_DIR);
    let store = DeployRecordStore::new(&dir, &kr);
    let gate = gate_with(Verdict::Ready);
    let input = format!("{CREATION_HEX}{ARGS_HEX}");

    let mut pending = ChainRpc::creation(&input);
    pending.receipt = serde_json::Value::Null;
    assert!(save(&store, &p, &gate, pending, Ok(STD_JSON.into())).unwrap_err().contains("not confirmed"));

    let mut reverted = ChainRpc::creation(&input);
    reverted.receipt["status"] = "0x0".into();
    assert!(save(&store, &p, &gate, reverted, Ok(STD_JSON.into())).unwrap_err().contains("reverted"));

    let mut call = ChainRpc::creation(&input);
    call.tx["to"] = ADDR.into();
    assert!(save(&store, &p, &gate, call, Ok(STD_JSON.into())).unwrap_err().contains("not a contract creation"));

    let mut empty = ChainRpc::creation(&input);
    empty.tx["input"] = "0x".into();
    assert!(save(&store, &p, &gate, empty, Ok(STD_JSON.into())).unwrap_err().contains("no init code"));

    assert!(sealed_files(&dir).is_empty(), "nothing was written");
    assert!(kr.get(KEY_ACCOUNT).unwrap().is_none(), "no key minted for a refused save");
}

#[test]
fn code_that_is_not_the_projects_contract_is_refused() {
    let d = TestDir::new("foreign");
    let p = project(&d);
    let kr = MemKeyring::default();
    let store = DeployRecordStore::new(d.path().join(RECORDS_DIR), &kr);
    let e = save(&store, &p, &gate_with(Verdict::Ready), ChainRpc::creation("6080604052deadbeef"), Ok(STD_JSON.into())).unwrap_err();
    assert!(e.contains("not this project's compiled contract"), "{e}");
}

#[test]
fn a_build_without_a_ready_gate_record_is_refused() {
    let d = TestDir::new("gate");
    let p = project(&d);
    let kr = MemKeyring::default();
    let store = DeployRecordStore::new(d.path().join(RECORDS_DIR), &kr);
    let input = format!("{CREATION_HEX}{ARGS_HEX}");
    let e = save(&store, &p, &GateStore::default(), ChainRpc::creation(&input), Ok(STD_JSON.into())).unwrap_err();
    assert!(e.contains("no deploy-gate record"), "{e}");
    let e = save(&store, &p, &gate_with(Verdict::NotReady), ChainRpc::creation(&input), Ok(STD_JSON.into())).unwrap_err();
    assert!(e.contains("NOT READY"), "{e}");
    assert!(store.load(&project_id(&p.root)).unwrap().is_empty());
}

#[test]
fn a_missing_or_bad_standard_json_input_is_noted_not_invented() {
    let d = TestDir::new("stdjson");
    let p = project(&d);
    let input = format!("{CREATION_HEX}{ARGS_HEX}");
    let cases: [(Result<String, String>, &str); 3] = [
        (Err("forge could not be run: not found".into()), "forge could not be run"),
        (Ok("not json".into()), "not JSON"),
        (Ok(format!("\"{}\"", "x".repeat(MAX_STANDARD_JSON_BYTES))), "larger than"),
    ];
    for (sj, note) in cases {
        let kr = MemKeyring::default();
        let sub = TestDir::new("stdjson-case");
        let store = DeployRecordStore::new(sub.path().join(RECORDS_DIR), &kr);
        save(&store, &p, &gate_with(Verdict::Ready), ChainRpc::creation(&input), sj).unwrap();
        let r = &store.load(&project_id(&p.root)).unwrap()[0];
        assert_eq!(r.standard_json_input, None);
        assert!(r.standard_json_note.as_deref().is_some_and(|n| n.contains(note)), "{:?}", r.standard_json_note);
    }
}

#[test]
fn a_file_moved_to_another_project_or_edited_does_not_open() {
    let d = TestDir::new("aad");
    let p = project(&d);
    let kr = MemKeyring::default();
    let dir = d.path().join(RECORDS_DIR);
    let store = DeployRecordStore::new(&dir, &kr);
    save(&store, &p, &gate_with(Verdict::Ready), ChainRpc::creation(&format!("{CREATION_HEX}{ARGS_HEX}")), Ok(STD_JSON.into())).unwrap();
    let pid = project_id(&p.root);
    let file = dir.join(format!("{pid}.sealed"));
    let bytes = std::fs::read(&file).unwrap();

    let other = project_id(&d.path().join("another-project"));
    assert_ne!(other, pid);
    std::fs::write(dir.join(format!("{other}.sealed")), &bytes).unwrap();
    assert!(store.load(&other).unwrap_err().contains("damaged or was moved"));

    let mut edited = bytes.clone();
    let last = edited.len() - 1;
    edited[last] ^= 1;
    std::fs::write(&file, &edited).unwrap();
    assert!(store.load(&pid).unwrap_err().contains("damaged or was moved"));
}

#[test]
fn a_lost_key_with_records_on_disk_refuses_and_never_mints_a_new_one() {
    let d = TestDir::new("lostkey");
    let p = project(&d);
    let kr = MemKeyring::default();
    let dir = d.path().join(RECORDS_DIR);
    let store = DeployRecordStore::new(&dir, &kr);
    save(&store, &p, &gate_with(Verdict::Ready), ChainRpc::creation(&format!("{CREATION_HEX}{ARGS_HEX}")), Ok(STD_JSON.into())).unwrap();
    kr.delete(KEY_ACCOUNT).unwrap();

    assert!(store.load(&project_id(&p.root)).unwrap_err().contains("recovery kit"));
    // Another project, with no file of its own, also refuses: the key covers every project.
    assert!(store.load(&project_id(&d.path().join("x"))).unwrap_err().contains("recovery kit"));
    let mut second = ChainRpc::creation(&format!("{CREATION_HEX}{ARGS_HEX}"));
    second.receipt["transactionHash"] = TX.replace('a', "b").into();
    let e = save_deploy_record(&store, &client(second), &p, &gate_with(Verdict::Ready), &TX.replace('a', "b"), |_| Ok(STD_JSON.into()), 2).unwrap_err();
    assert!(e.contains("recovery kit"), "{e}");
    assert!(kr.get(KEY_ACCOUNT).unwrap().is_none(), "no new key was minted over existing records");
}

#[test]
fn an_unreachable_keychain_keeps_the_records_closed() {
    let d = TestDir::new("deadkr");
    let p = project(&d);
    let dir = d.path().join(RECORDS_DIR);
    let store = DeployRecordStore::new(&dir, &DeadKeyring);
    let e = save(&store, &p, &gate_with(Verdict::Ready), ChainRpc::creation(&format!("{CREATION_HEX}{ARGS_HEX}")), Ok(STD_JSON.into())).unwrap_err();
    assert!(e.contains("keychain is not reachable"), "{e}");
    assert!(sealed_files(&dir).is_empty());
}

#[test]
fn a_project_holds_a_bounded_number_of_records() {
    let d = TestDir::new("cap");
    let p = project(&d);
    let kr = MemKeyring::default();
    let store = DeployRecordStore::new(d.path().join(RECORDS_DIR), &kr);
    let gate = gate_with(Verdict::Ready);
    let base = assemble(
        &p,
        read_creation(&client(ChainRpc::creation(&format!("{CREATION_HEX}{ARGS_HEX}"))), TX).unwrap(),
        read_artifact(&p).unwrap(),
        &gate,
        Err("n/a".into()),
        1,
    )
    .unwrap();
    let pid = project_id(&p.root);
    for i in 0..MAX_RECORDS_PER_PROJECT {
        let mut r = base.clone();
        r.tx_hash = format!("0x{i:064x}");
        store.save(&pid, r).unwrap();
    }
    let mut over = base.clone();
    over.tx_hash = format!("0x{:064x}", MAX_RECORDS_PER_PROJECT);
    let e = store.save(&pid, over).unwrap_err();
    assert!(e.contains("already holds"), "{e}");
    assert_eq!(store.load(&pid).unwrap().len(), MAX_RECORDS_PER_PROJECT);
}

#[test]
fn project_ids_are_stable_and_distinct_per_folder() {
    let a = project_id(std::path::Path::new("/Users/m/projects/lemon"));
    assert_eq!(a, project_id(std::path::Path::new("/Users/m/projects/lemon")));
    assert_ne!(a, project_id(std::path::Path::new("/Users/m/projects/lime")));
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn delete_my_local_data_and_the_recovery_kit_cover_the_records_key() {
    let local = include_str!("local_data.rs");
    assert!(local.contains(&format!("(\"{KEY_ACCOUNT}\"")), "local_data lists {KEY_ACCOUNT}");
    assert!(
        crate::recovery_kit::DEVICE_KEYS.iter().any(|k| k.account == KEY_ACCOUNT && k.len == 32),
        "the recovery kit backs up {KEY_ACCOUNT}"
    );
}

#[test]
fn every_deploy_record_command_names_its_data_source() {
    let src = include_str!("deploy_records.rs");
    for cmd in ["deploy_record_save", "deploy_records_list", "deploy_record_get"] {
        let at = src.find(&format!("pub async fn {cmd}(")).unwrap_or_else(|| panic!("{cmd} defined"));
        let doc_start = src[..at].rfind(&format!("/// **{cmd}**")).unwrap_or_else(|| panic!("{cmd} documented"));
        assert!(src[doc_start..at].contains("Data source"), "{cmd} names its data source (Rule 7)");
        let lib = include_str!("lib.rs");
        assert!(lib.contains(&format!("deploy_records::{cmd},")), "{cmd} is registered");
        let acl = include_str!("../permissions/main-window.toml");
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} is allowed for the main window");
    }
}
