// HUP-S6.6 — post-deploy tests. Pure: temp project dirs, fixture receipts and explorer answers.
use super::*;
use std::cell::RefCell;

const ADDR: &str = "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed";
const ADDR_CK: &str = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";

/// A temporary directory removed on drop (no extra crate).
struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "citrate-postdeploy-{}-{}",
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

/// A minimal rendered hello-mint project: the two lock files, the app sources and a build.
fn project() -> TestDir {
    let d = TestDir::new();
    let root = d.path();
    std::fs::create_dir_all(root.join("app/src")).unwrap();
    std::fs::create_dir_all(root.join("app/dist/assets")).unwrap();
    std::fs::create_dir_all(root.join("app/node_modules/x")).unwrap();
    std::fs::create_dir_all(root.join("contracts/src")).unwrap();
    std::fs::write(
        root.join(LOCK_FILE),
        serde_json::json!({"schema": 1, "template": "hello-mint", "solc": "0.8.36",
            "params": {"name": "Lemon Drops", "contract": "LemonDrops"}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        root.join("contracts").join(LOCK_FILE),
        serde_json::json!({"schema": 1, "template": "erc721", "solc": "0.8.36",
            "params": {"contract": "LemonDrops"}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        root.join("contracts/src/Token.sol"),
        "contract LemonDrops {}",
    )
    .unwrap();
    std::fs::write(root.join("app/package.json"), "{\"name\":\"hello-mint\"}").unwrap();
    std::fs::write(root.join("app/.env.example"), "VITE_TARGET=fork\n").unwrap();
    std::fs::write(root.join("app/src/main.tsx"), "export {};").unwrap();
    std::fs::write(root.join("app/node_modules/x/index.js"), "junk").unwrap();
    std::fs::write(root.join("app/dist/index.html"), "<html></html>").unwrap();
    std::fs::write(root.join("app/dist/assets/app.js"), "console.log(1)").unwrap();
    d
}

// ---- project ----

#[test]
fn a_hello_mint_project_is_recognized_by_its_lock_file() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    assert_eq!(p.contract_name, "LemonDrops");
    assert_eq!(p.solc, "0.8.36");
    assert!(p.app_dir.ends_with("app"));
}

#[test]
fn other_projects_are_refused() {
    let d = TestDir::new();
    assert!(open_project(d.path()).is_err(), "no lock file");
    std::fs::write(
        d.path().join(LOCK_FILE),
        serde_json::json!({"template": "erc20", "params": {"contract": "X"}}).to_string(),
    )
    .unwrap();
    let e = open_project(d.path()).unwrap_err();
    assert!(e.contains("hello-mint"), "{e}");
}

#[test]
fn a_contract_name_that_is_not_an_identifier_is_refused() {
    let d = project();
    std::fs::write(
        d.path().join(LOCK_FILE),
        serde_json::json!({"template": "hello-mint", "solc": "0.8.36",
            "params": {"contract": "Evil; rm -rf"}})
        .to_string(),
    )
    .unwrap();
    assert!(open_project(d.path()).is_err());
}

// ---- site switch ----

#[test]
fn switching_the_site_writes_target_and_checksummed_address() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let path = switch_site_to_citrate(&p, ADDR).unwrap();
    assert!(path.ends_with(".env.local"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("VITE_TARGET=citrate\n"), "{text}");
    assert!(
        text.contains(&format!("VITE_CONTRACT_ADDRESS={ADDR_CK}\n")),
        "{text}"
    );
    assert_eq!(site_contract(&p).unwrap().as_deref(), Some(ADDR));
}

#[test]
fn switching_keeps_other_settings_and_replaces_old_targets() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    std::fs::write(
        p.app_dir.join(".env.local"),
        "# mine\nVITE_TARGET=fork\nVITE_FORK_RPC_URL=http://127.0.0.1:8545\nVITE_CONTRACT_ADDRESS=0x0000000000000000000000000000000000000001\nVITE_EXTRA=keep\n",
    )
    .unwrap();
    switch_site_to_citrate(&p, ADDR).unwrap();
    let text = std::fs::read_to_string(p.app_dir.join(".env.local")).unwrap();
    assert!(text.contains("# mine"));
    assert!(text.contains("VITE_EXTRA=keep"));
    assert!(!text.contains("VITE_TARGET=fork"));
    assert!(!text.contains("0x0000000000000000000000000000000000000001"));
    assert_eq!(text.matches("VITE_TARGET=").count(), 1);
    assert_eq!(text.matches("VITE_CONTRACT_ADDRESS=").count(), 1);
}

#[test]
fn site_contract_is_none_until_the_site_targets_40204() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    assert_eq!(site_contract(&p).unwrap(), None);
    std::fs::write(
        p.app_dir.join(".env.local"),
        format!("VITE_TARGET=fork\nVITE_CONTRACT_ADDRESS={ADDR}\n"),
    )
    .unwrap();
    assert_eq!(
        site_contract(&p).unwrap(),
        None,
        "a fork address is not a 40204 deploy"
    );
}

// ---- receipt ----

#[test]
fn a_creation_receipt_yields_the_contract_address() {
    let v = serde_json::json!({
        "transactionHash": "0xabc", "blockNumber": "0x10", "status": "0x1",
        "contractAddress": ADDR_CK
    });
    let r = parse_receipt(&v).unwrap().unwrap();
    assert_eq!(r.contract_address.as_deref(), Some(ADDR));
    assert_eq!(r.block_number, 16);
    assert_eq!(r.status, Some(1));
    assert!(
        parse_receipt(&serde_json::Value::Null).unwrap().is_none(),
        "pending"
    );
}

#[test]
fn a_reverted_or_non_creation_receipt_has_no_contract() {
    let rev = serde_json::json!({"transactionHash": "0xabc", "blockNumber": "0x10", "status": "0x0",
        "contractAddress": ADDR});
    let r = parse_receipt(&rev).unwrap().unwrap();
    assert_eq!(
        r.contract_address, None,
        "a reverted creation deployed nothing"
    );
    let call = serde_json::json!({"transactionHash": "0xabc", "blockNumber": "0x10", "status": "0x1",
        "contractAddress": null});
    assert_eq!(
        parse_receipt(&call).unwrap().unwrap().contract_address,
        None
    );
}

#[test]
fn a_tx_hash_is_32_bytes_of_hex() {
    assert!(parse_tx_hash(&format!("0x{}", "ab".repeat(32))).is_ok());
    assert!(parse_tx_hash("0xabc").is_err());
    assert!(parse_tx_hash(&"ab".repeat(32)).is_err(), "needs 0x");
}

// ---- verification ----

struct RecordingHttp {
    status: u16,
    body: String,
    posts: RefCell<Vec<(String, serde_json::Value)>>,
}

impl crate::contract_reader::ExplorerHttp for RecordingHttp {
    fn get(&self, _url: &str) -> Result<(u16, String), String> {
        Err("unexpected GET".into())
    }
    fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<(u16, String), String> {
        self.posts
            .borrow_mut()
            .push((url.to_string(), body.clone()));
        Ok((self.status, self.body.clone()))
    }
}

fn req() -> VerifyRequest {
    VerifyRequest {
        address: ADDR.to_string(),
        standard_json: "{\"language\":\"Solidity\"}".to_string(),
        compiler_version: "0.8.36".to_string(),
        constructor_args_hex: Some("0xabcd".to_string()),
    }
}

#[test]
fn verification_posts_standard_json_to_the_explorer() {
    let http = RecordingHttp {
        status: 200,
        body: serde_json::json!({"guid": "vrf_1", "status": "pass", "matchType": "full",
            "contractName": "LemonDrops", "message": "ok"})
        .to_string(),
        posts: RefCell::new(Vec::new()),
    };
    let out = submit_verification(&http, &req()).unwrap();
    assert_eq!(out.status, VerifyStatus::Verified);
    assert_eq!(out.guid.as_deref(), Some("vrf_1"));
    let posts = http.posts.borrow();
    assert_eq!(
        posts[0].0,
        format!("{}/api/verify", crate::activity::EXPLORER_BASE)
    );
    let b = &posts[0].1;
    assert_eq!(b["address"], ADDR);
    assert_eq!(b["format"], "solidity-standard-json-input");
    assert_eq!(b["compilerVersion"], "0.8.36");
    assert_eq!(b["constructorArguments"], "abcd");
    assert_eq!(b["source"], "{\"language\":\"Solidity\"}");
}

#[test]
fn verification_outcomes_are_reported_honestly() {
    let partial = parse_verify_response(
        200,
        &serde_json::json!({"status": "pass", "matchType": "partial", "message": "m"}).to_string(),
    );
    assert_eq!(partial.status, VerifyStatus::Partial);
    let fail = parse_verify_response(
        422,
        &serde_json::json!({"status": "fail", "message": "bytecode mismatch"}).to_string(),
    );
    assert_eq!(fail.status, VerifyStatus::Failed);
    assert!(fail.message.contains("bytecode mismatch"));
    let rl = parse_verify_response(429, "{\"error\":\"rate limited\"}");
    assert_eq!(rl.status, VerifyStatus::Unavailable);
    assert!(rl.message.contains("rate limit"));
    let busy = parse_verify_response(503, "{\"error\":\"verifier busy\"}");
    assert_eq!(busy.status, VerifyStatus::Unavailable);
    let junk = parse_verify_response(200, "<html>");
    assert_eq!(junk.status, VerifyStatus::Unavailable);
}

#[test]
fn verification_inputs_are_checked_before_any_request() {
    let http = RecordingHttp {
        status: 200,
        body: "{}".into(),
        posts: RefCell::new(Vec::new()),
    };
    let mut bad = req();
    bad.compiler_version = "latest; rm".into();
    assert!(submit_verification(&http, &bad).is_err());
    let mut bad = req();
    bad.standard_json = "not json".into();
    assert!(submit_verification(&http, &bad).is_err());
    let mut bad = req();
    bad.constructor_args_hex = Some("0xzz".into());
    assert!(submit_verification(&http, &bad).is_err());
    assert!(http.posts.borrow().is_empty());
}

// ---- IPFS site pin ----

#[test]
fn site_files_are_collected_without_dependencies_and_need_an_index() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let files = collect_site_files(&p.app_dir.join("dist")).unwrap();
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["assets/app.js", "index.html"]);
    std::fs::remove_file(p.app_dir.join("dist/index.html")).unwrap();
    let e = collect_site_files(&p.app_dir.join("dist")).unwrap_err();
    assert!(e.contains("npm run build"), "{e}");
}

