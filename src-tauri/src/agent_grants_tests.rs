// HUP-S2.1 (wiring) — citrate-core's folder-grant store: the versioned grant document in app data,
// fail-closed loading (a corrupted file grants nothing and says so), folder grants with separate
// read and write, revocation, and the read-only 24 h full-access window behind a one-shot,
// time-boxed HIC-1 confirmation. The document format is the citrate-agent-grants `GrantState`;
// `tests/fixtures/agent-grants/core-grant-state-v1.json` is the cross-repo contract fixture.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const T0: u64 = 1_790_000_000;

/// `base/home` (the member's home, with `.ssh`), `base/home/work/app` (a project),
/// `base/appdata` (core's app data, where the store lives).
struct Fx {
    base: PathBuf,
}
impl Fx {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "core-agent-grants-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["home/work/app/src", "home/.ssh", "home/.aws", "appdata"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        Fx {
            base: base.canonicalize().unwrap(),
        }
    }
    fn home(&self) -> PathBuf {
        self.base.join("home")
    }
    fn app(&self) -> PathBuf {
        self.base.join("home/work/app")
    }
    fn store(&self) -> GrantStore {
        GrantStore::new(self.base.join("appdata"), self.home())
    }
    fn file(&self) -> PathBuf {
        self.base.join("appdata").join(GRANTS_FILE)
    }
}
impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn ok(st: Loaded) -> GrantState {
    match st {
        Loaded::Ok(s) => s,
        Loaded::Corrupted(e) => panic!("expected a valid document, got corrupted: {e}"),
    }
}

#[test]
fn no_file_means_no_grants() {
    let fx = Fx::new();
    let st = ok(fx.store().load());
    assert_eq!(st, GrantState::default());
    assert_eq!(st.version, 1);
    let v = fx.store().view(T0);
    assert_eq!(v.status, "ok");
    assert!(v.grants.is_empty());
    assert!(v.error.is_none());
}

#[test]
fn granting_a_folder_creates_separate_read_and_write_grants_and_persists_them() {
    let fx = Fx::new();
    let store = fx.store();
    let ids = store.add_folder(&fx.app(), true, true, T0).unwrap();
    assert_eq!(ids, vec!["g-1".to_string(), "g-2".to_string()]);
    let st = ok(store.load());
    assert_eq!(st.next_id, 3);
    assert_eq!(st.grants[0].access, Access::Read);
    assert_eq!(st.grants[1].access, Access::Write);
    for g in &st.grants {
        assert_eq!(g.kind, GrantKind::Folder);
        assert_eq!(g.root, fx.app().display().to_string());
        assert_eq!(g.scope, "subtree");
        assert_eq!(g.expires_at, None);
        assert_eq!(g.revoked_at, None);
        assert!(!g.granted_by.trim().is_empty() && !g.reason.trim().is_empty());
    }
    // Read only: one grant.
    let ids = store.add_folder(&fx.app(), true, false, T0).unwrap();
    assert_eq!(ids, vec!["g-3".to_string()]);
    assert!(matches!(
        store.add_folder(&fx.app(), false, false, T0),
        Err(GrantsError::Invalid(_))
    ));
}

#[cfg(unix)]
#[test]
fn the_store_file_is_private_to_the_member() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    fx.store().add_folder(&fx.app(), true, false, T0).unwrap();
    let mode = std::fs::metadata(fx.file()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn a_symlinked_folder_is_stored_as_its_target() {
    let fx = Fx::new();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fx.app(), fx.home().join("shortcut")).unwrap();
        fx.store()
            .add_folder(&fx.home().join("shortcut"), true, false, T0)
            .unwrap();
        let st = ok(fx.store().load());
        assert_eq!(st.grants[0].root, fx.app().display().to_string());
    }
}

