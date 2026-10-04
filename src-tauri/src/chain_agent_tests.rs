//! HUP-S7.3 + S7.5 (core): the anchor schedule's gate, planning and settlement, and the chain
//! settings. The sidecar is replaced by a recording port (test double); the ceremony is real.

use super::*;
use crate::ceremony::anchor::{anchor_calldata, AnchorReceipt};
use crate::hermes::chain::{PlannedAnchor, PlannedCall};
use std::cell::RefCell;

const REG: &str = "0x00000000000000000000000000000000000000a1";

fn hex32(b: u8) -> String {
    format!("0x{}", hex::encode([b; 32]))
}

fn ready_plan(day: u64, b: u8, to: &str) -> PlannedAnchor {
    PlannedAnchor {
        plan: "ready".into(),
        day,
        date: Some("2026-09-30".into()),
        commitment: Some(hex32(b)),
        call: Some(PlannedCall {
            chain_id: 40204,
            to: Some(to.into()),
            value: 0,
            kind: "nightly_merkle".into(),
            root: hex32(b),
            data: format!("0x{}", hex::encode(anchor_calldata(&[b; 32]))),
        }),
    }
}

#[derive(Default)]
struct Port {
    status: serde_json::Value,
    plans: BTreeMap<u64, PlannedAnchor>,
    calls: RefCell<Vec<String>>,
}
impl AnchorPort for Port {
    fn status(&self) -> Result<serde_json::Value, String> {
        self.calls.borrow_mut().push("status".into());
        Ok(self.status.clone())
    }
    fn plan(&self, day: u64, registry: &str) -> Result<PlannedAnchor, String> {
        self.calls
            .borrow_mut()
            .push(format!("plan {day} {registry}"));
        self.plans
            .get(&day)
            .cloned()
            .ok_or_else(|| format!("no plan for {day}"))
    }
    fn confirm(&self, day: u64, commitment: &str, tx: &str, block: u64) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("confirm {day} {commitment} {tx} {block}"));
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// addresses