#[cfg(unix)]
#[test]
fn site_files_never_follow_symlinks() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let outside = d.path().join("secret.txt");
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, p.app_dir.join("dist/leak.txt")).unwrap();
    let e = collect_site_files(&p.app_dir.join("dist")).unwrap_err();
    assert!(e.contains("link"), "{e}");
}

#[test]
fn the_directory_upload_is_one_multipart_body_under_a_root_folder() {
    let files = vec![
        ("assets/app.js".to_string(), b"js".to_vec()),
        ("index.html".to_string(), b"<html>".to_vec()),
    ];
    let (boundary, body) = directory_multipart(&files);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains(&format!("--{boundary}\r\n")));
    assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    // The root folder and the nested folder are declared as directories.
    assert!(text.contains("filename=\"site\"\r\nContent-Type: application/x-directory"));
    assert!(text.contains("filename=\"site%2Fassets\"\r\nContent-Type: application/x-directory"));
    assert!(text.contains("filename=\"site%2Fassets%2Fapp.js\""));
    assert!(text.contains("filename=\"site%2Findex.html\""));
}

#[test]
fn the_site_cid_is_the_root_folder_line_of_the_add_stream() {
    let ndjson = "{\"Name\":\"site/index.html\",\"Hash\":\"bafyfile\",\"Size\":\"10\"}\n{\"Name\":\"site\",\"Hash\":\"bafyroot\",\"Size\":\"99\"}\n";
    assert_eq!(parse_add_root(ndjson).unwrap(), "bafyroot");
    assert!(parse_add_root("{\"Name\":\"other\",\"Hash\":\"x\"}").is_err());
    assert!(parse_add_root("").is_err());
}

