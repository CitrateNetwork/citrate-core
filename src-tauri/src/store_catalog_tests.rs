// K5a: the Store's signed catalog, staged installs that arrive paused, and one-call removal.
// Fixtures are signed with a throwaway test key made in each test; the production key slot is
// empty and every command that needs the catalog refuses with it empty.
use super::*;
use citrate_components::catalog::CATALOG_DOMAIN;
use citrate_components::error::ComponentError;
use std::cell::RefCell;
use std::collections::HashMap;

const NOW: u64 = 1_790_000_000;
const NOW_MS: u64 = NOW * 1000;

// ---- test doubles (test-only; nothing here ships) -------------------------

/// A minisign-format Ed25519 key, generated fresh per test.
struct TestKey {
    sk: ed25519_dalek::SigningKey,
    key_id: [u8; 8],
}

impl TestKey {
    fn generate() -> Self {
        TestKey {
            sk: ed25519_dalek::SigningKey::from_bytes(&rand::random::<[u8; 32]>()),
            key_id: rand::random(),
        }
    }

    fn public_b64(&self) -> String {
        use base64::Engine as _;
        let mut bin = Vec::with_capacity(42);
        bin.extend_from_slice(b"Ed");
        bin.extend_from_slice(&self.key_id);
        bin.extend_from_slice(self.sk.verifying_key().as_bytes());
        base64::engine::general_purpose::STANDARD.encode(bin)
    }

    fn root(&self) -> Result<TrustRoot, CatalogError> {
        Ok(TrustRoot::from_base64(&self.public_b64()).expect("test key parses"))
    }

    /// A prehashed (`ED`) minisign signature with the catalog's trusted comment.
    fn sign(&self, data: &[u8]) -> String {
        use base64::Engine as _;
        use blake2::Digest as _;
        use ed25519_dalek::Signer as _;
        let h = blake2::Blake2b512::digest(data);
        let sig = self.sk.sign(&h).to_bytes();
        let mut bin1 = Vec::with_capacity(74);
        bin1.extend_from_slice(b"ED");
        bin1.extend_from_slice(&self.key_id);
        bin1.extend_from_slice(&sig);
        let tc = format!("{CATALOG_DOMAIN} test");
        let mut global = sig.to_vec();
        global.extend_from_slice(tc.as_bytes());
        let gsig = self.sk.sign(&global).to_bytes();
        let b64 = base64::engine::general_purpose::STANDARD;
        format!(
            "untrusted comment: test key\n{}\ntrusted comment: {tc}\n{}\n",
            b64.encode(bin1),
            b64.encode(gsig)
        )
    }
}

/// Serves URLs from memory and records what was asked for.
#[derive(Default)]
struct MemFetcher {
    files: RefCell<HashMap<String, Vec<u8>>>,
    served: RefCell<Vec<String>>,
}

impl MemFetcher {
    fn put(&self, url: &str, bytes: &[u8]) {
        self.files.borrow_mut().insert(url.to_string(), bytes.to_vec());
    }
}

impl Fetcher for MemFetcher {
    fn fetch(&self, url: &str, max: u64, sink: &mut dyn std::io::Write) -> Result<u64, ComponentError> {
        self.served.borrow_mut().push(url.to_string());
        let files = self.files.borrow();
        let b = files
            .get(url)
            .ok_or_else(|| ComponentError::Fetch(format!("HTTP 404 for {url}")))?;
        citrate_components::fetch::copy_capped(&b[..], sink, max)
    }
}

struct MemTransport(Vec<u8>);

impl crate::model::ModelTransport for MemTransport {
    fn total_size(&self) -> std::result::Result<u64, crate::model::ModelError> {
        Ok(self.0.len() as u64)
    }
    fn get_from(&self, offset: u64) -> std::result::Result<Box<dyn std::io::Read + Send>, crate::model::ModelError> {
        let start = (offset as usize).min(self.0.len());
        Ok(Box::new(std::io::Cursor::new(self.0[start..].to_vec())))
    }
}