#[test]
fn optional_pins_are_read_only_when_well_formed() {
    let book = |v: &str| format!(r#"{{"addresses":{{"AnchorRegistry":"{v}"}}}}"#);
    assert_eq!(optional_pin(r#"{"addresses":{}}"#, "AnchorRegistry"), None);
    assert_eq!(optional_pin("not json", "AnchorRegistry"), None);
    assert_eq!(optional_pin(&book("0x1234"), "AnchorRegistry"), None);
    assert_eq!(
        optional_pin(&book(&format!("0x{}", "0".repeat(40))), "AnchorRegistry"),
        None,
        "the zero address is not a deployment"
    );
    assert_eq!(
        optional_pin(
            &book("0x00000000000000000000000000000000000000AB"),
            "AnchorRegistry"
        ),
        Some("0x00000000000000000000000000000000000000ab".into())
    );
}

/// Honest-state tripwire (flipped by HUP fan-out 6): the shipped book pins both registries, the
/// addresses citrate-chain's canonical 40204 book names and `scripts/sync-addresses.py --rpc`
/// found code at (2026-10-04). A reroll or redeploy that moves them fails this on purpose: the
/// surface, the schedule and the anvil rehearsal must then be rechecked.
#[test]
fn the_shipped_book_pins_the_anchor_and_benchmark_registries() {
    assert_eq!(
        anchor_registry().as_deref(),
        Some("0x41e0f9a4dcd29c650dc58ee569bf267fd9ba4817")
    );
    assert_eq!(
        benchmark_registry().as_deref(),
        Some("0x84247a5f65370947c792181a3afed5ac0f452ec8")
    );
}

/// Pinned does not mean on: the member's settings still default to off, so the shipped book
/// starts no schedule and shares nothing until the member turns a feature on.
#[test]
fn a_pinned_registry_changes_nothing_until_the_member_turns_it_on() {
    let off = ChainSettings::default();
    assert_eq!(
        anchor_gate(anchor_registry().as_deref(), &off),
        AnchorGate::Off
    );
    assert!(!off.anchor_nightly && !off.share_benchmarks);
}

// ---------------------------------------------------------------------------------------------
// settings + gate

#[test]
fn settings_default_off_and_survive_a_round_trip() {
    let dir = std::env::temp_dir().join(format!(
        "n4-chain-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let p = dir.join("hermes").join("chain-settings.json");
    assert_eq!(load_settings(&p), ChainSettings::default());
    assert!(!ChainSettings::default().anchor_nightly);
    assert!(!ChainSettings::default().share_benchmarks);
    let on = ChainSettings {
        anchor_nightly: true,
        share_benchmarks: false,
    };
    save_settings(&p, &on).unwrap();
    assert_eq!(load_settings(&p), on);
    std::fs::write(&p, "{not json").unwrap();
    assert_eq!(
        load_settings(&p),
        ChainSettings::default(),
        "unreadable = off"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_can_be_turned_on_before_its_registry_is_deployed() {
    let on = |a, b| ChainSettings {
        anchor_nightly: a,
        share_benchmarks: b,
    };
    assert!(apply_settings(on(true, false), false, true).is_err());
    assert!(apply_settings(on(false, true), true, false).is_err());
    assert_eq!(
        apply_settings(on(false, false), false, false),
        Ok(on(false, false))
    );
    assert_eq!(
        apply_settings(on(true, true), true, true),
        Ok(on(true, true))
    );
}

#[test]
fn the_gate_needs_a_deployed_registry_and_the_members_choice() {
    let off = ChainSettings::default();
    let on = ChainSettings {
        anchor_nightly: true,
        share_benchmarks: false,
    };
    assert_eq!(anchor_gate(None, &on), AnchorGate::NotDeployed);
    assert_eq!(anchor_gate(Some(REG), &off), AnchorGate::Off);
    assert_eq!(anchor_gate(Some(REG), &on), AnchorGate::Ready);
    assert!(anchor_status_line(AnchorGate::NotDeployed)
        .contains("not in this app's 40204 address book"));
    assert!(benchmark_status_line(None, &off).contains("not in this app's 40204 address book"));
    for line in [
        anchor_status_line(AnchorGate::NotDeployed),
        anchor_status_line(AnchorGate::Off),
        anchor_status_line(AnchorGate::Ready),
        benchmark_status_line(None, &off),
        benchmark_status_line(Some(REG), &on),
    ] {
        assert!(
            !line.contains('\u{2014}'),
            "no em-dashes in member-facing text: {line}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// the nightly tick

#[test]
fn a_tick_that_is_not_ready_touches_nothing() {
    let port = Port::default();
    let c = AnchorCeremony::new();
    for (gate, reg) in [
        (AnchorGate::NotDeployed, None),
        (AnchorGate::Off, Some(REG)),
        (AnchorGate::Ready, None),
    ] {
        let r = nightly_tick(&port, &c, gate, reg).unwrap();
        assert!(r.raised.is_empty());
    }
    assert!(port.calls.borrow().is_empty(), "no sidecar call at all");
    assert!(c.pending().is_empty());
}

#[test]
fn a_ready_tick_raises_one_card_per_ready_day_and_signs_nothing() {
    let mut port = Port {
        status: serde_json::json!({
            "pendingDays": [{"day": 20001}, {"day": 20002}],
            "awaitingConfirmation": [{"day": 20000}],
        }),
        ..Port::default()
    };
    port.plans.insert(20000, ready_plan(20000, 0xa0, REG));
    port.plans.insert(20001, ready_plan(20001, 0xa1, REG));
    port.plans.insert(
        20002,
        PlannedAnchor {
            plan: "empty".into(),
            day: 20002,
            date: None,
            commitment: None,
            call: None,
        },
    );
    let c = AnchorCeremony::new();
    let r = nightly_tick(&port, &c, AnchorGate::Ready, Some(REG)).unwrap();
    let days: Vec<u64> = r.raised.iter().map(|v| v.day).collect();
    assert_eq!(days, vec![20000, 20001]);
    assert_eq!(r.skipped, vec![(20002, "plan: empty".to_string())]);
    assert_eq!(c.pending().len(), 2);
    // the second pass raises nothing new
    let r2 = nightly_tick(&port, &c, AnchorGate::Ready, Some(REG)).unwrap();
    assert_eq!(r2.raised.len(), 2, "same cards, same ids");
    assert_eq!(c.pending().len(), 2);
    assert!(port
        .calls
        .borrow()
        .iter()
        .all(|c| c == "status" || c.starts_with("plan ")));
}

#[test]
fn a_plan_aimed_elsewhere_is_never_raised() {
    let mut port = Port {
        status: serde_json::json!({ "pendingDays": [{"day": 20001}] }),
        ..Port::default()
    };
    port.plans.insert(
        20001,
        ready_plan(20001, 0xa1, "0x00000000000000000000000000000000000000ee"),
    );
    let c = AnchorCeremony::new();
    let r = nightly_tick(&port, &c, AnchorGate::Ready, Some(REG)).unwrap();
    assert!(r.raised.is_empty());
    assert_eq!(r.skipped.len(), 1);
    assert!(c.pending().is_empty());
}

#[test]
fn malformed_ready_plans_are_refused() {
    let mut p = ready_plan(1, 1, REG);
    p.call.as_mut().unwrap().kind = "per_capsule".into();
    assert!(request_from_plan(&p).is_err());
    let mut p = ready_plan(1, 1, REG);
    p.commitment = Some("0x12".into());
    assert!(request_from_plan(&p).is_err());
    let mut p = ready_plan(1, 1, REG);
    p.call.as_mut().unwrap().to = None;
    assert!(request_from_plan(&p).is_err());
    let mut p = ready_plan(1, 1, REG);
    p.call = None;
    assert!(request_from_plan(&p).is_err());
    let mut p = ready_plan(1, 1, REG);
    p.plan = "incomplete".into();
    assert_eq!(request_from_plan(&p), Ok(None));
}

// ---------------------------------------------------------------------------------------------
// settlement: anchored only on a confirming receipt

fn receipt(block: Option<u64>, status: Option<u64>) -> AnchorReceipt {
    AnchorReceipt {
        day: 20000,
        commitment: [0xa0; 32],
        tx_hash: format!("0x{}", "cd".repeat(32)),
        block_number: block,
        status,
        nonce: None,
        from: None,
        gas_used: None,
        effective_gas_price_wei: None,
    }
}

#[test]
fn a_day_is_marked_anchored_only_on_a_mined_successful_receipt() {
    let port = Port::default();
    assert_eq!(settle(&port, &receipt(None, None)), Ok(false));
    assert_eq!(settle(&port, &receipt(Some(9), Some(0))), Ok(false));
    assert_eq!(settle(&port, &receipt(Some(9), None)), Ok(false));
    assert!(port.calls.borrow().is_empty());
    assert_eq!(settle(&port, &receipt(Some(9), Some(1))), Ok(true));
    assert_eq!(
        port.calls.borrow().as_slice(),
        &[format!(
            "confirm 20000 {} 0x{} 9",
            hex32(0xa0),
            "cd".repeat(32)
        )]
    );
}

#[test]
fn days_to_anchor_merges_pending_and_unconfirmed() {
    let v = serde_json::json!({
        "pendingDays": [{"day": 3}, {"day": 1}],
        "awaitingConfirmation": [{"day": 1}, {"day": 2}],
        "anchored": [{"day": 0}],
    });
    assert_eq!(days_to_anchor(&v), vec![1, 2, 3]);
    assert!(days_to_anchor(&serde_json::json!({})).is_empty());
}

#[test]
fn every_owner_decision_is_labelled_pending_sign_off() {
    for d in PENDING_OWNER_SIGN_OFF {
        assert!(d.contains("Pending owner sign-off"), "{d}");
    }
}

// ---------------------------------------------------------------------------------------------
// the sidecar data folders

#[test]
fn core_passes_the_sidecar_its_three_data_folders_and_never_turns_trajectories_on() {
    let base = std::path::Path::new("/data/hermes");
    let dirs = crate::hermes::chain::data_dirs(base);
    let names: Vec<&str> = dirs.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        names,
        vec![
            "CITRATE_HERMES_METERING_DIR",
            "CITRATE_HERMES_RECORDS_DIR",
            "CITRATE_HERMES_ANCHOR_DIR"
        ]
    );
    assert!(dirs.iter().all(|(_, p)| p.starts_with(base)));
    assert!(!names.contains(&"CITRATE_HERMES_TRAJECTORIES"));
}

#[test]
fn metering_days_are_shape_checked_before_reaching_the_sidecar() {
    use crate::hermes::chain::valid_day;
    assert!(valid_day("2026-10-01"));
    for bad in [
        "2026-1-01",
        "2026/10/01",
        "../x",
        "2026-10-01&x=1",
        "",
        "abcd-ef-gh",
    ] {
        assert!(!valid_day(bad), "{bad}");
    }
}

#[test]
fn a_day_waiting_on_its_receipt_never_gets_a_second_card() {
    let mut port = Port {
        status: serde_json::json!({ "awaitingConfirmation": [{"day": 20000}, {"day": 20001}] }),
        ..Port::default()
    };
    port.plans.insert(20000, ready_plan(20000, 0xa0, REG));
    port.plans.insert(20001, ready_plan(20001, 0xa1, REG));
    let c = AnchorCeremony::new();
    let in_flight: std::collections::BTreeSet<u64> = [20000].into_iter().collect();
    let r = nightly_tick_with(&port, &c, AnchorGate::Ready, Some(REG), &in_flight).unwrap();
    assert_eq!(
        r.raised.iter().map(|v| v.day).collect::<Vec<_>>(),
        vec![20001]
    );
    assert_eq!(
        r.skipped,
        vec![(
            20000,
            "waiting for the previous anchor's receipt".to_string()
        )]
    );
    assert!(!port
        .calls
        .borrow()
        .iter()
        .any(|c| c.starts_with("plan 20000")));
}

#[test]
fn turning_anchoring_off_drops_every_pending_card_unsigned() {
    let mut port = Port {
        status: serde_json::json!({ "pendingDays": [{"day": 20000}] }),
        ..Port::default()
    };
    port.plans.insert(20000, ready_plan(20000, 0xa0, REG));
    let c = AnchorCeremony::new();
    nightly_tick(&port, &c, AnchorGate::Ready, Some(REG)).unwrap();
    assert_eq!(c.pending().len(), 1);
    // still on: nothing dropped
    assert_eq!(
        drop_pending_when_off(
            &c,
            &ChainSettings {
                anchor_nightly: true,
                share_benchmarks: false
            }
        ),
        0
    );
    assert_eq!(c.pending().len(), 1);
    assert_eq!(drop_pending_when_off(&c, &ChainSettings::default()), 1);
    assert!(c.pending().is_empty());
}

// ---------------------------------------------------------------------------------------------
// review fixes (n4 adversarial review)

#[test]
fn the_card_date_comes_from_the_day_not_from_the_sidecar() {
    let mut p = ready_plan(20_000, 0xc1, REG);
    p.date = Some("tomorrow, approve now".into());
    let req = request_from_plan(&p).unwrap().unwrap();
    assert_eq!(req.date, "2024-10-04");
    let c = AnchorCeremony::new();
    let v = c.request(req, REG).unwrap();
    assert!(
        v.decoded.action.contains("2024-10-04"),
        "{}",
        v.decoded.action
    );
    let mut far = ready_plan(u64::MAX, 0xc1, REG);
    far.date = None;
    assert!(request_from_plan(&far).is_err());
}

struct DownPort;
impl AnchorPort for DownPort {
    fn status(&self) -> Result<serde_json::Value, String> {
        Err("down".into())
    }
    fn plan(&self, _d: u64, _r: &str) -> Result<PlannedAnchor, String> {
        Err("down".into())
    }
    fn confirm(&self, _d: u64, _c: &str, _t: &str, _b: u64) -> Result<(), String> {
        Err("down".into())
    }
}

fn in_flight_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "citrate-anchor-inflight-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    d.join(IN_FLIGHT_FILE)
}

fn held(tag: &str) -> InFlightAnchors {
    InFlightAnchors::load(Some(in_flight_path(tag)))
}

#[test]
fn a_sent_anchor_is_never_forgotten_after_broadcast() {
    // Mined and confirmed, but the sidecar could not record it: kept for the re-poll, so the day
    // is not raised again.
    let h = held("a");
    let port: &dyn AnchorPort = &DownPort;
    let (anchored, line) = after_broadcast(Ok(port), &receipt(Some(9), Some(1)), &h);
    assert!(!anchored);
    assert!(line.contains("block 9"), "{line}");
    assert!(h.days().contains(&20000));
    // Hermes not reachable at all: same.
    let h = held("b");
    let (anchored, _) = after_broadcast(
        Err("Hermes is not running".into()),
        &receipt(Some(9), Some(1)),
        &h,
    );
    assert!(!anchored);
    assert!(h.days().contains(&20000));
    // Receipt unknown: kept.
    let h = held("c");
    let (anchored, _) = after_broadcast(
        Err("Hermes is not running".into()),
        &receipt(None, None),
        &h,
    );
    assert!(!anchored);
    assert!(h.days().contains(&20000));
    // Reverted: not kept (the day may be raised again), not anchored.
    let h = held("d");
    h.record_sent(&receipt(None, None))
        .expect("record before send");
    let ok_port = Port::default();
    let (anchored, _) = after_broadcast(Ok(&ok_port), &receipt(Some(9), Some(0)), &h);
    assert!(!anchored);
    assert!(h.days().is_empty());
    // Confirmed and recorded: anchored, nothing kept.
    h.record_sent(&receipt(None, None))
        .expect("record before send");
    let (anchored, line) = after_broadcast(Ok(&ok_port), &receipt(Some(9), Some(1)), &h);
    assert!(anchored);
    assert_eq!(line, "Anchored in block 9.");
    assert!(h.days().is_empty());
}

#[test]
fn an_in_flight_anchor_survives_a_restart() {
    // A day sent but not yet mined must still be known after the app restarts, or the nightly
    // pass would raise a second card and a second signed anchor for it.
    let path = in_flight_path("restart");
    {
        let h = InFlightAnchors::load(Some(path.clone()));
        h.record_sent(&receipt(None, None))
            .expect("record before send");
    }
    let h = InFlightAnchors::load(Some(path.clone()));
    assert_eq!(h.blocked(), None);
    assert!(h.days().contains(&20000));
    let mut port = Port {
        status: serde_json::json!({ "awaitingConfirmation": [{"day": 20000}] }),
        ..Port::default()
    };
    port.plans.insert(20000, ready_plan(20000, 0xa0, REG));
    let c = AnchorCeremony::new();
    let r = nightly_tick_with(&port, &c, AnchorGate::Ready, Some(REG), &h.days()).unwrap();
    assert!(r.raised.is_empty(), "no second card for a day in flight");
    assert_eq!(r.skipped.len(), 1);
    // Settled: removed on disk too.
    h.settle_day(20000);
    assert!(InFlightAnchors::load(Some(path)).days().is_empty());
}

#[cfg(unix)]
#[test]
fn the_in_flight_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let path = in_flight_path("perms");
    let h = InFlightAnchors::load(Some(path.clone()));
    h.record_sent(&receipt(None, None)).expect("record");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn an_unreadable_in_flight_file_blocks_new_anchors_instead_of_forgetting() {
    let path = in_flight_path("bad");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{ not json").unwrap();
    let h = InFlightAnchors::load(Some(path.clone()));
    assert!(h.blocked().is_some());
    assert!(h.record_sent(&receipt(None, None)).is_err());
    assert!(
        std::fs::read(&path).unwrap().starts_with(b"{ not json"),
        "kept as is"
    );
    // No app data folder at all: also blocked, never silently in memory.
    assert!(InFlightAnchors::load(None).blocked().is_some());
}

#[test]
fn approve_passes_the_vault_gate_and_records_before_sending() {
    let src = include_str!("chain_agent.rs");
    let i = src
        .find("pub async fn hermes_anchor_approve(")
        .expect("command");
    let body = &src[i..i + 2500];
    assert!(
        body.contains("AnchorGuards"),
        "approve must pass the guards"
    );
    assert!(body.contains("before_send"));
    assert!(
        body.contains("CustodyState"),
        "the member's vault is the unlock gate"
    );
    assert!(body.contains("with_placeholder_caps"), "gas caps apply");
}

// ---------------------------------------------------------------------------------------------
// re-poll: a transaction that can no longer be mined releases its day

fn sent(nonce: Option<u64>) -> AnchorReceipt {
    AnchorReceipt {
        nonce,
        from: nonce.map(|_| format!("0x{}", "ab".repeat(20))),
        ..receipt(None, None)
    }
}

#[test]
fn a_mined_receipt_decides_the_day_whatever_the_nonce_says() {
    let rc = crate::rpc::Receipt {
        tx_hash: sent(Some(5)).tx_hash,
        block_number: 42,
        status: Some(1),
        gas_used: Some(48_000),
        effective_gas_price: Some(1_000_000_000),
    };
    match repoll_decision(&sent(Some(5)), Some(9), Some(rc)) {
        Repoll::Mined(done) => {
            assert_eq!((done.block_number, done.status), (Some(42), Some(1)));
            assert!(receipt_confirms(&done));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_receipt_and_a_mined_nonce_past_it_releases_the_day() {
    assert_eq!(
        repoll_decision(&sent(Some(5)), Some(6), None),
        Repoll::Dropped
    );
}

#[test]
fn otherwise_a_sent_day_stays_held() {
    // Not yet mined, nonce not passed (equal: the transaction may still be the next one mined).
    assert_eq!(repoll_decision(&sent(Some(5)), Some(5), None), Repoll::Wait);
    assert_eq!(repoll_decision(&sent(Some(5)), Some(4), None), Repoll::Wait);
    // The nonce could not be read.
    assert_eq!(repoll_decision(&sent(Some(5)), None, None), Repoll::Wait);
    // An older record without a nonce never releases on its own.
    assert_eq!(repoll_decision(&sent(None), Some(100), None), Repoll::Wait);
}

#[test]
fn an_older_in_flight_file_without_nonces_still_loads() {
    let path = in_flight_path("older-file");
    let old = serde_json::json!([{
        "day": 20000,
        "commitment": format!("0x{}", "a0".repeat(32)),
        "txHash": format!("0x{}", "cd".repeat(32)),
        "blockNumber": null,
        "status": null
    }]);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, old.to_string()).unwrap();
    let h = InFlightAnchors::load(Some(path));
    assert!(h.blocked().is_none());
    assert_eq!(h.all()[0].nonce, None);
    assert!(h.days().contains(&20000));
}

/// Review follow-up: a send the node refused outright (for example an unfunded anchor key) is not
/// on the way. Its record is forgotten, on disk too, so the next nightly pass can raise the day
/// again; it would otherwise wait on a transaction that can never be mined.
#[test]
fn a_refused_send_does_not_hold_the_day() {
    let path = in_flight_path("refused");
    let h = InFlightAnchors::load(Some(path.clone()));
    let sent = receipt(None, None);
    h.record_sent(&sent).expect("record before send");
    // Another transaction for the day is never forgotten by this one's refusal.
    let other = AnchorReceipt {
        tx_hash: format!("0x{}", "ef".repeat(32)),
        ..sent.clone()
    };
    h.forget_unsent(&other);
    assert!(h.days().contains(&20000));
    assert!(InFlightAnchors::load(Some(path.clone()))
        .days()
        .contains(&20000));
    h.forget_unsent(&sent);
    assert!(h.days().is_empty());
    let reloaded = InFlightAnchors::load(Some(path));
    assert!(reloaded.days().is_empty(), "forgotten on disk too");
    let mut port = Port {
        status: serde_json::json!({ "awaitingConfirmation": [{"day": 20000}] }),
        ..Port::default()
    };
    port.plans.insert(20000, ready_plan(20000, 0xa0, REG));
    let c = AnchorCeremony::new();
    let r = nightly_tick_with(&port, &c, AnchorGate::Ready, Some(REG), &reloaded.days()).unwrap();
    assert_eq!(r.raised.len(), 1, "the day can be anchored again");
}
