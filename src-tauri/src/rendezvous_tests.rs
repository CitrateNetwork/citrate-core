// GROW-S2 (Join 9) app-side tests. Included into `rendezvous::tests`.
//
// The daemon and relay sides aren't built yet, so these test against the contract itself: the §13
// derivation vectors (copied to kit/src/fixtures/rendezvous-v1.json) and the §9 JSON shapes, written
// out here exactly as the contract prints them. No daemon, no network.

use super::*;

const FIXTURE: &str = include_str!("../../kit/src/fixtures/rendezvous-v1.json");

fn fixture() -> serde_json::Value {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

fn row(name: &str) -> String {
    fixture()["derivations"][name]
        .as_str()
        .unwrap_or_else(|| panic!("row {name}"))
        .to_string()
}

fn input(name: &str) -> serde_json::Value {
    fixture()["inputs"][name].clone()
}

fn token() -> String {
    input("token").as_str().expect("token").to_string()
}

fn hex32(s: &str) -> [u8; 32] {
    let v = hex::decode(s).expect("hex");
    v.try_into().expect("32 bytes")
}

// ---- §13 derivation vectors ---------------------------------------------------------------

#[test]
fn ipc_token_is_hex_of_the_token_utf8_bytes() {
    assert_eq!(token_ipc_hex(&token()), row("token_utf8_hex"));
    // Never the base64url-decoded value (that would be 32 bytes, 64 hex chars).
    assert_eq!(token_ipc_hex(&token()).len(), token().len() * 2);
}

#[test]
fn token_hash_matches_the_vector_and_the_published_hash() {
    // The same BLAKE3(T) the app already publishes for INVITE-S2 (one source of truth).
    assert_eq!(crate::invites::blake3_token_hash(&token()), row("token_hash"));
}

#[test]
fn auth_and_lookup_match_the_vectors() {
    assert_eq!(hex::encode(auth(&token())), row("auth"));
    assert_eq!(lookup_hex(&token()), row("lookup"));
    // lookup = BLAKE3(auth): checked from the auth row alone, too.
    assert_eq!(
        hex::encode(blake3::hash(&hex32(&row("auth"))).as_bytes()),
        row("lookup")
    );
}

#[test]
fn the_fixture_labels_are_the_contract_labels() {
    let f = fixture();
    assert_eq!(f["labels"]["auth_context"], AUTH_CONTEXT);
    assert_eq!(
        hex::encode(f["labels"]["req_mac_prefix"].as_str().expect("label")),
        row("req_mac_input_prefix_hex")
    );
    assert_eq!(
        hex::encode(f["labels"]["welcome_mac_prefix"].as_str().expect("label")),
        row("welcome_mac_input_prefix_hex")
    );
}

/// The MAC rows are computed by the daemons, not by this app. They are re-derived here from the
/// fixture's inputs and labels so the copied file is checked whole: a mistyped row fails here.
#[test]
fn every_mac_row_in_the_copied_fixture_is_consistent() {
    let f = fixture();
    let t = token();
    let mac_ctx = f["labels"]["mac_context"].as_str().expect("label");
    let mac_key = blake3::derive_key(mac_ctx, t.as_bytes());
    assert_eq!(hex::encode(mac_key), row("mac_key"));

    let kp_ref = blake3::hash(input("key_package_ascii").as_str().expect("kp").as_bytes());
    assert_eq!(hex::encode(kp_ref.as_bytes()), row("kp_ref"));

    let group = hex::decode(input("group_id_hex").as_str().expect("g")).expect("hex");
    let joiner = hex::decode(input("joiner_hex").as_str().expect("j")).expect("hex");
    let lookup = hex32(&row("lookup"));

    let mut req = b"citrate-rv-v1|req|".to_vec();
    req.extend_from_slice(&group);
    req.extend_from_slice(&lookup);
    req.extend_from_slice(&joiner);
    req.extend_from_slice(kp_ref.as_bytes());
    assert_eq!(
        hex::encode(blake3::keyed_hash(&mac_key, &req).as_bytes()),
        row("req_mac")
    );

    let wh = blake3::hash(input("welcome_ciphertext_ascii").as_str().expect("w").as_bytes());
    let th = blake3::hash(input("ratchet_tree_ascii").as_str().expect("t").as_bytes());
    assert_eq!(hex::encode(wh.as_bytes()), row("welcome_hash"));
    assert_eq!(hex::encode(th.as_bytes()), row("tree_hash"));
    let epoch = input("epoch").as_u64().expect("epoch");
    let mut wel = b"citrate-rv-v1|welcome|".to_vec();
    wel.extend_from_slice(&group);
    wel.extend_from_slice(&lookup);
    wel.extend_from_slice(&joiner);
    wel.extend_from_slice(kp_ref.as_bytes());
    wel.extend_from_slice(wh.as_bytes());
    wel.extend_from_slice(th.as_bytes());
    wel.extend_from_slice(&epoch.to_be_bytes());
    assert_eq!(
        hex::encode(blake3::keyed_hash(&mac_key, &wel).as_bytes()),
        row("welcome_mac")
    );

    let ch = blake3::hash(input("commit_ciphertext_ascii").as_str().expect("c").as_bytes());
    assert_eq!(hex::encode(ch.as_bytes()), row("commit_hash"));
}

// ---- §9 view shapes ---------------------------------------------------------------------------

/// The §9 `JoinView` example, as printed (comments removed).
const JOIN_VIEW_EXAMPLE: &str = r#"{ "group": "1111111111111111111111111111111111111111111111111111111111111111", "name": "Aperture", "state": "waiting",
  "reason": null, "expires_at": 1761980000000, "grace_until": 1762239200000, "updated_at": 1760860000000 }"#;