struct Usage(Option<&'static str>);

impl crate::model_delete::ModelUsage for Usage {
    fn in_use(&self, _canonical: &Path) -> Option<String> {
        self.0.map(str::to_string)
    }
}

fn tmp(tag: &str) -> PathBuf {
    let n: u64 = rand::random();
    std::env::temp_dir().join(format!("citrate-core-store-{tag}-{n:016x}"))
}

fn sha(b: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(b))
}

// ---- fixtures ---------------------------------------------------------------

const DAEMON_URL: &str = "https://store.test/daily-brief.json";
const MODEL_URL: &str = "https://store.test/tiny-model.gguf";

fn daemon_template() -> Vec<u8> {
    br#"{"name":"Daily brief","prompt":"Summarise what changed in my files today.","schedule":"0 8 * * *"}"#.to_vec()
}

fn model_bytes() -> Vec<u8> {
    let mut b = b"GGUF".to_vec();
    b.extend_from_slice(&[7u8; 60]);
    b
}

fn listing_json(id: &str, kind: &str, tier: &str, url: &str, body: &[u8], extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"{kind}","name":"{id} name","summary":"What {id} does.","version":"1.0.0","publisher":"Citrate","first_party":true,"tier":"{tier}","artifact":{{"url":"{url}","sha256":"{}","size":{}}},"permissions":[{{"id":"local_model","reason":"Runs on your local model."}},{{"id":"files_read","reason":"Reads the folders you granted."}}]{extra}}}"#,
        sha(body),
        body.len()
    )
}

const BUDGET: &str = r#","budget":{"max_runs_per_day":2,"max_tokens_per_day":8000,"max_tokens_per_run":4000,"max_spend_salt":"0"}"#;

fn catalog_json(seq: u64, listings: &[String]) -> String {
    format!(
        r#"{{"schema":1,"sequence":{seq},"issued_at":{},"expires_at":{},"listings":[{}]}}"#,
        NOW - 60,
        NOW + 7 * 86_400,
        listings.join(",")
    )
}

fn standard_listings() -> Vec<String> {
    vec![
        listing_json("daily-brief", "daemon", "member", DAEMON_URL, &daemon_template(), BUDGET),
        listing_json("tiny-model", "model", "public", MODEL_URL, &model_bytes(), ""),
        listing_json("studio", "app", "public", "https://store.test/studio.dmg", b"app", ""),
        listing_json("pin-tile", "widget", "public", "https://store.test/w.json", b"w", ""),
        listing_json("solidity", "skill", "public", "https://store.test/s.json", b"s", ""),
        listing_json("github-mcp", "mcp_server", "public", "https://store.test/m.json", b"m", ""),
        listing_json("fleet-ops", "daemon", "enterprise", DAEMON_URL, &daemon_template(), BUDGET),
    ]
}

