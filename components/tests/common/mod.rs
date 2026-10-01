//! Shared test helpers: a minisign-format test key generated per test, a fetcher that serves
//! files from a local directory, and archive builders. Test-only; nothing here ships.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use blake2::Digest as _;
use citrate_components::error::ComponentError;
use citrate_components::fetch::Fetcher;
use ed25519_dalek::Signer as _;

/// A throwaway minisign-format Ed25519 key, generated fresh in each test.
pub struct TestKey {
    sk: ed25519_dalek::SigningKey,
    key_id: [u8; 8],
}

impl TestKey {
    pub fn generate() -> Self {
        let seed: [u8; 32] = rand::random();
        let key_id: [u8; 8] = rand::random();
        Self {
            sk: ed25519_dalek::SigningKey::from_bytes(&seed),
            key_id,
        }
    }

    /// The public key line minisign writes (base64 of `Ed` ‖ key id ‖ public key).
    pub fn public_b64(&self) -> String {
        let mut bin = Vec::with_capacity(42);
        bin.extend_from_slice(b"Ed");
        bin.extend_from_slice(&self.key_id);
        bin.extend_from_slice(self.sk.verifying_key().as_bytes());
        base64::engine::general_purpose::STANDARD.encode(bin)
    }

    /// A prehashed (`ED`) minisign signature with the given trusted comment.
    pub fn sign(&self, data: &[u8], trusted_comment: &str) -> String {
        let h = blake2::Blake2b512::digest(data);
        self.sign_hash(&h, trusted_comment, *b"ED")
    }

    /// A legacy (`Ed`, not prehashed) minisign signature.
    pub fn sign_legacy(&self, data: &[u8], trusted_comment: &str) -> String {
        self.sign_hash(data, trusted_comment, *b"Ed")
    }

    fn sign_hash(&self, msg: &[u8], trusted_comment: &str, alg: [u8; 2]) -> String {
        let sig = self.sk.sign(msg).to_bytes();
        let mut bin1 = Vec::with_capacity(74);
        bin1.extend_from_slice(&alg);
        bin1.extend_from_slice(&self.key_id);
        bin1.extend_from_slice(&sig);
        let mut global = Vec::new();
        global.extend_from_slice(&sig);
        global.extend_from_slice(trusted_comment.as_bytes());
        let gsig = self.sk.sign(&global).to_bytes();
        let b64 = base64::engine::general_purpose::STANDARD;
        format!(
            "untrusted comment: signature from a test key\n{}\ntrusted comment: {}\n{}\n",
            b64.encode(bin1),
            trusted_comment,
            b64.encode(gsig)
        )
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(data))
}

/// Serves `https://fixture.test/<name>` from files registered in memory, written to disk.
pub struct DirFetcher {
    pub dir: PathBuf,
    pub served: std::cell::RefCell<Vec<String>>,
}

