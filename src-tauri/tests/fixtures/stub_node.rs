//! Cross-platform native node fixture for the NodeManager unit tests.
//!
//! This executable mirrors only the process contract those tests exercise: it
//! accepts `--data-dir`, requires the storage-key environment variable, writes
//! non-plaintext bytes plus the encryption marker, and remains alive until the
//! supervisor terminates it. It is test-only and never enters a production path.

use std::path::PathBuf;
use std::time::Duration;

const STORAGE_KEY_ENV: &str = "CITRATE_STORAGE_KEY";
const SENTINEL: &[u8] = b"CITRATE_PLAINTEXT_SENTINEL_v1";

fn main() {
    if let Err(message) = run() {
        eprintln!("stub_node: {message}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let data_dir = data_dir_from_args(std::env::args().skip(1))?;
    let key_hex = std::env::var(STORAGE_KEY_ENV)
        .map_err(|_| format!("{STORAGE_KEY_ENV} env required"))?;
    let key = decode_hex(&key_hex)?;

    std::fs::create_dir_all(&data_dir)
        .map_err(|error| format!("create data directory: {error}"))?;
    let ciphertext: Vec<u8> = SENTINEL
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect();
    std::fs::write(data_dir.join("data.rocks"), ciphertext)
        .map_err(|error| format!("write ciphertext fixture: {error}"))?;
    std::fs::write(
        data_dir.join("encryption.meta"),
        br#"{"cipher":"stub-xor","value_format":1}"#,
    )
    .map_err(|error| format!("write encryption marker: {error}"))?;

    loop {
        std::thread::sleep(Duration::from_secs(3_600));
    }
}

fn data_dir_from_args(args: impl Iterator<Item = String>) -> Result<PathBuf, String> {
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        if argument == "--data-dir" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--data-dir requires a value".to_string());
        }
    }
    Err("--data-dir required".to_string())
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() || value.len() % 2 != 0 {
        return Err(format!("{STORAGE_KEY_ENV} must be non-empty even-length hex"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)
                .map_err(|_| format!("{STORAGE_KEY_ENV} must contain ASCII hex"))?;
            u8::from_str_radix(text, 16)
                .map_err(|_| format!("{STORAGE_KEY_ENV} contains invalid hex"))
        })
        .collect()
}
