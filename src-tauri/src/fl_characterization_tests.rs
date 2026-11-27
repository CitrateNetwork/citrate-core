// L8-WP01 L8-RUST (characterization, citrate-core a826e441). Included into `fl_payouts::tests`
// from fl_payouts_tests.rs; registry ci/characterization.d/l8.json; human view
// docs/fl/CHARACTERIZATION.md.
//
// Pins, before any L8 change:
//  - the training reads (training.rs:1-7, commands :182 and :211) go to PatronageLedger only, with
//    the pinned selectors, and no training write command exists (crediting waits on @rule8 and
//    gateSec);
//  - the kit payouts gate, under the shipped r1007 profile, refuses every WorkMarket write while
//    payouts are dark and lets FederatedRoundLedger commitRound and finalize through without a
//    code read (this calls kit's check_intent; it edits nothing in kit);
//  - the eight collector safety tests in fl_collect_tests.rs stay declared by name.
// A failure here means one of those changed. Change an expectation only in the L8 WP the
// registry names for this suite, with a stricter test, in a commit that says why.

mod lq_l8_fl {
    use crate::ceremony::{IntentKind, SignatureIntent};
    use crate::payouts_gate::{
        check_call, check_intent, selector_of, FlContract, FlPin, MoneyClass, PayoutsPolicy,
        PayoutsRefusal, MONEY_SELECTORS,
    };
    use crate::rpc::{RpcClient, RpcError, RpcTransport};
    use serde_json::Value;
    use std::cell::{Cell, RefCell};

    /// The book's PatronageLedger and FederatedRoundLedger on r1007 (block 0).
    const PATRONAGE: &str = "0x41f68b9e710585b064459a0ec706148fbf503659";
    const FRL: &str = "0x25fc02fbfad9589d0dde84f858d0962bfa0253c5";
    /// An address r1007 pins for no FL contract.
    const ELSEWHERE: &str = "0x5555555555555555555555555555555555555555";
    /// A WorkMarket address, for the test-only policy that pins one.
    const WM_ADDR: [u8; 20] = [0x11; 20];
    const WM_HEX: &str = "0x1111111111111111111111111111111111111111";
    const MEMBER: &str = "0x00000000000000000000000000000000000000ff";

    const COMMIT_ROUND: &str = concat!(
        "commitRound((bytes32,uint64,bytes32,bytes32,uint16,",
        "uint64,bytes32,bytes32,uint32,bytes32))"
    );
    const FINALIZE: &str = "finalize(bytes32)";

    /// The WorkMarket selectors other contracts share, so the gate refuses them only at the pin.
    const GENERIC_WM: [&str; 3] = [
        "withdraw()",
        "resolveDispute(uint256,bool)",
        "refund(uint256)",
    ];

    const GATED: &str = "real member SALT crediting is @rule8-gated (gateSec not yet passed)";
    const NO_LEDGER: &str = "the settlement ledger address is not confirmed on 40204 yet";

    /// The collector safety tests LQ requires by name (lane L8 section 4, WP01).
    const COLLECTOR_SAFETY_TESTS: [&str; 8] = [
        "a_page_robots_disallows_is_refused",
        "no_robots_txt_and_no_license_is_not_permission",
        "denied_and_private_hosts_are_refused_before_any_fetch",
        "a_429_or_403_stops_the_run_and_nothing_more_is_fetched",
        "robots_is_checked_again_at_run_time",
        "an_approved_run_writes_provenance_through_the_store_and_records_the_scan",
        "bypass_closure_the_store_is_out_of_reach_of_the_agents_file_tools",
        "every_fetch_is_pinned_to_the_one_vetted_answer_and_a_rebinding_answer_is_never_used",
    ];

