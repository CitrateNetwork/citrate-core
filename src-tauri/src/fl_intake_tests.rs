// HUP-S9.4 (n5) — a round's result on this device: the FL_ROUND_V1 bundle, the merged adapter it
// names, and fetching both over https. Written red-first (the module body was absent).
//
// The HTTP server here is a local fixture (std TcpListener on 127.0.0.1) that serves fixed bytes
// per path. It exists only in this test file; production fetches the URL the member gives.

use super::*;
use rand::RngCore;
use std::io::{Read, Write};
use std::net::TcpListener;

fn tmpdir(tag: &str) -> PathBuf {
    let mut d = std::env::temp_dir();
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    d.push(format!("n5-fl-intake-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn h32(byte: &str) -> String {
    format!("0x{}", byte.repeat(32))
}

fn sha_hex(b: &[u8]) -> String {
    hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b))
}

/// The bundle shape citrate-compute-pool's `citrate-fl-round` writes (FL_ROUND_V1 §6), with the
/// fields core reads. `participants` and the hash lists are shortened to what the checks need.
fn bundle(adapter_sha: &str, base_sha: &str, participants: usize, min: u16) -> serde_json::Value {
    let roster: Vec<String> = (0..participants.max(3))
        .map(|i| format!("0x{}", format!("{:02x}", 0x10 + i).repeat(20)))
        .collect();
    let parts: Vec<serde_json::Value> = (0..participants)
        .map(|i| {
            serde_json::json!({
                "worker": roster[i],
                "job": format!("fl-x-{i}"),
                "delta_root": h32("aa"),
                "delta_sha256": h32("bb"),
                "result": { "task": "lora_delta" }
            })
        })
        .collect();
    serde_json::json!({
        "version": "citrate-fl-round-bundle/1",
        "ordinal": 0,
        "round_id": h32("29"),
        "config_hash": h32("9b"),
        "config": {
            "chain_id": 1337,
            "ledger": format!("0x{}", "76".repeat(20)),
            "cluster_id": h32("09"),
            "base_model_sha256": format!("0x{base_sha}"),
            "start_adapter_sha256": h32("55"),
            "roster": roster,
            "min_participants": min,
            "chunk_dim": 1024,
            "value_scale_log2": 8,
            "threshold_pos": 32768,
            "threshold_neg": -32768,
            "confidence": "nonzero",
            "weight": "uniform",
            "max_values": 67108864u64
        },
        "participants": parts,
        "n_values": 2049,
        "chunks": 3,
        "input_hashes": [h32("01"), h32("02"), h32("03")],
        "output_hashes": [h32("04"), h32("05"), h32("06")],
        "participants_root": h32("64"),
        "input_root": h32("06"),
        "output_root": h32("5e"),
        "adapter_sha256": format!("0x{adapter_sha}"),
        "record_digest": h32("69"),
        "state_counts": [10, 2000, 0, 39],
        "excluded": []
    })
}

const ADAPTER: &[u8] = b"GGUF\x03\x00\x00\x00merged-adapter";
const BASE: &str = "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2";

#[test]
fn parses_the_round_bundle_and_keeps_only_typed_fields() {
    let a = sha_hex(ADAPTER);
    let b = parse_bundle(&bundle(&a, BASE, 3, 3).to_string()).unwrap();
    assert_eq!(b.adapter_sha256, a);
    assert_eq!(b.base_model_sha256, BASE);
    assert_eq!(b.participants, 3);
    assert_eq!(b.min_participants, 3);
    assert_eq!(b.chain_id, 1337);
    assert_eq!(b.state_counts, [10, 2000, 0, 39]);
    assert_eq!(b.round_id, "29".repeat(32));
}

