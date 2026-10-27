// GROW-S1 task 12 — the pending v2 invite store. Included into `pending_join::tests`.
//
// Pins: the invite (code + link key) lives only in the keyring slot (or Rust memory when the keyring
// is unreachable), never in what crosses `invoke`; it is wiped on clear, on expiry and on the
// redeem of the same code; a malformed link is refused before anything is stored; and nothing in
// this module prints.

use super::*;
use citrate_core_kit::custody::{CustodyError, Keyring};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const CODE: &str = "Ab3dE7gh";
const KEY: &str = "AAECAwQFBgcICQoLDA0ODw"; // 16 bytes 0..15, canonical base64url
const T0: u64 = 1_800_000_000;

#[derive(Default)]
struct FakeKeyring {
    slots: Mutex<HashMap<String, Vec<u8>>>,
    /// The keyring is unreachable while this is set (it can come back).
    dead: AtomicBool,
}

struct Handle(Arc<FakeKeyring>);

impl Keyring for Handle {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        if self.0.dead.load(Ordering::SeqCst) {
            return Err(CustodyError::KeyringUnavailable);
        }
        Ok(self.0.slots.lock().expect("lock").get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        if self.0.dead.load(Ordering::SeqCst) {
            return Err(CustodyError::KeyringUnavailable);
        }
        self.0.slots
            .lock()
            .expect("lock")
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        if self.0.dead.load(Ordering::SeqCst) {
            return Err(CustodyError::KeyringUnavailable);
        }
        self.0.slots.lock().expect("lock").remove(account);
        Ok(())
    }
}

struct Rig {
    kr: Arc<FakeKeyring>,
    clock: Arc<AtomicU64>,
    /// Every code the store told the join client to forget, in order.
    forgot: Arc<Mutex<Vec<String>>>,
    store: PendingJoinStore,
}

fn rig_with(kr: FakeKeyring) -> Rig {
    let kr = Arc::new(kr);
    let clock = Arc::new(AtomicU64::new(T0));
    let c = clock.clone();
    let forgot = Arc::new(Mutex::new(Vec::new()));
    let f = forgot.clone();
    let store = PendingJoinStore::new(
        Box::new(Handle(kr.clone())),
        Box::new(move || c.load(Ordering::SeqCst)),
        Box::new(move |code: &str| f.lock().expect("lock").push(code.to_string())),
    );
    Rig {
        kr,
        clock,
        forgot,
        store,
    }
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
    let dbg = {
        let mut g = r.store.lock();
        format!("{:?}", r.store.load(&mut g))
    };
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
    let cap = |exp: &str| {
        r.store
            .resolve_held(|_| Ok(view_for(exp)))
            .expect("resolved");
    };
    cap(EXP_T0_PLUS_60);
    assert_eq!(r.store.get().map(|v| v.expires_at), Some(T0 + 60));
    cap("2100-01-01T00:00:00.000Z");
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
        dead: AtomicBool::new(true),
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
    assert!(lib.contains(
        "app.manage(pending_join::build_pending_join_state(app.handle().clone()));"
    ));
    // The production hook drops the join client's opened invite for the code.
    let src = include_str!("pending_join.rs");
    let build = &src[src.find("pub fn build_pending_join_state").expect("fn")..];
    let build = &build[..build.find("\n}\n").expect("end")];
    assert!(build.contains("join.0.forget_opened(code)"), "{build}");
}

#[test]
fn redeem_wipes_the_pending_invite_for_that_code() {
    let join_src = include_str!("join.rs");
    assert!(join_src.contains("crate::pending_join::forget_code(&app, &code)"));
}

fn view_for(exp: &str) -> ResolvedView {
    ResolvedView {
        code: CODE.to_string(),
        kind: join::JoinKind::Direct,
        cluster_name: Some("Garden club".to_string()),
        goal: None,
        inviter: Some("@dana".to_string()),
        inviter_verified: true,
        exp: exp.to_string(),
        actionable: true,
        key_missing: false,
    }
}

/// `T0` is 2027-01-15T08:00:00Z, so this signed expiry is `T0 + 60`.
const EXP_T0_PLUS_60: &str = "2027-01-15T08:01:00.000Z";

