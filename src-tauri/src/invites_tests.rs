// D-12: one invite flow, request and approve. Store, inviter, invitee, the two talking through a
// fake relay inbox, and the tripwire that keeps a second invite mechanism out.

use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_800_000_000;
const GROUP: &str = "abababababababababababababababab";
const ANA: &str = "0x00000000000000000000000000000000000000aa";
const BO: &str = "0x00000000000000000000000000000000000000bb";

/// A fresh, empty directory for one test (pid-scoped, so parallel runs and worktrees don't meet).
fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "citrate-invites-d12-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

/// `<dir>/invites/pending.json` for one test (the store creates nothing until it writes).
fn store(tag: &str) -> PathBuf {
    let d = dir(tag).join("invites");
    std::fs::create_dir_all(&d).expect("invites dir");
    d.join("pending.json")
}

/// The relay's server-blind inbox: ciphertexts under a key, as `submit_claim` / `poll_claims` see it.
#[derive(Default)]
struct Relay {
    inbox: RefCell<HashMap<String, Vec<String>>>,
    polls: RefCell<usize>,
    submits: RefCell<usize>,
}

impl Relay {
    fn poll(&self, key: String) -> Result<Vec<String>, String> {
        *self.polls.borrow_mut() += 1;
        Ok(self.inbox.borrow().get(&key).cloned().unwrap_or_default())
    }
    fn submit(&self, key: String, ct: String) -> Result<(), String> {
        *self.submits.borrow_mut() += 1;
        self.inbox.borrow_mut().entry(key).or_default().push(ct);
        Ok(())
    }
}

fn offline(_: String) -> Result<Vec<String>, String> {
    Err("connect: relay unreachable".to_string())
}

fn make(path: &Path, now: u64) -> InviteView {
    create_at(path, GROUP, "", "Larry", "Garden club", now).expect("create")
}

/// The invitee side asks to join with `link` from its own store.
fn ask(invitee: &Path, link: &str, who: &str, relay: &Relay) -> MyRequestView {
    request_at(invitee, link, who, NOW, |k, c| relay.submit(k, c)).expect("request")
}

fn incoming(path: &Path, relay: &Relay) -> Vec<IncomingRequest> {
    requests_at(path, GROUP, NOW, |k| relay.poll(k)).expect("requests")
}

fn ids(path: &Path) -> Vec<String> {
    invite_views_at(path, GROUP, NOW)
        .expect("views")
        .into_iter()
        .map(|v| v.id)
        .collect()
}

fn no_welcome(g: &str) -> Result<(), String> {
    Err(format!("relay: no pending welcome for group {g}"))
}

// ---------------------------------------------------------------------------
// Links.
// ---------------------------------------------------------------------------

#[test]
fn a_link_carries_group_token_key_expiry_and_labels() {
    let path = store("link");
    let v = make(&path, NOW);
    let l = parse_link(&v.link).expect("parse");
    assert_eq!(l.group, GROUP);
    assert_eq!(l.expires_at, NOW + INVITE_TTL_SECS);
    assert_eq!(l.group_name, "Garden club");
    assert_eq!(l.inviter, "Larry");
    assert_eq!(invite_id(&l.token), v.id);
    assert_eq!(v.expires_at, NOW + INVITE_TTL_SECS);
}

#[test]
fn a_link_from_an_earlier_version_is_refused() {
    // 0.5.1's link: no expiry. Earlier: no key either.
    let t = "_WMmXGgqOhccY2YmQuQHMKglU9pQ8qEqwoMd1LpJlog";
    let k = "03eb4c197761a08ddecb42860d56ba2527afe17bd4de0ee774bab2c27ff7b55595";
    for link in [
        format!("citrate://invite?g={GROUP}&t={t}&k={k}&n=4142"),
        format!("citrate://invite?g={GROUP}&t={t}"),
    ] {
        assert_eq!(parse_link(&link).map(|_| ()), Err(EARLIER_LINK.to_string()), "{link}");
    }
}

#[test]
fn a_malformed_link_is_refused_without_echoing_its_token() {
    let t = "_WMmXGgqOhccY2YmQuQHMKglU9pQ8qEqwoMd1LpJlog";
    let k = "03eb4c197761a08ddecb42860d56ba2527afe17bd4de0ee774bab2c27ff7b55595";
    let bad = [
        String::new(),
        "https://citrate.ai/join/ABCDEFGH#key".to_string(),
        format!("citrate://invite?g=&t={t}&k={k}&e=1"),
        format!("citrate://invite?g={GROUP}&t={t}x&k={k}&e=1"),
        format!("citrate://invite?g={GROUP}&t={t}&k=04{}&e=1", &k[2..]),
        format!("citrate://invite?g={GROUP}&t={t}&k={k}&e=+1"),
        format!("citrate://invite?g=a/b&t={t}&k={k}&e=1"),
        format!("citrate://invite?g={GROUP}&t={t}&k={k}&e=1&pad={}", "x".repeat(2100)),
    ];
    for link in bad {
        let e = parse_link(&link).expect_err("refused");
        assert!(!e.contains(t), "the token is never echoed: {e}");
    }
}

#[test]
fn labels_drop_control_and_direction_characters_and_are_cut() {
    assert_eq!(clean_label("  Ana\u{202e}lis\n "), "Analis");
    assert_eq!(clean_label(&"x".repeat(100)).chars().count(), 64);
    // A label in the link is display only and cleaned the same way.
    let path = store("labels");
    let v = create_at(&path, GROUP, "", "Bo\u{2066}b\u{7}", "", NOW).expect("create");
    assert_eq!(parse_link(&v.link).expect("parse").inviter, "Bob");
}