#[test]
fn pin_links_name_the_local_and_public_gateways() {
    let pin = site_pin_result("bafyroot", 2, 8);
    assert_eq!(
        pin.local_gateway_url,
        "http://127.0.0.1:48080/ipfs/bafyroot/"
    );
    assert_eq!(pin.public_gateway_url, "https://ipfs.io/ipfs/bafyroot/");
    assert!(pin.note.contains("online"));
}

// ---- Vercel export ----

#[test]
fn vercel_export_needs_the_site_switched_to_40204_first() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let e = vercel_export(&p).unwrap_err();
    assert!(e.contains("40204"), "{e}");
}

#[test]
fn vercel_export_writes_a_deployable_directory() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    switch_site_to_citrate(&p, ADDR).unwrap();
    let out = vercel_export(&p).unwrap();
    let dir = std::path::PathBuf::from(&out.dir);
    assert!(dir.join("package.json").is_file());
    assert!(dir.join("src/main.tsx").is_file());
    assert!(
        !dir.join("node_modules").exists(),
        "dependencies are not copied"
    );
    assert!(!dir.join("dist").exists(), "Vercel builds it");
    assert!(
        !dir.join(".env.local").exists(),
        "local settings stay local"
    );
    let vj: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("vercel.json")).unwrap()).unwrap();
    assert_eq!(vj["outputDirectory"], "dist");
    assert_eq!(vj["framework"], "vite");
    let env = std::fs::read_to_string(dir.join(".env.production")).unwrap();
    assert!(env.contains("VITE_TARGET=citrate\n"));
    assert!(env.contains(&format!("VITE_CONTRACT_ADDRESS={ADDR_CK}\n")));
    assert!(out.commands.iter().any(|c| c.contains("vercel")));
    // Running it again replaces the earlier export.
    vercel_export(&p).unwrap();
}

