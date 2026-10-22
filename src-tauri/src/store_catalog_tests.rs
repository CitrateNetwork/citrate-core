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
    /// The chat server's selection (never started in tests).
    serve: crate::serve::LlamaServerManager,
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
            serve: crate::serve::LlamaServerManager::new(
                PathBuf::from("/nonexistent/llama-server"),
                base.join("models").join(crate::model::MODEL_FILE),
                base.join("crash.jsonl"),
                0,
            ),
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
        &'a self,
        usage: &'a Usage,
        transport: &'a dyn Fn(&str) -> Box<dyn crate::model::ModelTransport>,
    ) -> Targets<'a> {
        Targets {
            daemons_path: self.daemons.clone(),
            models_dir: self.models.clone(),
            bundled_models: None,
            usage,
            selection: &self.serve,
            model_transport: transport,
        }
    }

    fn fx_remove(&self, id: &str, usage: &Usage) -> Result<String, Refusal> {
        let transport = |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(Vec::new())) };
        remove_sync(&self.root, &self.fx_targets(usage, &transport), id, NOW_MS)
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

// ---- the install record is kept with the install --------------------------------------------

#[test]
fn the_install_record_is_saved_before_the_target_so_a_failed_save_creates_nothing() {
    let fx = Fx::standard("recordfirst");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    // The Store state file cannot be written (its temporary file path is a folder).
    std::fs::create_dir_all(fx.root.join("state.tmp")).expect("block");
    let err = fx.fx_install(&st.stage_id, 1).unwrap_err();
    assert_eq!(err.code, "install_failed", "{err:?}");
    assert!(fx.fx_daemon_book().daemons.is_empty(), "no untracked daemon");
}

#[test]
fn an_install_stays_tracked_when_the_final_record_save_fails() {
    let fx = Fx::standard("tracked");
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    let usage = Usage(None);
    let root = fx.root.clone();
    // The model target is created after the record; block the record's final save meanwhile.
    let transport = move |_: &str| -> Box<dyn crate::model::ModelTransport> {
        let _ = std::fs::create_dir_all(root.join("state.tmp"));
        Box::new(MemTransport(model_bytes()))
    };
    let t = fx.fx_targets(&usage, &transport);
    let iv = install_sync(&fx.root, &fx.key.root(), &t, 0, &st.stage_id, NOW, NOW_MS, 0, &mut |_, _| {})
        .expect("the install itself succeeded");
    assert_eq!(iv.state, "not_selected");
    std::fs::remove_dir_all(fx.root.join("state.tmp")).expect("unblock");
    let listed = fx.fx_installed();
    assert_eq!(listed.len(), 1, "the Store still tracks it");
    assert_eq!(listed[0].target, "tiny-model.gguf");
    assert_eq!(listed[0].state, "not_selected");
    assert!(fx.fx_remove("tiny-model", &Usage(None)).is_ok());
    assert!(!fx.models.join("tiny-model.gguf").exists());
}

#[test]
fn a_failed_target_leaves_no_record_behind() {
    let fx = Fx::standard("failedtarget");
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    let mut wrong = model_bytes();
    wrong[10] = 1;
    assert_eq!(
        fx.fx_install_with(&st.stage_id, 0, wrong, &Usage(None), NOW).unwrap_err().code,
        "hash_failed"
    );
    assert!(fx.fx_installed().is_empty());
    // And it can be reviewed and installed again.
    let st = fx.fx_stage("tiny-model", 0).expect("stage again");
    assert!(fx.fx_install(&st.stage_id, 0).is_ok());
}

// ---- K5-F: no removal while the install is in flight -----------------------------------------

/// Serves the model only after the test says so, and reports when the download has started.
struct GatedTransport {
    body: Vec<u8>,
    started: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
    go: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl crate::model::ModelTransport for GatedTransport {
    fn total_size(&self) -> std::result::Result<u64, crate::model::ModelError> {
        Ok(self.body.len() as u64)
    }
    fn get_from(&self, offset: u64) -> std::result::Result<Box<dyn std::io::Read + Send>, crate::model::ModelError> {
        if let Ok(tx) = self.started.lock() {
            let _ = tx.send(());
        }
        if let Ok(rx) = self.go.lock() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(20));
        }
        let start = (offset as usize).min(self.body.len());
        Ok(Box::new(std::io::Cursor::new(self.body[start..].to_vec())))
    }
}