#[test]
fn refuses_a_bundle_that_is_not_a_v1_round_or_is_inconsistent() {
    let a = sha_hex(ADAPTER);
    let mut v = bundle(&a, BASE, 3, 3);
    v["version"] = serde_json::json!("citrate-fl-round-bundle/2");
    assert!(parse_bundle(&v.to_string()).unwrap_err().contains("version"));
    // fewer contributions than the round's own minimum
    assert!(parse_bundle(&bundle(&a, BASE, 2, 3).to_string()).unwrap_err().contains("participants"));
    // below the FL_ROUND_V1 floor of three devices
    assert!(parse_bundle(&bundle(&a, BASE, 2, 2).to_string()).unwrap_err().contains("three"));
    // chunks must be ceil(n_values / chunk_dim) and match the hash lists
    let mut v = bundle(&a, BASE, 3, 3);
    v["chunks"] = serde_json::json!(4);
    assert!(parse_bundle(&v.to_string()).unwrap_err().contains("chunks"));
    let mut v = bundle(&a, BASE, 3, 3);
    v["input_hashes"] = serde_json::json!([h32("01")]);
    assert!(parse_bundle(&v.to_string()).unwrap_err().contains("chunks"));
    // malformed hashes
    let mut v = bundle(&a, BASE, 3, 3);
    v["adapter_sha256"] = serde_json::json!("0x1234");
    assert!(parse_bundle(&v.to_string()).is_err());
    let mut v = bundle(&a, BASE, 3, 3);
    v["config"]["ledger"] = serde_json::json!("0x12");
    assert!(parse_bundle(&v.to_string()).is_err());
    // not JSON / too large
    assert!(parse_bundle("{").is_err());
    let huge = "x".repeat(MAX_BUNDLE_BYTES as usize + 1);
    assert!(parse_bundle(&huge).unwrap_err().contains("too large"));
}

#[test]
fn checking_a_round_binds_the_adapter_file_and_the_served_base() {
    let d = tmpdir("check");
    let adapter = d.join("merged.gguf");
    std::fs::write(&adapter, ADAPTER).unwrap();
    let a = sha_hex(ADAPTER);
    let b = parse_bundle(&bundle(&a, BASE, 3, 3).to_string()).unwrap();
    let p = check_round(&b, &adapter, BASE, NOW).unwrap();
    assert_eq!(p.adapter_sha256, a);
    assert_eq!(p.round_id, "29".repeat(32));
    assert_eq!(p.checked_at_ms, NOW);
    // Honest about what was not checked.
    assert!(p.chain_record.contains("not checked"), "{}", p.chain_record);
    assert!(p.notes.iter().any(|n| n.contains("1337")), "a non-40204 round says so: {:?}", p.notes);
    // the adapter file is not the round's adapter
    let other = d.join("other.gguf");
    std::fs::write(&other, b"GGUF\x03\x00\x00\x00another").unwrap();
    assert!(check_round(&b, &other, BASE, NOW).unwrap_err().contains("does not match"));
    // not GGUF
    let bad = d.join("bad.gguf");
    std::fs::write(&bad, b"NOPE").unwrap();
    let bb = parse_bundle(&bundle(&sha_hex(b"NOPE"), BASE, 3, 3).to_string()).unwrap();
    assert!(check_round(&bb, &bad, BASE, NOW).unwrap_err().contains("GGUF"));
    // trained for another base model
    let err = check_round(&b, &adapter, &"0".repeat(64), NOW).unwrap_err();
    assert!(err.contains("base model"), "{err}");
}

#[test]
fn the_served_model_is_hashed_and_a_changed_file_is_hashed_again() {
    let d = tmpdir("basehash");
    let g = d.join("custom.gguf");
    std::fs::write(&g, b"custom model bytes").unwrap();
    assert_eq!(served_model_sha256(&g).unwrap(), sha_hex(b"custom model bytes"));
    // Same path, different length: the cache does not answer with the old hash.
    std::fs::write(&g, b"custom model bytes, replaced").unwrap();
    assert_eq!(served_model_sha256(&g).unwrap(), sha_hex(b"custom model bytes, replaced"));
    assert!(served_model_sha256(&d.join("missing.gguf")).is_err());
}

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

