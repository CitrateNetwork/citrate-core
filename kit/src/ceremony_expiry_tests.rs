// Pending-request lifetime and chain binding for the SignatureCeremony.
//
// Two properties, each checked on every path that reaches a wallet signer:
// - a pending request is approvable for at most `PENDING_TTL`, measured on both the monotonic and
//   the wall clock (whichever says more time has passed), and an expired request is removed;
// - the chain id the approval card shows is the chain id the signature commits to, checked
//   immediately before the signer is called. A mismatch signs nothing.
//
// Included into `ceremony::tests`, so the helpers of `ceremony_tests.rs` (vaults, intents, the
// scripted `MockRpc`, `bcfg`) are in scope.

use std::sync::atomic::AtomicU64 as TestAtomicU64;
use std::sync::Arc;

/// A clock the test moves by hand: monotonic and wall time are set independently, so clock
/// skew, a clock set back, and a machine that slept can each be simulated.
struct FakeClock {
    base: std::time::Instant,
    mono_ms: TestAtomicU64,
    wall_ms: TestAtomicU64,
}

/// The wall clock the fake starts at (an arbitrary 2026 instant).
const WALL0: u64 = 1_790_000_000_000;

impl FakeClock {
    fn new() -> Arc<Self> {
        Arc::new(FakeClock {
            base: std::time::Instant::now(),
            mono_ms: TestAtomicU64::new(0),
            wall_ms: TestAtomicU64::new(WALL0),
        })
    }
    fn read(&self) -> ClockReading {
        ClockReading {
            mono: self.base + std::time::Duration::from_millis(self.mono_ms.load(Ordering::SeqCst)),
            wall_ms: self.wall_ms.load(Ordering::SeqCst),
        }
    }
    /// Advance both clocks together by `ms` (ordinary time passing).
    fn advance(&self, ms: u64) {
        self.mono_ms.fetch_add(ms, Ordering::SeqCst);
        self.wall_ms.fetch_add(ms, Ordering::SeqCst);
    }
    fn set_mono(&self, ms: u64) {
        self.mono_ms.store(ms, Ordering::SeqCst);
    }
    fn set_wall(&self, ms: u64) {
        self.wall_ms.store(ms, Ordering::SeqCst);
    }
}

fn ceremony_on(clock: &Arc<FakeClock>) -> SignatureCeremony {
    let c = Arc::clone(clock);
    SignatureCeremony::with_clock(PENDING_TTL, Box::new(move || c.read()))
}

/// The RPC script a successful transfer needs: nonce, gas price, send, receipt.
fn transfer_script() -> Vec<JsonValue> {
    vec![
        ok(JsonValue::String("0x7".into())),
        ok(JsonValue::String("0x77359400".into())),
        ok(JsonValue::String(
            "0xabc0000000000000000000000000000000000000000000000000000000000abc".into(),
        )),
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]
}

fn methods_called(rpc: &RpcClient<MockRpc>) -> Vec<String> {
    rpc_transport(rpc)
        .requests
        .borrow()
        .iter()
        .map(|r| r["method"].as_str().unwrap_or("").to_string())
        .collect()
}

fn rpc_transport(rpc: &RpcClient<MockRpc>) -> &MockRpc {
    rpc.transport()
}

fn a_transfer(chain_id: u64, extra: Option<(&str, JsonValue)>) -> SignatureIntent {
    let mut raw = serde_json::json!({
        "from": canonical_addr_lower(),
        "to": "0x3535353535353535353535353535353535353535",
        "value": "0x1",
        "data": "0x",
    });
    if let Some((k, v)) = extra {
        raw[k] = v;
    }
    SignatureIntent {
        origin: "https://app.citrate.ai".to_string(),
        kind: IntentKind::Transaction,
        chain_id,
        raw: raw.to_string(),
    }
}

fn sign_in_text(chain_line: &str) -> String {
    format!(
        "app.citrate.ai wants you to sign in with your Ethereum account:\n{CANONICAL_ADDRESS}\n\nSign in to Citrate.\n\nURI: https://app.citrate.ai\nVersion: 1\n{chain_line}\nNonce: abcdef123456\nIssued At: 2026-10-23T10:00:00Z"
    )
}

// =========================================================================
// Lifetime
// =========================================================================