struct Fx {
    key: TestKey,
    fetcher: MemFetcher,
    root: PathBuf,
    models: PathBuf,
    daemons: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = tmp(tag);
        Fx {
            key: TestKey::generate(),
            fetcher: MemFetcher::default(),
            root: base.join("store"),
            models: base.join("models"),
            daemons: base.join("daemons.json"),
        }
    }

    fn fx_publish(&self, json: &str) {
        self.fetcher.put(STORE_CATALOG_URL, json.as_bytes());
        self.fetcher.put(STORE_CATALOG_SIG_URL, self.key.sign(json.as_bytes()).as_bytes());
        self.fetcher.put(DAEMON_URL, &daemon_template());
    }

    fn standard(tag: &str) -> Self {
        let fx = Fx::new(tag);
        fx.fx_publish(&catalog_json(3, &standard_listings()));
        fx
    }

    fn fx_view(&self, rank: i32) -> CatalogView {
        catalog_view_sync(&self.root, &self.key.root(), &self.fetcher, rank, NOW)
    }

    fn fx_stage(&self, id: &str, rank: i32) -> Result<StagedView, Refusal> {
        stage_sync(&self.root, &self.key.root(), &self.fetcher, &self.models, rank, id, NOW)
    }

    fn fx_install_with(&self, stage_id: &str, rank: i32, body: Vec<u8>, usage: &Usage, now: u64) -> Result<InstalledView, Refusal> {
        let transport = move |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(body.clone())) };
        let t = self.fx_targets(usage, &transport);
        install_sync(&self.root, &self.key.root(), &t, rank, stage_id, now, NOW_MS, 0, &mut |_, _| {})
    }

    fn fx_install(&self, stage_id: &str, rank: i32) -> Result<InstalledView, Refusal> {
        self.fx_install_with(stage_id, rank, model_bytes(), &Usage(None), NOW)
    }

    fn fx_targets<'a>(
        &self,
        usage: &'a Usage,
        transport: &'a dyn Fn(&str) -> Box<dyn crate::model::ModelTransport>,
    ) -> Targets<'a> {
        Targets {
            daemons_path: self.daemons.clone(),
            models_dir: self.models.clone(),
            bundled_models: None,
            usage,
            model_transport: transport,
        }
    }

    fn fx_remove(&self, id: &str, usage: &Usage) -> Result<String, Refusal> {
        let transport = |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(Vec::new())) };
        remove_sync(&self.root, &self.fx_targets(usage, &transport), id)
    }

    fn fx_installed(&self) -> Vec<InstalledView> {
        let usage = Usage(None);
        let transport = |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(Vec::new())) };
        installed_sync(&self.root, &self.fx_targets(&usage, &transport), NOW_MS).expect("installed")
    }

    fn fx_daemon_book(&self) -> crate::daemons::DaemonsView {
        crate::daemons::with_book_file(&self.daemons, false, |b| {
            Ok(crate::daemons::DaemonsView {
                all_paused: b.all_paused(),
                daemons: b.list(NOW_MS, 0),
            })
        })
        .expect("book")
    }
}

// ---- the empty production key slot ------------------------------------------

#[test]
fn the_empty_production_slot_fails_closed_before_any_network_or_disk() {
    let fx = Fx::standard("nokey");
    let v = catalog_view_sync(&fx.root, &catalog_trust_root(), &fx.fetcher, 2, NOW);
    assert_eq!(v.state, "no_key");
    assert_eq!(
        v.message.as_deref(),
        Some("Store catalog not available yet: signing key not issued")
    );
    assert!(v.listings.is_empty());
    assert!(fx.fetcher.served.borrow().is_empty(), "nothing fetched");
    assert!(!fx.root.exists(), "nothing written");

    let err = stage_sync(&fx.root, &catalog_trust_root(), &fx.fetcher, &fx.models, 2, "daily-brief", NOW).unwrap_err();
    assert_eq!(err.code, "no_key");
    let usage = Usage(None);
    let transport = |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(Vec::new())) };
    let err = install_sync(&fx.root, &catalog_trust_root(), &fx.fx_targets(&usage, &transport), 2, &"0".repeat(32), NOW, NOW_MS, 0, &mut |_, _| {}).unwrap_err();
    assert_eq!(err.code, "no_key");
    assert!(fx.fetcher.served.borrow().is_empty());
}

// ---- signature verify ---------------------------------------------------------

#[test]
fn a_good_catalog_lists_every_kind_with_honest_install_states() {
    let fx = Fx::standard("good");
    let v = fx.fx_view(1);
    assert_eq!(v.state, "ready", "{:?}", v.message);
    assert_eq!(v.sequence, Some(3));
    assert!(v.key_fingerprint.is_some());
    let by = |id: &str| v.listings.iter().find(|l| l.id == id).cloned().expect(id);
    let d = by("daily-brief");
    assert_eq!(d.kind, "daemon");
    assert!(d.installable && !d.locked && d.first_party);
    assert_eq!(d.permissions.len(), 2);
    assert_eq!(d.budget.as_ref().map(|b| b.max_spend_salt.as_str()), Some("0"));
    assert!(by("tiny-model").installable);
    for id in ["studio", "pin-tile", "solidity", "github-mcp"] {
        let l = by(id);
        assert!(!l.installable, "{id}");
        assert!(l.install_note.as_deref().unwrap_or("").starts_with("Not installable yet"), "{id}");
    }
    assert!(by("fleet-ops").locked, "enterprise listing is locked for a member");
}