#[test]
fn credential_folders_app_data_missing_folders_and_root_writes_are_refused() {
    let fx = Fx::new();
    let store = fx.store();
    for bad in [
        fx.home().join(".ssh"),
        fx.home().join(".aws"),
        fx.base.join("appdata"),
        fx.home().join("missing"),
    ] {
        assert!(
            store.add_folder(&bad, true, false, T0).is_err(),
            "{} must be refused",
            bad.display()
        );
    }
    std::fs::write(fx.home().join("file.txt"), "x").unwrap();
    assert!(store
        .add_folder(&fx.home().join("file.txt"), true, false, T0)
        .is_err());
    assert!(store.add_folder(Path::new("/"), false, true, T0).is_err());
    assert!(store
        .add_folder(Path::new("relative/dir"), true, false, T0)
        .is_err());
    assert!(ok(store.load()).grants.is_empty(), "nothing was stored");
}

#[test]
fn revoking_is_immediate_final_and_kept_for_the_list() {
    let fx = Fx::new();
    let store = fx.store();
    store.add_folder(&fx.app(), true, false, T0).unwrap();
    store.revoke("g-1", T0 + 5).unwrap();
    let st = ok(store.load());
    assert_eq!(st.grants[0].revoked_at, Some(T0 + 5));
    assert!(matches!(
        store.revoke("g-1", T0 + 6),
        Err(GrantsError::Invalid(_))
    ));
    assert!(matches!(
        store.revoke("g-9", T0 + 6),
        Err(GrantsError::Invalid(_))
    ));
    let v = store.view(T0 + 10);
    assert_eq!(v.grants[0].status, "revoked");
}

#[test]
fn a_corrupted_file_grants_nothing_and_says_so() {
    let fx = Fx::new();
    let store = fx.store();
    std::fs::write(fx.file(), "{ not json").unwrap();
    assert!(matches!(store.load(), Loaded::Corrupted(_)));
    let v = store.view(T0);
    assert_eq!(v.status, "corrupted");
    assert!(v.error.as_deref().unwrap_or("").contains("grants nothing"));
    assert!(v.grants.is_empty());
    // The agent receives an empty document.
    assert_eq!(store.document_for_agent(), GrantState::default());
    // No change is written over a corrupted file.
    assert!(matches!(
        store.add_folder(&fx.app(), true, false, T0),
        Err(GrantsError::Corrupted(_))
    ));
    assert_eq!(std::fs::read_to_string(fx.file()).unwrap(), "{ not json");
    // Reset keeps the bad file aside and starts empty.
    let kept = store.reset_corrupted(T0).unwrap();
    assert!(kept.exists());
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "{ not json");
    assert_eq!(ok(store.load()), GrantState::default());
    store.add_folder(&fx.app(), true, false, T0).unwrap();
}

#[test]
fn tampered_documents_are_refused_whole() {
    let fx = Fx::new();
    let store = fx.store();
    let good = || {
        let mut s = GrantState {
            next_id: 2,
            ..GrantState::default()
        };
        s.grants.push(Grant {
            id: "g-1".into(),
            kind: GrantKind::FullAccess,
            root: fx.home().display().to_string(),
            access: Access::Read,
            scope: "subtree".into(),
            granted_at: T0,
            expires_at: Some(T0 + FULL_ACCESS_SECS),
            granted_by: "member".into(),
            reason: "r".into(),
            revoked_at: None,
        });
        s
    };
    store.save(&good()).unwrap();
    assert!(matches!(store.load(), Loaded::Ok(_)));
    type Mutant = Box<dyn Fn(&mut GrantState)>;
    let mutants: Vec<Mutant> = vec![
        Box::new(|s| s.version = 2),
        Box::new(|s| s.grants[0].access = Access::Write),
        Box::new(|s| s.grants[0].expires_at = Some(T0 + FULL_ACCESS_SECS + 1)),
        Box::new(|s| s.grants[0].expires_at = None),
        Box::new(|s| s.grants[0].expires_at = Some(T0)),
        Box::new(|s| s.grants[0].root = "relative".into()),
        Box::new(|s| s.grants[0].root = "/a/../b".into()),
        Box::new(|s| s.grants[0].id = "g-2".into()),
        Box::new(|s| s.grants[0].id = "x".into()),
        Box::new(|s| s.grants[0].granted_by = " ".into()),
        Box::new(|s| s.grants[0].reason = "".into()),
        Box::new(|s| s.grants[0].scope = "everything".into()),
        Box::new(|s| {
            let g = s.grants[0].clone();
            s.grants.push(g);
        }),
    ];
    for (i, m) in mutants.iter().enumerate() {
        let mut s = good();
        m(&mut s);
        std::fs::write(fx.file(), serde_json::to_string(&s).unwrap()).unwrap();
        assert!(
            matches!(store.load(), Loaded::Corrupted(_)),
            "mutant {i} must be refused"
        );
    }
    // Unknown fields are refused too.
    let mut v = serde_json::to_value(good()).unwrap();
    v["grants"][0]["extra"] = serde_json::json!(true);
    std::fs::write(fx.file(), v.to_string()).unwrap();
    assert!(matches!(store.load(), Loaded::Corrupted(_)));
}

