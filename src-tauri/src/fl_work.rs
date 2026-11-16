//! citrate-core 0.5.4 — **Settings › Work: the member's worker terms and HIC-2 envelope, sealed on
//! this device** (FL design board 11, core #683).
//!
//! Two records, each one sealed file:
//!
//! ```text
//! <app data>/fl-work/
//!   terms.sealed      WorkerTerms: floor (SALT per unit), hours, weekdays only, mains only,
//!                     the data classes rounds may read
//!   envelope.sealed   the HIC-2 envelope: on or off, its name, its caps (joins per week, machine
//!                     hours per round), when the member approved it, and this week's use
//! file = MAGIC "CITRFLW\0" | version (1) | key generation (u32-be) | envelope
//! AAD  = MAGIC | version | generation | "|" | file name stem
//! ```
//!
//! **Crypto: the same at-rest construction as the sealed Hermes history (`hermes_history.rs`,
//! #679), ported from `citrate-homestead/crates/homestead-crypto`:** a fresh 32-byte DEK per file
//! under AES-256-GCM, wrapped by a KEK from HKDF-SHA256 over an ML-KEM-768 (FIPS 203) shared secret
//! encapsulated to this device's work key. Only the HKDF info string differs, for domain
//! separation. The work key is a 64-byte ML-KEM-768 seed minted from the OS RNG and kept in the OS
//! keyring (legacy service, account [`KEY_ACCOUNT`]); it never reaches the webview and is not in
//! the recovery kit. Files are owner-only `0600` in a `0700` folder and replaced atomically.
//!
//! **Fail closed.** A file that does not open (tamper, a copy under the other name, a key the
//! keyring no longer has) is reported, never shown and never overwritten by a read. Files with no
//! key are refused; a new key is never minted over them. An unreachable keyring refuses every read
//! and write.
//!
//! **Authority (HIC).** Saving terms and approving an envelope are written only after the member's
//! own confirm card in the app (Settings › Work, or a core-rendered Hermes policy card); turning an
//! envelope off lowers authority and applies at once. Offer text can never change the envelope:
//! nothing here reads an offer. Only the floor is enforced on chain (WorkMarket `join(minPrice)`).
//!
//! Rule 3: nothing here signs, holds a wallet key or sends anything. Rule 7: the data source of
//! every `fl_work_*` command is this folder, opened with the work key in the OS keyring. No network.

use std::path::PathBuf;
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{KemCore, MlKem768, B32};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::custody::Keyring;

/// OS keyring account of the work key.
pub const KEY_ACCOUNT: &str = "fl-work-key-v1";
/// The folder under app data.
pub const WORK_DIR: &str = "fl-work";
/// Most data classes a member's terms may list.
pub const MAX_DATA_CLASSES: usize = 32;
/// Envelope caps, inclusive.
pub const MAX_JOINS_PER_WEEK: u32 = 50;
pub const MAX_HOURS_PER_ROUND: u32 = 72;

const EXT: &str = "sealed";
const TERMS_STEM: &str = "terms";
const ENVELOPE_STEM: &str = "envelope";
const MAX_FILE_BYTES: usize = 64 * 1024;
const MAGIC: &[u8; 8] = b"CITRFLW\0";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = 8 + 1 + 4;
const NONCE_LEN: usize = 12;
const DEK_LEN: usize = 32;
const SEED_LEN: usize = 64;
const HKDF_INFO: &[u8] = b"citrate.core.fl-work.kek.v1";
const KEY_MAGIC: &[u8; 4] = b"FLW1";
const KEY_LEN: usize = 4 + 4 + SEED_LEN;
const MS_PER_WEEK: u64 = 7 * 86_400_000;

// ----------------------------------------------------------------------------- the records

/// The hours a member's machine may work: a daily window, or any time (`"any"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Hours {
    Window { start: String, end: String },
    Any(String),
}

/// The member's terms. Mirrors `src/fl/workerTerms.ts` `WorkerTerms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerTerms {
    /// Decimal SALT per unit, e.g. "1.20".
    pub floor_salt_per_unit: String,
    pub hours: Hours,
    pub weekdays_only: bool,
    pub mains_only: bool,
    pub data_classes: Vec<String>,
}