#[test]
fn ttl_an_expired_personal_sign_request_cannot_be_approved() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let view = c.request(personal_sign_intent("sign me later"));
    clock.advance(PENDING_TTL_MS + 1);
    assert_eq!(
        c.approve(&v, &view.id, false),
        Err(CeremonyError::Expired),
        "a request older than its TTL must not be signed"
    );
    assert!(c.status(&view.id).is_none(), "an expired request is no longer shown");
    assert!(c.is_expired(&view.id), "the id is known to have expired, not to have been decided");
}

#[test]
fn ttl_an_expired_transaction_is_never_signed_or_sent() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let view = c.request(a_transfer(40204, None));
    clock.advance(PENDING_TTL_MS + 1);
    let rpc = RpcClient::with_transport(MockRpc::new(transfer_script()));
    assert_eq!(
        c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2)),
        Err(CeremonyError::Expired)
    );
    assert!(
        methods_called(&rpc).is_empty(),
        "an expired request makes no RPC call at all: {:?}",
        methods_called(&rpc)
    );
}

#[test]
fn ttl_a_request_inside_its_ttl_signs_and_one_at_the_ttl_does_not() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let early = c.request(personal_sign_intent("just in time"));
    let late = c.request(personal_sign_intent("too late"));
    clock.advance(PENDING_TTL_MS - 1);
    assert!(c.approve(&v, &early.id, false).is_ok(), "1 ms inside the TTL still signs");
    clock.advance(1);
    assert_eq!(
        c.approve(&v, &late.id, false),
        Err(CeremonyError::Expired),
        "at exactly the TTL the request has expired"
    );
}

#[test]
fn ttl_an_expired_request_is_removed_and_reject_reports_it() {
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let old = c.request(personal_sign_intent("old"));
    clock.advance(PENDING_TTL_MS);
    let fresh = c.request(personal_sign_intent("fresh"));
    assert_eq!(c.pending_count(), 1, "only the fresh request is still held");
    assert!(c.status(&fresh.id).is_some());
    assert_eq!(c.reject(&old.id), Err(CeremonyError::Expired));
    assert!(!c.is_expired(&fresh.id));
}

#[test]
fn ttl_setting_the_wall_clock_back_does_not_extend_a_request() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let view = c.request(personal_sign_intent("skew"));
    // The monotonic clock passes the TTL while the wall clock is set back an hour.
    clock.set_mono(PENDING_TTL_MS);
    clock.set_wall(WALL0 - 3_600_000);
    assert_eq!(c.approve(&v, &view.id, false), Err(CeremonyError::Expired));
}

#[test]
fn ttl_a_machine_that_slept_past_the_ttl_expires_the_request_on_wake() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let view = c.request(personal_sign_intent("overnight"));
    // Monotonic time barely moved (it may not count sleep); the wall clock moved 8 hours.
    clock.set_mono(1_000);
    clock.set_wall(WALL0 + 8 * 3_600_000);
    assert_eq!(c.approve(&v, &view.id, false), Err(CeremonyError::Expired));
}

#[test]
fn ttl_a_wall_clock_jump_forward_inside_the_ttl_is_not_an_expiry() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let view = c.request(personal_sign_intent("small skew"));
    clock.set_mono(1_000);
    clock.set_wall(WALL0 + PENDING_TTL_MS - 1);
    assert!(c.approve(&v, &view.id, false).is_ok());
}

#[test]
fn ttl_a_raw_ack_retry_keeps_the_original_deadline() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    // Opaque calldata to a contract: raw-ack gated.
    let view = c.request(a_transfer(40204, Some(("data", JsonValue::String("0xdeadbeef01".into())))));
    assert!(view.requires_raw_ack);
    let rpc = RpcClient::with_transport(MockRpc::new(Vec::new()));
    assert_eq!(
        c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2)),
        Err(CeremonyError::RawAckRequired)
    );
    clock.advance(PENDING_TTL_MS);
    assert_eq!(
        c.approve_and_broadcast(&v, &rpc, &view.id, true, bcfg(2)),
        Err(CeremonyError::Expired),
        "re-inserting a request after a missing ack must not restart its TTL"
    );
    assert!(methods_called(&rpc).is_empty());
}

/// An RPC transport that moves the fake clock forward when the nonce is read, so the TTL runs
/// out between the consume step and the signature.
struct SlowRpc {
    inner: MockRpc,
    clock: Arc<FakeClock>,
}
impl RpcTransport for SlowRpc {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        if body["method"] == "eth_getTransactionCount" {
            self.clock.advance(PENDING_TTL_MS);
        }
        self.inner.call(body)
    }
}

