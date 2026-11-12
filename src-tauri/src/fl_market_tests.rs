//! FLM-CORE-1: the offers read model against what the merged WorkMarket really emits.
//!
//! The chain side of these tests is a RECORDED transport: it answers `eth_getLogs`, `eth_call
//! getOffer` and `eth_getBlockByNumber` from `tests/fixtures/fl_market/`, which
//! `scripts/fl/flm-market-anvil.sh` captured from the merged WorkMarket (citrate-chain 39b75f88) on
//! a loopback anvil. The last test reads a live anvil the same script starts (ignored by default).

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::*;
use crate::rpc::RpcError;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fl_market");
const MARKET: &str = "0x9fe46736679d2d9a65f0992f2272de9f3c7fa6e0";
const GENESIS: &str = "0x26159e7c146dc8e50b775856e49d32087e1fe49944ad0b5da828fa5fdb98f4ab";
const POSTER: &str = "0x70997970c51812dc3a010c7d01b50e0d17dc79c8";
const GROUP_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
/// The comms address the poster's wallet is linked to in these tests.
const POSTER_MEMBER: &str = "0x00000000000000000000000000000000000000aa";
/// Block time of the recorded head: two days after the offers opened (offer 4 is past deadline).
const NOW: u64 = 0x6af6_c7da + 60;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/{name}")).expect("fixture")
}

fn recorded_logs() -> Vec<Value> {
    serde_json::from_str(&fixture("logs.json")).expect("logs.json")
}

fn hex_u64_of(v: &Value) -> u64 {
    u64::from_str_radix(v.as_str().expect("hex").trim_start_matches("0x"), 16).expect("hex")
}

/// Answers from the recorded run. `calls` records every request for assertions.
struct Recorded {
    genesis: String,
    head: u64,
    now: u64,
    logs: Vec<Value>,
    offers: BTreeMap<u64, String>,
    calls: RefCell<Vec<Value>>,
}