#[test]
fn the_exp_fixture_is_t0_plus_a_minute() {
    assert_eq!(join::parse_iso_utc_secs(EXP_T0_PLUS_60), Some(T0 + 60));
}

/// The command path: Review resolves the held link (the resolver sees exactly the canonical
/// `<code>#K`) and the signed expiry caps the hold.
#[test]
fn resolve_caps_the_hold_to_the_signed_expiry() {
    let r = rig();
    r.store.set(&link()).expect("set");
    let mut seen = String::new();
    let v = r
        .store
        .resolve_held(|l| {
            seen = l.to_string();
            Ok(view_for(EXP_T0_PLUS_60))
        })
        .expect("resolved");
    assert_eq!(seen, format!("{CODE}#{KEY}"));
    assert_eq!(v.code, CODE);
    assert_eq!(r.store.get().map(|g| g.expires_at), Some(T0 + 60));
}

/// The command path: a code the service reports gone is wiped (memory and keyring slot); any
/// other failure keeps it so Review can be retried.
#[test]
fn resolve_not_found_wipes_the_hold_and_other_errors_keep_it() {
    for (e, kept) in [
        (JoinError::Unavailable, true),
        (JoinError::Network, true),
        (JoinError::RateLimited, true),
        (JoinError::Signature, true),
        (JoinError::NotFound, false),
    ] {
        let r = rig();
        r.store.set(&link()).expect("set");
        let err = r
            .store
            .resolve_held(|_| Err(e.clone()))
            .expect_err("error passes through");
        assert_eq!(err, e.to_string());
        assert!(!err.contains(KEY) && !err.contains(CODE), "{err}");
        assert_eq!(r.store.get().is_some(), kept, "{e:?}");
        assert_eq!(slot(&r).is_some(), kept, "{e:?}");
    }
}

#[test]
fn resolve_with_nothing_held_never_calls_the_service() {
    let r = rig();
    let mut called = false;
    let err = r
        .store
        .resolve_held(|_| {
            called = true;
            Ok(view_for(EXP_T0_PLUS_60))
        })
        .expect_err("nothing waiting");
    assert!(err.contains("no invite is waiting"));
    assert!(!called);
}

/// Interleaving: Review for invite A is in flight when a deep link replaces it with B. A's
/// "not found" must not wipe B.
#[test]
fn a_not_found_for_a_replaced_invite_does_not_wipe_the_new_one() {
    let r = rig();
    r.store.set(&link()).expect("set A");
    let _ = r.store.resolve_held(|_| {
        r.store.set("citrate://join/Zz9yX8wv").expect("set B");
        Err(JoinError::NotFound)
    });
    assert_eq!(
        r.store.link().as_deref().map(|s| s.as_str()),
        Some("Zz9yX8wv"),
        "B is still held"
    );
    assert!(slot(&r).is_some());
}

/// Interleaving: A's signed expiry must not shorten B's hold, and A's details are not handed
/// back as if they were B's.
#[test]
fn a_resolve_for_a_replaced_invite_neither_caps_nor_answers_for_the_new_one() {
    let r = rig();
    r.store.set(&link()).expect("set A");
    r.clock.store(T0 + 5, Ordering::SeqCst);
    let out = r.store.resolve_held(|_| {
        r.store.set("citrate://join/Zz9yX8wv").expect("set B");
        Ok(view_for(EXP_T0_PLUS_60))
    });
    assert_eq!(
        r.store.get().map(|g| g.expires_at),
        Some(T0 + 5 + PENDING_JOIN_TTL_SECS),
        "B keeps its own hold"
    );
    let err = out.expect_err("a stale answer is not returned");
    assert!(err.contains("changed"), "{err}");
}

/// Interleaving: a Cancel while Review is in flight stays cancelled.
#[test]
fn a_cancel_during_resolve_stays_cancelled() {
    let r = rig();
    r.store.set(&link()).expect("set");
    let out = r.store.resolve_held(|_| {
        r.store.clear().expect("clear");
        Ok(view_for(EXP_T0_PLUS_60))
    });
    assert!(out.is_err());
    assert!(r.store.get().is_none());
    assert!(slot(&r).is_none());
}