#[test]
fn vercel_export_never_replaces_a_folder_it_did_not_write() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    switch_site_to_citrate(&p, ADDR).unwrap();
    let dir = p.root.join(EXPORT_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mine.txt"), "keep").unwrap();
    let e = vercel_export(&p).unwrap_err();
    assert!(e.contains("not written by Citrate"), "{e}");
    assert!(dir.join("mine.txt").is_file());
}

#[cfg(unix)]
#[test]
fn vercel_export_never_follows_a_link_out_of_the_app() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    switch_site_to_citrate(&p, ADDR).unwrap();
    let outside = d.path().join("secret.txt");
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, p.app_dir.join("src/leak.txt")).unwrap();
    let out = vercel_export(&p).unwrap();
    let dir = std::path::PathBuf::from(&out.dir);
    assert!(dir.join("src/main.tsx").is_file());
    assert!(
        std::fs::symlink_metadata(dir.join("src/leak.txt")).is_err(),
        "a link in app/ is skipped, never copied or followed"
    );
}

#[cfg(unix)]
#[test]
fn a_build_folder_that_is_itself_a_link_is_refused() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let elsewhere = d.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("index.html"), "<html></html>").unwrap();
    std::fs::remove_dir_all(p.app_dir.join("dist")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, p.app_dir.join("dist")).unwrap();
    let e = collect_site_files(&p.app_dir.join("dist")).unwrap_err();
    assert!(e.contains("link"), "{e}");
}