#[test]
fn a_tampered_catalog_is_refused_as_a_signature_failure() {
    let fx = Fx::standard("tamper");
    let json = catalog_json(3, &standard_listings()).replace("\"tier\":\"member\"", "\"tier\":\"public\"");
    fx.fetcher.put(STORE_CATALOG_URL, json.as_bytes());
    let v = fx.fx_view(1);
    assert_eq!(v.state, "signature_failed");
    assert!(v.listings.is_empty());
}

#[test]
fn a_catalog_signed_by_another_key_is_refused() {
    let fx = Fx::standard("wrongkey");
    let other = TestKey::generate();
    let v = catalog_view_sync(&fx.root, &other.root(), &fx.fetcher, 1, NOW);
    assert_eq!(v.state, "signature_failed");
}

#[test]
fn a_failed_fetch_is_its_own_state() {
    let fx = Fx::new("nofetch");
    let v = fx.fx_view(1);
    assert_eq!(v.state, "fetch_failed");
    assert!(v.message.unwrap_or_default().contains("Could not fetch"));
}

#[test]
fn an_older_catalog_after_a_newer_one_is_a_rollback() {
    let fx = Fx::standard("rollback");
    assert_eq!(fx.fx_view(1).state, "ready");
    fx.fx_publish(&catalog_json(2, &standard_listings()));
    assert_eq!(fx.fx_view(1).state, "rollback");
}

// ---- hash mismatch --------------------------------------------------------------

#[test]
fn a_daemon_template_that_does_not_match_the_catalog_hash_is_refused() {
    let fx = Fx::standard("hash");
    assert_eq!(fx.fx_view(1).state, "ready");
    let mut wrong = daemon_template();
    let last = wrong.len() - 3;
    wrong[last] = b'X';
    fx.fetcher.put(DAEMON_URL, &wrong);
    let err = fx.fx_stage("daily-brief", 1).unwrap_err();
    assert_eq!(err.code, "hash_failed", "{err:?}");
    // A short body is a size mismatch, also a hash failure.
    fx.fetcher.put(DAEMON_URL, &daemon_template()[..10]);
    assert_eq!(fx.fx_stage("daily-brief", 1).unwrap_err().code, "hash_failed");
    assert!(fx.fx_daemon_book().daemons.is_empty(), "nothing installed");
}

#[test]
fn a_model_that_does_not_match_the_catalog_hash_is_not_kept() {
    let fx = Fx::standard("modelhash");
    assert_eq!(fx.fx_view(0).state, "ready");
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    assert_eq!(st.hash_check, "on_download");
    let mut wrong = model_bytes();
    wrong[10] = 1;
    let err = fx.fx_install_with(&st.stage_id, 0, wrong, &Usage(None), NOW).unwrap_err();
    assert_eq!(err.code, "hash_failed", "{err:?}");
    assert!(!crate::model::is_file_ready(&fx.models, "tiny-model.gguf"));
    assert!(fx.fx_installed().is_empty());
}

// ---- tier gate -------------------------------------------------------------------

#[test]
fn member_rank_is_fail_closed() {
    assert_eq!(member_rank(false, Some("enterprise"), None, NOW), 0, "signed out reaches public only");
    assert_eq!(member_rank(true, None, None, NOW), 0);
    assert_eq!(member_rank(true, Some("commercial.kyc"), None, NOW), 1);
    assert_eq!(member_rank(true, Some("enterprise"), None, NOW), 2);
    assert_eq!(member_rank(true, Some("platinum"), None, NOW), -1, "unknown tier reaches nothing");
    let later_ms = ((NOW + 86_400) * 1000).to_string();
    let earlier_ms = ((NOW - 86_400) * 1000).to_string();
    assert_eq!(member_rank(true, Some("commercial"), Some(&later_ms), NOW), 1);
    assert_eq!(member_rank(true, Some("commercial"), Some(&earlier_ms), NOW), 0, "lapsed");
    assert_eq!(member_rank(true, Some("commercial"), Some("2027-01-01"), NOW), 1);
    assert_eq!(member_rank(true, Some("commercial"), Some("2026-01-01T00:00:00Z"), NOW), 0);
    assert_eq!(member_rank(true, Some("commercial"), Some("soon"), NOW), 0, "unparseable expiry is lapsed");
}