#[test]
fn full_access_needs_a_one_shot_confirmation_and_is_read_only_for_24_hours() {
    let fx = Fx::new();
    let store = fx.store();
    let c = store.full_access_prepare(T0).unwrap();
    assert_eq!(c.root, fx.home().display().to_string());
    assert_eq!(c.grant_expires_at, T0 + FULL_ACCESS_SECS);
    assert_eq!(c.confirm_by, T0 + CONFIRM_WINDOW_SECS);
    assert!(c.statement.contains("read"));
    assert!(c.statement.contains("cannot change"));
    // A wrong id does nothing.
    assert!(store.full_access_confirm("not-the-id", T0 + 1).is_err());
    assert!(ok(store.load()).grants.is_empty());
    let id = store.full_access_confirm(&c.id, T0 + 1).unwrap();
    let st = ok(store.load());
    let g = st.grants.iter().find(|g| g.id == id).unwrap();
    assert_eq!(g.kind, GrantKind::FullAccess);
    assert_eq!(g.access, Access::Read);
    assert_eq!(g.granted_at, T0 + 1);
    assert_eq!(g.expires_at, Some(T0 + 1 + FULL_ACCESS_SECS));
    // One shot.
    assert!(store.full_access_confirm(&c.id, T0 + 2).is_err());
    // While it is on, it cannot be stacked.
    assert!(store.full_access_prepare(T0 + 3).is_err());
    // The countdown, then expiry.
    let v = store.view(T0 + 1 + 3600);
    assert_eq!(v.grants[0].status, "active");
    assert_eq!(v.grants[0].remaining_secs, Some(FULL_ACCESS_SECS - 3600));
    assert_eq!(v.full_access_remaining_secs, Some(FULL_ACCESS_SECS - 3600));
    let v = store.view(T0 + 1 + FULL_ACCESS_SECS);
    assert_eq!(v.grants[0].status, "expired");
    assert_eq!(v.full_access_remaining_secs, None);
    // After expiry it may be turned on again.
    assert!(store.full_access_prepare(T0 + 1 + FULL_ACCESS_SECS).is_ok());
}

#[test]
fn a_confirmation_expires() {
    let fx = Fx::new();
    let store = fx.store();
    let c = store.full_access_prepare(T0).unwrap();
    assert!(store
        .full_access_confirm(&c.id, T0 + CONFIRM_WINDOW_SECS)
        .is_err());
    assert!(ok(store.load()).grants.is_empty());
    // A newer prepare replaces an older one.
    let a = store.full_access_prepare(T0).unwrap();
    let b = store.full_access_prepare(T0).unwrap();
    assert_ne!(a.id, b.id);
    assert!(store.full_access_confirm(&a.id, T0 + 1).is_err());
    assert!(store.full_access_confirm(&b.id, T0 + 1).is_ok());
}