#[test]
fn a_build_over_the_file_bound_is_refused() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let many = p.app_dir.join("dist/many");
    std::fs::create_dir_all(&many).unwrap();
    for i in 0..MAX_SITE_FILES {
        std::fs::write(many.join(format!("f{i}.txt")), "x").unwrap();
    }
    let e = collect_site_files(&p.app_dir.join("dist")).unwrap_err();
    assert!(e.contains("too large"), "{e}");
}

/// A one-method RPC transport answering `eth_getCode` with fixed code.
struct CodeRpc(&'static str);

impl crate::rpc::RpcTransport for CodeRpc {
    fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, crate::rpc::RpcError> {
        assert_eq!(body["method"], "eth_getCode");
        Ok(serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": self.0}))
    }
}

#[test]
fn the_site_is_switched_only_when_the_address_holds_code() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let none = crate::rpc::RpcClient::with_transport(CodeRpc("0x"));
    let e = switch_site_checked(&none, &p, ADDR).unwrap_err();
    assert!(e.contains("no contract code"), "{e}");
    assert_eq!(site_contract(&p).unwrap(), None, "nothing was written");
    let code = crate::rpc::RpcClient::with_transport(CodeRpc("0x6080604052"));
    let sw = switch_site_checked(&code, &p, ADDR).unwrap();
    assert_eq!(sw.address, ADDR_CK);
    assert_eq!(site_contract(&p).unwrap().as_deref(), Some(ADDR));
}

// ---- forge standard JSON ----

#[test]
fn the_forge_verify_arguments_name_the_template_contract() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let args = forge_standard_json_args(&p, ADDR);
    assert_eq!(
        args,
        vec![
            "verify-contract".to_string(),
            ADDR.to_string(),
            "src/Token.sol:LemonDrops".to_string(),
            "--show-standard-json-input".to_string(),
        ]
    );
}

// ---- end to end against a local anvil (scripts/e2e-postdeploy-reader.sh) ----
//
// Skipped unless the script set CITRATE_E2E_RPC (a loopback anvil with the hello-mint contract
// deployed), CITRATE_E2E_PROJECT (the rendered project), CITRATE_E2E_ADDRESS and
// CITRATE_E2E_DEPLOY_TX. CITRATE_E2E_KUBO_API, when set, also pins the site to a throwaway kubo.

fn e2e_env() -> Option<(String, String, String, String)> {
    Some((
        std::env::var("CITRATE_E2E_RPC").ok()?,
        std::env::var("CITRATE_E2E_PROJECT").ok()?,
        std::env::var("CITRATE_E2E_ADDRESS").ok()?,
        std::env::var("CITRATE_E2E_DEPLOY_TX").ok()?,
    ))
}

/// ABI-decode a single `string` return.
fn abi_string(hex_out: &str) -> String {
    let b = hex::decode(hex_out.trim_start_matches("0x")).expect("hex");
    let len = u64::from_str_radix(&hex::encode(&b[56..64]), 16).expect("len") as usize;
    String::from_utf8(b[64..64 + len].to_vec()).expect("utf8")
}

fn abi_u256(hex_out: &str) -> u128 {
    u128::from_str_radix(hex_out.trim_start_matches("0x").trim_start_matches('0'), 16).unwrap_or(0)
}

