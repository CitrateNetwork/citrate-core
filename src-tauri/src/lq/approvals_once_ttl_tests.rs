// L1-WP01 CH-9 (characterization, citrate-core a826e441). Included into
// `node_mcp_approvals::lq_characterize`.
//
// Pins today's approval inbox properties (node_mcp_approvals.rs module doc): a request is decided
// at most once, a client sees and cancels only its own requests, a pending request expires at
// exactly REQUEST_TTL_MS (the ceremony's pending TTL) and its ceremony is closed once, at most
// MAX_PENDING wait at once, and pending requests are never trimmed. Approval-card semantics are a
// lane ratchet (L1 section 2.2 item 6): no L1 WP may change these; a stricter replacement names CH-9.

mod lq_ch9_approvals_once_ttl {
    use crate::node_mcp_approvals::*;
    use serde_json::json;

    const T0: u64 = 1_000_000;

    fn sig(ceremony: &str) -> RequestKind {
        RequestKind::Signature {
            ceremony_id: ceremony.to_string(),
            ceremony: json!({"id": ceremony}),
        }
    }

    fn pin() -> RequestKind {
        RequestKind::Action {
            action: McpAction::PinAdd {
                cid: "bafyexample".to_string(),
            },
        }
    }

    fn submit(inbox: &ApprovalInbox, token: &str, kind: RequestKind, now: u64) -> McpRequest {
        inbox
            .submit(token, "mcp:test", kind, "a request".to_string(), now)
            .expect("room")
            .0
    }

    fn state_of(inbox: &ApprovalInbox, id: &str, token: &str, now: u64) -> RequestState {
        inbox
            .status_for(id, token, now)
            .0
            .expect("the owner sees its request")
            .state
    }

    #[test]
    fn lq_ch9_the_ttl_is_the_ceremony_pending_ttl() {
        assert_eq!(REQUEST_TTL_MS, crate::ceremony::PENDING_TTL_MS);
        assert_eq!(REQUEST_TTL_MS, 15 * 60 * 1000);
        assert_eq!(MAX_PENDING, 16);
        assert_eq!(MAX_KEPT, 64);
    }

    #[test]
    fn lq_ch9_a_request_is_decided_at_most_once() {
        for (first, second) in [(true, true), (true, false), (false, true), (false, false)] {
            let inbox = ApprovalInbox::new();
            let r = submit(&inbox, "tok-a", sig("c-1"), T0);
            assert!(inbox.begin_decision(&r.id, first, T0 + 1).is_ok());
            let again = inbox.begin_decision(&r.id, second, T0 + 2);
            assert!(
                again
                    .as_ref()
                    .is_err_and(|e| e.contains("no longer waiting")),
                "{first} then {second}: {again:?}"
            );
            if first {
                assert!(inbox.finish(&r.id, Ok(json!({"ok": true})), T0 + 3).is_some());
                // A finished request cannot be finished or decided again.
                assert!(inbox.finish(&r.id, Ok(json!({"ok": 2})), T0 + 4).is_none());
                assert!(inbox.begin_decision(&r.id, true, T0 + 5).is_err());
            }
        }
        let inbox = ApprovalInbox::new();
        assert!(inbox.begin_decision("mcpr-404", true, T0).is_err());
    }

    #[test]
    fn lq_ch9_a_client_sees_and_cancels_only_its_own_requests() {
        let inbox = ApprovalInbox::new();
        let r = submit(&inbox, "tok-a", sig("c-1"), T0);
        assert!(inbox.status_for(&r.id, "tok-b", T0).0.is_none());
        let (theirs, close) = inbox.cancel_by_client(&r.id, "tok-b", T0);
        assert!(theirs.is_none());
        assert!(close.is_empty());
        let close = inbox.revoke_token("tok-b", T0);
        assert!(close.is_empty());
        assert_eq!(state_of(&inbox, &r.id, "tok-a", T0), RequestState::Pending);
        // The owner cancels: rejected with the client reason, and the ceremony is closed.
        let (mine, close) = inbox.cancel_by_client(&r.id, "tok-a", T0 + 1);
        assert!(mine.is_some());
        assert_eq!(close, [CeremonyToClose("c-1".to_string())]);
        assert_eq!(
            state_of(&inbox, &r.id, "tok-a", T0 + 1),
            RequestState::Rejected {
                reason: CANCELLED_BY_CLIENT.to_string()
            }
        );
    }