#[test]
fn the_document_matches_the_runtime_contract_fixture() {
    let text = include_str!("../tests/fixtures/agent-grants/core-grant-state-v1.json");
    let parsed: GrantState = serde_json::from_str(text).unwrap();
    let again = serde_json::to_value(&parsed).unwrap();
    let orig: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        again, orig,
        "core writes exactly the fields agent-grants reads"
    );
    // And the store's own writes use the same shape.
    let fx = Fx::new();
    fx.store().add_folder(&fx.app(), true, false, T0).unwrap();
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fx.file()).unwrap()).unwrap();
    let keys: Vec<&str> = written["grants"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    let want: Vec<&str> = orig["grants"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    let (mut k, mut w) = (keys.clone(), want.clone());
    k.sort();
    w.sort();
    assert_eq!(k, w);
}

// --- sending the document to the agent sessions -------------------------------------------------

use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Rec {
    posts: StdMutex<Vec<(String, String)>>,
    reply: StdMutex<Vec<(u16, String)>>,
}
struct RecControl(std::sync::Arc<Rec>);
impl HermesControl for RecControl {
    fn get(&self, _url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp {
            status: 200,
            body: "{}".into(),
        })
    }
    fn post(
        &self,
        url: &str,
        _b: &str,
        body: &str,
    ) -> std::result::Result<ControlResp, HermesError> {
        self.0
            .posts
            .lock()
            .unwrap()
            .push((url.to_string(), body.to_string()));
        let (status, body) = self
            .0
            .reply
            .lock()
            .unwrap()
            .pop()
            .unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
}

fn hermes(rec: std::sync::Arc<Rec>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!(
        "core-grants-hermes-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

#[test]
fn a_session_opens_with_the_document_and_receives_every_change() {
    let fx = Fx::new();
    let store = fx.store();
    store.add_folder(&fx.app(), true, false, T0).unwrap();
    let rec = std::sync::Arc::new(Rec::default());
    rec.reply
        .lock()
        .unwrap()
        .push((201, r#"{"id":"s1-ab"}"#.into()));
    let m = hermes(rec.clone());
    let body = attach_grants(r#"{"model":"m"}"#, &store.document_for_agent()).unwrap();
    let id = m.session_open(&body).unwrap();
    assert_eq!(id, "s1-ab");
    {
        let posts = rec.posts.lock().unwrap();
        let sent: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
        assert_eq!(sent["model"], "m");
        assert_eq!(
            sent["grants"]["grants"][0]["root"],
            fx.app().display().to_string()
        );
    }
    // A change goes to every open session with the whole new document.
    store.revoke("g-1", T0 + 1).unwrap();
    let out = m.push_grants(&store.document_for_agent());
    assert_eq!(out.updated, 1);
    assert!(out.failed.is_empty());
    let posts = rec.posts.lock().unwrap();
    assert_eq!(
        posts[1].0,
        format!("{}/sessions/s1-ab/grants", m.control_url())
    );
    let sent: serde_json::Value = serde_json::from_str(&posts[1].1).unwrap();
    assert_eq!(sent["grants"][0]["revoked_at"], T0 + 1);
}

#[test]
fn a_closed_session_is_forgotten_and_a_refusal_is_reported() {
    let rec = std::sync::Arc::new(Rec::default());
    let m = hermes(rec.clone());
    // Two sessions open (replies pop from the end).
    rec.reply
        .lock()
        .unwrap()
        .push((201, r#"{"id":"s2-cd"}"#.into()));
    rec.reply
        .lock()
        .unwrap()
        .push((201, r#"{"id":"s1-ab"}"#.into()));
    let doc = GrantState::default();
    m.session_open(&attach_grants("{}", &doc).unwrap()).unwrap();
    m.session_open(&attach_grants("{}", &doc).unwrap()).unwrap();
    // s1 is gone (404), s2 refuses the document (400).
    rec.reply
        .lock()
        .unwrap()
        .push((400, r#"{"error":"bad document"}"#.into()));
    rec.reply.lock().unwrap().push((404, "".into()));
    let out = m.push_grants(&doc);
    assert_eq!(out.updated, 0);
    assert_eq!(out.failed.len(), 1);
    assert!(out.failed[0].contains("bad document"));
    // s1 is no longer pushed to; s2 still is.
    let out = m.push_grants(&doc);
    assert_eq!(out.updated, 1);
    let posts = rec.posts.lock().unwrap();
    assert!(posts.last().unwrap().0.ends_with("/sessions/s2-cd/grants"));
}

#[test]
fn with_no_sidecar_running_a_change_is_stored_and_reported_as_not_sent() {
    let dir = std::env::temp_dir().join(format!(
        "core-grants-hermes-off-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    let out = m.push_grants(&GrantState::default());
    assert_eq!(out.updated, 0);
    assert!(out.failed.is_empty());
}