#[test]
fn ttl_is_checked_again_immediately_before_signing() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let view = c.request(a_transfer(40204, None));
    let rpc = RpcClient::with_transport(SlowRpc {
        inner: MockRpc::new(transfer_script()),
        clock: Arc::clone(&clock),
    });
    assert_eq!(
        c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2)),
        Err(CeremonyError::Expired),
        "the TTL ran out while the nonce was read: nothing may be signed"
    );
    let sent = rpc
        .transport()
        .inner
        .requests
        .borrow()
        .iter()
        .any(|r| r["method"] == "eth_sendRawTransaction");
    assert!(!sent, "nothing reached eth_sendRawTransaction");
}

#[test]
fn ttl_racing_approvals_of_an_expired_request_all_fail_and_sign_nothing() {
    let (v, _p) = vault_with_wallet();
    let clock = FakeClock::new();
    let c = ceremony_on(&clock);
    let view = c.request(personal_sign_intent("race an expired one"));
    clock.advance(PENDING_TTL_MS);
    let results: Vec<_> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..8).map(|_| s.spawn(|| c.approve(&v, &view.id, false))).collect();
        hs.into_iter().map(|h| h.join().expect("join")).collect()
    });
    assert!(
        results.iter().all(|r| r == &Err(CeremonyError::Expired)),
        "every racing approval of an expired request is refused: {results:?}"
    );
    assert_eq!(c.pending_count(), 0);
}

#[test]
fn ttl_approve_racing_expiry_yields_exactly_one_outcome() {
    // An approval and the clock crossing the TTL race. Whatever the interleaving, the request
    // is either signed once (approved in time) or expired (never signed), never both, and the
    // pending set is empty afterwards.
    let (v, _p) = vault_with_wallet();
    for round in 0..64 {
        let clock = FakeClock::new();
        let c = ceremony_on(&clock);
        let view = c.request(personal_sign_intent("race the clock"));
        clock.advance(PENDING_TTL_MS - 1);
        let (approved, swept) = std::thread::scope(|s| {
            let a = s.spawn(|| c.approve(&v, &view.id, false));
            let b = s.spawn(|| {
                clock.advance(1);
                c.status(&view.id)
            });
            (a.join().expect("join"), b.join().expect("join"))
        });
        match &approved {
            Ok(_) => assert!(!c.is_expired(&view.id), "round {round}: signed and expired"),
            Err(CeremonyError::Expired) => {
                assert!(c.is_expired(&view.id), "round {round}: refused but not expired")
            }
            Err(e) => panic!("round {round}: unexpected {e:?}"),
        }
        assert!(swept.is_none() || approved.is_err(), "round {round}");
        assert_eq!(c.pending_count(), 0, "round {round}");
        assert_eq!(
            c.approve(&v, &view.id, false).is_ok(),
            false,
            "round {round}: a second approval never signs"
        );
    }
}

#[test]
fn ttl_an_id_from_an_earlier_run_never_approves_a_later_runs_request() {
    // Pending requests live in memory only, so a restart drops them. An id the previous run
    // handed out must not name a request in the next run either.
    let (v, _p) = vault_with_wallet();
    let before = SignatureCeremony::new();
    let old = before.request(personal_sign_intent("before the restart"));
    drop(before);
    let after = SignatureCeremony::new();
    let _new = after.request(personal_sign_intent("after the restart"));
    assert_eq!(
        after.approve(&v, &old.id, false),
        Err(CeremonyError::UnknownCeremony),
        "an id from before the restart signs nothing after it"
    );
}

#[test]
fn ttl_pending_requests_are_never_written_to_disk() {
    // Structural: the pending set is in-memory state. If it is ever persisted, persisted entries
    // must carry their creation time and be checked against the TTL on load; this test then has
    // to change in the same reviewed PR.
    let src = strip_test_module(include_str!("ceremony.rs"));
    for needle in ["std::fs", "fs::write", "File::create", "write_secret_file"] {
        assert!(!src.contains(needle), "ceremony.rs persists something: `{needle}`");
    }
    let pending_def = src.find("struct Pending {").expect("Pending is defined");
    let derive_line = src[..pending_def].lines().rev().find(|l| l.contains("#[derive"));
    assert!(
        derive_line.is_some_and(|l| !l.contains("Serialize") && !l.contains("Deserialize")),
        "Pending must not be serializable: {derive_line:?}"
    );
}