/// Serves `routes` (path -> (status, extra header, body)); anything else is 404.
fn server(routes: Vec<(&'static str, u16, &'static str, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            let hit = routes.iter().find(|r| r.0 == path);
            let (code, extra, body) = match hit {
                Some((_, c, e, b)) => (*c, *e, b.clone()),
                None => (404, "", b"no".to_vec()),
            };
            let head = format!(
                "HTTP/1.1 {code} X\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn fetch_url_policy() {
    assert!(check_fetch_url("https://mirror.example.org/a.gguf").is_ok());
    assert!(check_fetch_url("https://mirror.example.org/a.gguf?sig=1").is_ok());
    assert!(check_fetch_url("http://127.0.0.1:9/a.gguf").is_ok());
    assert!(check_fetch_url("http://mirror.example.org/a.gguf").is_err());
    assert!(check_fetch_url("https://u:p@mirror.example.org/a.gguf").is_err());
    assert!(check_fetch_url("https://mirror.example.org/a.gguf#x").is_err());
    assert!(check_fetch_url("file:///etc/passwd").is_err());
    assert!(check_fetch_url("not a url").is_err());
}

#[test]
fn fetches_an_adapter_by_hash_and_keeps_only_the_verified_bytes() {
    let base = server(vec![
        ("/a.gguf", 200, "", ADAPTER.to_vec()),
        ("/redirect", 302, "location: /a.gguf\r\n", vec![]),
        ("/notgguf", 200, "", b"NOPE".to_vec()),
    ]);
    let dir = tmpdir("fetch");
    let a = sha_hex(ADAPTER);
    let got = fetch_adapter(&format!("{base}/a.gguf"), &a, &dir, 1024).unwrap();
    assert_eq!(got, dir.join(format!("{a}.gguf")));
    assert_eq!(std::fs::read(&got).unwrap(), ADAPTER);
    // wrong expected hash: refused, nothing left behind
    let dir2 = tmpdir("fetch-bad");
    let err = fetch_adapter(&format!("{base}/a.gguf"), &"0".repeat(64), &dir2, 1024).unwrap_err();
    assert!(err.contains("does not match"), "{err}");
    assert_eq!(std::fs::read_dir(&dir2).unwrap().count(), 0);
    // over the size cap
    let err = fetch_adapter(&format!("{base}/a.gguf"), &a, &dir2, 8).unwrap_err();
    assert!(err.contains("larger"), "{err}");
    assert_eq!(std::fs::read_dir(&dir2).unwrap().count(), 0);
    // redirects are not followed
    assert!(fetch_adapter(&format!("{base}/redirect"), &a, &dir2, 1024).is_err());
    // not GGUF
    let err = fetch_adapter(&format!("{base}/notgguf"), &sha_hex(b"NOPE"), &dir2, 1024).unwrap_err();
    assert!(err.contains("GGUF"), "{err}");
    // 404
    assert!(fetch_adapter(&format!("{base}/missing"), &a, &dir2, 1024).unwrap_err().contains("404"));
    assert_eq!(std::fs::read_dir(&dir2).unwrap().count(), 0);
}

#[test]
fn fetches_a_round_bundle_then_its_adapter() {
    let a = sha_hex(ADAPTER);
    let body = bundle(&a, BASE, 3, 3).to_string().into_bytes();
    let base = server(vec![("/round.json", 200, "", body), ("/merged.gguf", 200, "", ADAPTER.to_vec())]);
    let dir = tmpdir("fetch-round");
    let (b, path) = fetch_round(&format!("{base}/round.json"), &format!("{base}/merged.gguf"), &dir, 1024).unwrap();
    assert_eq!(b.adapter_sha256, a);
    assert_eq!(std::fs::read(path).unwrap(), ADAPTER);
    // an adapter URL that serves other bytes is refused against the bundle's hash
    let base2 = server(vec![
        ("/round.json", 200, "", bundle(&a, BASE, 3, 3).to_string().into_bytes()),
        ("/merged.gguf", 200, "", b"GGUF\x03\x00\x00\x00other".to_vec()),
    ]);
    let dir2 = tmpdir("fetch-round-bad");
    let err = fetch_round(&format!("{base2}/round.json"), &format!("{base2}/merged.gguf"), &dir2, 1024).unwrap_err();
    assert!(err.contains("does not match"), "{err}");
}

const NOW: u64 = 1_790_000_000_000;

/// Real-round check (manual): run with
/// `CITRATE_FL_BUNDLE=<bundle.json> CITRATE_FL_ADAPTER=<merged.gguf> CITRATE_FL_BASE=<base.gguf>
///  cargo test --lib fl_intake::tests::a_real_devnet_round -- --ignored --nocapture`.
#[test]
#[ignore = "needs a real round bundle, its merged adapter and the base model on disk"]
fn a_real_devnet_round_checks_against_the_real_base_model() {
    let bundle_path = std::env::var("CITRATE_FL_BUNDLE").unwrap();
    let adapter = PathBuf::from(std::env::var("CITRATE_FL_ADAPTER").unwrap());
    let base = PathBuf::from(std::env::var("CITRATE_FL_BASE").unwrap());
    let b = parse_bundle(&std::fs::read_to_string(bundle_path).unwrap()).unwrap();
    let base_sha = served_model_sha256(&base).unwrap();
    let p = check_round(&b, &adapter, &base_sha, NOW).unwrap();
    println!("{}", serde_json::to_string_pretty(&p).unwrap());
}