#[test]
fn remove_is_refused_while_that_install_is_in_flight() {
    let fx = Fx::standard("inflight");
    fx.fx_view(1);
    // A finished install, to show other installs stay removable meanwhile.
    let d = fx.fx_stage("daily-brief", 1).expect("stage daemon");
    fx.fx_install(&d.stage_id, 1).expect("install daemon");
    let st = fx.fx_stage("tiny-model", 1).expect("stage model");

    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel();
    let transport = GatedTransport {
        body: model_bytes(),
        started: std::sync::Mutex::new(started_tx),
        go: std::sync::Mutex::new(go_rx),
    };
    let slot = std::sync::Mutex::new(Some(transport));
    let (root, key) = (fx.root.clone(), fx.key.root());
    let (daemons_path, models_dir) = (fx.daemons.clone(), fx.models.clone());
    let stage_id = st.stage_id.clone();
    std::thread::scope(|scope| {
        let installer = scope.spawn(|| {
            let usage = Usage(None);
            let make = |_: &str| -> Box<dyn crate::model::ModelTransport> {
                match slot.lock().ok().and_then(|mut s| s.take()) {
                    Some(t) => Box::new(t),
                    None => Box::new(MemTransport(Vec::new())),
                }
            };
            let serve = crate::serve::LlamaServerManager::new(
                PathBuf::from("/nonexistent/llama-server"),
                models_dir.join(crate::model::MODEL_FILE),
                models_dir.join("crash.jsonl"),
                0,
            );
            let t = Targets {
                daemons_path,
                models_dir,
                bundled_models: None,
                usage: &usage,
                selection: &serve,
                model_transport: &make,
            };
            install_sync(&root, &key, &t, 1, &stage_id, NOW, NOW_MS, 0, &mut |_, _| {})
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the download started");

        // The in-flight install shows as installing and cannot be removed.
        let listed = fx.fx_installed();
        let m = listed.iter().find(|i| i.listing_id == "tiny-model").expect("tracked");
        assert_eq!(m.state, "installing");
        let err = fx.fx_remove("tiny-model", &Usage(None)).unwrap_err();
        assert_eq!(err.code, "install_running", "{err:?}");
        // Another, finished install is still removable.
        assert!(fx.fx_remove("daily-brief", &Usage(None)).is_ok());

        go_tx.send(()).expect("release the download");
        let iv = installer.join().expect("joined").expect("installed");
        assert_eq!(iv.state, "not_selected");
    });
    // Once finished, it is removable as usual.
    assert!(fx.fx_remove("tiny-model", &Usage(None)).is_ok());
}

#[test]
fn a_pending_record_left_by_an_install_that_is_not_running_is_removable() {
    // A crash between saving the record and creating the target leaves a pending record with no
    // install running: that is not in flight, so Remove clears it.
    let fx = Fx::standard("stalepending");
    fx.fx_view(1);
    let rec = InstallRecord {
        listing_id: "tiny-model".into(),
        kind: ListingKind::Model,
        name: "tiny-model name".into(),
        version: "1.0.0".into(),
        target: "tiny-model.gguf".into(),
        installed_at: NOW,
        pending: true,
        sha256: String::new(),
        size: 0,
        url: String::new(),
    };
    update_installs(&fx.root, |i| i.push(rec)).expect("seed record");
    assert_eq!(fx.fx_installed()[0].state, "missing");
    assert!(fx.fx_remove("tiny-model", &Usage(None)).is_ok());
}

// ---- the approved permissions bind the daemon's tools (deny by default) -------------------

#[test]
fn permissions_map_to_tools_deny_by_default() {
    let none: Vec<String> = Vec::new();
    assert!(tools_for_permissions(&none).is_empty(), "no permission, no tool");
    let local = vec!["local_model".to_string()];
    assert!(tools_for_permissions(&local).is_empty(), "the local model is not a tool");
    let unknown = vec!["everything".to_string(), "files_read".to_string()];
    assert!(tools_for_permissions(&unknown).is_empty(), "unknown and sidecar-only permissions grant nothing here");
    let mem = vec!["memory_read".to_string(), "local_model".to_string()];
    assert_eq!(tools_for_permissions(&mem), vec!["memory_recall".to_string(), "memory_search".to_string()]);
    let journal = vec!["journal_read".to_string()];
    assert_eq!(tools_for_permissions(&journal), vec!["journal_read".to_string()]);
    // Never the UI mover, whatever is declared.
    let all: Vec<String> = PERMISSION_TOOLS.iter().map(|(p, _)| p.to_string()).collect();
    assert!(!tools_for_permissions(&all).iter().any(|t| t == "app_navigate"));
}

#[test]
fn every_mapped_tool_is_a_real_agent_tool() {
    // AGENT_TOOLS is defined in harness.ts; its memory_search entry lives in knowledgeSearch.ts.
    let sources = [include_str!("../../src/agent/harness.ts"), include_str!("../../src/agent/knowledgeSearch.ts")];
    for (perm, tools) in PERMISSION_TOOLS {
        for t in *tools {
            let needle = format!("name: \"{t}\"");
            assert!(sources.iter().any(|s| s.contains(&needle)), "{perm} maps to {t}, which is not an agent tool");
        }
    }
}

fn claim_tools(fx: &Fx, daemon_id: &str) -> Option<Vec<String>> {
    crate::daemons::with_book_file(&fx.daemons, true, |b| {
        b.set_paused(daemon_id, false, NOW_MS)?;
        let claims = b.claim_due_with(NOW_MS + 18 * 3_600_000, 0, Some(&[]))?;
        Ok(claims.into_iter().find(|c| c.daemon_id == daemon_id).map(|c| c.allowed_tools))
    })
    .expect("claim")
    .expect("the daemon is due at 08:00 the next day")
}

#[test]
fn a_store_daemon_with_only_local_model_and_files_read_gets_no_tools() {
    let fx = Fx::standard("perm-none");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    let iv = fx.fx_install(&st.stage_id, 1).expect("install");
    assert_eq!(claim_tools(&fx, &iv.target), Some(Vec::new()), "no memory, journal or any other tool");
}

#[test]
fn a_store_daemon_with_memory_read_gets_only_memory_reads() {
    let fx = Fx::new("perm-mem");
    let listing = listing_json("mem-brief", "daemon", "public", DAEMON_URL, &daemon_template(), BUDGET)
        .replace(r#"{"id":"files_read","reason":"Reads the folders you granted."}"#, r#"{"id":"memory_read","reason":"Reads your memory."}"#);
    fx.fx_publish(&catalog_json(1, &[listing]));
    fx.fx_view(0);
    let st = fx.fx_stage("mem-brief", 0).expect("stage");
    let iv = fx.fx_install(&st.stage_id, 0).expect("install");
    let tools = claim_tools(&fx, &iv.target).expect("restricted");
    assert_eq!(tools, vec!["memory_recall".to_string(), "memory_search".to_string()]);
    assert!(!tools.iter().any(|t| t.starts_with("journal")));
}

// ---- removing a started daemon ------------------------------------------------------------------

#[test]
fn removing_a_daemon_with_a_run_in_flight_is_refused_until_the_run_ends() {
    let fx = Fx::standard("running");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    let iv = fx.fx_install(&st.stage_id, 1).expect("install");
    let at = NOW_MS + 18 * 3_600_000;
    let run_id = crate::daemons::with_book_file(&fx.daemons, true, |b| {
        b.set_paused(&iv.target, false, NOW_MS)?;
        Ok(b.claim_due_with(at, 0, Some(&[]))?.remove(0).run_id)
    })
    .expect("claimed");
    let transport = |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(Vec::new())) };
    let usage = Usage(None);
    let t = fx.fx_targets(&usage, &transport);
    let err = remove_sync(&fx.root, &t, "daily-brief", at + 1_000).unwrap_err();
    assert_eq!(err.code, "daemon_running", "{err:?}");
    assert_eq!(fx.fx_daemon_book().daemons.len(), 1, "still there");
    // The run ends (the app stopped it); now removal goes ahead.
    crate::daemons::with_book_file(&fx.daemons, true, |b| {
        b.finish_run(&iv.target, &run_id, 0, crate::daemons::RunOutcome::Stopped, "stopped", at + 2_000)
    })
    .expect("finished");
    assert!(remove_sync(&fx.root, &t, "daily-brief", at + 3_000).is_ok());
    // A completion arriving for the removed daemon is refused.
    let late = crate::daemons::with_book_file(&fx.daemons, true, |b| {
        b.finish_run(&iv.target, &run_id, 0, crate::daemons::RunOutcome::Answered, "late", at + 4_000)
    });
    assert!(late.is_err());
}

#[test]
fn a_released_stale_run_does_not_block_removal() {
    let fx = Fx::standard("stale");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    let iv = fx.fx_install(&st.stage_id, 1).expect("install");
    let at = NOW_MS + 18 * 3_600_000;
    crate::daemons::with_book_file(&fx.daemons, true, |b| {
        b.set_paused(&iv.target, false, NOW_MS)?;
        b.claim_due_with(at, 0, Some(&[])).map(|_| ())
    })
    .expect("claimed");
    let transport = |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(Vec::new())) };
    let usage = Usage(None);
    let t = fx.fx_targets(&usage, &transport);
    assert!(remove_sync(&fx.root, &t, "daily-brief", at + crate::daemons::STALE_RUN_MS + 1).is_ok());
}