/// Cancel during a keyring outage: the persisted invite can't be deleted yet. It must not come
/// back once the keyring does, and the delete is retried then.
#[test]
fn a_cancel_during_a_keyring_outage_does_not_come_back() {
    let r = rig();
    r.store.set(&link()).expect("set");
    assert!(slot(&r).is_some());
    r.kr.dead.store(true, Ordering::SeqCst);
    assert!(r.store.clear().is_err(), "the outage is reported");
    assert!(r.store.get().is_none());
    r.kr.dead.store(false, Ordering::SeqCst);
    assert!(r.store.get().is_none(), "not resurrected");
    assert!(r.store.link().is_none());
    assert!(slot(&r).is_none(), "the delete was retried");
}

/// A new invite after a failed cancel is a new invite: it is held, and the old slot stays dead.
#[test]
fn a_new_invite_after_a_failed_cancel_is_held() {
    let r = rig();
    r.store.set(&link()).expect("set A");
    r.kr.dead.store(true, Ordering::SeqCst);
    assert!(r.store.clear().is_err());
    let v = r.store.set("citrate://join/Zz9yX8wv").expect("set B");
    assert!(!v.persisted);
    assert_eq!(
        r.store.link().as_deref().map(|s| s.as_str()),
        Some("Zz9yX8wv")
    );
    r.kr.dead.store(false, Ordering::SeqCst);
    assert_eq!(
        r.store.link().as_deref().map(|s| s.as_str()),
        Some("Zz9yX8wv"),
        "B, never A"
    );
    let v = r.store.set("citrate://join/Zz9yX8wv").expect("set B again");
    assert!(v.persisted);
    assert_eq!(
        r.store.link().as_deref().map(|s| s.as_str()),
        Some("Zz9yX8wv")
    );
}

/// A slot that is valid JSON but whose link no longer parses is dropped, not trusted.
#[test]
fn a_well_formed_slot_with_a_bad_link_is_dropped() {
    let r = rig();
    for bad in ["https://evil.example/join/Ab3dE7gh", "Ab3dE7gh#short", ""] {
        let json = serde_json::json!({
            "link": bad,
            "stored_at": T0,
            "expires_at": T0 + 60,
        });
        r.kr.slots.lock().expect("lock").insert(
            PENDING_JOIN_ACCOUNT.to_string(),
            serde_json::to_vec(&json).expect("json"),
        );
        assert!(r.store.get().is_none(), "{bad:?}");
        assert!(r.store.link().is_none(), "{bad:?}");
        assert!(slot(&r).is_none(), "{bad:?}");
    }
}

fn forgot(r: &Rig) -> Vec<String> {
    r.forgot.lock().expect("lock").clone()
}

/// Cancel after Review drops the opened relay invite for that code, so `join_redeem` can't use it.
#[test]
fn cancel_forgets_the_opened_invite_for_the_held_code() {
    let r = rig();
    r.store.set(&link()).expect("set");
    r.store
        .resolve_held(|_| Ok(view_for(EXP_T0_PLUS_60)))
        .expect("resolved");
    assert!(forgot(&r).is_empty(), "a current answer forgets nothing");
    r.store.clear().expect("clear");
    assert_eq!(forgot(&r), vec![CODE.to_string()]);
}

/// Cancel during a keyring outage still drops the opened invite (the code is known in memory).
#[test]
fn cancel_during_a_keyring_outage_still_forgets_the_opened_invite() {
    let r = rig();
    r.store.set(&link()).expect("set");
    r.store
        .resolve_held(|_| Ok(view_for(EXP_T0_PLUS_60)))
        .expect("resolved");
    r.kr.dead.store(true, Ordering::SeqCst);
    assert!(r.store.clear().is_err());
    assert_eq!(forgot(&r), vec![CODE.to_string()]);
}

/// A different invite replacing the held one drops the old code's opened invite; the same link
/// again keeps it.
#[test]
fn replacing_the_invite_forgets_the_old_code_only_when_it_changes() {
    let r = rig();
    r.store.set(&link()).expect("set A");
    r.store.set(&link()).expect("set A again");
    assert!(forgot(&r).is_empty());
    r.store.set("citrate://join/Zz9yX8wv").expect("set B");
    assert_eq!(forgot(&r), vec![CODE.to_string()]);
}