/// The §9 `InviteView` example, with the full address the JSON carries.
const INVITE_VIEW_EXAMPLE: &str = r#"{ "lookup": "57e96326581e57d94d5673f8a3c42b21d54aa07ba0cdf62934d73cd947cfdd13", "group": "1111111111111111111111111111111111111111111111111111111111111111", "state": "added",
  "joiner": "0x1234567890abcdef1234567890abcdef12345678", "expires_at": 1761980000000,
  "updated_at": 1760860000000, "reason": null, "dropped": 0 }"#;

#[test]
fn join_view_parses_the_contract_example() {
    let v: JoinView = serde_json::from_str(JOIN_VIEW_EXAMPLE).expect("parse");
    assert_eq!(v.group, "11".repeat(32));
    assert_eq!(v.name, "Aperture");
    assert_eq!(v.state, JoinState::Waiting);
    assert_eq!(v.reason, None);
    assert_eq!(v.expires_at, Some(1_761_980_000_000));
    assert_eq!(v.grace_until, Some(1_762_239_200_000));
    assert_eq!(v.updated_at, Some(1_760_860_000_000));
}

#[test]
fn join_view_accepts_the_nulls_a_requesting_join_carries() {
    let v: JoinView = serde_json::from_str(
        r#"{"group":"aa","name":"X","state":"requesting","reason":null,"expires_at":null,"grace_until":null,"updated_at":null}"#,
    )
    .expect("parse");
    assert_eq!(v.state, JoinState::Requesting);
    assert_eq!((v.expires_at, v.grace_until, v.updated_at), (None, None, None));
}