#[test]
fn a_tier_locked_listing_cannot_be_staged_or_installed() {
    let fx = Fx::standard("tier");
    let v = fx.fx_view(0);
    let d = v.listings.iter().find(|l| l.id == "daily-brief").expect("listing");
    assert!(d.locked);
    assert_eq!(d.lock_reason.as_deref(), Some("Needs an active membership"));
    assert_eq!(fx.fx_stage("daily-brief", 0).unwrap_err().code, "tier_locked");
    assert_eq!(fx.fx_stage("fleet-ops", 1).unwrap_err().code, "tier_locked");
    // An unknown tier locks even public listings.
    let v = fx.fx_view(-1);
    assert!(v.listings.iter().all(|l| l.locked));
    assert_eq!(fx.fx_stage("tiny-model", -1).unwrap_err().code, "tier_locked");
    // A tier that lapses between review and approval is re-checked at install.
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    assert_eq!(fx.fx_install(&st.stage_id, 0).unwrap_err().code, "tier_locked");
    assert!(fx.fx_daemon_book().daemons.is_empty());
}

#[test]
fn kinds_without_an_installer_say_so_at_stage() {
    let fx = Fx::standard("kinds");
    fx.fx_view(1);
    for id in ["studio", "pin-tile", "solidity", "github-mcp"] {
        let err = fx.fx_stage(id, 1).unwrap_err();
        assert_eq!(err.code, "not_installable", "{id}");
        assert!(err.message.starts_with("Not installable yet"), "{id}");
    }
    assert_eq!(fx.fx_stage("no-such-listing", 1).unwrap_err().code, "not_listed");
}

// ---- install arrives paused ----------------------------------------------------------

#[test]
fn a_daemon_installs_paused_with_the_declared_budget() {
    let fx = Fx::standard("paused");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    assert_eq!(st.hash_check, "verified");
    assert_eq!(st.permissions.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["local_model", "files_read"]);
    let b = st.budget.clone().expect("budget");
    assert_eq!((b.max_runs_per_day, b.max_tokens_per_day, b.max_tokens_per_run), (2, 8000, 4000));
    assert!(fx.fx_daemon_book().daemons.is_empty(), "nothing installs before approval");

    let iv = fx.fx_install(&st.stage_id, 1).expect("install");
    assert_eq!(iv.state, "paused");
    let book = fx.fx_daemon_book();
    assert_eq!(book.daemons.len(), 1);
    let d = &book.daemons[0];
    assert!(d.paused, "arrives paused");
    assert_eq!(d.status, "paused");
    assert_eq!(d.next_run_ms, None, "a paused daemon has no next run");
    assert_eq!(d.budget.max_runs_per_day, 2);
    assert_eq!(d.budget.max_spend_salt, "0");
    assert_eq!(d.id, iv.target);
    let v = fx.fx_view(1);
    assert!(v.listings.iter().find(|l| l.id == "daily-brief").is_some_and(|l| l.installed));
    assert_eq!(fx.fx_stage("daily-brief", 1).unwrap_err().code, "already_installed");
}

#[test]
fn a_model_installs_verified_and_not_selected() {
    let fx = Fx::standard("model");
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    let iv = fx.fx_install(&st.stage_id, 0).expect("install");
    assert_eq!(iv.state, "not_selected");
    assert_eq!(iv.target, "tiny-model.gguf");
    assert!(crate::model::is_file_ready(&fx.models, "tiny-model.gguf"));
}