// ---- entitlement expiry is parsed strictly -------------------------------------------------------

#[test]
fn an_expiry_must_be_a_whole_valid_rfc3339_value() {
    let ok = |e: &str| member_rank(true, Some("commercial"), Some(e), NOW);
    // Trailing text, impossible dates and times, and a date-time with no offset all fail closed.
    for bad in [
        "2099-01-01garbage",
        "2099-01-01T00:00:00Zjunk",
        "2099-02-30",
        "2027-02-29",
        "2099-13-01",
        "2099-01-01T24:00:00Z",
        "2099-01-01T10:60:00Z",
        "2099-01-01T10:00:61Z",
        "2099-01-01T10:00:00",
        "2099-01-01T10:00Z",
        "2099-01-01T10:00:00+24:00",
        "2099-01-01T10:00:00+0100",
        "2099-1-01",
        "+2099-01-01",
        "2099-01-01T10:00:00.Z",
    ] {
        assert_eq!(ok(bad), 0, "{bad:?} must count as lapsed");
    }
    for good in ["2099-01-01", "2028-02-29", "2099-01-01T10:00:00Z", "2099-01-01t10:00:00.123z", "2099-01-01T10:00:00-05:30"] {
        assert_eq!(ok(good), 1, "{good:?}");
    }
    // The offset is applied: 01:00+02:00 is 23:00 UTC the day before.
    assert_eq!(expiry_secs("2026-09-21T01:00:00+02:00"), expiry_secs("2026-09-20T23:00:00Z"));
    assert_eq!(expiry_secs("2026-09-20T23:00:00-01:00"), expiry_secs("2026-09-21T00:00:00Z"));
    // A just-future local time whose UTC instant is past is lapsed.
    let past_utc = "2026-09-21T15:00:00+12:00"; // 03:00Z, before NOW (2026-09-21T14:13:20Z)
    assert_eq!(ok(past_utc), 0);
}