#[test]
fn every_contract_join_state_parses_and_round_trips() {
    let all = [
        ("requesting", JoinState::Requesting),
        ("waiting", JoinState::Waiting),
        ("joining", JoinState::Joining),
        ("joined", JoinState::Joined),
        ("expired", JoinState::Expired),
        ("revoked", JoinState::Revoked),
        ("taken", JoinState::Taken),
        ("dropped", JoinState::Dropped),
        ("withdrawn", JoinState::Withdrawn),
        ("unknown", JoinState::Unknown),
        ("welcome_expired", JoinState::WelcomeExpired),
        ("reply_unverified", JoinState::ReplyUnverified),
        ("relay_unreachable", JoinState::RelayUnreachable),
        ("relay_unsupported", JoinState::RelayUnsupported),
        ("failed", JoinState::Failed),
    ];
    for (wire, state) in all {
        let parsed: JoinState = serde_json::from_str(&format!("\"{wire}\"")).expect(wire);
        assert_eq!(parsed, state);
        assert_eq!(serde_json::to_string(&state).expect("ser"), format!("\"{wire}\""));
    }
    // A word the contract doesn't have is refused, never guessed.
    assert!(serde_json::from_str::<JoinState>("\"offline\"").is_err());
}

#[test]
fn every_contract_reason_code_is_carried_verbatim() {
    // §9 / §11 reason codes, including rev 6 (`legacy_redeemed`, `not_a_member`) and rev 7
    // (`already_delivered`). The view keeps the code; the UI maps it.
    for reason in [
        "no_rendezvous",
        "purged",
        "already_delivered",
        "group_mismatch",
        "binding_rejected",
        "domain_mismatch",
        "too_large",
        "quota_exceeded",
        "bad_binding",
        "already_member",
        "bad_mac",
        "bad_key_package",
        "mls",
    ] {
        let v: JoinView = serde_json::from_str(&format!(
            r#"{{"group":"aa","name":"X","state":"failed","reason":"{reason}","expires_at":null,"grace_until":null,"updated_at":1}}"#
        ))
        .expect(reason);
        assert_eq!(v.reason.as_deref(), Some(reason));
    }
}

#[test]
fn invite_view_parses_the_contract_example() {
    let v: InviteView = serde_json::from_str(INVITE_VIEW_EXAMPLE).expect("parse");
    assert_eq!(v.lookup, row("lookup"));
    assert_eq!(v.state, InviteState::Added);
    assert_eq!(v.joiner.as_deref(), Some("0x1234567890abcdef1234567890abcdef12345678"));
    assert_eq!(v.expires_at, 1_761_980_000_000);
    assert_eq!(v.updated_at, Some(1_760_860_000_000));
    assert_eq!(v.reason, None);
    assert_eq!(v.dropped, 0);
}

#[test]
fn every_contract_invite_state_parses_including_rev5_unknown() {
    let all = [
        ("open", InviteState::Open),
        ("adding", InviteState::Adding),
        ("added", InviteState::Added),
        ("picked_up", InviteState::PickedUp),
        ("welcome_expired", InviteState::WelcomeExpired),
        ("expired", InviteState::Expired),
        ("revoked", InviteState::Revoked),
        ("failed", InviteState::Failed),
        ("relay_unreachable", InviteState::RelayUnreachable),
        ("unknown", InviteState::Unknown),
    ];
    for (wire, state) in all {
        let parsed: InviteState = serde_json::from_str(&format!("\"{wire}\"")).expect(wire);
        assert_eq!(parsed, state);
        assert_eq!(serde_json::to_string(&state).expect("ser"), format!("\"{wire}\""));
    }
    assert!(serde_json::from_str::<InviteState>("\"joined\"").is_err());
}

#[test]
fn invite_view_carries_rev5_and_rev6_reasons() {
    for (state, reason) in [
        ("unknown", "purged"),
        ("unknown", "legacy_redeemed"),
        ("failed", "not_a_member"),
        ("failed", "too_large"),
    ] {
        let v: InviteView = serde_json::from_str(&format!(
            r#"{{"lookup":"ab","group":"cd","state":"{state}","joiner":null,"expires_at":5,"updated_at":null,"reason":"{reason}","dropped":2}}"#
        ))
        .expect(reason);
        assert_eq!(v.reason.as_deref(), Some(reason));
        assert_eq!(v.joiner, None);
        assert_eq!(v.dropped, 2);
    }
}