#[test]
fn a_stage_is_single_use_and_expires() {
    let fx = Fx::standard("single");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    fx.fx_install(&st.stage_id, 1).expect("install");
    assert_eq!(fx.fx_install(&st.stage_id, 1).unwrap_err().code, "stage_expired");
    assert_eq!(fx.fx_daemon_book().daemons.len(), 1);

    let fx = Fx::standard("expire");
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    let late = NOW + STAGE_TTL_SECS;
    let err = fx.fx_install_with(&st.stage_id, 0, model_bytes(), &Usage(None), late).unwrap_err();
    assert_eq!(err.code, "stage_expired");
    assert_eq!(fx.fx_install(&"z".repeat(32), 0).unwrap_err().code, "stage_expired");
}

#[test]
fn a_listing_that_changed_after_review_must_be_reviewed_again() {
    let fx = Fx::standard("changed");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    let changed: Vec<String> = standard_listings()
        .into_iter()
        .map(|l| l.replace("\"max_runs_per_day\":2", "\"max_runs_per_day\":3"))
        .collect();
    fx.fx_publish(&catalog_json(4, &changed));
    assert_eq!(fx.fx_view(1).state, "ready");
    assert_eq!(fx.fx_install(&st.stage_id, 1).unwrap_err().code, "changed");
    assert!(fx.fx_daemon_book().daemons.is_empty());
}

// ---- removal ----------------------------------------------------------------------------

#[test]
fn removal_is_one_call_and_needs_no_key() {
    let fx = Fx::standard("remove");
    fx.fx_view(1);
    let d = fx.fx_stage("daily-brief", 1).expect("stage");
    fx.fx_install(&d.stage_id, 1).expect("install daemon");
    let m = fx.fx_stage("tiny-model", 1).expect("stage");
    fx.fx_install(&m.stage_id, 1).expect("install model");
    assert_eq!(fx.fx_installed().len(), 2);

    // A model in use is not pulled out from under the server.
    assert_eq!(fx.fx_remove("tiny-model", &Usage(Some("the local model server is running on it"))).unwrap_err().code, "in_use");

    assert!(fx.fx_remove("daily-brief", &Usage(None)).is_ok());
    assert!(fx.fx_daemon_book().daemons.is_empty());
    assert!(fx.fx_remove("tiny-model", &Usage(None)).is_ok());
    assert!(!fx.models.join("tiny-model.gguf").exists());
    assert!(fx.fx_installed().is_empty());
    assert_eq!(fx.fx_remove("daily-brief", &Usage(None)).unwrap_err().code, "not_installed");
}

#[test]
fn a_target_removed_outside_the_store_shows_missing_and_still_removes() {
    let fx = Fx::standard("missing");
    fx.fx_view(1);
    let d = fx.fx_stage("daily-brief", 1).expect("stage");
    let iv = fx.fx_install(&d.stage_id, 1).expect("install");
    crate::daemons::with_book_file(&fx.daemons, true, |b| b.delete(&iv.target)).expect("delete");
    assert_eq!(fx.fx_installed()[0].state, "missing");
    assert!(fx.fx_remove("daily-brief", &Usage(None)).is_ok());
    assert!(fx.fx_installed().is_empty());
}

// ---- wiring --------------------------------------------------------------------------------

#[test]
fn every_store_command_is_async_registered_and_allowed_for_the_main_window_only() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    let src = include_str!("store_catalog.rs");
    for cmd in COMMANDS {
        assert!(lib.contains(&format!("store_catalog::{cmd},")), "{cmd} registered");
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} in main-window.toml");
        assert!(src.contains(&format!("pub async fn {cmd}(")), "{cmd} is async");
    }
}

#[test]
fn the_catalog_urls_are_https_and_apart_from_the_component_feed() {
    assert!(STORE_CATALOG_URL.starts_with("https://"));
    assert!(STORE_CATALOG_SIG_URL.starts_with(STORE_CATALOG_URL));
    assert_ne!(STORE_CATALOG_URL, crate::components::COMPONENT_MANIFEST_URL);
    assert!(!STORE_CATALOG_URL.contains("/updater/"));
}