    #[test]
    fn lq_ch9_a_pending_request_expires_at_exactly_the_ttl_and_its_ceremony_closes_once() {
        let inbox = ApprovalInbox::new();
        let a = submit(&inbox, "tok-a", sig("c-a"), T0);
        let b = submit(&inbox, "tok-a", pin(), T0);
        let last_ms = T0 + REQUEST_TTL_MS - 1;
        assert_eq!(state_of(&inbox, &a.id, "tok-a", last_ms), RequestState::Pending);
        let (_, close) = inbox.list(last_ms);
        assert!(close.is_empty());
        let (_, close) = inbox.list(T0 + REQUEST_TTL_MS);
        assert_eq!(close, [CeremonyToClose("c-a".to_string())], "an action closes nothing");
        assert_eq!(
            state_of(&inbox, &a.id, "tok-a", T0 + REQUEST_TTL_MS),
            RequestState::Expired
        );
        assert_eq!(
            state_of(&inbox, &b.id, "tok-a", T0 + REQUEST_TTL_MS),
            RequestState::Expired
        );
        let (_, close) = inbox.list(T0 + REQUEST_TTL_MS + 1);
        assert!(close.is_empty(), "the ceremony is closed once");
        assert!(inbox
            .begin_decision(&a.id, true, T0 + REQUEST_TTL_MS + 1)
            .is_err());
    }

    #[test]
    fn lq_ch9_at_most_max_pending_wait_and_a_decision_or_expiry_makes_room() {
        let inbox = ApprovalInbox::new();
        let mut ids = Vec::new();
        for i in 0..MAX_PENDING {
            ids.push(submit(&inbox, "tok-a", sig(&format!("c-{i}")), T0).id);
        }
        assert!(!inbox.has_room(T0).0);
        assert!(inbox
            .submit("tok-b", "mcp:other", pin(), "one more".to_string(), T0)
            .is_err());
        assert!(inbox.begin_decision(&ids[0], false, T0 + 1).is_ok());
        assert!(inbox.has_room(T0 + 1).0);
        let _ = submit(&inbox, "tok-b", pin(), T0 + 1);
        assert!(!inbox.has_room(T0 + 1).0);
        // Once the first batch expires there is room again.
        assert!(inbox.has_room(T0 + REQUEST_TTL_MS).0);
    }

    #[test]
    fn lq_ch9_pending_requests_are_never_trimmed() {
        let inbox = ApprovalInbox::new();
        for _ in 0..MAX_KEPT + 6 {
            let r = submit(&inbox, "tok-a", pin(), T0);
            assert!(inbox.begin_decision(&r.id, false, T0).is_ok());
        }
        let mut pending = Vec::new();
        for _ in 0..MAX_PENDING {
            pending.push(submit(&inbox, "tok-a", pin(), T0 + 1).id);
        }
        let (all, _) = inbox.list(T0 + 2);
        assert_eq!(all.len(), MAX_KEPT);
        for id in &pending {
            assert!(all
                .iter()
                .any(|r| &r.id == id && r.state == RequestState::Pending));
        }
    }

    /// A seeded walk over submit, decide, cancel, revoke and the clock: no request is ever decided
    /// twice, none returns to pending, and the pending count never passes MAX_PENDING.
    #[test]
    fn lq_ch9_seeded_walk_keeps_the_inbox_properties() {
        let inbox = ApprovalInbox::new();
        let mut seed: u64 = 0x5eed_0c49;
        let mut next = move |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let mut now = T0;
        let mut left: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let tokens = ["tok-a", "tok-b", "tok-c"];
        for _ in 0..3_000 {
            now += next(120_000);
            let (all, _) = inbox.list(now);
            let ids: Vec<String> = all.iter().map(|r| r.id.clone()).collect();
            match next(5) {
                0 | 1 => {
                    let token = tokens[next(3) as usize];
                    let kind = if next(2) == 0 { pin() } else { sig("c-w") };
                    let _ = inbox.submit(token, "mcp:walk", kind, "walk".to_string(), now);
                }
                2 if !ids.is_empty() => {
                    let id = &ids[next(ids.len() as u64) as usize];
                    let _ = inbox.begin_decision(id, next(2) == 0, now);
                }
                3 if !ids.is_empty() => {
                    let id = &ids[next(ids.len() as u64) as usize];
                    let _ = inbox.cancel_by_client(id, tokens[next(3) as usize], now);
                }
                _ => {
                    let _ = inbox.revoke_token(tokens[next(3) as usize], now);
                }
            }
            let (all, _) = inbox.list(now);
            let pending = all
                .iter()
                .filter(|r| r.state == RequestState::Pending)
                .count();
            assert!(pending <= MAX_PENDING, "{pending} pending");
            for r in &all {
                let seen = left.get(&r.id).copied();
                match (&r.state, seen) {
                    (RequestState::Pending, Some(at)) => {
                        panic!("{} went back to pending after leaving it at {at}", r.id)
                    }
                    (RequestState::Pending, None) => {
                        assert!(r.decided_ms.is_none(), "{}", r.id);
                        assert!(now.saturating_sub(r.created_ms) < REQUEST_TTL_MS, "{}", r.id);
                    }
                    (_, Some(at)) => assert_eq!(r.decided_ms, Some(at), "{} decided twice", r.id),
                    (_, None) => {
                        let at = r.decided_ms.expect("a decided request has a time");
                        left.insert(r.id.clone(), at);
                    }
                }
            }
        }
    }
}