#[test]
fn views_reach_the_webview_in_camel_case_with_no_secret_fields() {
    let views = TickViews {
        joins: vec![serde_json::from_str(JOIN_VIEW_EXAMPLE).expect("join")],
        invites: vec![serde_json::from_str(INVITE_VIEW_EXAMPLE).expect("invite")],
    };
    let json = serde_json::to_string(&views).expect("ser");
    for k in ["expiresAt", "graceUntil", "updatedAt", "\"joins\"", "\"invites\"", "\"dropped\""] {
        assert!(json.contains(k), "{k} in {json}");
    }
    assert!(!json.contains("expires_at"), "{json}");
    // No relay token, bearer derivative or MAC key field can cross invoke.
    let lower = json.to_lowercase();
    for banned in ["token", "auth", "mac", "secret", "priv", "\"t\""] {
        assert!(!lower.contains(banned), "{banned} in {json}");
    }
    // The token itself, its hex form and the MAC key never appear.
    assert!(!json.contains(&token()));
    assert!(!json.contains(&row("token_utf8_hex")));
    assert!(!json.contains(&row("mac_key")));
}

#[test]
fn the_join_state_event_name_is_stable() {
    assert_eq!(JOIN_STATE_EVENT, "join://state");
    assert!(TICK_INTERVAL <= std::time::Duration::from_secs(30));
}

// ---- daemon error classification ---------------------------------------------------------------

#[test]
fn an_older_daemon_answering_an_unknown_op_is_daemon_unsupported() {
    // What comms-member-daemon's server writes for a request line its `Request` enum can't parse.
    let msg = "bad request: unknown variant `rendezvousTick`, expected one of `createGroup`, `listGroups`, `joinGroup` at line 1 column 25";
    assert_eq!(classify_daemon_error(msg), RvError::DaemonUnsupported);
    let msg = "bad request: unknown variant `rendezvousOpen`, expected one of `createGroup`";
    assert_eq!(classify_daemon_error(msg), RvError::DaemonUnsupported);
}

#[test]
fn other_daemon_errors_stay_errors() {
    for msg in [
        "bad request: missing field `group` at line 1 column 30",
        "relay_unsupported",
        "comms relay unreachable",
        "unknown variant `rendezvousTick`",
    ] {
        assert_eq!(classify_daemon_error(msg), RvError::Daemon(msg.to_string()), "{msg}");
    }
}

#[test]
fn relay_unsupported_is_recognised_only_as_the_stable_code() {
    assert!(is_relay_unsupported(&RvError::Daemon("relay_unsupported".into())));
    assert!(!is_relay_unsupported(&RvError::Daemon("relay unreachable".into())));
    assert!(!is_relay_unsupported(&RvError::DaemonUnsupported));
    assert!(!is_relay_unsupported(&RvError::Ipc("relay_unsupported".into())));
}

// ---- M4 tick decisions ---------------------------------------------------------------------------

#[test]
fn a_scheduled_tick_runs_when_the_daemon_runs_or_there_is_local_work() {
    assert!(should_tick(true, false));
    assert!(should_tick(true, true));
    assert!(should_tick(false, true));
    assert!(!should_tick(false, false), "no daemon start for a device with nothing to do");
}

#[test]
fn only_non_terminal_joins_keep_the_device_ticking() {
    use JoinState::*;
    for s in [Requesting, Waiting, Joining, ReplyUnverified, RelayUnreachable] {
        assert!(join_needs_ticks(s), "{s:?}");
    }
    for s in [
        Joined,
        Expired,
        Revoked,
        Taken,
        Dropped,
        Withdrawn,
        Unknown,
        WelcomeExpired,
        RelayUnsupported,
        Failed,
    ] {
        assert!(!join_needs_ticks(s), "{s:?}");
    }
}

