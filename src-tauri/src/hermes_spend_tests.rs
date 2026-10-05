//! HUP-S7.5 (D-27): which transactions are Hermes's, and what reaches the sidecar's metering.
use super::spend::*;
use super::{meter_anchor, AnchorPort};
use crate::ceremony::anchor::AnchorReceipt;
use crate::ceremony::BroadcastResult;
use crate::hermes::chain::PlannedAnchor;
use std::sync::Mutex;

const ROUTER: &str = "0x00000000000000000000000000000000000000A7";
const TX: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";

fn mined(origin: &str, to: Option<&str>) -> BroadcastResult {
    BroadcastResult {
        tx_hash: TX.into(),
        block_number: Some(9),
        status: Some(1),
        gas_used: Some(90_000),
        effective_gas_price_wei: Some("1000000000".into()),
        value_wei: Some("3000000000000000000".into()),
        to: to.map(str::to_string),
        origin: origin.into(),
    }
}

#[test]
fn only_hermes_origins_are_metered_and_the_router_marks_a_registry_escalation() {
    let r = Some(ROUTER);
    let router_lower = ROUTER.to_ascii_lowercase();
    assert_eq!(
        purpose_for("agent:hermes", Some(&router_lower), r),
        Some("registry_escalation")
    );
    assert_eq!(purpose_for("agent:hermes", Some("0x01"), r), Some("agent"));
    assert_eq!(purpose_for("agent:hermes", None, r), Some("agent"));
    assert_eq!(
        purpose_for("agent:hermes", Some(&router_lower), None),
        Some("agent"),
        "no pinned router, no registry escalation"
    );
    assert_eq!(
        purpose_for("hermes:benchmark-share", None, r),
        Some("benchmark")
    );
    assert_eq!(
        purpose_for("hermes:nightly-anchor", None, r),
        Some("anchor")
    );
    for not_hermes in [
        "local-user",
        "agent:node-agent",
        "https://app.citrate.ai",
        "agent:hermesx",
        "hermesx:benchmark",
        "",
    ] {
        assert_eq!(
            purpose_for(not_hermes, Some(ROUTER), r),
            None,
            "{not_hermes}"
        );
    }
}

#[test]
fn a_complete_mined_receipt_becomes_the_sidecar_body() {
    let b = broadcast_body(&mined("agent:hermes", Some(ROUTER)), Some(ROUTER)).unwrap();
    assert_eq!(
        b,
        serde_json::json!({
            "txHash": TX,
            "purpose": "registry_escalation",
            "status": 1,
            "gasUsed": 90_000,
            "effectiveGasPriceWei": "1000000000",
            "valueWei": "3000000000000000000",
        })
    );
    let mut reverted = mined("agent:hermes", None);
    reverted.status = Some(0);
    assert_eq!(broadcast_body(&reverted, None).unwrap()["status"], 0);
}

#[test]
fn an_incomplete_or_foreign_receipt_is_not_metered() {
    let unmined = BroadcastResult {
        block_number: None,
        ..mined("agent:hermes", None)
    };
    let no_gas = BroadcastResult {
        gas_used: None,
        ..mined("agent:hermes", None)
    };
    let no_price = BroadcastResult {
        effective_gas_price_wei: None,
        ..mined("agent:hermes", None)
    };
    let odd_status = BroadcastResult {
        status: Some(2),
        ..mined("agent:hermes", None)
    };
    for r in [
        unmined,
        no_gas,
        no_price,
        odd_status,
        mined("local-user", None),
    ] {
        assert!(broadcast_body(&r, Some(ROUTER)).is_none(), "{r:?}");
    }
}

fn anchor(block: Option<u64>, gas: Option<u64>) -> AnchorReceipt {
    AnchorReceipt {
        day: 20_000,
        commitment: [0xa0; 32],
        tx_hash: TX.into(),
        block_number: block,
        status: block.map(|_| 1),
        nonce: Some(3),
        from: None,
        gas_used: gas,
        effective_gas_price_wei: gas.map(|_| "7".to_string()),
    }
}

/// A port that records what it was asked to meter.
#[derive(Default)]
struct Port(Mutex<Vec<serde_json::Value>>);
impl AnchorPort for Port {
    fn status(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::Value::Null)
    }
    fn plan(&self, _day: u64, _registry: &str) -> Result<PlannedAnchor, String> {
        Err("unused".into())
    }
    fn confirm(&self, _d: u64, _c: &str, _t: &str, _b: u64) -> Result<(), String> {
        Ok(())
    }
    fn chain_receipt(&self, body: &serde_json::Value) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(body.clone());
        Ok(())
    }
}

#[test]
fn a_mined_anchor_meters_its_gas_and_an_unmined_one_waits() {
    let p = Port::default();
    meter_anchor(&p, &anchor(None, None));
    meter_anchor(&p, &anchor(Some(5), None));
    assert!(p.0.lock().unwrap().is_empty(), "nothing is known yet");
    meter_anchor(&p, &anchor(Some(5), Some(61_000)));
    let got = p.0.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["purpose"], "anchor");
    assert_eq!(got[0]["gasUsed"], 61_000);
    assert_eq!(got[0]["valueWei"], "0", "an anchor moves no value");
}