/// Cancel while Review is in flight: the resolve opens the invite AFTER the cancel ran, so the
/// stale answer must drop it again.
#[test]
fn a_stale_resolve_forgets_what_it_opened() {
    let r = rig();
    r.store.set(&link()).expect("set");
    let out = r.store.resolve_held(|_| {
        r.store.clear().expect("clear");
        // The join client opens the envelope after this point.
        r.forgot.lock().expect("lock").push("<opened>".to_string());
        Ok(view_for(EXP_T0_PLUS_60))
    });
    assert!(out.is_err());
    let log = forgot(&r);
    assert_eq!(log.last().map(String::as_str), Some(CODE), "{log:?}");
}

/// A stale answer for the code that is held again (the same link re-pasted meanwhile) keeps the
/// opened invite: it is for the invite that is waiting.
#[test]
fn a_stale_resolve_for_the_same_code_keeps_it() {
    let r = rig();
    r.store.set(&link()).expect("set");
    let out = r.store.resolve_held(|_| {
        r.store.set(&link()).expect("set again");
        Ok(view_for(EXP_T0_PLUS_60))
    });
    assert!(out.is_err());
    assert!(forgot(&r).is_empty());
}

/// NotFound and expiry wipe the hold, and drop the opened invite with it.
#[test]
fn not_found_and_expiry_forget_the_opened_invite() {
    let r = rig();
    r.store.set(&link()).expect("set");
    let _ = r.store.resolve_held(|_| Err(JoinError::NotFound));
    assert_eq!(forgot(&r), vec![CODE.to_string()]);

    let r = rig();
    r.store.set(&link()).expect("set");
    r.clock.store(T0 + PENDING_JOIN_TTL_SECS, Ordering::SeqCst);
    assert!(r.store.get().is_none());
    assert_eq!(forgot(&r), vec![CODE.to_string()]);
}

/// Another store over the same keyring: what the app finds after a restart.
fn restarted(r: &Rig) -> PendingJoinStore {
    let c = r.clock.clone();
    PendingJoinStore::new(
        Box::new(Handle(r.kr.clone())),
        Box::new(move || c.load(Ordering::SeqCst)),
        Box::new(|_: &str| {}),
    )
}

/// A new invite that arrived while the keyring was down is saved as soon as the keyring is back,
/// over the old slot, so a restart then finds the new invite, never the one it replaced.
#[test]
fn a_replacement_during_an_outage_is_saved_once_the_keyring_is_back() {
    let r = rig();
    r.store.set(&link()).expect("set A");
    r.kr.dead.store(true, Ordering::SeqCst);
    let v = r.store.set("citrate://join/Zz9yX8wv").expect("set B");
    assert!(!v.persisted, "B is in memory only for now");
    assert!(restarted(&r).get().is_none(), "nothing readable while down");
    r.kr.dead.store(false, Ordering::SeqCst);
    assert_eq!(r.store.get().map(|g| g.persisted), Some(true), "saved now");
    let raw = String::from_utf8(slot(&r).expect("slot")).expect("utf8");
    assert!(raw.contains("Zz9yX8wv") && !raw.contains(CODE), "B replaced A");
    assert_eq!(
        restarted(&r).link().as_deref().map(|s| s.as_str()),
        Some("Zz9yX8wv"),
        "after a restart: B, never A"
    );
}

/// While the replacement is still unsaved, the old slot is never read back in this session, even
/// if the in-memory copy is cancelled.
#[test]
fn a_cancelled_unsaved_replacement_does_not_bring_back_the_old_invite() {
    let r = rig();
    r.store.set(&link()).expect("set A");
    r.kr.dead.store(true, Ordering::SeqCst);
    r.store.set("citrate://join/Zz9yX8wv").expect("set B");
    assert!(r.store.clear().is_err());
    r.kr.dead.store(false, Ordering::SeqCst);
    assert!(r.store.get().is_none());
    assert!(slot(&r).is_none(), "the old slot was deleted once the keyring was back");
    assert!(restarted(&r).get().is_none());
}