    fn r1007() -> &'static crate::network_profile::NetworkProfile {
        crate::network_profile::profile("r1007").expect("r1007")
    }

    fn pin_of(policy: &PayoutsPolicy, contract: FlContract) -> &FlPin {
        policy
            .pins
            .iter()
            .find(|p| p.contract == contract)
            .expect("the profile names every FL contract")
    }

    /// The shipped policy with a WorkMarket pin added (test-only: r1007 has none), still dark.
    fn with_workmarket_pinned(shipped: &PayoutsPolicy) -> PayoutsPolicy {
        let mut policy = shipped.clone();
        for pin in &mut policy.pins {
            if pin.contract == FlContract::WorkMarket {
                pin.address = Some(WM_ADDR);
                pin.code_hash = Some([0x22; 32]);
                pin.pending = None;
            }
        }
        policy
    }

    fn dark(class: MoneyClass) -> PayoutsRefusal {
        PayoutsRefusal::Dark {
            network: "r1007".to_string(),
            contract: FlContract::WorkMarket,
            class,
        }
    }

    /// A selector with one zero argument word.
    fn call(signature: &str) -> Vec<u8> {
        let mut data = selector_of(signature).to_vec();
        data.extend_from_slice(&[0u8; 32]);
        data
    }

    /// A transaction intent the ceremony would check, to `to` with `data`.
    fn tx(to: &str, data: &[u8]) -> SignatureIntent {
        let raw = serde_json::json!({
            "from": "0x9858effd232b4033e47d90003d41ec34ecaeda94",
            "to": to,
            "value": "0x0",
            "data": format!("0x{}", hex::encode(data)),
            "gas": "0x30000",
        })
        .to_string();
        SignatureIntent {
            origin: "lq-l8-characterization".to_string(),
            kind: IntentKind::Transaction,
            chain_id: 40204,
            raw,
        }
    }

    /// Scripted eth_call answers, recording every request body.
    struct Recorded {
        bodies: RefCell<Vec<Value>>,
        answers: RefCell<Vec<Value>>,
    }

    impl Recorded {
        fn new(answers: Vec<Value>) -> Recorded {
            Recorded {
                bodies: RefCell::new(Vec::new()),
                answers: RefCell::new(answers),
            }
        }

        /// (to, data) of every request, each checked to be a plain eth_call at "latest".
        fn calls(&self) -> Vec<(String, String)> {
            let mut out = Vec::new();
            for body in self.bodies.borrow().iter() {
                assert_eq!(body["method"], "eth_call");
                assert_eq!(body["params"][1], "latest");
                let call = &body["params"][0];
                let to = call["to"].as_str().unwrap_or_default().to_string();
                let data = call["data"].as_str().unwrap_or_default().to_string();
                out.push((to, data));
            }
            out
        }
    }

    impl RpcTransport for Recorded {
        fn call(&self, body: Value) -> Result<Value, RpcError> {
            self.bodies.borrow_mut().push(body);
            let next = self.answers.borrow_mut().remove(0);
            let reply = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": next });
            Ok(reply)
        }
    }

    fn word(n: u128) -> Value {
        serde_json::json!(format!("0x{n:064x}"))
    }

    #[test]
    fn today_training_status_reads_patronage_only() {
        // The ledger the training commands read is the book's PatronageLedger.
        assert_eq!(crate::addresses::patronage_ledger(), PATRONAGE);

        // The pinned selectors are the PatronageLedger functions they name.
        let pinned: [(&str, [u8; 4]); 3] = [
            ("roundMergedHash(bytes32)", [0xa4, 0x40, 0x2c, 0xa3]),
            ("units(address)", [0x00, 0xeb, 0xa3, 0x50]),
            ("pendingDividendOf(address)", [0x8c, 0x7b, 0xfd, 0x2c]),
        ];
        for (signature, selector) in pinned {
            assert_eq!(selector_of(signature), selector, "{signature}");
        }

        // training_status: roundMergedHash(bytes32) per round, stopping at the first unset one.
        let rpc = RpcClient::with_transport(Recorded::new(vec![word(9), word(0)]));
        let gid = [0x11u8; 32];
        assert_eq!(crate::training::scan_rounds(&rpc, PATRONAGE, &gid), Ok(1));
        let calls = rpc.transport().calls();
        assert_eq!(calls.len(), 2);
        for (to, data) in &calls {
            assert_eq!(to, PATRONAGE);
            assert!(data.starts_with("0xa4402ca3"), "{data}");
            assert_eq!(data.len(), 2 + 8 + 64, "{data}");
        }

        // training_reward: units(address), then pendingDividendOf(address), for the member.
        let rpc = RpcClient::with_transport(Recorded::new(vec![word(7), word(3)]));
        let read = crate::training::read_patronage(&rpc, PATRONAGE, MEMBER);
        assert_eq!(read, Ok((7, 3)));
        let member_word = format!("{:0>64}", &MEMBER[2..]);
        let units = format!("0x00eba350{member_word}");
        let pending = format!("0x8c7bfd2c{member_word}");
        let calls = rpc.transport().calls();
        let datas: Vec<String> = calls.iter().map(|(_, d)| d.clone()).collect();
        assert_eq!(datas, [units, pending]);
        assert!(calls.iter().all(|(to, _)| to == PATRONAGE));

        // No training write command: crediting waits on @rule8 and gateSec, and the V5 stubs
        // (training_start, training_contribute, training_claim) are gone.
        let src = include_str!("training.rs");
        assert!(src.contains(GATED));
        assert!(src.contains(NO_LEDGER));
        assert_eq!(src.matches("#[tauri::command]").count(), 2);
        assert!(src.contains("pub async fn training_status("));
        assert!(src.contains("pub async fn training_reward("));
        for gone in [
            "fn training_start(",
            "fn training_contribute(",
            "fn training_claim(",
        ] {
            assert!(!src.contains(gone), "{gone}");
        }
        for send in [
            "eth_sendRawTransaction",
            "eth_sendTransaction",
            "sign_request",
        ] {
            assert!(!src.contains(send), "{send}");
        }
    }

    #[test]
    fn payouts_gate_refuses_workmarket_writes_while_dark() {
        // The shipped r1007 profile: payouts dark, and WorkMarket not pinned.
        let shipped = r1007().payouts_policy();
        assert_eq!(shipped.network, "r1007");
        assert!(!shipped.payouts_open);
        let wm = pin_of(&shipped, FlContract::WorkMarket);
        assert_eq!((wm.address, wm.code_hash), (None, None));

        let wm_calls: Vec<_> = MONEY_SELECTORS
            .iter()
            .filter(|m| m.contract == FlContract::WorkMarket)
            .collect();
        assert_eq!(wm_calls.len(), 20);
        assert_eq!(MONEY_SELECTORS.len(), 26);
        let generic: Vec<&str> = wm_calls
            .iter()
            .filter(|m| !m.distinctive)
            .map(|m| m.signature)
            .collect();
        assert_eq!(generic, GENERIC_WM);

        let reads = Cell::new(0usize);
        let mut read_code = |_: &str| {
            reads.set(reads.get() + 1);
            Err::<Vec<u8>, String>("no code read is expected while dark".to_string())
        };

        // r1007 as shipped: a distinctive WorkMarket call to any address is not this network's
        // WorkMarket and is refused; a generic selector at an unpinned address is no WorkMarket
        // call at all (r1007 pins no WorkMarket), so the gate leaves it.
        for m in &wm_calls {
            let intent = tx(ELSEWHERE, &call(m.signature));
            let got = check_intent(&shipped, &intent, &mut read_code);
            if m.distinctive {
                let want = PayoutsRefusal::NotThisNetworksContract {
                    network: "r1007".to_string(),
                    contract: FlContract::WorkMarket,
                    class: m.class,
                    to: ELSEWHERE.to_string(),
                };
                assert_eq!(got, Err(want), "{}", m.signature);
            } else {
                assert_eq!(got, Ok(()), "{}", m.signature);
            }
        }

        // With a WorkMarket pinned and payouts still dark, every write to it is refused: each
        // money selector, an unknown selector and a plain transfer.
        let pinned = with_workmarket_pinned(&shipped);
        for m in &wm_calls {
            let intent = tx(WM_HEX, &call(m.signature));
            let got = check_intent(&pinned, &intent, &mut read_code);
            assert_eq!(got, Err(dark(m.class)), "{}", m.signature);
        }
        let unknown = tx(WM_HEX, &[0xde, 0xad, 0xbe, 0xef]);
        let got = check_intent(&pinned, &unknown, &mut read_code);
        assert_eq!(got, Err(dark(MoneyClass::OtherWorkMarketWrite)));
        let got = check_call(&pinned, Some(WM_ADDR), &[], &mut read_code);
        assert_eq!(got, Err(dark(MoneyClass::OtherWorkMarketWrite)));

        // The words the member sees, and no code was read while dark.
        let join = tx(WM_HEX, &call("join(uint256,uint128,bytes32[])"));
        let got = check_intent(&pinned, &join, &mut read_code);
        let words = got.expect_err("join refused").to_string();
        let head = "FL payouts are not open on network \"r1007\"";
        let tail = "so this WorkMarket join call is not signed";
        assert!(words.starts_with(head), "{words}");
        assert!(words.ends_with(tail), "{words}");
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn payouts_gate_passes_frl_commit_and_finalize() {
        // Neither is a money class: the ABI selectors, and no gated entry carries them.
        assert_eq!(selector_of(COMMIT_ROUND), [0x74, 0xfc, 0xa8, 0x6f]);
        assert_eq!(selector_of(FINALIZE), [0x92, 0x58, 0x4d, 0x80]);
        for m in MONEY_SELECTORS {
            assert!(m.signature != COMMIT_ROUND && m.signature != FINALIZE);
        }

        let shipped = r1007().payouts_policy();
        let frl = pin_of(&shipped, FlContract::FederatedRoundLedger);
        let frl_hex = frl.address.map(|a| format!("0x{}", hex::encode(a)));
        assert_eq!(frl_hex.as_deref(), Some(FRL));
        // The flag set on a test-only clone: still not gated, still no code read.
        let mut flagged = r1007().clone();
        flagged.payouts_open = true;
        let flagged = flagged.payouts_policy();
        assert!(flagged.payouts_open);

        let reads = Cell::new(0usize);
        let mut read_code = |_: &str| {
            reads.set(reads.get() + 1);
            Err::<Vec<u8>, String>("no code read is expected for FRL commits".to_string())
        };
        for policy in [&shipped, &flagged] {
            for signature in [COMMIT_ROUND, FINALIZE] {
                let intent = tx(FRL, &call(signature));
                let got = check_intent(policy, &intent, &mut read_code);
                assert_eq!(got, Ok(()), "{signature} on {}", policy.network);
            }
        }
        assert_eq!(reads.get(), 0);

        // The gate is live on FRL: a challenge is a money class there, refused while dark.
        let challenge = MONEY_SELECTORS
            .iter()
            .find(|m| m.contract == FlContract::FederatedRoundLedger)
            .expect("an FRL challenge entry");
        assert_eq!(challenge.class, MoneyClass::Challenge);
        let intent = tx(FRL, &call(challenge.signature));
        let got = check_intent(&shipped, &intent, &mut read_code);
        let want = PayoutsRefusal::Dark {
            network: "r1007".to_string(),
            contract: FlContract::FederatedRoundLedger,
            class: MoneyClass::Challenge,
        };
        assert_eq!(got, Err(want));
    }

    #[test]
    fn collector_safety_tests_stay_declared() {
        let src = include_str!("fl_collect_tests.rs");
        let lines: Vec<&str> = src.lines().map(str::trim).collect();
        for name in COLLECTOR_SAFETY_TESTS {
            let decl = format!("fn {name}() {{");
            let Some(at) = lines.iter().position(|l| *l == decl) else {
                panic!("fl_collect_tests.rs no longer declares {name}");
            };
            assert!(
                at > 0 && lines[at - 1] == "#[test]",
                "{name} is not under #[test]"
            );
        }
        // And the file is compiled in: fl_collect.rs pulls it into its test module.
        let host = include_str!("fl_collect.rs");
        assert!(host.contains("include!(\"fl_collect_tests.rs\");"));
    }
}