#[test]
fn active_join_groups_lists_each_live_group_once() {
    let mk = |group: &str, state: JoinState| JoinView {
        group: group.into(),
        name: "n".into(),
        state,
        reason: None,
        expires_at: None,
        grace_until: None,
        updated_at: None,
    };
    let views = TickViews {
        joins: vec![
            mk("bb", JoinState::Waiting),
            mk("aa", JoinState::Requesting),
            mk("bb", JoinState::Waiting),
            mk("cc", JoinState::Joined),
        ],
        invites: vec![],
    };
    assert_eq!(active_join_groups(&views), vec!["aa".to_string(), "bb".to_string()]);
}

// ---- M3: the invitee's Join (§12 fallback, forget rule) ----------------------------------------

fn view(state: JoinState, reason: Option<&str>) -> JoinView {
    JoinView {
        group: "g".into(),
        name: "n".into(),
        state,
        reason: reason.map(str::to_string),
        expires_at: None,
        grace_until: None,
        updated_at: None,
    }
}

#[test]
fn legacy_fallback_only_on_relay_unsupported_or_no_rendezvous() {
    assert_eq!(decide_join(&Ok(view(JoinState::RelayUnsupported, None))), Some(JoinNext::Legacy));
    assert_eq!(
        decide_join(&Ok(view(JoinState::Unknown, Some("no_rendezvous")))),
        Some(JoinNext::Legacy)
    );
    // A daemon from before the rendezvous ops can only do the legacy path.
    assert_eq!(decide_join(&Err(RvError::DaemonUnsupported)), Some(JoinNext::Legacy));
}

#[test]
fn never_legacy_on_requesting_relay_unreachable_or_purged() {
    for (state, reason) in [
        (JoinState::Requesting, None),
        (JoinState::RelayUnreachable, None),
        (JoinState::Unknown, Some("purged")),
        (JoinState::Unknown, None),
    ] {
        assert_ne!(decide_join(&Ok(view(state, reason))), Some(JoinNext::Legacy), "{state:?} {reason:?}");
    }
    // An IPC or daemon error is an error, not a fallback.
    assert_eq!(decide_join(&Err(RvError::Ipc("socket".into()))), None);
    assert_eq!(decide_join(&Err(RvError::Daemon("bad".into()))), None);
}

#[test]
fn the_opened_invite_is_kept_until_the_daemon_has_a_definite_answer() {
    use JoinState::*;
    for s in [Requesting, RelayUnreachable] {
        assert_eq!(decide_join(&Ok(view(s, None))), Some(JoinNext::Rendezvous { forget: false }), "{s:?}");
    }
    for s in [
        Waiting, Joining, Joined, Expired, Revoked, Taken, Dropped, Withdrawn, WelcomeExpired,
        ReplyUnverified, Failed,
    ] {
        assert_eq!(decide_join(&Ok(view(s, None))), Some(JoinNext::Rendezvous { forget: true }), "{s:?}");
    }
    assert_eq!(
        decide_join(&Ok(view(Unknown, Some("purged")))),
        Some(JoinNext::Rendezvous { forget: true })
    );
}

#[test]
fn join_redeem_view_names_the_path_and_carries_only_the_view() {
    let r = serde_json::to_value(JoinRedeemView::Rendezvous { join: view(JoinState::Waiting, None) })
        .expect("ser");
    assert_eq!(r["path"], "rendezvous");
    assert_eq!(r["join"]["state"], "waiting");
    assert_eq!(serde_json::to_value(JoinRedeemView::Legacy).expect("ser"), serde_json::json!({"path":"legacy"}));
}