/// What a save returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermsReceipt {
    pub saved_at: u64,
    /// sha256 (hex) of the terms as stored.
    pub terms_hash: String,
    pub sealed: bool,
}

/// The HIC-2 envelope as stored. Mirrors `src/fl/authority.ts` (via `EnvelopeRecord`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope {
    pub on: bool,
    pub name: String,
    pub joins_per_week: u32,
    pub max_hours_per_round: u32,
    /// When the member approved it on their confirm card (ms since the epoch).
    pub approved_at: u64,
    /// When it was turned off, if it was.
    pub off_at: Option<u64>,
    /// The start of the current week of use (ms) and the joins made inside it.
    pub week_start: u64,
    pub joins_this_week: u32,
}

/// What the member approves.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvelopeInput {
    pub name: String,
    pub joins_per_week: u32,
    pub max_hours_per_round: u32,
}

fn hhmm(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && b[..2].iter().chain(&b[3..]).all(u8::is_ascii_digit)
        && s[..2].parse::<u8>().map(|h| h < 24).unwrap_or(false)
        && s[3..].parse::<u8>().map(|m| m < 60).unwrap_or(false)
}

fn salt_amount(s: &str) -> bool {
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (s, None),
    };
    !whole.is_empty()
        && whole.len() <= 6
        && whole.bytes().all(|b| b.is_ascii_digit())
        && frac.map_or(true, |f| {
            !f.is_empty() && f.len() <= 18 && f.bytes().all(|b| b.is_ascii_digit())
        })
}

fn data_class(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

impl WorkerTerms {
    /// Why these terms are refused, or `None`. Words match `termsProblem` in workerTerms.ts.
    pub fn problem(&self) -> Option<String> {
        if !salt_amount(&self.floor_salt_per_unit) {
            return Some("The floor must be a SALT amount like 1.20".into());
        }
        match &self.hours {
            Hours::Any(a) if a == "any" => {}
            Hours::Any(_) => return Some("Hours must be times like 23:00, or any".into()),
            Hours::Window { start, end } if !(hhmm(start) && hhmm(end)) => {
                return Some("Hours must be times like 23:00".into())
            }
            Hours::Window { .. } => {}
        }
        if self.data_classes.len() > MAX_DATA_CLASSES {
            return Some(format!("At most {MAX_DATA_CLASSES} data classes"));
        }
        if self.data_classes.iter().any(|c| !data_class(c)) {
            return Some("A data class must be letters, digits, dots, dashes or underscores".into());
        }
        None
    }
}

impl EnvelopeInput {
    pub fn problem(&self) -> Option<String> {
        let n = self.name.trim();
        if n.is_empty() || n.chars().count() > 64 || n.chars().any(char::is_control) {
            return Some("Name the envelope (up to 64 characters)".into());
        }
        if !(1..=MAX_JOINS_PER_WEEK).contains(&self.joins_per_week) {
            return Some(format!("Joins per week must be 1 to {MAX_JOINS_PER_WEEK}"));
        }
        if !(1..=MAX_HOURS_PER_ROUND).contains(&self.max_hours_per_round) {
            return Some(format!(
                "Machine hours per round must be 1 to {MAX_HOURS_PER_ROUND}"
            ));
        }
        None
    }
}

// ----------------------------------------------------------------------------- the envelope

struct WorkKey {
    gen: u32,
    seed: Zeroizing<[u8; SEED_LEN]>,
}

impl WorkKey {
    fn mint(gen: u32) -> Self {
        let mut seed = Zeroizing::new([0u8; SEED_LEN]);
        OsRng.fill_bytes(&mut seed[..]);
        WorkKey { gen, seed }
    }

    fn pair(
        &self,
    ) -> (
        ml_kem::kem::DecapsulationKey<ml_kem::MlKem768Params>,
        ml_kem::kem::EncapsulationKey<ml_kem::MlKem768Params>,
    ) {
        let d = B32::try_from(&self.seed[..32]).expect("32-byte half");
        let z = B32::try_from(&self.seed[32..]).expect("32-byte half");
        MlKem768::generate_deterministic(&d, &z)
    }

    fn encode(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(KEY_LEN));
        out.extend_from_slice(KEY_MAGIC);
        out.extend_from_slice(&self.gen.to_be_bytes());
        out.extend_from_slice(&self.seed[..]);
        out
    }

    fn decode(b: &[u8]) -> Result<Self, String> {
        if b.len() != KEY_LEN || &b[..4] != KEY_MAGIC {
            return Err("the work key in the keychain is malformed".into());
        }
        let mut seed = Zeroizing::new([0u8; SEED_LEN]);
        seed.copy_from_slice(&b[8..]);
        Ok(WorkKey {
            gen: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
            seed,
        })
    }
}