// ---- sidecar-hosted tool sets a Store daemon was not approved for --------------------------

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn a_no_permission_store_daemon_refuses_to_start_with_web_search_on_and_starts_with_it_off() {
    let fx = Fx::new("hostsets");
    let listing = listing_json("bare", "daemon", "public", DAEMON_URL, &daemon_template(), BUDGET)
        .replace(r#""permissions":[{"id":"local_model","reason":"Runs on your local model."},{"id":"files_read","reason":"Reads the folders you granted."}]"#, r#""permissions":[]"#);
    assert!(listing.contains(r#""permissions":[]"#));
    fx.fx_publish(&catalog_json(1, &[listing]));
    fx.fx_view(0);
    let st = fx.fx_stage("bare", 0).expect("stage");
    let id = fx.fx_install(&st.stage_id, 0).expect("install").target;
    let search_on = env(&[("CITRATE_HERMES_SEARCH", "1"), ("CITRATE_HERMES_READER", "jina")]);
    // Start (resume) is refused while Hermes offers web search to every session.
    let err = crate::daemons::with_book_file(&fx.daemons, true, |b| b.resume_checked(&id, NOW_MS, Some(&search_on)))
        .unwrap_err();
    assert!(err.contains("Web search"), "{err}");
    assert!(err.contains("Settings"), "says how to run it sandboxed: {err}");
    assert!(fx.fx_daemon_book().daemons[0].paused, "still paused");
    // With every host tool set off it starts.
    crate::daemons::with_book_file(&fx.daemons, true, |b| b.resume_checked(&id, NOW_MS, Some(&[])))
        .expect("starts");
    assert!(!fx.fx_daemon_book().daemons[0].paused);
    // A run that comes due while web search is on is not started, and says why.
    let at = NOW_MS + 18 * 3_600_000;
    let claims = crate::daemons::with_book_file(&fx.daemons, true, |b| b.claim_due_with(at, 0, Some(&search_on)))
        .expect("claim");
    assert!(claims.is_empty(), "not started");
    let v = &fx.fx_daemon_book().daemons[0];
    assert_eq!(v.last_outcome.as_deref(), Some("not_started"));
    assert!(v.last_note.as_deref().unwrap_or("").contains("Web search"));
    // The next due run with the set off is claimed.
    let later = at + 24 * 3_600_000;
    let claims = crate::daemons::with_book_file(&fx.daemons, true, |b| b.claim_due_with(later, 0, Some(&[])))
        .expect("claim");
    assert_eq!(claims.len(), 1);
}

#[test]
fn every_sidecar_tool_set_blocks_a_store_daemon_not_approved_for_it() {
    use crate::daemons::{host_block_reason, HOST_TOOL_SETS};
    for set in HOST_TOOL_SETS {
        // A flag set is on at its value; a path set is on at any non-empty value.
        let value = if set.on_value.is_empty() { "/app/data/hermes/x" } else { set.on_value };
        let on = env(&[(set.env_key, value)]);
        assert!(host_block_reason(&[], &on).is_some(), "{} blocks a no-permission daemon", set.id);
        // The permission that covers it lets it through.
        let covered = tools_for_permissions(&[set.permission.to_string()]);
        assert!(host_block_reason(&covered, &on).is_none(), "{} is covered by {}", set.id, set.permission);
    }
    // Nothing on: nothing blocks.
    assert!(host_block_reason(&[], &[]).is_none());
    // A value that does not turn a flag set on does not count.
    assert!(host_block_reason(&[], &env(&[("CITRATE_HERMES_SEARCH", "0"), ("CITRATE_HERMES_MCP", "")])).is_none());
}

#[test]
fn a_member_written_daemon_is_not_held_to_store_permissions() {
    let mut book = crate::daemons::DaemonBook::default();
    let v = book
        .save(
            crate::daemons::DaemonInput {
                id: None,
                name: "Mine".into(),
                prompt: "Do my task.".into(),
                schedule: "0 8 * * *".into(),
                budget: None,
            },
            NOW_MS,
            0,
        )
        .expect("saved");
    book.set_paused(&v.id, true, NOW_MS).expect("pause");
    assert!(book.resume_checked(&v.id, NOW_MS, Some(&env(&[("CITRATE_HERMES_SEARCH", "1")]))).is_ok());
}

// ---- unknown or changed sidecar environment ----------------------------------------------------

#[test]
fn an_unknown_sidecar_environment_blocks_a_store_daemon_claim() {
    let (fx, id) = {
        let fx = Fx::new("unknownenv");
        let listing = listing_json("bare", "daemon", "public", DAEMON_URL, &daemon_template(), BUDGET);
        fx.fx_publish(&catalog_json(1, &[listing]));
        fx.fx_view(0);
        let st = fx.fx_stage("bare", 0).expect("stage");
        let id = fx.fx_install(&st.stage_id, 0).expect("install").target;
        (fx, id)
    };
    crate::daemons::with_book_file(&fx.daemons, true, |b| b.set_paused(&id, false, NOW_MS)).expect("start");
    let at = NOW_MS + 18 * 3_600_000;
    // Hermes restarting (crash backoff, a settings change): its environment is unknown.
    let claims = crate::daemons::with_book_file(&fx.daemons, true, |b| b.claim_due_with(at, 0, None)).expect("claim");
    assert!(claims.is_empty(), "not claimed while the environment is unknown");
    let v = &fx.fx_daemon_book().daemons[0];
    assert_eq!(v.last_outcome.as_deref(), Some("not_started"));
    // A member daemon is unaffected.
    let mut book = crate::daemons::DaemonBook::default();
    let m = book
        .save(crate::daemons::DaemonInput { id: None, name: "Mine".into(), prompt: "Do it.".into(), schedule: "0 8 * * *".into(), budget: None }, NOW_MS, 0)
        .expect("saved");
    assert_eq!(book.claim_due_with(at, 0, None).expect("claim").len(), 1, "{}", m.id);
}

#[test]
fn opening_a_store_daemon_session_rechecks_the_current_environment() {
    use crate::daemons::{enabled_host_set_ids, store_open_check};
    let none: Vec<String> = Vec::new();
    let off: Vec<(String, String)> = Vec::new();
    let search_on = env(&[("CITRATE_HERMES_SEARCH", "1")]);
    let claimed_off = enabled_host_set_ids(&off);
    // Same (all off) at claim and at open: opens.
    assert!(store_open_check(&none, Some(&claimed_off), Some(&off)).is_ok());
    // Hermes is restarting at open: refused.
    assert!(store_open_check(&none, Some(&claimed_off), None).is_err());
    // Web search was turned on between the claim and the open: refused.
    let err = store_open_check(&none, Some(&claimed_off), Some(&search_on)).unwrap_err();
    assert!(err.contains("changed") || err.contains("Web search"), "{err}");
    // Covered by the approved permission but still a change since the claim: refused.
    let covered = tools_for_permissions(&["web_search".to_string()]);
    assert!(store_open_check(&covered, Some(&claimed_off), Some(&search_on)).is_err());
    // Claimed with no record of the sets (an older book): refused.
    assert!(store_open_check(&none, None, Some(&off)).is_err());
}

#[test]
fn a_claim_records_the_host_sets_it_was_checked_against() {
    let fx = Fx::standard("claimsets");
    fx.fx_view(1);
    let st = fx.fx_stage("daily-brief", 1).expect("stage");
    let id = fx.fx_install(&st.stage_id, 1).expect("install").target;
    let at = NOW_MS + 18 * 3_600_000;
    crate::daemons::with_book_file(&fx.daemons, true, |b| {
        b.set_paused(&id, false, NOW_MS)?;
        b.claim_due_with(at, 0, Some(&[])).map(|_| ())
    })
    .expect("claimed");
    let sets = crate::daemons::with_book_file(&fx.daemons, false, |b| Ok(b.claimed_host_sets(&id))).expect("read");
    assert_eq!(sets, Some(Vec::new()));
}

#[test]
fn the_check_reads_what_the_sidecar_inherits_from_the_launch_environment() {
    use crate::daemons::host_block_reason;
    // The app was launched with a tool-set flag core never set: the child inherits it.
    let inherited = env(&[("CITRATE_HERMES_SEARCH", "1"), ("HOME", "/Users/x")]);
    let core_set = env(&[("CITRATE_HERMES_ADDR", "127.0.0.1:1")]);
    let effective = crate::hermes::effective_child_env(inherited.into_iter(), &core_set);
    assert!(effective.iter().any(|(k, v)| k == "CITRATE_HERMES_SEARCH" && v == "1"));
    assert!(host_block_reason(&[], &effective).is_some(), "the inherited flag is seen");
    // Core's own value wins over an inherited one.
    let effective = crate::hermes::effective_child_env(env(&[("CITRATE_HERMES_SEARCH", "1")]).into_iter(), &env(&[("CITRATE_HERMES_SEARCH", "0")]));
    assert!(host_block_reason(&[], &effective).is_none());
}

// ---- a model install interrupted between the rename and the verify --------------------------

/// A Store record and a completed-but-unverified model file, as the app leaves them when it
/// exits after `ModelManager` renamed the download and before `verify()` recorded it.
fn interrupted_model(tag: &str, body: &[u8]) -> Fx {
    let fx = Fx::standard(tag);
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    fx.fx_install(&st.stage_id, 0).expect("install");
    // Roll the disk back to the crash window: the record is pending and the verified flag is gone.
    update_installs(&fx.root, |i| {
        for r in i.iter_mut() {
            r.pending = true;
        }
    })
    .expect("pending");
    std::fs::remove_file(fx.models.join("tiny-model.gguf.status.json")).expect("unverify");
    std::fs::write(fx.models.join("tiny-model.gguf"), body).expect("bytes");
    assert!(!crate::model::is_file_ready(&fx.models, "tiny-model.gguf"));
    fx
}

#[test]
fn an_interrupted_model_install_with_the_right_bytes_is_verified_on_the_next_start() {
    let fx = interrupted_model("crash-ok", &model_bytes());
    let listed = fx.fx_installed();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].state, "not_selected", "re-verified by its hash");
    assert!(crate::model::is_file_ready(&fx.models, "tiny-model.gguf"));
    let st = read_store_file(&fx.root).expect("state");
    assert!(!st.installs[0].pending, "the record is confirmed");
    // And it is removable as usual.
    assert!(fx.fx_remove("tiny-model", &Usage(None)).is_ok());
    assert!(!fx.models.join("tiny-model.gguf").exists());
}

#[test]
fn an_interrupted_model_install_with_wrong_bytes_is_cleaned_up_with_its_record() {
    let mut wrong = model_bytes();
    wrong[10] = 1;
    let fx = interrupted_model("crash-bad", &wrong);
    assert!(fx.fx_installed().is_empty(), "the record goes with the bad file");
    for suffix in ["", ".part", ".status.json", ".download.json"] {
        assert!(!fx.models.join(format!("tiny-model.gguf{suffix}")).exists(), "{suffix}");
    }
    // The listing can be installed again.
    let st = fx.fx_stage("tiny-model", 0).expect("stage again");
    assert!(fx.fx_install(&st.stage_id, 0).is_ok());
}

#[test]
fn removing_a_pending_model_record_also_removes_its_unverified_file() {
    let fx = interrupted_model("crash-remove", &model_bytes());
    // Removal before any reconcile: the Store's own unfinished file goes with its record.
    assert!(fx.fx_remove("tiny-model", &Usage(None)).is_ok());
    assert!(!fx.models.join("tiny-model.gguf").exists());
    assert!(read_store_file(&fx.root).expect("state").installs.is_empty());
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

// ---- Codex P2s on #356 at e1831b4 ----------------------------------------------------------

#[test]
fn a_model_download_that_fails_partway_leaves_no_untracked_bytes() {
    let fx = Fx::standard("partial");
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    // The stream ends after part of the file: a short stream, the `.part` was written to.
    let partial = model_bytes()[..30].to_vec();
    let err = fx.fx_install_with(&st.stage_id, 0, partial, &Usage(None), NOW).unwrap_err();
    assert_eq!(err.code, "hash_failed", "{err:?}");
    let part = fx.models.join("tiny-model.gguf.part");
    let tracked = !fx.fx_installed().is_empty();
    if tracked {
        // Kept as a removable pending record: Remove deletes the bytes.
        assert!(fx.fx_remove("tiny-model", &Usage(None)).is_ok());
    }
    assert!(!part.exists(), "no partial bytes are left on disk (tracked: {tracked})");
    for suffix in ["", ".status.json", ".download.json"] {
        assert!(!fx.models.join(format!("tiny-model.gguf{suffix}")).exists(), "{suffix}");
    }
    // And the listing can be installed again from scratch.
    let st = fx.fx_stage("tiny-model", 0).expect("stage again");
    assert!(fx.fx_install(&st.stage_id, 0).is_ok());
}

#[test]
fn removing_the_selected_model_resets_the_selection_and_drops_its_adapter() {
    let fx = Fx::standard("selected");
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    fx.fx_install(&st.stage_id, 0).expect("install");
    let chosen = fx.models.join("tiny-model.gguf");
    // The member selected it in Models and loaded an adapter; the server is stopped.
    let serve = crate::serve::LlamaServerManager::new(
        PathBuf::from("/nonexistent/llama-server"),
        chosen.clone(),
        fx.models.join("crash.jsonl"),
        0,
    );
    serve.set_lora(Some(fx.models.join("adapter.gguf")));
    let usage = Usage(None);
    let transport = |_: &str| -> Box<dyn crate::model::ModelTransport> { Box::new(MemTransport(Vec::new())) };
    let t = Targets {
        daemons_path: fx.daemons.clone(),
        models_dir: fx.models.clone(),
        bundled_models: None,
        usage: &usage,
        selection: &serve,
        model_transport: &transport,
    };
    assert!(remove_sync(&fx.root, &t, "tiny-model", NOW_MS).is_ok());
    assert!(!chosen.exists());
    assert_ne!(serve.current_model_path(), chosen, "the selection no longer points at the deleted file");
    assert_eq!(serve.current_model_path(), fx.models.join(crate::model::MODEL_FILE));
    assert!(serve.lora().is_none(), "the adapter is dropped with it");
}

#[test]
fn tripwire_every_model_delete_goes_through_the_shared_operation() {
    // `model_delete::delete` removes the files only; the shared `delete_and_forget` also resets
    // the chat server's selection. No production caller may use the bare one.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&dir).expect("src") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(!text.contains("model_delete::delete("), "{name}: use model_delete::delete_and_forget");
        if name == "model_delete.rs" {
            let bare = text.matches("delete(&").count() + text.matches("delete(models_dir").count();
            assert_eq!(bare, 1, "model_delete.rs: only delete_and_forget calls the bare delete");
            assert!(text.contains("delete_and_forget(&models"), "the Models screen command uses the shared operation");
        }
    }
}

// ---- Codex P2 on #356 at 175c9ce: a model record owns only the bytes it installed ----------

/// Put a different verified model at `file` the way the Models screen does (its own bytes, its
/// own pinned hash, verified by `ModelManager`).
fn replace_model_outside_the_store(fx: &Fx, file: &str, body: &[u8]) {
    std::fs::write(fx.models.join(file), body).expect("bytes");
    crate::model::ModelManager::new(
        fx.models.clone(),
        Box::new(MemTransport(Vec::new())),
        sha(body),
        body.len() as u64,
    )
    .with_file(file.to_string())
    .verify()
    .expect("the replacement verifies against its own hash");
    assert!(crate::model::is_file_ready(&fx.models, file));
}

/// Store install A, delete it in Models, then a different model B lands under the same file name.
fn replaced_model(tag: &str, b: &[u8]) -> Fx {
    let fx = Fx::standard(tag);
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    fx.fx_install(&st.stage_id, 0).expect("install");
    crate::model_delete::delete_and_forget(&fx.models, None, "tiny-model.gguf", &Usage(None), &fx.serve)
        .expect("deleted in Models");
    replace_model_outside_the_store(&fx, "tiny-model.gguf", b);
    fx
}

#[test]
fn a_replacement_model_under_the_same_name_is_not_reported_or_removed_as_the_store_install() {
    // Same size, one byte different (a size check alone would not tell them apart), and a
    // different size.
    let mut same_size = model_bytes();
    same_size[20] = 9;
    let mut longer = model_bytes();
    longer.extend_from_slice(b"more");
    for (tag, b) in [("replaced-same-size", same_size), ("replaced-longer", longer)] {
        let fx = replaced_model(tag, &b);
        let listed = fx.fx_installed();
        assert_eq!(listed.len(), 1, "{tag}");
        assert_eq!(listed[0].state, "missing", "{tag}: B is not the Store's install");
        fx.fx_remove("tiny-model", &Usage(None)).expect("the stale record is removable");
        assert_eq!(std::fs::read(fx.models.join("tiny-model.gguf")).expect("B kept"), b, "{tag}");
        assert!(crate::model::is_file_ready(&fx.models, "tiny-model.gguf"), "{tag}: B still verified");
        assert!(read_store_file(&fx.root).expect("state").installs.is_empty(), "{tag}: only the record went");
    }
}

#[test]
fn a_pending_record_never_removes_a_verified_replacement() {
    let mut other = model_bytes();
    other[30] = 3;
    let fx = replaced_model("replaced-pending", &other);
    update_installs(&fx.root, |i| {
        for r in i.iter_mut() {
            r.pending = true;
        }
    })
    .expect("pending");
    assert_eq!(fx.fx_installed()[0].state, "missing");
    fx.fx_remove("tiny-model", &Usage(None)).expect("removable");
    assert_eq!(std::fs::read(fx.models.join("tiny-model.gguf")).expect("kept"), other);
}

#[test]
fn a_model_whose_verified_flag_names_no_hash_is_not_claimed_by_the_store() {
    // A status file that records only `verified` (no hash) says nothing about which bytes were
    // verified, so the Store does not claim the file: it shows missing and Remove keeps it.
    let fx = Fx::standard("no-hash");
    fx.fx_view(0);
    let st = fx.fx_stage("tiny-model", 0).expect("stage");
    fx.fx_install(&st.stage_id, 0).expect("install");
    std::fs::write(fx.models.join("tiny-model.gguf.status.json"), br#"{"verified":true}"#).expect("status");
    assert!(crate::model::is_file_ready(&fx.models, "tiny-model.gguf"));
    assert_eq!(fx.fx_installed()[0].state, "missing");
    fx.fx_remove("tiny-model", &Usage(None)).expect("removable");
    assert!(fx.models.join("tiny-model.gguf").exists(), "an unclaimed file is never deleted");
}

#[test]
fn the_store_install_itself_is_still_reported_and_removed() {
    // The same bytes the listing pins, verified again (e.g. re-downloaded in Models): that is the
    // content the Store installed, so it is reported and removable as before.
    let fx = replaced_model("replaced-identical", &model_bytes());
    assert_eq!(fx.fx_installed()[0].state, "not_selected");
    fx.fx_remove("tiny-model", &Usage(None)).expect("remove");
    assert!(!fx.models.join("tiny-model.gguf").exists());
}