#[test]
fn the_join_path_sends_the_hex_token_and_falls_back_only_through_decide_join() {
    let src = include_str!("rendezvous.rs");
    let body = &src[src.find("pub(crate) fn join_opened").expect("fn")..];
    let body = &body[..body.find("\n}\n").expect("end")];
    assert!(body.contains("token_ipc_hex(token)"));
    assert_eq!(body.matches("redeem_link(app, legacy_link)").count(), 1);
    // The legacy self-admit runs only when decide_join says so.
    for (answer, legacy) in [
        (Err(RvError::DaemonUnsupported), true),
        (Ok(view(JoinState::RelayUnsupported, None)), true),
        (Ok(view(JoinState::Unknown, Some("no_rendezvous"))), true),
        (Ok(view(JoinState::Waiting, None)), false),
        (Ok(view(JoinState::RelayUnreachable, None)), false),
        (Err(RvError::Ipc("timed out".into())), false),
    ] {
        let ran = std::cell::Cell::new(false);
        let _ = join_opened_at(
            None,
            "g",
            || answer.clone(),
            || {
                ran.set(true);
                Ok(())
            },
        );
        assert_eq!(ran.get(), legacy, "{answer:?}");
    }
}

// ---- Review of 56edc33: citrate://invite links (paste box, pending banner) join through the
// rendezvous too. Once a rendezvous exists, the relay refuses the legacy RedeemInvite for that
// token in every state (§4 rev 6), so the legacy path must not go first.

#[test]
fn citrate_invite_links_try_the_rendezvous_first() {
    let src = include_str!("invites.rs");
    let body = &src[src.find("pub async fn group_invite_redeem").expect("fn")..];
    let body = &body[..body.find("\n}\n").expect("end")];
    assert!(body.contains("crate::rendezvous::join_opened("), "{body}");
    assert!(!body.contains("redeem_link(&app"), "the legacy redeem runs only inside join_opened");
    assert!(body.contains("crate::rendezvous::tick_soon(&app)"));
}

#[test]
fn the_legacy_fallback_sends_the_daemon_encoding() {
    let src = include_str!("invites.rs");
    let body = &src[src.find("pub(crate) fn redeem_link").expect("fn")..];
    let body = &body[..body.find("\n}\n").expect("end")];
    assert!(body.contains("redeem_token_wire(&token)?"));
    // Same bytes as the rendezvous ops (§9): hex of the token's UTF-8 bytes.
    let t = token();
    assert_eq!(crate::invites::redeem_token_wire(&t).as_deref(), Ok(token_ipc_hex(&t).as_str()));
}

// ---- review round 2: which devices tick ----------------------------------------------------------

fn pending(lookup: &str, created_at: u64) -> crate::invites::PendingInvite {
    crate::invites::minted_invite(
        "g",
        "t",
        "@h",
        created_at,
        String::new(),
        String::new(),
        decision(lookup),
    )
}

/// A mint that recorded `lookup` ("" = no rendezvous was opened).
fn decision(lookup: &str) -> crate::invites::MintDecision {
    let open = if lookup.is_empty() {
        Err(RvError::DaemonUnsupported)
    } else {
        Ok(crate::comms::RendezvousOpened {
            lookup: lookup.to_string(),
            expires_at: 1,
        })
    };
    crate::invites::decide_mint(Ok(()), open, lookup).expect("the legacy publish worked")
}

/// pysol-dev #3: only an unexpired invite with a rendezvous, or a join in progress, is local work.
/// Pre-S2 invites (no lookup) and invites past their 14-day TTL never start the daemon.
#[test]
fn local_work_counts_only_live_rendezvous_invites_and_active_joins() {
    let now = 1_800_000_000;
    let day = 24 * 60 * 60;
    let lookup = "ab".repeat(32);
    assert!(!local_work(&[], &[], now));
    assert!(local_work(&["g".to_string()], &[], now));
    assert!(local_work(&[], &[pending(&lookup, now - day)], now));
    assert!(local_work(&[], &[pending(&lookup, now - 14 * day + 60)], now));
    assert!(
        !local_work(&[], &[pending("", now - day)], now),
        "a pre-S2 invite has no rendezvous to tick"
    );
    assert!(
        !local_work(&[], &[pending(&lookup, now - 14 * day)], now),
        "an invite past its TTL has nothing left at the relay"
    );
    assert!(
        !local_work(&[], &[pending(&lookup, now - 30 * day), pending("", now)], now),
        "a mix of expired and pre-S2 invites"
    );
}

