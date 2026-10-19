// GROW-S1 task 12 — the pending v2 invite store. Included into `pending_join::tests`.
//
// Pins: the invite (code + link key) lives only in the keyring slot (or Rust memory when the keyring
// is unreachable), never in what crosses `invoke`; it is wiped on clear, on expiry and on the
// redeem of the same code; a malformed link is refused before anything is stored; and nothing in
// this module prints.

use super::*;
use citrate_core_kit::custody::{CustodyError, Keyring};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const CODE: &str = "Ab3dE7gh";
const KEY: &str = "AAECAwQFBgcICQoLDA0ODw"; // 16 bytes 0..15, canonical base64url
const T0: u64 = 1_800_000_000;

#[derive(Default)]
struct FakeKeyring {
    slots: Mutex<HashMap<String, Vec<u8>>>,
    dead: bool,
}

struct Handle(Arc<FakeKeyring>);

impl Keyring for Handle {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        if self.0.dead {
            return Err(CustodyError::KeyringUnavailable);
        }
        Ok(self.0.slots.lock().expect("lock").get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        if self.0.dead {
            return Err(CustodyError::KeyringUnavailable);
        }
        self.0.slots
            .lock()
            .expect("lock")
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        if self.0.dead {
            return Err(CustodyError::KeyringUnavailable);
        }
        self.0.slots.lock().expect("lock").remove(account);
        Ok(())
    }
}

struct Rig {
    kr: Arc<FakeKeyring>,
    clock: Arc<AtomicU64>,
    store: PendingJoinStore,
}

fn rig_with(kr: FakeKeyring) -> Rig {
    let kr = Arc::new(kr);
    let clock = Arc::new(AtomicU64::new(T0));
    let c = clock.clone();
    let store = PendingJoinStore::new(
        Box::new(Handle(kr.clone())),
        Box::new(move || c.load(Ordering::SeqCst)),
    );
    Rig { kr, clock, store }
}

fn rig() -> Rig {
    rig_with(FakeKeyring::default())
}

fn slot(r: &Rig) -> Option<Vec<u8>> {
    r.kr.slots
        .lock()
        .expect("lock")
        .get(PENDING_JOIN_ACCOUNT)
        .cloned()
}

fn link() -> String {
    format!("https://citrate.ai/join/{CODE}#{KEY}")
}

#[test]
fn set_accepts_every_link_shape_and_stores_code_and_key_in_the_keyring_slot() {
    for l in [
        link(),
        format!("https://www.citrate.ai/join/{CODE}#{KEY}"),
        format!("citrate://join/{CODE}#{KEY}"),
        format!("{CODE}#{KEY}"),
        format!("  citrate://join/{CODE}#{KEY}\n"),
    ] {
        let r = rig();
        let v = r.store.set(&l).expect("set");
        assert!(v.has_key, "{l}");
        assert!(v.persisted);
        assert_eq!(v.stored_at, T0);
        assert_eq!(v.expires_at, T0 + PENDING_JOIN_TTL_SECS);
        let raw = String::from_utf8(slot(&r).expect("slot")).expect("utf8");
        assert!(raw.contains(&format!("{CODE}#{KEY}")), "canonical form kept");
        assert_eq!(
            r.store.link().as_deref().map(|s| s.as_str()),
            Some(format!("{CODE}#{KEY}").as_str())
        );
    }
}

#[test]
fn a_code_without_its_key_is_kept_and_reported_as_keyless() {
    let r = rig();
    let v = r.store.set(&format!("https://citrate.ai/join/{CODE}")).expect("set");
    assert!(!v.has_key);
    assert_eq!(r.store.get().map(|g| g.has_key), Some(false));
}

#[test]
fn malformed_links_are_refused_and_nothing_is_stored() {
    let r = rig();
    for bad in [
        "",
        "hello",
        "https://evil.example/join/Ab3dE7gh#AAECAwQFBgcICQoLDA0ODw",
        "https://citrate.ai/join/Ab3dE7gh#short",
        "citrate://invite?g=a&t=b",
        "https://citrate.ai/join/0O1lI000",
    ] {
        assert!(r.store.set(bad).is_err(), "{bad:?}");
    }
    let huge = format!("https://citrate.ai/join/{CODE}#{}", "A".repeat(4096));
    assert!(r.store.set(&huge).is_err());
    assert!(slot(&r).is_none());
    assert!(r.store.get().is_none());
}

