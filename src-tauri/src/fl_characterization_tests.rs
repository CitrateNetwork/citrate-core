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
//  - the two command bodies (training_status_sync :191, training_reward_sync :222) build their
//    own RpcClient::citrate(), so no test transport reaches them; their source is pinned instead:
//    the ledger only from patronage_ledger()?, and no chain read beyond the helpers tested here;
//  - the eight collector safety tests in fl_collect_tests.rs stay declared by name, live: at
//    module scope, under #[test], never #[ignore]d, #[should_panic], cfg'd out or behind a
//    cfg_attr; and the file stays compiled in:
//    fl_collect.rs includes it in its column-0 #[cfg(test)] mod tests with no other attribute,
//    and lib.rs declares mod fl_collect with no cfg. (This suite cannot see its own host chain
//    cfg'd out, since it would not run; flRegistry.characterization.test.ts checks that one.)
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
    use sha3::{Digest, Keccak256};
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

    /// The source of the fn that starts with `head`, through its closing `}` at column 0.
    fn fn_body<'a>(src: &'a str, head: &str) -> &'a str {
        let start = src.find(head).expect("the fn is declared");
        let len = src[start..].find("\n}\n").expect("the fn closes");
        &src[start..start + len + 2]
    }

    /// The names a body calls (`name(` or `path::name(`), in order, whole-line comments skipped.
    fn calls_in(body: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in body.lines().map(str::trim) {
            if line.starts_with("//") {
                continue;
            }
            let mut token = String::new();
            for c in line.chars() {
                if c.is_ascii_alphanumeric() || c == '_' || c == ':' {
                    token.push(c);
                    continue;
                }
                let named = token.starts_with(|t: char| t.is_ascii_alphabetic());
                if c == '(' && named {
                    out.push(token.clone());
                }
                token.clear();
            }
        }
        out
    }

    /// How often `word` appears in `src` as a whole identifier.
    fn uses_of(src: &str, word: &str) -> usize {
        src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|w| *w == word)
            .count()
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
        // Round n's key is keccak256(abi.encode(bytes32 groupId, uint256 n)), n from 1, so the
        // two calls ask for rounds 1 and 2 in that order.
        let round_call = |n: u8| {
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(&gid);
            buf[63] = n;
            let key: String = Keccak256::digest(buf)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            format!("0xa4402ca3{key}")
        };
        let datas: Vec<String> = calls.iter().map(|(_, d)| d.clone()).collect();
        assert_eq!(datas, [round_call(1), round_call(2)]);
        assert!(calls.iter().all(|(to, _)| to == PATRONAGE));

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
        // GATED is the module doc's statement (training.rs:5): no code returns it, since no write
        // command exists to refuse. NO_LEDGER is patronage_ledger()'s refusal, pinned below where it
        // is returned; the shipped book never reaches it. A WP that adds a write path asserts its
        // refusal on the value the code returns.
        let src = include_str!("training.rs");
        let mut module_doc = src.lines().take_while(|l| l.starts_with("//!"));
        assert!(module_doc.any(|l| l.contains(GATED)));
        assert_eq!(src.matches(NO_LEDGER).count(), 1);
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

        // The command bodies build RpcClient::citrate() themselves, so no test transport reaches
        // them; their source is pinned. The ledger comes only from patronage_ledger()?, which is
        // the one crate::addresses read; the only selectors are the three pinned above; and the
        // only chain reads are scan_rounds and read_patronage, tested above, plus the round lookup.
        let (prod, _) = src
            .split_once("#[cfg(test)]\nmod tests")
            .expect("training.rs has its test module");
        assert_eq!(prod.matches("RpcClient::citrate()").count(), 2);
        assert_eq!(prod.matches("addresses").count(), 1);
        let ledger_fn = fn_body(prod, "fn patronage_ledger() ");
        let book_read = "let a = crate::addresses::patronage_ledger();";
        assert!(ledger_fn.contains(book_read));
        let refusal = format!("return Err(\"{NO_LEDGER}\".to_string());");
        assert!(ledger_fn.contains(&refusal));
        assert_eq!(prod.matches(": [u8; 4] = [").count(), 3);
        let uses: Vec<&str> = prod.lines().filter(|l| l.starts_with("use ")).collect();
        let want_uses = [
            "use serde::Serialize;",
            "use sha3::{Digest, Keccak256};",
            "use crate::agent_sbt::decode_u128;",
            "use crate::rpc::RpcClient;",
        ];
        assert_eq!(uses, want_uses);

        let status = fn_body(prod, "pub fn training_status_sync(");
        let want_status = [
            "training_status_sync",
            "patronage_ledger",
            "RpcClient::citrate",
            "group_bytes32",
            "scan_rounds",
            "Ok",
            "to_string",
        ];
        assert_eq!(calls_in(status), want_status);
        let take_ledger = "let ledger = patronage_ledger()?;";
        assert!(status.contains(take_ledger));
        let scan = "let current = scan_rounds(&rpc, ledger, &gid)?;";
        assert!(status.contains(scan));

        let reward = fn_body(prod, "pub fn training_reward_sync(");
        let want_reward = [
            "training_reward_sync",
            "patronage_ledger",
            "RpcClient::citrate",
            "crate::wallet::address",
            "map_err",
            "to_string",
            "read_patronage",
            "training_status_sync",
            "Ok",
            "to_string",
            "to_string",
            "to_string",
        ];
        assert_eq!(calls_in(reward), want_reward);
        assert!(reward.contains(take_ledger));
        let read = "let (weight, pending_wei) = read_patronage(&rpc, ledger, &member)?;";
        assert!(reward.contains(read));
        let lookup = "let round = training_status_sync(app, group)?.round;";
        assert!(reward.contains(lookup));

        // Each body names its client and its ledger once to build them and once to read.
        for body in [status, reward] {
            assert_eq!(uses_of(body, "rpc"), 2, "{body}");
            assert_eq!(uses_of(body, "ledger"), 2, "{body}");
            assert_eq!(uses_of(body, "RpcClient"), 1, "{body}");
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

    /// Source with each line trimmed and whole-line `//` comments (doc comments too) blanked.
    fn code_lines(src: &str) -> String {
        let lines: Vec<&str> = src
            .lines()
            .map(str::trim)
            .map(|l| if l.starts_with("//") { "" } else { l })
            .collect();
        lines.join("\n")
    }

    /// True where the item before an attribute block ends (or the source starts).
    fn item_ends(code: &str) -> bool {
        code.is_empty() || code.ends_with(['}', ';', '{'])
    }

    /// The attributes directly above byte `at` of `code`, nearest first, whitespace removed. Any
    /// other text between them and the item before (a trailing comment, a stray token) is an
    /// error, so the check fails closed.
    fn attributes_above(code: &str, at: usize) -> Result<Vec<String>, String> {
        let mut rest = code[..at].trim_end();
        let mut attrs: Vec<String> = Vec::new();
        while !item_ends(rest) {
            let line = rest.rsplit('\n').next().unwrap_or(rest);
            let Some(body) = rest.strip_suffix(']') else {
                return Err(format!("unexpected text above the fn: {line:?}"));
            };
            let mut depth = 1usize;
            let mut open = None;
            for (i, b) in body.bytes().enumerate().rev() {
                match b {
                    b']' => depth += 1,
                    b'[' if depth == 1 => {
                        open = Some(i);
                        break;
                    }
                    b'[' => depth -= 1,
                    _ => {}
                }
            }
            let Some(open) = open else {
                return Err(format!("an unbalanced attribute: {line:?}"));
            };
            let Some(before) = body[..open].trim_end().strip_suffix('#') else {
                return Err(format!("a bracket that is no attribute: {line:?}"));
            };
            attrs.push(body[open + 1..].split_whitespace().collect());
            rest = before.trim_end();
        }
        Ok(attrs)
    }

    /// Why a code line of `src` could turn a test off wherever it sits, or None: an #[ignore], a
    /// #[should_panic] (it inverts the test), a cfg_attr, a cfg other than cfg(test) or an inner
    /// `#![...]` anywhere on it (a cfg on an enclosing module turns off every test in it), an
    /// allow(unnameable_test_items) (it silences the lint on a test rustc never collects), or a
    /// `mod` item at all.
    fn disabling(line: &str) -> Option<&'static str> {
        if line.contains("unnameable_test_items") {
            return Some("an allow for a test nested where rustc never collects it");
        }
        let squashed: String = line.split_whitespace().collect();
        for (i, _) in squashed.match_indices('#') {
            let at = &squashed[i..];
            if at == "#" || at == "#!" || at == "#[" {
                return Some("an attribute split across lines");
            }
            if at.starts_with("#![") {
                return Some("an inner attribute");
            }
            if at.starts_with("#[ignore") {
                return Some("an #[ignore]");
            }
            if at.starts_with("#[should_panic") {
                return Some("a #[should_panic]");
            }
            if at.starts_with("#[cfg") && !at.starts_with("#[cfg(test)]") {
                return Some("a cfg other than cfg(test)");
            }
        }
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        if line.split(|c: char| !is_word(c)).any(|w| w == "mod") {
            return Some("a mod item");
        }
        None
    }

    /// The end of the literal that starts at `chars[i]`, or None when none starts there: a
    /// string (`"..."`, `b"..."`), a raw string (`r"..."`, `r#"..."#`, `br#"..."#`) or a char
    /// (`'x'`, `'\n'`). A lifetime (`'a`) is no literal. Err when a literal never closes.
    fn literal_end(chars: &[char], i: usize) -> Result<Option<usize>, String> {
        let unclosed = || Err("a literal that never closes".to_string());
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        let fresh = i == 0 || !ident(chars[i - 1]);
        let raw_at = match chars[i] {
            'r' if fresh => Some(i + 1),
            'b' if fresh && chars.get(i + 1) == Some(&'r') => Some(i + 2),
            _ => None,
        };
        if let Some(mut j) = raw_at {
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                let mut k = j + 1;
                while k < chars.len() {
                    let tail = &chars[k + 1..];
                    let closes = tail.len() >= hashes && tail[..hashes].iter().all(|c| *c == '#');
                    if chars[k] == '"' && closes {
                        return Ok(Some(k + 1 + hashes));
                    }
                    k += 1;
                }
                return unclosed();
            }
            return Ok(None);
        }
        match chars[i] {
            '"' => {
                let mut k = i + 1;
                while k < chars.len() {
                    match chars[k] {
                        '\\' => k += 2,
                        '"' => return Ok(Some(k + 1)),
                        _ => k += 1,
                    }
                }
                unclosed()
            }
            '\'' if chars.get(i + 1) == Some(&'\\') => {
                // The escaped char is at i + 2, so the closing quote is the first one after it.
                let rest = chars.get(i + 3..).unwrap_or_default();
                match rest.iter().position(|c| *c == '\'') {
                    Some(n) => Ok(Some(i + 4 + n)),
                    None => unclosed(),
                }
            }
            '\'' if chars.get(i + 2) == Some(&'\'') => Ok(Some(i + 3)),
            _ => Ok(None),
        }
    }

    /// The brace depth at byte `at` of `code`, braces inside a string, a char, a raw string or a
    /// `//` comment aside. Err (fail closed) on a block comment, a `}` with none open, braces that
    /// do not balance by the end, or `at` inside a literal.
    fn depth_at(code: &str, at: usize) -> Result<usize, String> {
        let chars: Vec<char> = code.chars().collect();
        let offsets: Vec<usize> = code.char_indices().map(|(b, _)| b).collect();
        let (mut depth, mut i, mut found) = (0usize, 0usize, None);
        while i < chars.len() {
            if offsets[i] == at {
                found = Some(depth);
            }
            if let Some(end) = literal_end(&chars, i)? {
                let end_byte = offsets.get(end).copied().unwrap_or(code.len());
                if offsets[i] < at && end_byte > at {
                    return Err("the declaration sits inside a literal".to_string());
                }
                i = end;
                continue;
            }
            match (chars[i], chars.get(i + 1)) {
                ('/', Some('*')) => return Err("a block comment can hide a declaration".to_string()),
                ('/', Some('/')) => {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                ('{', _) => depth += 1,
                ('}', _) => {
                    depth = depth.checked_sub(1).ok_or("a } with no { open")?;
                }
                _ => {}
            }
            i += 1;
        }
        if depth != 0 {
            return Err("braces that do not balance".to_string());
        }
        found.ok_or_else(|| "no item starts there".to_string())
    }

    /// Ok when `src` declares `fn <name>() {` as a test that runs: at module scope (brace depth 0:
    /// rustc never collects a #[test] in a fn body, a closure, an impl or a const block), under
    /// #[test], with no #[ignore], #[should_panic], cfg_attr or cfg other than cfg(test) anywhere
    /// in its attribute block. Any code line `disabling` names fails it too (a backstop the block
    /// parse cannot see past), and so does any block comment.
    fn live_test(src: &str, name: &str) -> Result<(), String> {
        let code = code_lines(src);
        for line in code.lines() {
            if let Some(why) = disabling(line) {
                return Err(format!("a test could be turned off by {why}: {line}"));
            }
        }
        let decl = format!("fn {name}() {{");
        let mut at = None;
        let mut offset = 0;
        for line in code.split('\n') {
            if line == decl {
                at = Some(offset);
                break;
            }
            offset += line.len() + 1;
        }
        let Some(at) = at else {
            return Err(format!("{name} is not declared"));
        };
        let depth = depth_at(&code, at).map_err(|why| format!("{name}: {why}"))?;
        if depth != 0 {
            return Err(format!("{name} is nested {depth} deep, where rustc never collects it"));
        }
        let attrs = attributes_above(&code, at)?;
        if !attrs.iter().any(|a| a == "test") {
            return Err(format!("{name} is not under #[test]"));
        }
        for a in &attrs {
            let off = ["ignore", "should_panic", "cfg_attr"]
                .iter()
                .any(|p| a.starts_with(p));
            if off || (a.starts_with("cfg") && a != "cfg(test)") {
                return Err(format!("{name} is turned off by #[{a}]"));
            }
        }
        Ok(())
    }

    /// A whole-line `//` comment (a doc comment too) blanked, and a trailing `//` comment after
    /// a closing `]` dropped (so `#[cfg(any())] // note` still reads as an attribute), indentation
    /// kept otherwise.
    fn blank_comment(line: &str) -> &str {
        if line.trim_start().starts_with("//") {
            return "";
        }
        for (i, _) in line.match_indices("//") {
            let head = line[..i].trim_end();
            if head.ends_with(']') {
                return head;
            }
        }
        line
    }

    /// True when `line` (its trailing comment dropped by `blank_comment`) ends an attribute.
    fn is_attribute(line: &str) -> bool {
        line.trim_end().ends_with(']')
    }

    /// A block comment's opener, built so that this file carries none for the registry check's
    /// comment stripper to misread.
    const BLOCK_OPEN: &str = concat!("/", "*");

    /// The lines of `src` with whole-line comments blanked. A block comment fails the host
    /// checks closed, since it can hide a declaration.
    fn host_lines(src: &str) -> Result<Vec<&str>, String> {
        if src.contains(BLOCK_OPEN) {
            return Err("a block comment: the host checks read line comments only".to_string());
        }
        Ok(src.lines().map(blank_comment).collect())
    }

    /// The nearest non-blank line above line `i`, or "" at the top.
    fn line_above<'a>(lines: &[&'a str], i: usize) -> &'a str {
        let mut above = lines[..i].iter().rev().copied();
        above.find(|l| !l.trim().is_empty()).unwrap_or("")
    }

    /// The indexes of the lines that contain `needle`.
    fn lines_with(lines: &[&str], needle: &str) -> Vec<usize> {
        let mut at = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if line.contains(needle) {
                at.push(i);
            }
        }
        at
    }

    fn is_include(line: &str) -> bool {
        line.starts_with("    include!(\"") && line.ends_with("\");")
    }

    /// Ok when `host` compiles `file` into its tests: `include!("<file>");` once, inside the
    /// column-0 block `#[cfg(test)]`, `mod tests {`, include lines only, `}`, with no attribute
    /// stacked on that block and no inner `#![cfg...]` anywhere in the host.
    fn compiled_in(host: &str, file: &str) -> Result<(), String> {
        let lines = host_lines(host)?;
        for line in &lines {
            let squashed: String = line.split_whitespace().collect();
            if squashed.starts_with("#![cfg") {
                return Err(format!("an inner attribute in the host: {line}"));
            }
        }
        let named = format!("include!(\"{file}\")");
        let at = lines_with(&lines, &named);
        let [i] = at.as_slice() else {
            return Err(format!("{file} is included {} times", at.len()));
        };
        let i = *i;
        if lines[i] != format!("    {named};").as_str() {
            return Err(format!("not a plain include line: {:?}", lines[i]));
        }
        let mut top = i;
        while top > 0 && is_include(lines[top - 1]) {
            top -= 1;
        }
        let mut end = i + 1;
        while end < lines.len() && is_include(lines[end]) {
            end += 1;
        }
        if lines.get(end).copied() != Some("}") {
            return Err("the test module holds more than its includes".to_string());
        }
        if top < 2 || lines[top - 1] != "mod tests {" || lines[top - 2] != "#[cfg(test)]" {
            return Err(format!("{file} is not in a column-0 #[cfg(test)] mod tests"));
        }
        if is_attribute(line_above(&lines, top - 2)) {
            return Err("an attribute stacked on the test module".to_string());
        }
        Ok(())
    }

    /// Ok when `lib` declares `mod <module>;` once, at column 0, with no attribute on it, and
    /// carries no crate-level `#![cfg(...)]`.
    fn declared_in_lib(lib: &str, module: &str) -> Result<(), String> {
        let lines = host_lines(lib)?;
        for line in &lines {
            let squashed: String = line.split_whitespace().collect();
            if squashed.starts_with("#![cfg(") {
                return Err(format!("a crate-level cfg: {line}"));
            }
        }
        let decl = format!("mod {module};");
        let inline = format!("mod {module} {{");
        let mut at = lines_with(&lines, &decl);
        at.extend(lines_with(&lines, &inline));
        let [i] = at.as_slice() else {
            return Err(format!("mod {module} is declared {} times", at.len()));
        };
        let i = *i;
        if lines[i] != decl.as_str() {
            return Err(format!("not a plain column-0 declaration: {:?}", lines[i]));
        }
        if is_attribute(line_above(&lines, i)) {
            return Err(format!("an attribute on mod {module}"));
        }
        Ok(())
    }

    #[test]
    fn collector_safety_tests_stay_declared() {
        let src = include_str!("fl_collect_tests.rs");
        for name in COLLECTOR_SAFETY_TESTS {
            if let Err(why) = live_test(src, name) {
                panic!("fl_collect_tests.rs: {why}");
            }
        }
        // And the file is compiled in: fl_collect.rs pulls it into a test module nothing cfgs
        // out, and lib.rs declares fl_collect with no cfg.
        if let Err(why) = compiled_in(include_str!("fl_collect.rs"), "fl_collect_tests.rs") {
            panic!("fl_collect.rs: {why}");
        }
        if let Err(why) = declared_in_lib(include_str!("lib.rs"), "fl_collect") {
            panic!("lib.rs: {why}");
        }
    }

    #[test]
    fn collector_check_rejects_an_ignored_or_cfgd_out_test() {
        // A safety test that no longer runs is a lost test, wherever the attribute sits.
        let lost = [
            "fn t() {\n}\n",
            "#[ignore]\n#[test]\nfn t() {\n}\n",
            "#[test]\n#[ignore = \"flaky\"]\nfn t() {\n}\n",
            "#[test] #[ignore]\nfn t() {\n}\n",
            "# [ignore]\n#[test]\nfn t() {\n}\n",
            "#[ignore] // for now\n#[test]\nfn t() {\n}\n",
            "#[ignore = \"see [1]\"]\n#[test]\nfn t() {\n}\n",
            "#[cfg(any())]\n#[test]\nfn t() {\n}\n",
            "#[cfg(not(test))]\n#[test]\nfn t() {\n}\n",
            "#[cfg_attr(unix, ignore)]\n#[test]\nfn t() {\n}\n",
            "#[cfg_attr(\n    unix,\n    ignore\n)]\n#[test]\nfn t() {\n}\n",
        ];
        for case in lost {
            assert!(live_test(case, "t").is_err(), "{case}");
        }
        let live = [
            "#[test]\nfn t() {\n}\n",
            "/// Doc.\n#[test]\n#[allow(clippy::bool_assert_comparison)]\nfn t() {\n}\n",
            "#[cfg(test)]\n#[test]\nfn t() {\n}\n",
            "fn x() {\n}\n\n#[test]\nfn t() {\n}\n",
        ];
        for case in live {
            assert_eq!(live_test(case, "t"), Ok(()), "{case}");
        }
    }

    #[test]
    fn collector_check_rejects_a_test_in_a_cfgd_out_module() {
        // A cfg on an enclosing module turns off every test in it while each fn keeps its own
        // #[test] block, so the collector file carries no cfg but cfg(test), no inner attribute
        // and no mod, wherever it sits.
        let inner = "    #[test]\n    fn t() {\n    }\n";
        let lost = [
            format!("#[cfg(any())]\nmod off {{\n{inner}}}\n"),
            format!("#[cfg(not(test))]\npub(crate) mod off {{\n{inner}}}\n"),
            format!("fn x() {{}} #[cfg(any())] mod off {{\n{inner}}}\n"),
            format!("#[cfg(test)] #[cfg(any())] mod off {{\n{inner}}}\n"),
            format!("#[cfg(\n    any()\n)]\nmod off {{\n{inner}}}\n"),
            format!("mod off {{\n    #![cfg(any())]\n{inner}}}\n"),
            format!("#![cfg(any())]\n{inner}"),
            format!("mod plain {{\n{inner}}}\n"),
        ];
        for case in &lost {
            assert!(live_test(case, "t").is_err(), "{case}");
        }
        let live = "#[derive(Default)]\nstruct S;\n\n#[cfg(test)]\n#[test]\nfn t() {\n}\n";
        assert_eq!(live_test(live, "t"), Ok(()));
        // Line by line: an attribute split across lines fails closed; cfg(test) and a derive pass.
        for line in ["#", "#!", "#[", "x(); #[cfg(any())]", "# [ cfg ( any ( ) ) ]"] {
            assert!(disabling(line).is_some(), "{line}");
        }
        for line in ["#[cfg(test)]", "#[derive(Default)]", "#[tokio::test]", "fn model() {"] {
            assert_eq!(disabling(line), None, "{line}");
        }
    }

    #[test]
    fn host_check_rejects_a_cfgd_out_test_module() {
        // A cfg'd-out host test module, or lib.rs line, drops every test the file declares while
        // each name stays in place.
        let inc = "    include!(\"g_tests.rs\");";
        let good = format!("fn x() {{\n}}\n\n#[cfg(test)]\nmod tests {{\n{inc}\n}}\n");
        let live = [
            good.clone(),
            good.replace(
                "mod tests {\n",
                "mod tests {\n    include!(\"other_tests.rs\");\n",
            ),
            good.replace("#[cfg(test)]", "// The tests.\n#[cfg(test)]"),
        ];
        for case in &live {
            assert_eq!(compiled_in(case, "g_tests.rs"), Ok(()), "{case}");
        }
        let lost = [
            good.replace("#[cfg(test)]", "#[cfg(any())]"),
            good.replace("#[cfg(test)]", "#[cfg(not(test))]"),
            good.replace("#[cfg(test)]", "#[cfg(any())]\n#[cfg(test)]"),
            good.replace("#[cfg(test)]", "#[cfg(test)]\n#[cfg(any())]"),
            good.replace("#[cfg(test)]", "#[cfg(any())] // for now\n#[cfg(test)]"),
            good.replace(inc, "    // include!(\"g_tests.rs\");"),
            good.replace(inc, &format!("    #[cfg(any())]\n{inc}")),
            good.replace("mod tests {\n", "mod tests {\n    #![cfg(any())]\n"),
            format!("#![cfg(any())]\n{good}"),
            format!("{BLOCK_OPEN}\n{good}*/\n"),
            good.replace(inc, ""),
            format!("{good}\n#[cfg(test)]\nmod more {{\n{inc}\n}}\n"),
        ];
        for case in &lost {
            assert!(compiled_in(case, "g_tests.rs").is_err(), "{case}");
        }
        let lib = "mod a;\n// The guard.\nmod guard;\nmod b;\n";
        assert_eq!(declared_in_lib(lib, "guard"), Ok(()));
        let lost_lib = [
            lib.replace("mod guard;", "#[cfg(any())]\nmod guard;"),
            lib.replace("mod guard;", "#[cfg(test)] mod guard;"),
            lib.replace("mod guard;", "// mod guard;"),
            lib.replace("mod guard;", &format!("{BLOCK_OPEN}\nmod guard;\n*/")),
            lib.replace("mod guard;", "mod off {\n    mod guard;\n}"),
            lib.replace("mod guard;\n", ""),
            format!("#![cfg(any())]\n{lib}"),
        ];
        for case in &lost_lib {
            assert!(declared_in_lib(case, "guard").is_err(), "{case}");
        }
    }

    #[test]
    fn collector_check_rejects_a_nested_or_inverted_test() {
        // rustc collects a #[test] fn at module scope only: one inside a fn body, a closure, an
        // impl or a const block never runs (an allow(unnameable_test_items) silences the lint).
        // A #[should_panic] test passes only while the behavior it guards is broken. A block
        // comment can hide a declaration. Each counts as lost.
        let inner = "    #[test]\n    fn t() {\n    }\n";
        let lost = [
            format!("fn off() {{\n{inner}}}\n"),
            format!("#[allow(unnameable_test_items, dead_code)] fn off() {{\n{inner}}}\n"),
            format!("#[allow(unnameable_test_items)]\nfn off() {{\n{inner}}}\n"),
            format!("const _: () = {{\n{inner}}};\n"),
            format!("struct S;\nimpl S {{\n{inner}}}\n"),
            format!("fn off() {{\n    let _ = || {{\n{inner}    }};\n}}\n"),
            "#[test]\n#[should_panic]\nfn t() {\n}\n".to_string(),
            "#[should_panic(expected = \"dark\")]\n#[test]\nfn t() {\n}\n".to_string(),
            format!("{BLOCK_OPEN}\nfn helper() {{\n}}\n#[test]\nfn t() {{\n}}\n*/\n"),
            format!("{BLOCK_OPEN} a note */\n#[test]\nfn t() {{\n}}\n"),
            "fn helper() {\n    let _ = 1;\n\n#[test]\nfn t() {\n}\n".to_string(),
        ];
        for case in &lost {
            assert!(live_test(case, "t").is_err(), "{case}");
        }
        // Braces inside a string, a char, a raw string or a trailing comment are not scopes.
        let live = [
            "fn helper() {\n    let _ = \"}\";\n    let _ = '}';\n}\n\n#[test]\nfn t() {\n    let _ = '{';\n}\n",
            "fn helper<'a>(s: &'a str) -> &'a str {\n    s\n}\n\n#[test]\nfn t() {\n}\n",
            "const R: &str = r#\"}\"#;\nfn h() { // }\n}\n#[test]\nfn t() {\n}\n",
            "#[test]\n#[allow(clippy::bool_assert_comparison)]\nfn t() {\n}\n",
        ];
        for case in live {
            assert_eq!(live_test(case, "t"), Ok(()), "{case}");
        }

        // The review's case on the real file: the DNS-rebinding guard wrapped in a fn body.
        let real = include_str!("fl_collect_tests.rs");
        let name = COLLECTOR_SAFETY_TESTS[7];
        assert_eq!(live_test(real, name), Ok(()));
        let block = format!("#[test]\nfn {name}() {{\n");
        let start = real.find(&block).expect("the guard is declared");
        let end = start + real[start..].find("\n}\n").expect("the guard closes") + 3;
        let (head, rest) = real.split_at(start);
        let (guard, tail) = rest.split_at(end - start);
        for open in [
            "#[allow(unnameable_test_items, dead_code)] fn off() {\n",
            "fn off() {\n",
        ] {
            let wrapped = format!("{head}{open}{guard}}}\n{tail}");
            assert!(live_test(&wrapped, name).is_err(), "{open}");
        }
    }
}