fn aead_seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| "the work settings could not be sealed".to_string())?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn aead_open(key: &[u8; 32], aad: &[u8], blob: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if blob.len() < NONCE_LEN {
        return None;
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .ok()
        .map(Zeroizing::new)
}

fn kek_from_shared(ss: &[u8]) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, ss);
    let mut kek = Zeroizing::new([0u8; 32]);
    hk.expand(HKDF_INFO, &mut kek[..])
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    kek
}

fn put_part(out: &mut Vec<u8>, part: &[u8]) {
    out.extend_from_slice(&(part.len() as u32).to_be_bytes());
    out.extend_from_slice(part);
}

fn take_part<'b>(b: &'b [u8], i: &mut usize) -> Option<&'b [u8]> {
    let len = b.get(*i..*i + 4)?;
    let n = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
    *i += 4;
    let part = b.get(*i..i.checked_add(n)?)?;
    *i += n;
    Some(part)
}

fn header(gen: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.push(FORMAT_VERSION);
    h.extend_from_slice(&gen.to_be_bytes());
    h
}

fn aad(head: &[u8], stem: &str) -> Vec<u8> {
    let mut a = head.to_vec();
    a.push(b'|');
    a.extend_from_slice(stem.as_bytes());
    a
}

fn seal_file(key: &WorkKey, stem: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let (_, ek) = key.pair();
    let (kem_ct, shared) = ek
        .encapsulate(&mut OsRng)
        .map_err(|_| "the work settings could not be sealed".to_string())?;
    let kek = kek_from_shared(shared.as_slice());
    let mut dek = Zeroizing::new([0u8; DEK_LEN]);
    OsRng.fill_bytes(&mut dek[..]);
    let head = header(key.gen);
    let aad = aad(&head, stem);
    let payload = aead_seal(&dek, &aad, plaintext)?;
    let wrapped = aead_seal(&kek, &aad, &dek[..])?;
    let mut out = head;
    put_part(&mut out, kem_ct.as_slice());
    put_part(&mut out, &wrapped);
    put_part(&mut out, &payload);
    Ok(out)
}

fn open_sealed(key: &WorkKey, stem: &str, bytes: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC || bytes[8] != FORMAT_VERSION {
        return None;
    }
    if u32::from_be_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]) != key.gen {
        return None;
    }
    let head = &bytes[..HEADER_LEN];
    let mut i = HEADER_LEN;
    let kem_ct = take_part(bytes, &mut i)?;
    let wrapped = take_part(bytes, &mut i)?;
    let payload = take_part(bytes, &mut i)?;
    if i != bytes.len() {
        return None;
    }
    let ct = ml_kem::Ciphertext::<MlKem768>::try_from(kem_ct).ok()?;
    let (dk, _) = key.pair();
    let shared = dk.decapsulate(&ct).ok()?;
    let kek = kek_from_shared(shared.as_slice());
    let aad = aad(head, stem);
    let mut dek_vec = aead_open(&kek, &aad, wrapped)?;
    if dek_vec.len() != DEK_LEN {
        dek_vec.zeroize();
        return None;
    }
    let mut dek = Zeroizing::new([0u8; DEK_LEN]);
    dek.copy_from_slice(&dek_vec);
    aead_open(&dek, &aad, payload)
}