// =========================================================================
// Chain binding
// =========================================================================

#[test]
fn chain_a_card_showing_another_chain_is_not_signed_as_a_40204_transaction() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(a_transfer(1, None));
    assert_eq!(view.chain_id, 1, "the card shows the requested chain");
    let rpc = RpcClient::with_transport(MockRpc::new(transfer_script()));
    assert_eq!(
        c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2)),
        Err(CeremonyError::ChainMismatch {
            shown: 1,
            signed: "40204".into()
        })
    );
    assert!(!methods_called(&rpc).contains(&"eth_sendRawTransaction".to_string()));
}

#[test]
fn chain_transaction_bytes_naming_another_chain_are_refused() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    for bad in [
        JsonValue::String("0x1".into()),
        JsonValue::from(1u64),
        JsonValue::String("1".into()),
        JsonValue::String("not a number".into()),
        JsonValue::Null,
        serde_json::json!({ "id": 40204 }),
    ] {
        let view = c.request(a_transfer(40204, Some(("chainId", bad.clone()))));
        let rpc = RpcClient::with_transport(MockRpc::new(transfer_script()));
        let r = c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2));
        assert!(
            matches!(r, Err(CeremonyError::ChainMismatch { shown: 40204, .. })),
            "chainId {bad} must be refused, got {r:?}"
        );
        assert!(
            !methods_called(&rpc).contains(&"eth_sendRawTransaction".to_string()),
            "chainId {bad}: nothing sent"
        );
    }
}

#[test]
fn chain_transaction_bytes_naming_the_shown_chain_sign_in_every_encoding() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    for good in [
        JsonValue::String("0x9d0c".into()),
        JsonValue::String("0x9D0C".into()),
        JsonValue::from(40204u64),
        JsonValue::String("40204".into()),
    ] {
        let view = c.request(a_transfer(40204, Some(("chainId", good.clone()))));
        let rpc = RpcClient::with_transport(MockRpc::new(transfer_script()));
        assert!(
            c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2)).is_ok(),
            "chainId {good} matches the card and signs"
        );
    }
}

#[test]
fn chain_the_signed_transaction_commits_to_the_chain_the_card_showed() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(a_transfer(40204, Some(("chainId", JsonValue::String("0x9d0c".into())))));
    let rpc = RpcClient::with_transport(MockRpc::new(transfer_script()));
    c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2))
        .expect("signed and sent");
    let raw_hex = rpc
        .transport()
        .requests
        .borrow()
        .iter()
        .find(|r| r["method"] == "eth_sendRawTransaction")
        .and_then(|r| r["params"][0].as_str().map(str::to_string))
        .expect("a raw tx was sent");
    let raw = hex::decode(raw_hex.trim_start_matches("0x")).expect("hex");
    let tx = rlp::Rlp::new(&raw);
    let v_field: u64 = tx.val_at(6).expect("v");
    assert_eq!(
        (v_field - 35) / 2,
        view.chain_id,
        "EIP-155 v encodes the chain the card showed"
    );
}

#[test]
fn chain_a_sign_in_message_for_another_chain_is_refused() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(personal_sign_intent(&sign_in_text("Chain ID: 1")));
    assert_eq!(view.chain_id, 40204);
    assert_eq!(
        c.approve(&v, &view.id, false),
        Err(CeremonyError::ChainMismatch {
            shown: 40204,
            signed: "1".into()
        })
    );
}

#[test]
fn chain_a_message_with_conflicting_or_unreadable_chain_lines_is_refused() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    for lines in [
        "Chain ID: 40204\nchain id: 1",
        "Chain ID: forty",
        "  CHAIN ID:   0x9d0c",
        "Chain ID: 1\nChain ID: 1",
    ] {
        let view = c.request(personal_sign_intent(&sign_in_text(lines)));
        assert!(
            matches!(c.approve(&v, &view.id, false), Err(CeremonyError::ChainMismatch { .. })),
            "{lines:?} must be refused"
        );
    }
}

#[test]
fn chain_a_message_naming_the_shown_chain_or_no_chain_signs() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    for text in [
        sign_in_text("Chain ID: 40204"),
        sign_in_text("  chain id:40204  "),
        "hello, no chain here".to_string(),
    ] {
        let view = c.request(personal_sign_intent(&text));
        assert!(c.approve(&v, &view.id, false).is_ok(), "{text:?} signs");
    }
}