impl DirFetcher {
    pub fn new(dir: &Path) -> Self {
        std::fs::create_dir_all(dir).unwrap();
        Self {
            dir: dir.to_path_buf(),
            served: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Registers `bytes` and returns its URL.
    pub fn put(&self, name: &str, bytes: &[u8]) -> String {
        std::fs::write(self.dir.join(name), bytes).unwrap();
        format!("https://fixture.test/{name}")
    }
}

impl Fetcher for DirFetcher {
    fn fetch(
        &self,
        url: &str,
        max_bytes: u64,
        sink: &mut dyn Write,
    ) -> Result<u64, ComponentError> {
        self.served.borrow_mut().push(url.to_string());
        let name = url
            .strip_prefix("https://fixture.test/")
            .ok_or_else(|| ComponentError::Fetch(format!("not a fixture url: {url}")))?;
        let f = std::fs::File::open(self.dir.join(name))
            .map_err(|e| ComponentError::Fetch(e.to_string()))?;
        citrate_components::fetch::copy_capped(f, sink, max_bytes)
    }
}

/// One archive entry for [`tar_gz`] / [`tar_xz`].
pub enum Entry<'a> {
    File(&'a str, &'a [u8], u32),
    Dir(&'a str),
    Symlink(&'a str, &'a str),
    Hardlink(&'a str, &'a str),
    /// A raw header path written byte for byte (to smuggle `..` or absolute paths past the
    /// builder's own checks).
    RawPath(&'a str, &'a [u8]),
}

pub fn tar_bytes(entries: &[Entry]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for e in entries {
        match e {
            Entry::File(p, data, mode) => {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(*mode);
                h.set_entry_type(tar::EntryType::Regular);
                b.append_data(&mut h, p, *data).unwrap();
            }
            Entry::Dir(p) => {
                let mut h = tar::Header::new_gnu();
                h.set_size(0);
                h.set_mode(0o755);
                h.set_entry_type(tar::EntryType::Directory);
                b.append_data(&mut h, p, std::io::empty()).unwrap();
            }
            Entry::Symlink(p, target) => {
                let mut h = tar::Header::new_gnu();
                h.set_size(0);
                h.set_mode(0o777);
                h.set_entry_type(tar::EntryType::Symlink);
                b.append_link(&mut h, p, target).unwrap();
            }
            Entry::Hardlink(p, target) => {
                let mut h = tar::Header::new_gnu();
                h.set_size(0);
                h.set_mode(0o644);
                h.set_entry_type(tar::EntryType::Link);
                b.append_link(&mut h, p, target).unwrap();
            }
            Entry::RawPath(p, data) => {
                let mut h = tar::Header::new_old();
                {
                    let name = &mut h.as_old_mut().name;
                    name[..p.len()].copy_from_slice(p.as_bytes());
                }
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_entry_type(tar::EntryType::Regular);
                h.set_cksum();
                b.append(&h, *data).unwrap();
            }
        }
    }
    b.into_inner().unwrap()
}

pub fn tar_gz(entries: &[Entry]) -> Vec<u8> {
    let raw = tar_bytes(entries);
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap()
}

pub fn tar_xz(entries: &[Entry]) -> Vec<u8> {
    let raw = tar_bytes(entries);
    let mut out = Vec::new();
    lzma_rs::xz_compress(&mut std::io::Cursor::new(raw), &mut out).unwrap();
    out
}

/// A fresh, empty directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let n: u64 = rand::random();
        let p = std::env::temp_dir().join(format!("citrate-components-test-{tag}-{n:016x}"));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub const NOW: u64 = 1_790_000_000; // 2026-09 (fixed clock for the tests)
pub const DAY: u64 = 86_400;

/// A one-component manifest JSON for `name`/`version` whose single artifact (for `platform`)
/// is `artifact`, served by `fetcher` and signed by `key`.
#[allow(clippy::too_many_arguments)]
pub fn manifest_json(
    key: &TestKey,
    fetcher: &DirFetcher,
    sequence: u64,
    name: &str,
    version: &str,
    platform: &str,
    format: &str,
    artifact: &[u8],
    entrypoints: &[&str],
) -> String {
    let file = format!("{name}-{version}-{platform}.{}", format.replace('.', "-"));
    let url = fetcher.put(&file, artifact);
    let sig = key.sign(
        artifact,
        &format!("citrate-components-artifact {name} {version}"),
    );
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        platform.to_string(),
        serde_json::json!({
            "url": url,
            "sha256": sha256_hex(artifact),
            "size": artifact.len(),
            "format": format,
            "signature": sig,
        }),
    );
    serde_json::json!({
        "schema": 1,
        "channel": "stable",
        "sequence": sequence,
        "issued_at": NOW - DAY,
        "expires_at": NOW + 6 * DAY,
        "components": [{
            "name": name,
            "version": version,
            "kind": "toolchain",
            "license": "MIT",
            "entrypoints": entrypoints,
            "artifacts": artifacts,
        }],
    })
    .to_string()
}

pub fn sign_manifest(key: &TestKey, json: &str) -> String {
    key.sign(json.as_bytes(), "citrate-components-manifest seq")
}