#[test]
fn e2e_anvil_deploy_is_read_back_and_post_deploy_steps_run() {
    let Some((rpc, project_dir, address, tx)) = e2e_env() else {
        eprintln!("skipped: CITRATE_E2E_RPC is not set (run scripts/e2e-postdeploy-reader.sh)");
        return;
    };
    let target = crate::contract_reader::parse_target(Some(&rpc)).expect("loopback target");
    let client =
        crate::rpc::RpcClient::with_transport(crate::rpc::HttpTransport::new(target.rpc_url()));

    // The deploy receipt names the contract.
    let receipt = parse_receipt(
        &crate::contract_reader::rpc_raw(
            &client,
            "eth_getTransactionReceipt",
            serde_json::json!([tx]),
        )
        .expect("receipt"),
    )
    .expect("parses")
    .expect("mined");
    let addr = normalize_address(&address).expect("address");
    assert_eq!(receipt.contract_address.as_deref(), Some(addr.as_str()));

    // The reader logic reads it back.
    assert!(crate::contract_reader::code_size(&client, &addr).expect("code") > 0);
    let name = crate::contract_reader::view_call(&client, &addr, &[0x06, 0xfd, 0xde, 0x03])
        .expect("name()");
    assert_eq!(abi_string(&name), "Lemon Drops");
    let sel = |sig: &str| Keccak256::digest(sig.as_bytes())[..4].to_vec();
    let max = crate::contract_reader::view_call(&client, &addr, &sel("MAX_SUPPLY()"))
        .expect("MAX_SUPPLY()");
    assert_eq!(abi_u256(&max), 500);
    let minted = crate::contract_reader::view_call(&client, &addr, &sel("totalMinted()"))
        .expect("totalMinted()");
    assert_eq!(abi_u256(&minted), 0);
    // A write's gas estimate comes from the node (mint(1) at the template price).
    let price = abi_u256(
        &crate::contract_reader::view_call(&client, &addr, &sel("PRICE()")).expect("PRICE()"),
    );
    let mut mint = sel("mint(uint256)");
    mint.extend_from_slice(&[0u8; 31]);
    mint.push(1);
    let gas = crate::contract_reader::estimate_write_gas(
        &client,
        "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
        &addr,
        &mint,
        price,
    )
    .expect("estimate");
    assert!(gas > 21_000);

    // Post-deploy on the rendered project.
    let p = open_project(std::path::Path::new(&project_dir)).expect("project");
    let std_json = forge_standard_json(&p, &addr).expect("forge standard json");
    let v: serde_json::Value = serde_json::from_str(&std_json).expect("json");
    assert_eq!(v["language"], "Solidity");
    switch_site_to_citrate(&p, &addr).expect("switch");
    assert_eq!(site_contract(&p).expect("read"), Some(addr.clone()));
    let exp = vercel_export(&p).expect("export");
    assert!(std::path::Path::new(&exp.dir).join("vercel.json").is_file());

    if let Ok(api) = std::env::var("CITRATE_E2E_KUBO_API") {
        let pin = pin_site(&p, &api).expect("pin");
        assert!(pin.cid.starts_with("bafy"), "{}", pin.cid);
        // The CID is the site folder: index.html and assets/ at its root.
        let ls = ureq::post(&format!("{api}/api/v0/ls?arg={}", pin.cid))
            .send_empty()
            .expect("ls")
            .into_body()
            .read_to_string()
            .expect("ls body");
        let v: serde_json::Value = serde_json::from_str(&ls).expect("ls json");
        let names: Vec<&str> = v["Objects"][0]["Links"]
            .as_array()
            .expect("links")
            .iter()
            .filter_map(|l| l["Name"].as_str())
            .collect();
        assert_eq!(names, ["assets", "index.html"]);
        eprintln!("e2e: pinned site CID {}", pin.cid);
    }
}

// ---- verify runs forge with a pinned compiler and a minimal environment ----

#[test]
fn verify_runs_forge_with_the_lock_files_compiler_and_a_minimal_env() {
    let d = project();
    let p = open_project(d.path()).expect("project");
    let env = forge_verify_env(&p).expect("env");
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("FOUNDRY_SOLC"), Some("0.8.36"), "the compiler is the lock file's version");
    for (k, _) in &env {
        assert!(
            ["PATH", "HOME", "TMPDIR", "FOUNDRY_SOLC"].contains(&k.as_str()),
            "{k} must not reach forge"
        );
    }
}