// ------------------------------------------------------------------------------ the store

static STORE_LOCK: Mutex<()> = Mutex::new(());

/// The sealed work settings under one folder, keyed from `keyring`.
pub struct WorkStore<'a> {
    dir: PathBuf,
    keyring: &'a dyn Keyring,
}

impl<'a> WorkStore<'a> {
    pub fn new(dir: impl Into<PathBuf>, keyring: &'a dyn Keyring) -> Self {
        Self {
            dir: dir.into(),
            keyring,
        }
    }

    fn path(&self, stem: &str) -> PathBuf {
        self.dir.join(format!("{stem}.{EXT}"))
    }

    fn unreachable() -> String {
        "the keychain is not reachable; your work settings stay closed".to_string()
    }

    fn any_sealed(&self) -> bool {
        [TERMS_STEM, ENVELOPE_STEM]
            .iter()
            .any(|s| std::fs::symlink_metadata(self.path(s)).is_ok())
    }

    /// The work key. Minted only when `mint` and no sealed file exists.
    fn key(&self, mint: bool) -> Result<Option<WorkKey>, String> {
        let stored = self
            .keyring
            .get(KEY_ACCOUNT)
            .map_err(|_| Self::unreachable())?;
        if let Some(raw) = stored.map(Zeroizing::new) {
            return WorkKey::decode(&raw).map(Some);
        }
        if self.any_sealed() {
            return Err("your work settings exist but their key is not in the keychain; they cannot be opened".into());
        }
        if !mint {
            return Ok(None);
        }
        let k = WorkKey::mint(1);
        self.keyring
            .set(KEY_ACCOUNT, &k.encode())
            .map_err(|_| Self::unreachable())?;
        Ok(Some(k))
    }

    fn read_stem(&self, stem: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let path = self.path(stem);
        let bytes = match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("a work settings file cannot be read: {}", e.kind())),
            Ok(m) if !m.is_file() || m.len() as usize > MAX_FILE_BYTES => Vec::new(),
            Ok(_) => std::fs::read(&path)
                .map_err(|e| format!("a work settings file cannot be read: {}", e.kind()))?,
        };
        let Some(key) = self.key(false)? else {
            return Err("your work settings exist but their key is not in the keychain; they cannot be opened".into());
        };
        open_sealed(&key, stem, &bytes)
            .map(Some)
            .ok_or_else(|| format!("{stem}.{EXT} does not open on this device (changed, moved or from another device); it was left as it is"))
    }

    fn write_stem(&self, stem: &str, plain: &[u8]) -> Result<(), String> {
        let key = self.key(true)?.expect("minted");
        let sealed = seal_file(&key, stem, plain)?;
        let path = self.path(stem);
        let staged = crate::atomic_file::stage_secret(&path, &sealed)
            .map_err(|e| format!("your work settings could not be written: {}", e.kind()))?;
        crate::atomic_file::commit_staged(&staged, &path)
            .map_err(|e| format!("your work settings could not be written: {}", e.kind()))
    }

    fn read_json<T: for<'de> Deserialize<'de>>(&self, stem: &str) -> Result<Option<T>, String> {
        match self.read_stem(stem)? {
            None => Ok(None),
            Some(plain) => serde_json::from_slice(&plain)
                .map(Some)
                .map_err(|_| format!("{stem}.{EXT} opened but is not a record this app reads")),
        }
    }

    pub fn terms(&self) -> Result<Option<WorkerTerms>, String> {
        let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        self.read_json(TERMS_STEM)
    }

    pub fn save_terms(&self, terms: WorkerTerms, now: u64) -> Result<TermsReceipt, String> {
        if let Some(p) = terms.problem() {
            return Err(p);
        }
        let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let plain = Zeroizing::new(serde_json::to_vec(&terms).map_err(|e| e.to_string())?);
        let hash = hex::encode(Sha256::digest(&plain[..]));
        self.write_stem(TERMS_STEM, &plain)?;
        Ok(TermsReceipt {
            saved_at: now,
            terms_hash: hash,
            sealed: true,
        })
    }

    /// The envelope, with its week of use rolled forward to `now`.
    pub fn envelope(&self, now: u64) -> Result<Option<Envelope>, String> {
        let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        Ok(self.read_json::<Envelope>(ENVELOPE_STEM)?.map(|e| roll_week(e, now)))
    }

    pub fn approve_envelope(&self, input: EnvelopeInput, now: u64) -> Result<Envelope, String> {
        if let Some(p) = input.problem() {
            return Err(p);
        }
        let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let env = Envelope {
            on: true,
            name: input.name.trim().to_string(),
            joins_per_week: input.joins_per_week,
            max_hours_per_round: input.max_hours_per_round,
            approved_at: now,
            off_at: None,
            week_start: now,
            joins_this_week: 0,
        };
        self.write_stem(
            ENVELOPE_STEM,
            &serde_json::to_vec(&env).map_err(|e| e.to_string())?,
        )?;
        Ok(env)
    }

    /// Turn the envelope off at once. With none stored there is nothing to turn off.
    pub fn envelope_off(&self, now: u64) -> Result<Option<Envelope>, String> {
        let _g = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let Some(mut env) = self.read_json::<Envelope>(ENVELOPE_STEM)? else {
            return Ok(None);
        };
        env.on = false;
        env.off_at = Some(now);
        self.write_stem(
            ENVELOPE_STEM,
            &serde_json::to_vec(&env).map_err(|e| e.to_string())?,
        )?;
        Ok(Some(env))
    }
}