// ---------------------------------------------------------------------------
// The whole flow.
// ---------------------------------------------------------------------------

#[test]
fn request_then_approve_adds_the_requester_and_their_app_finishes_the_join() {
    let (owner, invitee) = (store("flow-owner"), store("flow-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);

    let mine = ask(&invitee, &v.link, ANA, &relay);
    assert_eq!(mine.state, RequestState::Waiting);
    assert_eq!((mine.group_name.as_str(), mine.inviter.as_str()), ("Garden club", "Larry"));
    // Before approval: no Welcome, so still waiting.
    let views = check_requests_at(&invitee, NOW, no_welcome, |k| relay.poll(k)).expect("check");
    assert_eq!(views[0].state, RequestState::Waiting);

    let req = incoming(&owner, &relay);
    assert_eq!(req.len(), 1);
    assert_eq!((req[0].address.as_str(), req[0].invite_id.as_str()), (ANA, v.id.as_str()));

    let added = RefCell::new(Vec::new());
    approve_at(&owner, GROUP, &v.id, ANA, NOW, |k| relay.poll(k), |g, m| {
        added.borrow_mut().push((g.to_string(), m.to_string()));
        Ok(())
    }, |_, _| Ok(false))
    .expect("approved");
    assert_eq!(added.into_inner(), vec![(GROUP.to_string(), ANA.to_string())]);
    assert!(ids(&owner).is_empty(), "the invite is used up");

    // The Welcome arrived: the invitee's check joins and the request is done.
    let views = check_requests_at(&invitee, NOW, |_| Ok(()), |k| relay.poll(k)).expect("check");
    assert_eq!(views[0].state, RequestState::Joined);
    assert!(check_requests_at(&invitee, NOW, no_welcome, |k| relay.poll(k))
        .expect("check")
        .is_empty());
}

#[test]
fn asking_twice_with_the_same_link_sends_once() {
    let (owner, invitee) = (store("twice-owner"), store("twice-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    ask(&invitee, &v.link, ANA, &relay);
    ask(&invitee, &v.link, ANA, &relay);
    assert_eq!(*relay.submits.borrow(), 1);
    assert!(preview_at(&invitee, &v.link, NOW).expect("preview").requested);
}

#[test]
fn an_expired_link_is_refused_before_anything_is_sent() {
    let (owner, invitee) = (store("exp-owner"), store("exp-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    let later = NOW + INVITE_TTL_SECS;
    assert!(preview_at(&invitee, &v.link, later).expect("preview").expired);
    let r = request_at(&invitee, &v.link, ANA, later, |k, c| relay.submit(k, c));
    assert!(r.is_err(), "{r:?}");
    assert_eq!(*relay.submits.borrow(), 0, "nothing sent");
}

// ---------------------------------------------------------------------------
// Approve is refused once the invite is gone, expired, declined or taken.
// ---------------------------------------------------------------------------

fn never_add(_: &str, _: &str) -> Result<(), String> {
    panic!("add must not be called")
}

/// A relay holding a request from each of `who` on invite `v` (each from its own device store).
fn requested(tag: &str, v: &InviteView, who: &[&str]) -> Relay {
    let relay = Relay::default();
    for (n, a) in who.iter().enumerate() {
        ask(&store(&format!("{tag}-asker-{n}")), &v.link, a, &relay);
    }
    relay
}

/// Approve `who` on `v`, with the requests in `relay`.
fn approve(
    owner: &Path,
    v: &InviteView,
    who: &str,
    now: u64,
    relay: &Relay,
    add: impl FnOnce(&str, &str) -> Result<(), String>,
    is_member: impl FnMut(&str, &str) -> Result<bool, String>,
) -> Result<(), String> {
    approve_at(owner, GROUP, &v.id, who, now, |k| relay.poll(k), add, is_member)
}

fn held(owner: &Path) -> String {
    invite_views_at(owner, GROUP, NOW).expect("views")[0].approving.clone()
}

/// An approval whose add errored and whose roster couldn't be read: held for Ana.
fn hold_unknown(owner: &Path, v: &InviteView, relay: &Relay) {
    let e = approve(owner, v, ANA, NOW, relay, |_, _| Err("timed out".into()), |_, _| {
        Err("relay down".into())
    })
    .expect_err("unknown");
    assert!(e.contains("Check again"), "{e}");
    assert_eq!(held(owner), ANA);
}

/// A crash right after the hold was written (before or after AddMember): only the hold is left.
fn crash_after_hold(owner: &Path, v: &InviteView) {
    update_store_at(owner, |s| {
        let i = s.invites.iter_mut().find(|i| invite_id(&i.token) == v.id).expect("invite");
        i.approving = ANA.to_string();
    })
    .expect("hold");
}

#[test]
fn approve_is_refused_once_the_invite_was_used() {
    let owner = store("used-owner");
    let v = make(&owner, NOW);
    let relay = requested("used", &v, &[ANA, BO]);
    assert_eq!(incoming(&owner, &relay).len(), 2);
    approve(&owner, &v, ANA, NOW, &relay, |_, _| Ok(()), |_, _| Ok(false)).expect("ana");
    let e = approve(&owner, &v, BO, NOW, &relay, never_add, |_, _| Ok(false)).expect_err("single use");
    assert_eq!(e, INVITE_GONE);
    assert!(incoming(&owner, &relay).is_empty(), "a used invite takes no more requests");
}

#[test]
fn approve_is_refused_after_a_revoke() {
    let owner = store("rev-owner");
    let v = make(&owner, NOW);
    let relay = requested("rev", &v, &[ANA]);
    revoke_at(&owner, GROUP, &v.id, |k| relay.poll(k), |k, c| relay.submit(k, c)).expect("revoke");
    let e = approve(&owner, &v, ANA, NOW, &relay, never_add, |_, _| Ok(false)).expect_err("revoked");
    assert_eq!(e, INVITE_GONE);
}

#[test]
fn approve_is_refused_after_expiry() {
    let owner = store("expiry-owner");
    let v = make(&owner, NOW);
    let relay = requested("expiry", &v, &[ANA]);
    let later = NOW + INVITE_TTL_SECS;
    let e = approve(&owner, &v, ANA, later, &relay, never_add, |_, _| Ok(false)).expect_err("expired");
    assert!(e.contains("expired"), "{e}");
    assert!(invite_views_at(&owner, GROUP, later).expect("views").is_empty());
    // The next invite drops the expired one, key and all.
    make(&owner, later);
    let raw = std::fs::read_to_string(&owner).expect("store");
    assert!(!raw.contains(&v.link), "the expired invite is gone from the file");
}

#[test]
fn approve_is_refused_for_a_declined_request() {
    let owner = store("dec-owner");
    let v = make(&owner, NOW);
    let relay = requested("dec", &v, &[ANA]);
    decline_at(&owner, GROUP, &v.id, ANA, |k| relay.poll(k), |k, c| relay.submit(k, c))
        .expect("decline");
    let e = approve(&owner, &v, ANA, NOW, &relay, never_add, |_, _| Ok(false)).expect_err("declined");
    assert!(e.contains("declined"), "{e}");
}

#[test]
fn approve_needs_a_request_from_that_address_on_that_invite() {
    let owner = store("noreq-owner");
    let v = make(&owner, NOW);
    let other = make(&owner, NOW);
    // Bo asked on the other link only; Ana asked on this one.
    let relay = requested("noreq", &v, &[ANA]);
    ask(&store("noreq-bo"), &other.link, BO, &relay);
    let e = approve(&owner, &v, BO, NOW, &relay, never_add, |_, _| Ok(false)).expect_err("no request");
    assert!(e.contains("no request from that address"), "{e}");
    assert_eq!(held(&owner), "", "nothing held");
    // The relay can't be asked: refused, nothing held, nothing added.
    let e = approve_at(&owner, GROUP, &v.id, ANA, NOW, offline, never_add, |_, _| Ok(false))
        .expect_err("offline");
    assert!(e.contains("Couldn't check the request"), "{e}");
    assert_eq!(held(&owner), "");
}

#[test]
fn a_second_approval_cannot_race_the_first() {
    let owner = store("race-owner");
    let v = make(&owner, NOW);
    let relay = requested("race", &v, &[ANA, BO]);
    let adds = RefCell::new(0);
    // While Ana's add is in flight: an approval for Bo, a nested approval for Ana, a check, a
    // decline and a revoke all arrive. None of them acts; AddMember goes out once.
    approve(&owner, &v, ANA, NOW, &relay, |_, _| {
        *adds.borrow_mut() += 1;
        for who in [BO, ANA] {
            let e = approve(&owner, &v, who, NOW, &relay, never_add, |_, _| panic!("no roster read"));
            assert!(e.expect_err("in flight").contains("right now"));
        }
        assert!(check_held_at(&owner, GROUP, &v.id, NOW, |_, _| panic!("no check")).is_err());
        assert!(decline_at(&owner, GROUP, &v.id, BO, offline, |_, _| Ok(())).is_err());
        assert!(revoke_at(&owner, GROUP, &v.id, offline, |_, _| Ok(())).is_err());
        Ok(())
    }, |_, _| Ok(false))
    .expect("ana");
    assert_eq!(*adds.borrow(), 1);
    assert!(ids(&owner).is_empty());
}

#[test]
fn an_add_that_failed_reopens_the_invite() {
    let owner = store("failed-add");
    let v = make(&owner, NOW);
    let relay = requested("failed-add", &v, &[ANA, BO]);
    let e = approve(&owner, &v, ANA, NOW, &relay, |_, _| Err("no key package".into()), |_, _| Ok(false))
        .expect_err("not added");
    assert!(e.contains("no key package"), "{e}");
    assert_eq!(held(&owner), "");
    approve(&owner, &v, BO, NOW, &relay, |_, _| Ok(()), |_, _| Ok(false)).expect("bo");
}

#[test]
fn an_add_whose_answer_was_lost_counts_once_the_roster_has_them() {
    let owner = store("lost-answer");
    let v = make(&owner, NOW);
    let relay = requested("lost-answer", &v, &[ANA]);
    approve(&owner, &v, ANA, NOW, &relay, |_, _| Err("timed out".into()), |_, _| Ok(true))
        .expect("added after all");
    assert!(ids(&owner).is_empty());
}

#[test]
fn an_add_with_an_unknown_outcome_holds_the_invite_for_that_requester() {
    let owner = store("unknown-add");
    let v = make(&owner, NOW);
    let relay = requested("unknown-add", &v, &[ANA, BO]);
    hold_unknown(&owner, &v, &relay);
    // A restart here: the hold is in the file, and nobody else can be approved on it.
    let bo = approve(&owner, &v, BO, NOW, &relay, never_add, |_, _| Ok(false));
    assert!(bo.expect_err("held").contains("someone else"));
    assert!(decline_at(&owner, GROUP, &v.id, ANA, offline, |_, _| Ok(())).is_err());
}

// ---------------------------------------------------------------------------
// A held approval always has a way out (Check again, Cancel, start-up, expiry).
// ---------------------------------------------------------------------------

#[test]
fn a_retry_asks_the_roster_first_and_never_adds_twice() {
    let owner = store("retry");
    let v = make(&owner, NOW);
    let relay = requested("retry", &v, &[ANA]);
    hold_unknown(&owner, &v, &relay);
    // The add had landed: Approve again finds them on the roster and sends nothing.
    approve(&owner, &v, ANA, NOW, &relay, never_add, |_, _| Ok(true)).expect("done");
    assert!(ids(&owner).is_empty());
}

#[test]
fn a_retry_adds_once_when_the_roster_says_they_are_not_in() {
    let owner = store("retry-add");
    let v = make(&owner, NOW);
    let relay = requested("retry-add", &v, &[ANA]);
    hold_unknown(&owner, &v, &relay);
    let adds = RefCell::new(0);
    approve(&owner, &v, ANA, NOW, &relay, |_, _| {
        *adds.borrow_mut() += 1;
        Ok(())
    }, |_, _| Ok(false))
    .expect("added");
    assert_eq!(*adds.borrow(), 1);
    // A retry whose roster can't be read sends nothing and keeps the hold.
    let v2 = make(&owner, NOW);
    let relay2 = requested("retry-add-2", &v2, &[ANA]);
    hold_unknown(&owner, &v2, &relay2);
    let e = approve(&owner, &v2, ANA, NOW, &relay2, never_add, |_, _| Err("down".into()));
    assert!(e.expect_err("unknown").contains("Check again"));
    assert_eq!(
        invite_views_at(&owner, GROUP, NOW).expect("views")[0].approving,
        ANA
    );
}

#[test]
fn after_a_relay_restart_the_invite_row_settles_a_held_approval() {
    for (present, want) in [(true, HeldCheck::Added), (false, HeldCheck::Released)] {
        let owner = store(&format!("relay-restart-{present}"));
        let v = make(&owner, NOW);
        let relay = requested(&format!("relay-restart-{present}"), &v, &[ANA]);
        hold_unknown(&owner, &v, &relay);
        // The relay restarted: its inbox is empty, so no request row comes back.
        let relay = Relay::default();
        assert!(incoming(&owner, &relay).is_empty());
        assert_eq!(held(&owner), ANA, "the invite row still shows the hold");
        let got = check_held_at(&owner, GROUP, &v.id, NOW, |_, _| Ok(present)).expect("check");
        assert_eq!(got, want);
        if present {
            assert!(ids(&owner).is_empty(), "used");
        } else {
            assert_eq!(held(&owner), "", "open again");
        }
    }
}

#[test]
fn a_check_that_cant_read_the_roster_keeps_the_hold_and_cancel_still_works() {
    let owner = store("cancel");
    let v = make(&owner, NOW);
    let relay = requested("cancel", &v, &[ANA]);
    hold_unknown(&owner, &v, &relay);
    assert!(check_held_at(&owner, GROUP, &v.id, NOW, |_, _| Err("down".into())).is_err());
    assert_eq!(held(&owner), ANA);
    // Cancel: release the hold and revoke the invite, key and all.
    revoke_at(&owner, GROUP, &v.id, offline, |_, _| Ok(())).expect("cancel");
    assert!(ids(&owner).is_empty());
    assert!(!std::fs::read_to_string(&owner).expect("store").contains(&v.link));
}

#[test]
fn a_crash_after_the_hold_is_settled_at_start_up() {
    let owner = store("crash-hold");
    let v = make(&owner, NOW);
    crash_after_hold(&owner, &v);
    // Restart: the add never went out; the roster says they aren't in, so the hold is released.
    let r = recheck_held_at(&owner, NOW, |_, _| Ok(false)).expect("recheck");
    assert_eq!(r, vec![(v.id.clone(), Ok(HeldCheck::Released))]);
    assert_eq!(held(&owner), "");
}

#[test]
fn a_crash_after_addmember_before_the_release_is_settled_at_start_up() {
    let owner = store("crash-added");
    let v = make(&owner, NOW);
    crash_after_hold(&owner, &v);
    // Restart: the add had landed.
    let r = recheck_held_at(&owner, NOW, |_, _| Ok(true)).expect("recheck");
    assert_eq!(r, vec![(v.id.clone(), Ok(HeldCheck::Added))]);
    assert!(ids(&owner).is_empty());
    // And a start-up whose roster can't be read leaves a hold for Check again / Cancel.
    let v2 = make(&owner, NOW);
    crash_after_hold(&owner, &v2);
    let r = recheck_held_at(&owner, NOW, |_, _| Err("down".into())).expect("recheck");
    assert!(r[0].1.is_err());
    assert_eq!(held(&owner), ANA);
}

#[test]
fn a_hold_past_expiry_is_settled_by_the_roster_and_never_kept_forever() {
    let later = NOW + 10 * INVITE_TTL_SECS;
    let owner = store("expired-hold");
    let v = make(&owner, NOW);
    crash_after_hold(&owner, &v);
    assert_eq!(
        check_held_at(&owner, GROUP, &v.id, later, |_, _| Ok(false)).expect("check"),
        HeldCheck::Expired
    );
    assert!(!std::fs::read_to_string(&owner).expect("store").contains(&v.link), "key gone");
    let v2 = make(&owner, NOW);
    crash_after_hold(&owner, &v2);
    assert_eq!(
        check_held_at(&owner, GROUP, &v2.id, later, |_, _| Ok(true)).expect("check"),
        HeldCheck::Added
    );
    assert!(invite_views_at(&owner, GROUP, later).expect("views").is_empty());
}

#[test]
fn a_request_says_which_link_it_came_in_on() {
    let owner = store("label");
    let v = create_at(&owner, GROUP, "for Ana", "Larry", "Garden club", NOW).expect("create");
    let relay = requested("label", &v, &[ANA]);
    assert_eq!(incoming(&owner, &relay)[0].invite_label, "for Ana");
}

// ---------------------------------------------------------------------------
// Decline, revoke and what the requester sees.
// ---------------------------------------------------------------------------

#[test]
fn a_declined_request_is_hidden_and_the_requester_is_told() {
    let (owner, invitee) = (store("told-owner"), store("told-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    ask(&invitee, &v.link, ANA, &relay);
    let n = decline_at(&owner, GROUP, &v.id, ANA, |k| relay.poll(k), |k, c| relay.submit(k, c))
        .expect("decline");
    assert!(n.notified);
    assert!(incoming(&owner, &relay).is_empty());
    let views = check_requests_at(&invitee, NOW, no_welcome, |k| relay.poll(k)).expect("check");
    assert_eq!(views[0].state, RequestState::Declined);
}

#[test]
fn a_revoke_removes_the_key_and_the_requester_is_told() {
    let (owner, invitee) = (store("revoke-owner"), store("revoke-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    ask(&invitee, &v.link, ANA, &relay);
    let n = revoke_at(&owner, GROUP, &v.id, |k| relay.poll(k), |k, c| relay.submit(k, c))
        .expect("revoke");
    assert!(n.notified);
    assert!(ids(&owner).is_empty());
    let raw = std::fs::read_to_string(&owner).expect("store");
    assert!(!raw.contains(&v.link), "the key went with the invite");
    let views = check_requests_at(&invitee, NOW, no_welcome, |k| relay.poll(k)).expect("check");
    assert_eq!(views[0].state, RequestState::Revoked);
}

#[test]
fn a_revoke_with_the_relay_offline_still_revokes_and_says_nobody_was_told() {
    let owner = store("revoke-offline");
    let v = make(&owner, NOW);
    let n = revoke_at(&owner, GROUP, &v.id, offline, |_, _| Ok(())).expect("revoke");
    assert!(!n.notified);
    assert!(ids(&owner).is_empty());
}

#[test]
fn a_request_with_no_answer_reads_as_expired_once_the_link_has() {
    let (owner, invitee) = (store("noans-owner"), store("noans-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    ask(&invitee, &v.link, ANA, &relay);
    let later = NOW + INVITE_TTL_SECS;
    let views = check_requests_at(&invitee, later, no_welcome, |k| relay.poll(k)).expect("check");
    assert_eq!(views[0].state, RequestState::Expired);
    // An approval that landed before expiry still joins after it.
    let views = check_requests_at(&invitee, later, |_| Ok(()), |k| relay.poll(k)).expect("check");
    assert_eq!(views[0].state, RequestState::Joined);
}

#[test]
fn an_unreachable_relay_or_daemon_is_never_read_as_an_answer() {
    let (owner, invitee) = (store("unreach-owner"), store("unreach-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    ask(&invitee, &v.link, ANA, &relay);
    let later = NOW + INVITE_TTL_SECS;
    let views =
        check_requests_at(&invitee, later, |_| Err("connect: refused".into()), offline).expect("ok");
    assert_eq!(views[0].state, RequestState::Waiting);
    assert!(views[0].note.contains("Can't check"), "{:?}", views[0]);
    let views = check_requests_at(&invitee, NOW, no_welcome, offline).expect("ok");
    assert_eq!(views[0].state, RequestState::Waiting);
    assert!(views[0].note.contains("relay"), "{:?}", views[0]);
    // The inviter's side: no requests list at all, rather than "no requests".
    assert!(requests_at(&owner, GROUP, NOW, offline).is_err());
}

#[test]
fn only_a_request_sealed_to_the_invite_for_its_group_and_token_is_shown() {
    let (owner, invitee) = (store("only-owner"), store("only-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    let other = make(&owner, NOW);
    let l = parse_link(&v.link).expect("link");
    let inbox = request_inbox(&l.token);
    let seal = |body: serde_json::Value, key: &str| {
        hex::encode(crate::invite_seal::seal_to(key, body.to_string().as_bytes()).expect("seal"))
    };
    let (_, reply) = crate::invite_seal::new_invite_keypair();
    let ok = |addr: &str| json_request(&l.group, &l.token, addr, &reply);
    let other_key = parse_link(&other.link).expect("other").key;
    let junk = vec![
        "zz".to_string(),
        hex::encode(b"not sealed"),
        seal(ok(ANA), &other_key),                                        // another invite's key
        seal(json_request("ffff", &l.token, ANA, &reply), &l.key),          // another group
        seal(json_request(&l.group, "wrong", ANA, &reply), &l.key),         // another token
        seal(json_request(&l.group, &l.token, "0xnope", &reply), &l.key),   // bad address
        seal(json_request(&l.group, &l.token, BO, "02zz"), &l.key),         // bad reply key
        seal(ok(&ANA.to_ascii_uppercase().replace("0X", "0x")), &l.key),     // Ana, once
        seal(ok(ANA), &l.key),                                              // Ana again
    ];
    relay.inbox.borrow_mut().insert(inbox, junk);
    let req = incoming(&owner, &relay);
    assert_eq!(req.len(), 1, "{req:?}");
    assert_eq!(req[0].address, ANA);
    let _ = invitee;
}

fn json_request(group: &str, token: &str, address: &str, reply: &str) -> serde_json::Value {
    serde_json::json!({"v": 1, "group": group, "token": token, "address": address, "reply": reply})
}

#[test]
fn an_answer_someone_else_left_in_the_reply_inbox_is_not_read() {
    let (owner, invitee) = (store("forged-owner"), store("forged-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    ask(&invitee, &v.link, ANA, &relay);
    let mine = view_store_at(&invitee).expect("store").requests[0].clone();
    // Someone holding the link and Ana's reply key's inbox name (they can't, but even so) leaves a
    // "declined" sealed to a key that isn't Ana's.
    let (_, stranger) = crate::invite_seal::new_invite_keypair();
    let body = serde_json::json!({"v": 1, "group": GROUP, "token": mine.token, "decision": "declined"});
    let ct = crate::invite_seal::seal_to(&stranger, body.to_string().as_bytes()).expect("seal");
    relay
        .inbox
        .borrow_mut()
        .insert(reply_inbox(&mine.token, &mine.reply_pub), vec![hex::encode(ct)]);
    let views = check_requests_at(&invitee, NOW, no_welcome, |k| relay.poll(k)).expect("check");
    assert_eq!(views[0].state, RequestState::Waiting);
}

#[test]
fn dismissing_a_request_forgets_it() {
    let (owner, invitee) = (store("dismiss-owner"), store("dismiss-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    let mine = ask(&invitee, &v.link, ANA, &relay);
    dismiss_at(&invitee, &mine.id).expect("dismiss");
    assert!(check_requests_at(&invitee, NOW, no_welcome, |k| relay.poll(k))
        .expect("check")
        .is_empty());
}

#[test]
fn what_the_app_sees_never_carries_a_private_key() {
    let (owner, invitee) = (store("nokey-owner"), store("nokey-invitee"));
    let relay = Relay::default();
    let v = make(&owner, NOW);
    let mine = ask(&invitee, &v.link, ANA, &relay);
    let s_owner = view_store_at(&owner).expect("owner");
    let s_invitee = view_store_at(&invitee).expect("invitee");
    let secrets = [
        s_owner.invites[0].priv_key.expose_hex().to_string(),
        s_invitee.requests[0].reply_key.expose_hex().to_string(),
    ];
    let shown = [
        serde_json::to_string(&invite_views_at(&owner, GROUP, NOW).expect("views")).expect("json"),
        serde_json::to_string(&incoming(&owner, &relay)).expect("json"),
        serde_json::to_string(&mine).expect("json"),
        serde_json::to_string(&preview_at(&invitee, &v.link, NOW).expect("p")).expect("json"),
        format!("{s_owner:?} {s_invitee:?}"),
    ];
    for s in &secrets {
        assert_eq!(s.len(), 64);
        for v in &shown {
            assert!(!v.contains(s), "a private key in {v}");
        }
    }
}

// ---------------------------------------------------------------------------
// The store.
// ---------------------------------------------------------------------------

/// The files beside `path` named `pending.json.<tag>-…`.
fn set_aside_files(path: &Path, tag: &str) -> Vec<PathBuf> {
    let prefix = format!("pending.json.{tag}-");
    let mut v: Vec<PathBuf> = std::fs::read_dir(path.parent().expect("dir"))
        .expect("read dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
        .collect();
    v.sort();
    v
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).expect("meta").permissions().mode() & 0o777
}

/// The 0.5.1 store as 0.5.1 wrote it (the upgrade fixture: throwaway token and key).
const V051_PENDING: &[u8] = include_bytes!("../../fixtures/upgrade-v0.5.1/app-data/invites/pending.json");

#[test]
fn an_earlier_versions_store_is_set_aside_kept_and_a_new_one_starts() {
    let path = store("earlier");
    std::fs::write(&path, V051_PENDING).expect("0.5.1 file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    }
    // The first read sets it aside and reads a new, empty store (not an error, not the old list).
    assert_eq!(invite_views_at(&path, GROUP, NOW), Ok(Vec::new()));
    let aside = set_aside_files(&path, "earlier");
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(std::fs::read(&aside[0]).expect("kept"), V051_PENDING, "bytes untouched");
    #[cfg(unix)]
    assert_eq!(mode(&aside[0]), 0o600);
    assert!(!path.exists());
    // The next write starts the new format and leaves the old file alone.
    make(&path, NOW);
    assert!(std::fs::read_to_string(&path).expect("new").contains(STORE_FORMAT));
    assert_eq!(std::fs::read(&aside[0]).expect("kept"), V051_PENDING);
}

#[test]
fn a_store_in_an_unknown_format_is_set_aside_by_a_writer_too() {
    for (tag, bytes) in [
        ("fmt-later", &br#"{"format":"citrate-invites/request-approve-9","invites":[]}"#[..]),
        ("fmt-none", &br#"{"invites":[]}"#[..]),
        ("fmt-scalar", &b"42"[..]),
    ] {
        let path = store(tag);
        std::fs::write(&path, bytes).expect("file");
        make(&path, NOW);
        assert_eq!(ids(&path).len(), 1, "{tag}");
        assert_eq!(set_aside_files(&path, "earlier").len(), 1, "{tag}");
    }
}

#[test]
fn a_damaged_store_is_an_error_for_every_reader_and_no_read_moves_it() {
    let path = store("damaged-readers");
    let damaged = br#"{"format":"citrate-invites/request-approve-1","invites":[{"group":"#;
    std::fs::write(&path, damaged).expect("damaged");
    let relay = Relay::default();
    let link = format!(
        "citrate://invite?g={GROUP}&t=_WMmXGgqOhccY2YmQuQHMKglU9pQ8qEqwoMd1LpJlog&k=03eb4c197761a08ddecb42860d56ba2527afe17bd4de0ee774bab2c27ff7b55595&e={}",
        NOW + 60
    );
    assert!(invite_views_at(&path, GROUP, NOW).is_err());
    assert!(requests_at(&path, GROUP, NOW, |k| relay.poll(k)).is_err());
    assert!(preview_at(&path, &link, NOW).is_err());
    assert!(check_requests_at(&path, NOW, |_| panic!("no join"), |k| relay.poll(k)).is_err());
    assert!(revoke_at(&path, GROUP, "0123456789abcdef", |k| relay.poll(k), |_, _| Ok(())).is_err());
    assert_eq!(*relay.polls.borrow(), 0, "nothing reached the relay");
    assert_eq!(std::fs::read(&path).expect("still there"), damaged, "a read never moves it");
    assert!(set_aside_files(&path, "damaged").is_empty());
}

#[test]
fn a_damaged_store_is_set_aside_by_the_next_write_and_the_one_after_starts_fresh() {
    let path = store("damaged-write");
    let damaged = b"{\"format\":\"citrate-invites/request-appr".to_vec();
    std::fs::write(&path, &damaged).expect("damaged");
    let e = create_at(&path, GROUP, "", "", "", NOW).expect_err("the write that finds it fails");
    assert!(e.contains("set aside") && e.contains("nothing was changed"), "{e}");
    let aside = set_aside_files(&path, "damaged");
    assert_eq!(aside.len(), 1);
    assert_eq!(std::fs::read(&aside[0]).expect("kept"), damaged);
    #[cfg(unix)]
    assert_eq!(mode(&aside[0]), 0o600);
    // A restart here reads as a new store.
    assert_eq!(invite_views_at(&path, GROUP, NOW), Ok(Vec::new()));
    make(&path, NOW);
    assert_eq!(ids(&path).len(), 1);
    assert_eq!(std::fs::read(&aside[0]).expect("still kept"), damaged);
}

#[test]
fn a_store_that_cant_be_read_is_never_moved() {
    let path = store("unreadable");
    std::fs::create_dir_all(&path).expect("a directory where the file should be");
    assert!(create_at(&path, GROUP, "", "", "", NOW).is_err());
    assert!(invite_views_at(&path, GROUP, NOW).is_err());
    assert!(path.is_dir(), "left where it was");
    assert!(set_aside_files(&path, "damaged").is_empty());
    assert!(set_aside_files(&path, "earlier").is_empty());
}

#[test]
fn racing_writers_on_a_damaged_store_set_it_aside_once_and_lose_nothing_new() {
    let path = store("damaged-race");
    std::fs::write(&path, b"{not json").expect("damaged");
    let results: Vec<Result<InviteView, String>> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                s.spawn(move || create_at(&path, GROUP, "", "", "", NOW))
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("join")).collect()
    });
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
    assert_eq!(set_aside_files(&path, "damaged").len(), 1);
    assert_eq!(ids(&path).len(), 7, "every other write landed");
}

#[cfg(unix)]
#[test]
fn the_store_is_owner_only_in_an_owner_only_directory() {
    use std::os::unix::fs::PermissionsExt;
    let path = store("modes");
    let d = path.parent().expect("dir");
    std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    make(&path, NOW);
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(d), 0o700);
    let leftovers: Vec<_> = std::fs::read_dir(d)
        .expect("dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "no temp file left behind");
}

// ---------------------------------------------------------------------------
// Tripwires.
// ---------------------------------------------------------------------------

/// The production part of a source file: everything before its `#[cfg(test)] mod tests` block
/// (a `#[cfg(test)]` on a single item, such as a test-only `mod` line, doesn't end it).
fn prod(src: &str) -> &str {
    src.split("#[cfg(test)]\nmod tests").next().unwrap_or(src)
}

fn read_src(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The calls that read the invite store (directly or through a view).
const STORE_READS: &[&str] = &[
    "view_store_at(",
    "invite_views_at(",
    "open_invites(",
    "requests_at(",
    "check_requests_at(",
    "preview_at(",
];

/// The invite reads in `src` (production code) that turn a failure into something: a default, an
/// empty list, `None`, or a skipped branch. One entry per offending call.
fn lenient_reads(src: &str) -> Vec<String> {
    const AFTER: &[&str] = &[".unwrap_or", ".ok()", ".ok()?", "Err(_)", "=> vec![]", "=> Vec::new()", ".unwrap()"];
    const BEFORE: &[&str] = &["if let Ok(", "while let Ok(", "let Ok(", "matches!("];
    let mut out = Vec::new();
    for call in STORE_READS {
        for (i, _) in src.match_indices(call) {
            let head = &src[i.saturating_sub(80)..i];
            if head.ends_with("fn ") || head.ends_with('_') {
                continue; // the definition itself, or a longer name ending in this one
            }
            // The rest of the statement the call is in.
            let rest = &src[i..];
            let end = rest.find(";\n").map_or(rest.len(), |e| e + 1).min(400);
            let tail = &rest[..end];
            let line_head = head.rsplit('\n').next().unwrap_or(head);
            if AFTER.iter().any(|b| tail.contains(b)) || BEFORE.iter().any(|b| line_head.contains(b)) {
                out.push(format!("{}{}", line_head.trim(), tail.trim()));
            }
        }
    }
    out
}

/// The store is read only under the lock, set aside for damage only by the locked writer, and no
/// reader of it anywhere in src-tauri/src turns a failure into "no invites".
#[test]
fn tripwire_the_store_is_never_read_leniently() {
    let src = read_src("src/invites.rs");
    let p = prod(&src);
    assert_eq!(p.matches("read_store(").count(), 3, "defined once, read by the two locked fns");
    assert_eq!(p.matches("set_aside(path, DAMAGED_TAG)").count(), 1);
    let upd = &p[p.find("fn update_store_at").expect("update")..];
    let upd = &upd[..upd.find("\n}\n").expect("end")];
    assert!(upd.contains("set_aside(path, DAMAGED_TAG)"), "only the writer sets damage aside");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("src") {
        let path = entry.expect("entry").path();
        let name = path.file_name().expect("name").to_string_lossy().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let s = std::fs::read_to_string(&path).expect("read");
        for l in lenient_reads(prod(&s)) {
            found.push(format!("{name}: {l}"));
        }
    }
    assert!(found.is_empty(), "invite reads that turn a failure into nothing: {found:#?}");
}

/// The scan catches every lenient shape the review named, and passes the strict ones.
#[test]
fn tripwire_the_lenient_read_scan_sees_each_shape() {
    for bad in [
        "let v = open_invites(&self.app, group).unwrap_or_default();\n",
        "let v = open_invites(&self.app, group).unwrap_or_else(|_| Vec::new());\n",
        "let v = open_invites(&self.app, group).unwrap_or(Vec::new());\n",
        "let v = invite_views_at(&p, g, now).ok();\n",
        "let v = match open_invites(&app, g) { Ok(v) => v, Err(_) => vec![] };\n",
        "let v = match open_invites(&app, g) { Ok(v) => v, Err(e) => vec![] };\n",
        "if let Ok(v) = open_invites(&app, g) { use_it(v) }\n",
        "let Ok(s) = view_store_at(path) else { return };\n",
    ] {
        assert_eq!(lenient_reads(bad).len(), 1, "{bad}");
    }
    for ok in [
        "let open = crate::invites::open_invites(&self.app, group)?;\n",
        "fn view_store_at(path: &Path) -> Result<Store, String> {\n",
        "invite_views_at(&store_path(&app)?, &group, now_unix())\n}\n",
        "let requests = view_store_at(path)?.requests;\n",
    ] {
        assert!(lenient_reads(ok).is_empty(), "{ok}");
    }
}

/// D-12: one invite mechanism. Fails if another comes back: a join_* / pending_join_* /
/// rendezvous_* command, the v2 link modules, the relay's self-admit or rendezvous calls, or a
/// citrate.ai/join link built in core.
#[test]
fn tripwire_there_is_one_invite_flow() {
    let lib = read_src("src/lib.rs");
    let handler = &lib[lib.find("generate_handler![").expect("handler")..];
    let handler = &handler[..handler.find("])").expect("end")];
    for line in handler.lines().map(str::trim) {
        let cmd = line.trim_end_matches(',').rsplit("::").next().unwrap_or("");
        assert!(
            !(cmd.starts_with("join_")
                || cmd.starts_with("pending_join_")
                || cmd.starts_with("rendezvous_")),
            "a second invite mechanism is registered: {line}"
        );
        if cmd.starts_with("group_invite") || cmd.starts_with("group_referral") {
            assert!(line.starts_with("invites::group_invite"), "invite commands live in invites: {line}");
        }
    }
    for m in ["mod join;", "mod pending_join;", "mod rendezvous;"] {
        assert!(!prod(&lib).contains(m), "src-tauri: {m}");
    }
    let kit = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../kit/src/lib.rs"))
        .expect("kit lib");
    assert!(!kit.contains("pub mod join;"), "kit: the v2 link client is back");
    let banned = [
        "RedeemInvite",
        "redeem_invite",
        "PublishInvite",
        "publish_invite",
        "RevokeInvite",
        "RendezvousOpen",
        "rendezvous_open",
        "rendezvous_tick",
        "citrate.ai/join",
    ];
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let kit_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kit/src");
    for d in [src_dir, kit_dir] {
        for entry in std::fs::read_dir(&d).expect("src") {
            let path = entry.expect("entry").path();
            let name = path.file_name().expect("name").to_string_lossy().to_string();
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
                continue;
            }
            let s = std::fs::read_to_string(&path).expect("read");
            for b in banned {
                assert!(!prod(&s).contains(b), "{name}: {b} (a second invite mechanism)");
            }
        }
    }
}

#[test]
fn tripwire_the_scans_see_what_they_look_for() {
    assert_eq!(prod("a\n#[cfg(test)]\nmod tests {}"), "a\n");
    assert_eq!(prod("a\n#[cfg(test)]\nmod x;\nb"), "a\n#[cfg(test)]\nmod x;\nb");
    let line = "rendezvous::rendezvous_tick,";
    assert!(line.trim_end_matches(',').rsplit("::").next().unwrap_or("").starts_with("rendezvous_"));
    let line = "join::join_mint,";
    assert!(line.trim_end_matches(',').rsplit("::").next().unwrap_or("").starts_with("join_"));
}
