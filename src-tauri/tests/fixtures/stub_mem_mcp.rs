// Native test fixture for MemoryDomain supervisor and encrypted-store tests.
//
// This is compiled directly with rustc by memory_tests.rs, so it deliberately
// uses only the standard library. The JSON-RPC transport has a separate
// in-process cross-platform integration test; this process exercises the
// sidecar CLI/env contract, fixture store creation, stale-path cleanup, and
// supervised lifetime without requiring a shell or Python on the test host.

use std::path::{Path, PathBuf};
use std::time::Duration;

const KEY_ENV: &str = "CITRATE_MEM_STORE_KEY";
const SENTINEL: &[u8] = b"CITRATE_MEM_PLAINTEXT_SENTINEL_v1";

fn main() {
    if let Err(error) = run() {
        eprintln!("stub_mem_mcp: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (store_dir, socket_path) = parse_args()?;
    let key_hex = std::env::var(KEY_ENV).map_err(|_| format!("{KEY_ENV} env required"))?;
    let key = decode_key(&key_hex)?;

    std::fs::create_dir_all(&store_dir)
        .map_err(|error| format!("create store directory {}: {error}", store_dir.display()))?;
    let ciphertext = SENTINEL
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect::<Vec<_>>();
    std::fs::write(store_dir.join("data.enc"), ciphertext)
        .map_err(|error| format!("write data.enc: {error}"))?;
    std::fs::write(
        store_dir.join("encryption.meta"),
        br#"{"cipher":"stub-xor","value_format":1}"#,
    )
    .map_err(|error| format!("write encryption.meta: {error}"))?;

    remove_stale_endpoint(&socket_path)?;
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let mut args = std::env::args_os().skip(1);
    let store_dir = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: stub_mem_mcp <store-path> <socket-path>".to_string())?;
    let socket_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: stub_mem_mcp <store-path> <socket-path>".to_string())?;
    if args.next().is_some() {
        return Err("usage: stub_mem_mcp <store-path> <socket-path>".to_string());
    }
    Ok((store_dir, socket_path))
}

fn decode_key(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err(format!("{KEY_ENV} must contain exactly 32 hex-encoded bytes"));
    }

    let mut key = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = decode_nibble(pair[0])?;
        let low = decode_nibble(pair[1])?;
        key[index] = (high << 4) | low;
    }
    Ok(key)
}

fn decode_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("{KEY_ENV} contains non-hexadecimal characters")),
    }
}

fn remove_stale_endpoint(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove stale endpoint {}: {error}", path.display())),
    }
}