#[test]
fn verify_refuses_a_project_whose_build_config_could_run_other_programs() {
    // A compiler given as a path in foundry.toml.
    let d = project();
    std::fs::write(
        d.path().join("contracts/foundry.toml"),
        "[profile.default]\nsolc = \"/tmp/evil-solc\"\n",
    )
    .unwrap();
    let p = open_project(d.path()).expect("project");
    let e = forge_verify_preflight(&p).expect_err("refused");
    assert!(e.contains("compiler"), "{e}");
    // A version string is fine.
    std::fs::write(
        d.path().join("contracts/foundry.toml"),
        "[profile.default]\nsolc = \"0.8.36\"\n",
    )
    .unwrap();
    assert!(forge_verify_preflight(&p).is_ok());
    // An env file forge would load.
    std::fs::write(d.path().join("contracts/.env"), "FOUNDRY_SOLC=/tmp/evil\n").unwrap();
    assert!(forge_verify_preflight(&p).expect_err(".env").contains(".env"));
    std::fs::remove_file(d.path().join("contracts/.env")).unwrap();
    // An unreadable config is refused, never skipped.
    std::fs::write(d.path().join("contracts/foundry.toml"), "solc = [").unwrap();
    assert!(forge_verify_preflight(&p).is_err());
    // A lock file whose compiler is not a version is refused.
    let mut p2 = open_project(d.path()).expect("project");
    p2.solc = "../../bin/solc".into();
    assert!(forge_verify_env(&p2).is_err());
}

// ---- HUP-S6: the gated ABI the ceremony decodes calls from ----

/// Runtime code of the fixture contract (any bytes; only equality and embedding are checked).
const RUNTIME_HEX: &str = "6080604052348015600e575f5ffd5b50";
/// Constructor code in front of the runtime, as solc lays out a contract without immutables.
const CTOR_HEX: &str = "6080604052600a600c";

/// Write `contracts/out/Token.sol/LemonDrops.json` with `mint(uint256)` (payable) in its ABI.
fn write_artifact(p: &HelloMintProject, init_hex: &str, runtime_hex: &str, immutables: bool) {
    let dir = p.contracts_dir.join("out/Token.sol");
    std::fs::create_dir_all(&dir).unwrap();
    let refs = if immutables {
        serde_json::json!({"7": [{"start": 1, "length": 32}]})
    } else {
        serde_json::json!({})
    };
    let art = serde_json::json!({
        "abi": [{"type": "function", "name": "mint", "stateMutability": "payable",
            "inputs": [{"name": "quantity", "type": "uint256"}], "outputs": []}],
        "bytecode": {"object": format!("0x{init_hex}")},
        "deployedBytecode": {"object": format!("0x{runtime_hex}"), "immutableReferences": refs},
    });
    std::fs::write(dir.join("LemonDrops.json"), art.to_string()).unwrap();
}

fn gate_record(init_hex: &str, verdict: crate::deploy_gate::Verdict) -> crate::deploy_gate::GateRecord {
    crate::deploy_gate::GateRecord {
        initcode_hash: crate::deploy_gate::initcode_hash(&hex::decode(init_hex).unwrap()),
        binding_hash: "0xbb".into(),
        compiler: crate::deploy_gate::CompilerSettings {
            solc_version: "0.8.36".into(),
            optimizer: true,
            optimizer_runs: 200,
            evm_version: "cancun".into(),
            via_ir: false,
        },
        verdict,
        items: vec![],
        evaluated_at_ms: 1,
    }
}

fn mint_intent() -> crate::ceremony::SignatureIntent {
    let mut data = "a0712d68".to_string(); // mint(uint256)
    data.push_str(&format!("{:064x}", 1));
    crate::ceremony::SignatureIntent {
        origin: "hello-mint page".into(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": "0x1111111111111111111111111111111111111111",
            "to": ADDR, "value": "0x5", "data": format!("0x{data}"),
            "gas": "0x7a120", "chainId": "0x9d0c",
        })
        .to_string(),
    }
}