impl Recorded {
    fn new(head: u64) -> Self {
        let offers = (1..=4)
            .map(|i| (i, fixture(&format!("get_offer_{i}.hex")).trim().to_string()))
            .collect();
        Self {
            genesis: GENESIS.to_string(),
            head,
            now: NOW,
            logs: recorded_logs(),
            offers,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn get_logs_ranges(&self) -> Vec<(u64, u64)> {
        self.calls
            .borrow()
            .iter()
            .filter(|c| c["method"] == "eth_getLogs")
            .map(|c| {
                let f = &c["params"][0];
                (hex_u64_of(&f["fromBlock"]), hex_u64_of(&f["toBlock"]))
            })
            .collect()
    }
}

impl crate::rpc::RpcTransport for Recorded {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        self.calls.borrow_mut().push(body.clone());
        let p = &body["params"];
        let result = match body["method"].as_str().unwrap_or("") {
            "eth_getBlockByNumber" if p[0] == "0x0" => json!({ "hash": self.genesis }),
            "eth_getBlockByNumber" => json!({
                "number": format!("0x{:x}", self.head),
                "timestamp": format!("0x{:x}", self.now),
            }),
            "eth_getLogs" => {
                let f = &p[0];
                let (from, to) = (hex_u64_of(&f["fromBlock"]), hex_u64_of(&f["toBlock"]));
                assert!(to <= self.head, "the read model asked past the head");
                Value::Array(
                    self.logs
                        .iter()
                        .filter(|l| {
                            let b = hex_u64_of(&l["blockNumber"]);
                            b >= from && b <= to
                        })
                        .cloned()
                        .collect(),
                )
            }
            "eth_call" => {
                let data = p[0]["data"].as_str().unwrap_or("");
                let id = u64::from_str_radix(&data[data.len() - 16..], 16).unwrap_or(0);
                match self.offers.get(&id) {
                    Some(r) => json!(r),
                    None => json!(format!("0x{}", "00".repeat(OFFER_WORDS * 32))),
                }
            }
            m => return Err(RpcError::Transport(format!("unexpected {m}"))),
        };
        Ok(json!({ "jsonrpc": "2.0", "id": 1, "result": result }))
    }
}

/// Requirements files from a directory, named by CID (the recorded files, or the live run's).
struct DirSource(String);

impl RequirementsSource for DirSource {
    fn fetch(&self, cid: &str, max: usize) -> Result<Vec<u8>, String> {
        let b = std::fs::read(format!("{}/{cid}", self.0)).map_err(|e| e.to_string())?;
        Ok(b.into_iter().take(max + 1).collect())
    }
}

fn synced() -> OfferBook {
    let t = Recorded::new(25);
    let mut book = OfferBook::new(MARKET, GENESIS, 0);
    sync(&t, &mut book, t.head, t.now);
    fetch_requirements(&DirSource(FIXTURES.into()), &mut book, t.now);
    book
}

fn ctx_with_role(role: &str, linked: bool) -> MemberContext {
    MemberContext {
        groups: vec![GroupMembership {
            group_id: GROUP_ID.into(),
            roster: vec![
                (POSTER_MEMBER.into(), role.into()),
                (
                    "0x00000000000000000000000000000000000000bb".into(),
                    "member".into(),
                ),
            ],
        }],
        links: if linked {
            vec![WalletLink {
                wallet: POSTER.into(),
                member: POSTER_MEMBER.into(),
            }]
        } else {
            Vec::new()
        },
    }
}

// ── decoding ───────────────────────────────────────────────────────

#[test]
fn topics_are_pinned_to_the_contract_events() {
    assert_eq!(
        topic(OFFER_OPENED_SIG),
        "0x875f1fa10fe57ef380a01ef857d32abe9a607f0894fcdf035637eda3c370451a"
    );
    assert_eq!(
        topic(OFFER_CLOSED_SIG),
        "0x33910d348349980653133cceb7aed6e2216c3ef8f0b3dccafe181486a83c730a"
    );
}

#[test]
fn decodes_the_recorded_market_logs() {
    let logs: Vec<RawLog> = recorded_logs()
        .iter()
        .map(|v| parse_log(v).expect("log"))
        .collect();
    let events: Vec<MarketEvent> = logs
        .iter()
        .map(|l| decode_event(l).expect("event"))
        .collect();
    assert_eq!(events.len(), 5);
    let MarketEvent::Opened(first) = &events[0] else {
        panic!("first log is OfferOpened")
    };
    assert_eq!(first.id, 1);
    assert_eq!(first.kind, OfferKind::FlRound);
    assert_eq!(first.poster, POSTER);
    assert!(!first.is_group());
    assert_eq!(first.price_per_unit, "1000000000000000000");
    assert_eq!(first.budget, "5000000000000000000");
    assert_eq!(first.requirements_cid, "bafkreiflmopen");
    let MarketEvent::Opened(group) = &events[1] else {
        panic!("second log is OfferOpened")
    };
    assert!(group.is_group());
    assert_eq!(Some(group.group_hash.clone()), group_hash(GROUP_ID));
    assert_eq!(
        events[4],
        MarketEvent::Closed {
            id: 4,
            final_state: OfferState::Expired
        }
    );
}

#[test]
fn a_log_with_dirty_padding_or_another_topic_is_refused() {
    let mut l = parse_log(&recorded_logs()[0]).expect("log");
    let n = l.data.len();
    l.data[n - 1] = 1;
    assert!(matches!(decode_event(&l), Err(MarketError::BadLog(_))));
    let mut other = parse_log(&recorded_logs()[0]).expect("log");
    other.topics[0] = topic("Joined(uint256,address,uint128)");
    assert!(matches!(decode_event(&other), Err(MarketError::BadLog(_))));
    let mut short = parse_log(&recorded_logs()[0]).expect("log");
    short.topics.pop();
    assert!(matches!(decode_event(&short), Err(MarketError::BadLog(_))));
}

#[test]
fn get_offer_terms_decode_and_must_match_the_log() {
    let logs: Vec<RawLog> = recorded_logs()
        .iter()
        .map(|v| parse_log(v).expect("log"))
        .collect();
    let MarketEvent::Opened(ev1) = decode_event(&logs[0]).expect("event") else {
        panic!("OfferOpened")
    };
    let ret = hex::decode(fixture("get_offer_1.hex").trim().trim_start_matches("0x")).expect("hex");
    let terms = decode_offer_terms(&ret, &ev1).expect("terms");
    assert_eq!(terms.state, OfferState::Open);
    assert_eq!(terms.max_workers, 8);
    assert_eq!(terms.workers, 0);
    assert_eq!(
        terms.coordinator,
        "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc"
    );
    assert_eq!(terms.dispute_window, 86_400);
    let req = std::fs::read(format!("{FIXTURES}/bafkreiflmopen")).expect("req");
    assert_eq!(terms.requirements_hash, hex0x(&keccak(&req)));
    // Offer 2's return is not offer 1's terms.
    let other =
        hex::decode(fixture("get_offer_2.hex").trim().trim_start_matches("0x")).expect("hex");
    assert!(matches!(
        decode_offer_terms(&other, &ev1),
        Err(MarketError::BadOffer(_))
    ));
    assert!(matches!(
        decode_offer_terms(&ret[..32 * 37], &ev1),
        Err(MarketError::BadOffer(_))
    ));
}

// ── the cursor ─────────────────────────────────────────────────────

#[test]
fn sync_reads_the_recorded_market_from_the_cursor() {
    let t = Recorded::new(25);
    let mut book = OfferBook::new(MARKET, GENESIS, 0);
    let r = sync(&t, &mut book, t.head, t.now);
    assert_eq!(r.stopped, None);
    assert_eq!(r.new_offers, vec![1, 2, 3, 4]);
    assert_eq!(r.read_to, Some(25 - CONFIRMATIONS));
    assert_eq!(r.behind, 0);
    assert_eq!(book.next_block, 25 - CONFIRMATIONS + 1);
    assert_eq!(book.offers[&4].closed, Some(OfferState::Expired));
    for id in 1..=3 {
        assert!(
            book.offers[&id].terms.is_some(),
            "live offer {id} has terms"
        );
    }
    assert!(
        book.offers[&4].terms.is_none(),
        "a closed offer is not re-read"
    );
    // A second sync at the same head reads no logs and finds nothing new.
    let t2 = Recorded::new(25);
    let r2 = sync(&t2, &mut book, t2.head, t2.now);
    assert!(r2.new_offers.is_empty());
    assert!(t2.get_logs_ranges().is_empty());
}

#[test]
fn sync_never_reads_inside_the_confirmation_depth() {
    let t = Recorded::new(4 + CONFIRMATIONS - 1);
    let mut book = OfferBook::new(MARKET, GENESIS, 0);
    let r = sync(&t, &mut book, t.head, t.now);
    assert!(
        r.new_offers.is_empty(),
        "the first offer (block 4) is not deep enough yet"
    );
    assert!(t.get_logs_ranges().iter().all(|(_, to)| *to < 4));
}

#[test]
fn sync_reads_in_contiguous_bounded_chunks() {
    let t = Recorded::new(3 * LOG_CHUNK_BLOCKS + 500);
    let mut book = OfferBook::new(MARKET, GENESIS, 0);
    sync(&t, &mut book, t.head, t.now);
    let ranges = t.get_logs_ranges();
    assert_eq!(ranges.len(), 4);
    let mut expect = 0;
    for (from, to) in ranges {
        assert_eq!(from, expect);
        assert!(to - from < LOG_CHUNK_BLOCKS);
        expect = to + 1;
    }
    assert_eq!(expect, t.head - CONFIRMATIONS + 1);
    assert_eq!(book.offers.len(), 4);
}

#[test]
fn a_refused_chunk_leaves_the_model_unchanged() {
    let logs: Vec<RawLog> = recorded_logs()
        .iter()
        .map(|v| parse_log(v).expect("log"))
        .collect();
    let mut book = OfferBook::new(MARKET, GENESIS, 0);
    let before = book.clone();
    assert_eq!(
        book.apply_chunk(1, 20, &logs),
        Err(MarketError::Gap {
            expected: 0,
            got: 1
        })
    );
    assert!(matches!(
        book.apply_chunk(0, 5, &logs),
        Err(MarketError::OutOfRange { .. })
    ));
    let mut foreign = logs.clone();
    foreign[1].address = "0x00000000000000000000000000000000000000ff".into();
    assert!(matches!(
        book.apply_chunk(0, 20, &foreign),
        Err(MarketError::WrongEmitter(_))
    ));
    assert_eq!(
        book.apply_chunk(0, 20, &logs[4..]),
        Err(MarketError::UnknownOffer(4))
    );
    assert_eq!(book, before, "nothing applied, cursor unmoved");
    assert_eq!(book.apply_chunk(0, 20, &logs).map(|v| v.len()), Ok(4));
    // The same open again with other terms is a conflict, not an overwrite.
    let mut forged = logs[0].clone();
    forged.block_number = 21;
    let n = forged.data.len();
    forged.data[n - 32] ^= 1; // the CID's first byte
    assert_eq!(
        book.apply_chunk(21, 30, &[forged]),
        Err(MarketError::Conflict(1))
    );
    // A replay of the same log in a later chunk changes nothing.
    let mut replay = logs[0].clone();
    replay.block_number = 22;
    assert_eq!(book.apply_chunk(21, 30, &[replay]), Ok(vec![]));
}

#[test]
fn the_model_persists_and_is_bound_to_its_market_and_generation() {
    let book = synced();
    let dir = std::env::temp_dir().join(format!("flm-core-1-{}", std::process::id()));
    let path = dir.join("market.json");
    book.save(&path).expect("save");
    let back = OfferBook::load(&path).expect("load").expect("present");
    assert_eq!(back, book);
    assert!(back.bound_to(MARKET, GENESIS));
    assert!(!back.bound_to(MARKET, "0x01"));
    assert!(!back.bound_to("0x00000000000000000000000000000000000000ff", GENESIS));
    std::fs::write(&path, b"{not json").expect("write");
    assert!(
        OfferBook::load(&path).is_err(),
        "a corrupt model is an error, not empty"
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(OfferBook::load(&path), Ok(None));
}

// ── requirements ───────────────────────────────────────────────────

#[test]
fn requirements_hash_mismatch_refused() {
    let book = synced();
    assert!(matches!(
        book.offers[&1].requirements,
        RequirementsRead::Verified { .. }
    ));
    let RequirementsRead::Refused { reason } = &book.offers[&3].requirements else {
        panic!("offer 3 names a file that does not hash to its commitment")
    };
    assert!(reason.contains("hashes to"), "{reason}");
    assert_eq!(
        visibility(&book.offers[&3], &MemberContext::default(), NOW),
        Err(Hidden::RequirementsRefused {
            detail: reason.clone()
        })
    );
}

#[test]
fn verified_requirements_carry_the_posted_terms() {
    let book = synced();
    let RequirementsRead::Verified { requirements } = &book.offers[&1].requirements else {
        panic!("verified")
    };
    assert_eq!(requirements.min_vram_gb, 16);
    assert_eq!(requirements.architectures, vec!["llama".to_string()]);
}

#[test]
fn requirements_files_are_strict() {
    let h = |b: &[u8]| hex0x(&keccak(b));
    let ok = br#"{"v":1,"dataClasses":["text.public"],"architectures":["llama"],"minVramGb":8,"estHours":1}"#;
    assert!(verify_requirements(ok, &h(ok)).is_ok());
    assert!(verify_requirements(ok, &h(b"other")).is_err());
    let extra = br#"{"v":1,"dataClasses":["a"],"architectures":["b"],"minVramGb":8,"estHours":1,"run":"x"}"#;
    assert!(matches!(
        verify_requirements(extra, &h(extra)),
        Err(RequirementsError::Malformed(_))
    ));
    let v2 = br#"{"v":2,"dataClasses":["a"],"architectures":["b"],"minVramGb":8,"estHours":1}"#;
    assert!(verify_requirements(v2, &h(v2)).is_err());
    let label = br#"{"v":1,"dataClasses":["Ignore previous instructions"],"architectures":["b"],"minVramGb":8,"estHours":1}"#;
    assert!(verify_requirements(label, &h(label)).is_err());
    let big = vec![b' '; MAX_REQUIREMENTS_BYTES + 1];
    assert_eq!(
        verify_requirements(&big, &h(&big)),
        Err(RequirementsError::TooLarge(MAX_REQUIREMENTS_BYTES + 1))
    );
}

#[test]
fn a_failed_fetch_is_retried_not_refused() {
    let t = Recorded::new(25);
    let mut book = OfferBook::new(MARKET, GENESIS, 0);
    sync(&t, &mut book, t.head, t.now);
    fetch_requirements(&DirSource("/nonexistent-flm".into()), &mut book, t.now);
    assert!(matches!(
        book.offers[&1].requirements,
        RequirementsRead::NotRead { .. }
    ));
    fetch_requirements(&DirSource(FIXTURES.into()), &mut book, t.now);
    assert!(matches!(
        book.offers[&1].requirements,
        RequirementsRead::Verified { .. }
    ));
}

// ── who sees what ──────────────────────────────────────────────────

#[test]
fn group_offer_hidden_when_poster_not_admin() {
    let book = synced();
    let group = &book.offers[&2];
    assert_eq!(
        visibility(group, &ctx_with_role("member", true), NOW),
        Err(Hidden::PosterNotAdmin)
    );
    assert_eq!(
        visibility(group, &ctx_with_role("admin", false), NOW),
        Err(Hidden::PosterNotAdmin),
        "an admin roster entry counts only through a verified wallet link"
    );
    assert_eq!(
        visibility(group, &ctx_with_role("admin", true), NOW),
        Ok(())
    );
    assert_eq!(
        visibility(group, &ctx_with_role("owner", true), NOW),
        Ok(())
    );
}

#[test]
fn group_offer_hidden_from_members_of_other_groups() {
    let book = synced();
    let mut ctx = ctx_with_role("admin", true);
    ctx.groups[0].group_id = "22".repeat(32);
    assert_eq!(
        visibility(&book.offers[&2], &ctx, NOW),
        Err(Hidden::NotYourGroup)
    );
    assert_eq!(
        visibility(&book.offers[&2], &MemberContext::default(), NOW),
        Err(Hidden::NotYourGroup)
    );
}

#[test]
fn open_offers_are_shown_and_closed_ones_say_why_not() {
    let book = synced();
    let none = MemberContext::default();
    assert_eq!(visibility(&book.offers[&1], &none, NOW), Ok(()));
    assert_eq!(
        visibility(&book.offers[&4], &none, NOW),
        Err(Hidden::NotTakingWorkers)
    );
    let deadline = book.offers[&1].opened.deadline;
    assert_eq!(
        visibility(&book.offers[&1], &none, deadline + 1),
        Err(Hidden::NotTakingWorkers)
    );
}

#[test]
fn an_offer_without_terms_is_not_shown() {
    let mut book = synced();
    if let Some(e) = book.offers.get_mut(&1) {
        e.terms = None;
    }
    assert_eq!(
        visibility(&book.offers[&1], &MemberContext::default(), NOW),
        Err(Hidden::TermsNotRead)
    );
}

#[test]
fn group_hash_is_keccak_of_the_32_byte_group_id() {
    let raw = hex::decode(GROUP_ID).expect("hex");
    assert_eq!(group_hash(GROUP_ID), Some(hex0x(&keccak(&raw))));
    assert_eq!(group_hash(&format!("0x{GROUP_ID}")), group_hash(GROUP_ID));
    assert_eq!(group_hash("abcd"), None);
}

// ── refresh ────────────────────────────────────────────────────────

#[test]
fn refresh_reads_nothing_from_another_generation() {
    let mut t = Recorded::new(25);
    t.genesis = format!("0x{}", "ab".repeat(32));
    let (view, _) = refresh(
        &t,
        &DirSource(FIXTURES.into()),
        MARKET,
        GENESIS,
        None,
        &MemberContext::default(),
    );
    assert!(!view.available);
    assert!(view.reason.unwrap_or_default().contains("block 0"));
    assert!(t.get_logs_ranges().is_empty());
}

#[test]
fn refresh_discards_a_model_for_another_market() {
    let t = Recorded::new(25);
    let mut stale = OfferBook::new("0x00000000000000000000000000000000000000ff", GENESIS, 99);
    stale.next_block = 99;
    let (view, book) = refresh(
        &t,
        &DirSource(FIXTURES.into()),
        MARKET,
        GENESIS,
        Some(stale),
        &MemberContext::default(),
    );
    assert!(view.available);
    let book = book.expect("a fresh model");
    assert!(book.bound_to(MARKET, GENESIS));
    assert_eq!(book.offers.len(), 4);
    let shown: Vec<u64> = view
        .offers
        .iter()
        .filter(|o| o.hidden.is_none())
        .map(|o| o.entry.opened.id)
        .collect();
    assert_eq!(
        shown,
        vec![1],
        "only the verified open offer, for a member in no group"
    );
}

/// The read model against the merged WorkMarket on a live loopback anvil. Run by
/// `scripts/fl/flm-market-anvil.sh`, which deploys the contracts and opens the offers.
#[test]
#[ignore = "needs the anvil started by scripts/fl/flm-market-anvil.sh"]
fn fl_market_reads_the_merged_work_market_on_anvil() {
    let rpc = std::env::var("FLM_ANVIL_RPC").expect("FLM_ANVIL_RPC");
    assert!(rpc.starts_with("http://127.0.0.1:"), "loopback only: {rpc}");
    let market = std::env::var("FLM_ANVIL_MARKET").expect("FLM_ANVIL_MARKET");
    let req_dir = std::env::var("FLM_ANVIL_REQ_DIR").expect("FLM_ANVIL_REQ_DIR");
    let group_id = std::env::var("FLM_ANVIL_GROUP_ID").expect("FLM_ANVIL_GROUP_ID");
    let t = crate::rpc::HttpTransport::new(rpc);
    let genesis = read_genesis_hash(&t).expect("block 0");
    let mut ctx = ctx_with_role("admin", true);
    ctx.groups[0].group_id = group_id.trim_start_matches("0x").to_string();
    let (view, book) = refresh(
        &t,
        &DirSource(req_dir),
        &market.to_ascii_lowercase(),
        &genesis,
        None,
        &ctx,
    );
    assert!(view.available, "{:?}", view.reason);
    let book = book.expect("model");
    assert_eq!(book.offers.len(), 4);
    let hidden: BTreeMap<u64, Option<Hidden>> = view
        .offers
        .iter()
        .map(|o| (o.entry.opened.id, o.hidden.clone()))
        .collect();
    assert_eq!(hidden[&1], None);
    assert_eq!(
        hidden[&2], None,
        "the group offer, poster linked to an admin"
    );
    assert!(matches!(
        hidden[&3],
        Some(Hidden::RequirementsRefused { .. })
    ));
    assert_eq!(hidden[&4], Some(Hidden::NotTakingWorkers));
    let mut member_ctx = ctx_with_role("member", true);
    member_ctx.groups[0].group_id = group_id.trim_start_matches("0x").to_string();
    assert_eq!(
        visibility(
            &book.offers[&2],
            &member_ctx,
            read_head(&t).expect("head").1
        ),
        Err(Hidden::PosterNotAdmin)
    );
    eprintln!(
        "live anvil: {} offers, sync {:?}",
        book.offers.len(),
        view.sync
    );
}