/// pysol-dev #3 (optional part): a daemon without the rendezvous ops stops the scheduled tick for
/// the session (the sidecar doesn't change while the app runs); any other answer keeps it.
#[test]
fn an_unsupported_daemon_stops_the_scheduled_tick() {
    assert!(!scheduled_tick_continues(&Err(RvError::DaemonUnsupported)));
    assert!(scheduled_tick_continues(&Err(RvError::Ipc("timed out".into()))));
    assert!(scheduled_tick_continues(&Err(RvError::Daemon("relay_unsupported".into()))));
    assert!(scheduled_tick_continues(&Ok(TickViews::default())));
}

// ---- review round 3: the restart marker is written before rendezvousJoin -------------------------

fn marker(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "citrate-rv-marker-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join("rendezvous-active.json")
}

/// Codex round 3 (P2): the daemon persists the join before it answers, so a lost answer can leave a
/// real join behind. The marker is written before the call and kept, so after a restart the device
/// still has local work and the tick resumes the join.
#[test]
fn a_join_whose_answer_is_lost_still_resumes_after_a_restart() {
    let m = marker("lost-answer");
    let seen_before_send = std::cell::Cell::new(false);
    let r = join_opened_at(
        Some(&m),
        "g1",
        || {
            seen_before_send.set(active_joins_at(&m).contains(&"g1".to_string()));
            Err(RvError::Ipc("comms daemon did not respond within 15000 ms".into()))
        },
        || panic!("no legacy fallback on an ambiguous answer"),
    );
    assert!(r.is_err());
    assert!(seen_before_send.get(), "the marker is written before rendezvousJoin is sent");
    // The immediate tick failed too, and the app restarts: no minted invites on this device.
    assert_eq!(active_joins_at(&m), vec!["g1".to_string()]);
    assert!(local_work(&active_joins_at(&m), &[], 1_800_000_000));
}

/// A tick that rewrites the marker while the join is in flight (from views taken before the daemon
/// had the join) doesn't lose it: the marker is written again after an answer that isn't definite.
#[test]
fn a_tick_that_rewrites_the_marker_mid_join_doesnt_lose_the_join() {
    let m = marker("tick-race");
    let r = join_opened_at(
        Some(&m),
        "g1",
        || {
            std::fs::write(&m, b"[]").expect("a stale tick rewrites the marker");
            Err(RvError::Ipc("connection reset".into()))
        },
        || Ok(()),
    );
    assert!(r.is_err());
    assert_eq!(active_joins_at(&m), vec!["g1".to_string()]);
    let ok = join_opened_at(
        Some(&m),
        "g2",
        || {
            std::fs::write(&m, b"[]").expect("stale tick");
            Ok(view(JoinState::Waiting, None))
        },
        || Ok(()),
    );
    assert!(ok.is_ok());
    assert!(active_joins_at(&m).contains(&"g2".to_string()));
}

/// The legacy path leaves no rendezvous work behind: the marker for that group is removed.
#[test]
fn the_legacy_path_clears_the_marker_for_its_group() {
    let m = marker("legacy");
    note_active_join_at(&m, "other");
    let r = join_opened_at(
        Some(&m),
        "g1",
        || Err(RvError::DaemonUnsupported),
        || Ok(()),
    );
    assert_eq!(r.map(|(v, forget)| (v, forget)), Ok((JoinRedeemView::Legacy, true)));
    assert_eq!(active_joins_at(&m), vec!["other".to_string()]);
}