#[test]
fn the_ceremony_decodes_the_mint_only_for_the_ready_gated_build_on_chain() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let init = format!("{CTOR_HEX}{RUNTIME_HEX}");
    write_artifact(&p, &init, RUNTIME_HEX, false);
    let on_chain: &'static str = Box::leak(format!("0x{RUNTIME_HEX}").into_boxed_str());
    let client = crate::rpc::RpcClient::with_transport(CodeRpc(on_chain));
    let cer = crate::ceremony::SignatureCeremony::new();
    let gate = crate::deploy_gate::GateStore::default();

    // No gate record: nothing registered, the mint stays raw-ack gated.
    let e = register_gated_abi(&client, &p, ADDR, &gate, &cer).unwrap_err();
    assert!(e.contains("no READY"), "{e}");
    gate.record(gate_record(&init, crate::deploy_gate::Verdict::NotReady)).unwrap();
    let e = register_gated_abi(&client, &p, ADDR, &gate, &cer).unwrap_err();
    assert!(e.contains("no READY"), "{e}");
    assert_eq!(cer.gated_contracts(), 0);
    assert!(cer.request(mint_intent()).requires_raw_ack);

    // READY and the code on chain is the artifact's runtime: the mint is decoded by name.
    gate.record(gate_record(&init, crate::deploy_gate::Verdict::Ready)).unwrap();
    register_gated_abi(&client, &p, ADDR, &gate, &cer).unwrap();
    assert_eq!(cer.gated_contracts(), 1);
    let v = cer.request(mint_intent());
    assert!(!v.requires_raw_ack);
    assert!(v.decoded.action.contains("mint(quantity=1)"), "{}", v.decoded.action);
}

#[test]
fn other_code_on_chain_or_immutables_are_never_registered() {
    let d = project();
    let p = open_project(d.path()).unwrap();
    let init = format!("{CTOR_HEX}{RUNTIME_HEX}");
    let gate = crate::deploy_gate::GateStore::default();
    gate.record(gate_record(&init, crate::deploy_gate::Verdict::Ready)).unwrap();
    let cer = crate::ceremony::SignatureCeremony::new();

    write_artifact(&p, &init, RUNTIME_HEX, false);
    let other = crate::rpc::RpcClient::with_transport(CodeRpc("0x6080604052deadbeef"));
    let e = register_gated_abi(&other, &p, ADDR, &gate, &cer).unwrap_err();
    assert!(e.contains("not the gated build"), "{e}");

    write_artifact(&p, &init, RUNTIME_HEX, true);
    let on_chain: &'static str = Box::leak(format!("0x{RUNTIME_HEX}").into_boxed_str());
    let same = crate::rpc::RpcClient::with_transport(CodeRpc(on_chain));
    let e = register_gated_abi(&same, &p, ADDR, &gate, &cer).unwrap_err();
    assert!(e.contains("immutables"), "{e}");
    assert_eq!(cer.gated_contracts(), 0);
}

#[test]
fn a_runtime_that_the_ready_init_code_does_not_carry_is_never_registered() {
    // An artifact whose creation code is a READY build but whose runtime field names some other
    // contract's code (the one at the address): the ABI must not be trusted for that address.
    let d = project();
    let p = open_project(d.path()).unwrap();
    let init = format!("{CTOR_HEX}{RUNTIME_HEX}");
    let foreign = "6080604052deadbeefcafe";
    write_artifact(&p, &init, foreign, false);
    let gate = crate::deploy_gate::GateStore::default();
    gate.record(gate_record(&init, crate::deploy_gate::Verdict::Ready)).unwrap();
    let cer = crate::ceremony::SignatureCeremony::new();
    let on_chain: &'static str = Box::leak(format!("0x{foreign}").into_boxed_str());
    let client = crate::rpc::RpcClient::with_transport(CodeRpc(on_chain));
    let e = register_gated_abi(&client, &p, ADDR, &gate, &cer).unwrap_err();
    assert!(e.contains("not the gated build"), "{e}");
    assert_eq!(cer.gated_contracts(), 0);
    assert!(cer.request(mint_intent()).requires_raw_ack);
}