#[test]
fn a_new_link_replaces_the_old_one() {
    let r = rig();
    r.store.set(&link()).expect("set");
    r.store.set("citrate://join/Zz9yX8wv").expect("set 2");
    assert_eq!(
        r.store.link().as_deref().map(|s| s.as_str()),
        Some("Zz9yX8wv")
    );
}

#[test]
fn the_view_that_crosses_invoke_never_carries_the_code_or_the_key() {
    let r = rig();
    let v = r.store.set(&link()).expect("set");
    let json = serde_json::to_string(&v).expect("json");
    assert!(!json.contains(KEY), "{json}");
    assert!(!json.contains(CODE), "{json}");
    let obj = serde_json::to_value(&v).expect("json");
    let mut keys: Vec<String> = obj
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    assert_eq!(keys, vec!["expiresAt", "hasKey", "persisted", "storedAt"]);
}

#[test]
fn debug_output_is_redacted() {
    let r = rig();
    r.store.set(&link()).expect("set");
    let dbg = format!("{:?}", r.store.load());
    assert!(!dbg.contains(KEY), "{dbg}");
    assert!(!dbg.contains(CODE), "{dbg}");
}

#[test]
fn clear_wipes_the_keyring_slot() {
    let r = rig();
    r.store.set(&link()).expect("set");
    r.store.clear().expect("clear");
    assert!(slot(&r).is_none());
    assert!(r.store.get().is_none());
    assert!(r.store.link().is_none());
}

#[test]
fn an_expired_invite_is_wiped_on_read() {
    let r = rig();
    r.store.set(&link()).expect("set");
    r.clock
        .store(T0 + PENDING_JOIN_TTL_SECS, Ordering::SeqCst);
    assert!(r.store.get().is_none());
    assert!(slot(&r).is_none(), "wiped, not just hidden");
    assert!(r.store.link().is_none());
}

#[test]
fn the_signed_expiry_can_only_shorten_the_hold() {
    let r = rig();
    r.store.set(&link()).expect("set");
    r.store.cap_expiry(T0 + 60);
    assert_eq!(r.store.get().map(|v| v.expires_at), Some(T0 + 60));
    r.store.cap_expiry(T0 + PENDING_JOIN_TTL_SECS * 10);
    assert_eq!(r.store.get().map(|v| v.expires_at), Some(T0 + 60));
    r.clock.store(T0 + 61, Ordering::SeqCst);
    assert!(r.store.get().is_none());
    assert!(slot(&r).is_none());
}

#[test]
fn forget_code_wipes_only_a_matching_invite() {
    let r = rig();
    r.store.set(&link()).expect("set");
    r.store.forget_code("Zz9yX8wv");
    assert!(r.store.get().is_some(), "a different code leaves it");
    r.store.forget_code(CODE);
    assert!(r.store.get().is_none());
    assert!(slot(&r).is_none());
}

#[test]
fn an_unreachable_keyring_holds_the_invite_in_memory_and_says_so() {
    let r = rig_with(FakeKeyring {
        dead: true,
        ..Default::default()
    });
    let v = r.store.set(&link()).expect("set");
    assert!(!v.persisted);
    assert_eq!(r.store.get().map(|g| g.persisted), Some(false));
    assert!(r.store.link().is_some());
    // The keyring delete reports the outage, but the memory copy is gone either way.
    assert!(r.store.clear().is_err());
    assert!(r.store.get().is_none());
}

#[test]
fn a_corrupt_slot_is_dropped_not_trusted() {
    let r = rig();
    r.kr.slots
        .lock()
        .expect("lock")
        .insert(PENDING_JOIN_ACCOUNT.to_string(), b"not json".to_vec());
    assert!(r.store.get().is_none());
    assert!(slot(&r).is_none());
}

#[test]
fn this_module_never_prints() {
    let src = include_str!("pending_join.rs");
    for needle in ["println!", "eprintln!", "print!(", "dbg!", "log::", "tracing::"] {
        assert!(!src.contains(needle), "pending_join.rs must not use {needle}");
    }
}

#[test]
fn pending_join_commands_are_registered_and_allowed_for_the_main_window() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in [
        "pending_join_set",
        "pending_join_get",
        "pending_join_clear",
        "pending_join_resolve",
        "join_share_qr",
    ] {
        assert!(
            lib.contains(&format!("::{cmd},")),
            "{cmd} registered in lib.rs"
        );
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
    }
    assert!(lib.contains("app.manage(pending_join::build_pending_join_state());"));
}

#[test]
fn redeem_wipes_the_pending_invite_for_that_code() {
    let join_src = include_str!("join.rs");
    assert!(join_src.contains("crate::pending_join::forget_code(&app, &code)"));
}