#[test]
fn chain_a_fallen_through_sign_in_for_another_chain_is_refused_on_its_card() {
    // A sign-in that is not budgetable becomes an ordinary card showing 40204. If its message
    // names another chain, approving that card must refuse.
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let req = SiweSignRequest {
        message: sign_in_text("Chain ID: 1"),
        attestation: None,
        taint: crate::web_budget::TaskTaint::Clean,
        hic_required: false,
        principal: "hermes".into(),
        claimed_origin: "https://app.citrate.ai".into(),
    };
    let view = c.siwe_fall_through(&req, crate::web_budget::FallThrough::NoBudget);
    let BudgetedOutcome::Pending { ceremony, .. } = view else {
        panic!("a fall-through is a pending card");
    };
    assert!(matches!(
        c.approve(&v, &ceremony.id, false),
        Err(CeremonyError::ChainMismatch { shown: 40204, .. })
    ));
}

#[test]
fn chain_mismatch_and_expiry_errors_say_nothing_was_signed() {
    let m = CeremonyError::ChainMismatch {
        shown: 40204,
        signed: "1".into(),
    }
    .to_string();
    assert!(m.contains("40204") && m.contains("chain 1") && m.contains("nothing was signed"), "{m}");
    let e = CeremonyError::Expired.to_string();
    assert!(e.contains("expired") && e.contains("nothing was signed"), "{e}");
}

// =========================================================================
// Tripwire: no wallet signer call without the expiry and chain checks.
// =========================================================================

/// Every call of a wallet signer in ceremony.rs must sit in a function that took its request
/// through `take_live` (expiry checked under the lock, consumed) and that calls `ready_to_sign`
/// (expiry re-checked, chain bound) before the signer. The one exemption is the budgeted
/// sign-in, which has no pending request or card: its message carries its own expiry and its
/// chain id is checked against `siwe::CHAIN_ALLOWLIST` by `siwe::check_budgetable`.
#[test]
fn tripwire_every_signer_call_is_preceded_by_the_expiry_and_chain_checks() {
    let src = strip_test_module(include_str!("ceremony.rs"));
    let signers = [
        ["wallet::", "sign_", "personal("].concat(),
        ["wallet::", "sign_", "transaction("].concat(),
        ["wallet::", "sign_", "message("].concat(),
    ];
    const EXEMPT: &[&str] = &["request_siwe_budgeted"];
    let ready = ["self.", "ready_to_sign("].concat();
    let take = ["self.", "take_live("].concat();
    let mut checked = Vec::new();
    for (at, _) in src.match_indices("wallet::") {
        let rest = &src[at..];
        if !signers.iter().any(|s| rest.starts_with(s.as_str())) {
            continue;
        }
        let line_start = src[..at].rfind('\n').map_or(0, |i| i + 1);
        if src[line_start..at].trim_start().starts_with("//") {
            continue;
        }
        let fn_at = src[..at].rfind("fn ").expect("a signer call sits in a fn");
        let name: String = src[fn_at + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if EXEMPT.contains(&name.as_str()) {
            checked.push(name);
            continue;
        }
        let body = &src[fn_at..at];
        let ready_at = body.rfind(ready.as_str());
        assert!(
            ready_at.is_some(),
            "`{name}` calls a wallet signer without `ready_to_sign` before it"
        );
        let between = &body[ready_at.unwrap_or(0)..];
        assert!(
            !between.contains("rpc.") && !between.contains(".lock()"),
            "`{name}`: nothing may run between `ready_to_sign` and the signer"
        );
        assert!(body.contains(take.as_str()), "`{name}` does not take its request via `take_live`");
        checked.push(name);
    }
    checked.sort();
    assert_eq!(
        checked,
        ["approve", "approve_and_broadcast", "request_siwe_budgeted"],
        "the set of signing functions changed; a new one must pass the checks above"
    );
    // The checks themselves: `ready_to_sign` re-reads the clock and binds the chain.
    let rts = src.find("fn ready_to_sign(").expect("ready_to_sign exists");
    let rts_body = &src[rts..rts + src[rts..].find("\n    }\n").expect("end of fn")];
    assert!(rts_body.contains("self.expired(") && rts_body.contains("committed_chain("));
    // The pending map is only drained through the TTL-aware paths.
    let removes = src.matches("pending.remove(").count();
    assert_eq!(removes, 2, "the pending map is drained only by take_live and the sweep");
}
