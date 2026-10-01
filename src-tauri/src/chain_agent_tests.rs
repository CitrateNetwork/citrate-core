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

/// Honest-state tripwire: the shipped book has neither registry (F-4: not deployed on 40204).
/// When the redeploy lands and the book is regenerated, this test fails on purpose: the surface
/// and the schedule must then be rechecked against the deployed contracts.
#[test]
fn the_shipped_book_has_no_anchor_or_benchmark_registry_yet() {
    assert_eq!(anchor_registry(), None);
    assert_eq!(benchmark_registry(), None);
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
    assert!(anchor_status_line(AnchorGate::NotDeployed).contains("not deployed on 40204"));
    assert!(benchmark_status_line(None, &off).contains("not deployed on 40204"));
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