/// A new week of use starts every seven days after the last one began.
fn roll_week(mut e: Envelope, now: u64) -> Envelope {
    if now >= e.week_start.saturating_add(MS_PER_WEEK) {
        let weeks = (now - e.week_start) / MS_PER_WEEK;
        e.week_start += weeks * MS_PER_WEEK;
        e.joins_this_week = 0;
    }
    e
}

// ------------------------------------------------------------------------------ commands

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn work_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(WORK_DIR))
}

fn with_store<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&WorkStore<'_>) -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    crate::blocking::off_main(move || {
        let keyring = crate::custody::OsKeyring::legacy();
        let dir = work_dir(&app)?;
        f(&WorkStore::new(dir, &keyring))
    })
}

/// **fl_work_terms_get** — the member's saved worker terms, or null when none are saved. Data
/// source: `<app data>/fl-work/terms.sealed`, opened with the work key in the OS keyring.
#[tauri::command]
pub async fn fl_work_terms_get(app_h: tauri::AppHandle) -> Result<Option<WorkerTerms>, String> {
    with_store(app_h, |s| s.terms()).await
}

/// **fl_work_terms_save** — seal the member's confirmed terms. Call only after the confirm card.
#[tauri::command]
pub async fn fl_work_terms_save(
    app_h: tauri::AppHandle,
    terms: WorkerTerms,
) -> Result<TermsReceipt, String> {
    with_store(app_h, move |s| s.save_terms(terms, now_ms())).await
}

/// **fl_work_envelope_get** — the HIC-2 envelope, or null when the member never approved one.
#[tauri::command]
pub async fn fl_work_envelope_get(app_h: tauri::AppHandle) -> Result<Option<Envelope>, String> {
    with_store(app_h, |s| s.envelope(now_ms())).await
}

/// **fl_work_envelope_approve** — seal an envelope the member approved on their card (HIC-1 to
/// grant HIC-2). Its use starts at zero.
#[tauri::command]
pub async fn fl_work_envelope_approve(
    app_h: tauri::AppHandle,
    envelope: EnvelopeInput,
) -> Result<Envelope, String> {
    with_store(app_h, move |s| s.approve_envelope(envelope, now_ms())).await
}

/// **fl_work_envelope_off** — turn the envelope off now (lowering authority needs no card).
#[tauri::command]
pub async fn fl_work_envelope_off(app_h: tauri::AppHandle) -> Result<Option<Envelope>, String> {
    with_store(app_h, |s| s.envelope_off(now_ms())).await
}

#[cfg(test)]
#[path = "fl_work_tests.rs"]
mod tests;
